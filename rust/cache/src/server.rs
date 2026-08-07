use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Seek, SeekFrom},
    path::Path,
    str::FromStr,
    sync::{Arc, OnceLock, RwLock},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use foyer::{
    BlockEngineConfig, Compression, Device, DeviceBuilder, FileDeviceBuilder, FsDeviceBuilder,
    HybridCache, HybridCacheBuilder, IoEngineConfig, NoopIoEngineConfig, PsyncIoEngineConfig,
};
#[cfg(target_os = "linux")]
use foyer::UringIoEngineConfig;
use futures::{
    FutureExt, Stream, StreamExt, TryStreamExt, future::ready, stream::FuturesUnordered,
};
use moka::future::Cache;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::{
    select,
    sync::oneshot::{self, Sender},
    task::JoinHandle,
    time::MissedTickBehavior,
};
use tokio_util::sync::CancellationToken;
use url::Url;
use xiv_core::{
    downloader::Downloader,
    file::{
        clut::ClutIndex,
        clut_lazy::LazyClut,
        patch_ref::PatchRef,
        slug::Slug,
        version::{GameVersion, PatchVersion},
    },
    thaliak::get_all_repositories,
};

/// Docker's default seccomp profile can deny `io_uring_setup`; probe a throwaway ring
/// and fall back to psync if so. Left at the single-ring default rather than tuned with
/// `.with_threads`/`.with_io_depth`, so the one-ring probe stays a faithful test of what
/// the real build will attempt.
#[cfg(target_os = "linux")]
fn select_io_engine_config() -> Box<dyn IoEngineConfig> {
    match io_uring::IoUring::new(1) {
        Ok(_) => {
            log::info!("disk cache I/O engine: io_uring");
            UringIoEngineConfig::new().boxed()
        }
        Err(e) => {
            log::info!("disk cache I/O engine: psync (io_uring unavailable: {e})");
            PsyncIoEngineConfig::new().boxed()
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn select_io_engine_config() -> Box<dyn IoEngineConfig> {
    log::info!("disk cache I/O engine: psync");
    PsyncIoEngineConfig::new().boxed()
}

use crate::{build, builder::ServerBuilder};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlugData {
    pub base_patch_url: String,
    pub repository: String,
    pub versions: Vec<GameVersion>,
    pub latest_version: GameVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum CacheKey {
    SlugList,
    Slug(Slug),
    ClutFile(Slug, GameVersion),
    PatchData(Slug, PatchVersion, PatchRef),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum CacheValue {
    SlugList(Vec<Slug>),
    Slug(SlugData),
    ClutFile(Vec<u8>),
    PatchData(Vec<u8>),
}

/// Which versions each repository has a CLUT for, and when that was last established.
#[derive(Debug, Default)]
struct ClutListing {
    listed: Option<Instant>,
    versions: HashMap<Slug, Vec<GameVersion>>,
}

#[derive(Deserialize)]
struct ContentEntry {
    name: String,
}

/// The size of an already-existing storage file or raw block device, used to fill
/// `storage_file` when no explicit capacity is given. `metadata().len()` reads as 0 for
/// a block special file, so this seeks to the end instead, which the kernel resolves to
/// the device's real size.
fn existing_size(path: &Path) -> Option<usize> {
    let size = File::open(path).ok()?.seek(SeekFrom::End(0)).ok()?;
    (size > 0).then_some(size as usize)
}

/// The listing endpoint for one repository's CLUTs, if they are held in a GitHub
/// repository. It names every CLUT a slug has in a single request, where asking after
/// each version separately would take thousands.
fn contents_api(clut_path: &str, slug: Slug) -> Option<String> {
    let rest = clut_path.strip_prefix("https://raw.githubusercontent.com/")?;
    let (owner, rest) = rest.split_once('/')?;
    let (repo, rest) = rest.split_once('/')?;
    let rest = rest.strip_prefix("refs/heads/").unwrap_or(rest);
    let (git_ref, dir) = rest.split_once('/')?;
    Some(format!(
        "https://api.github.com/repos/{owner}/{repo}/contents/{dir}/{slug}?ref={git_ref}"
    ))
}

struct SafeSender<T> {
    tx: Option<Sender<T>>,
}

impl<T> SafeSender<T> {
    pub fn new(tx: Sender<T>) -> Self {
        Self { tx: Some(tx) }
    }

    pub fn send(mut self, value: T) -> Result<(), T> {
        if let Some(tx) = self.tx.take() {
            tx.send(value)
        } else {
            Err(value)
        }
    }
}

impl<T> Drop for SafeSender<T> {
    fn drop(&mut self) {
        if self.tx.is_some() {
            log::debug!("dropping an unanswered patch data request");
        }
    }
}

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct Server(Arc<ServerImpl>);

#[derive(Debug)]
struct ServerImpl {
    cache: HybridCache<CacheKey, CacheValue>,
    http_client: Client,

    downloader: Downloader,

    clut_path: String,
    clut_cache: Cache<(Slug, GameVersion), Arc<LazyClut>>,

    clut_index: RwLock<ClutListing>,
    clut_index_interval: Duration,

    slugs: RwLock<Vec<(Slug, SlugData)>>,
    slug_updater_thread: OnceLock<JoinHandle<()>>,
    slug_updater_token: CancellationToken,
    // Shared queue for batching patch data requests
    patch_batch_tx: tokio::sync::mpsc::UnboundedSender<BatchPatchRequest>,
}

// Represents a single patch data request for batching
#[derive(Debug)]
pub struct BatchPatchRequest {
    pub slug: Slug,
    pub patch_version: PatchVersion,
    pub patch_ref: PatchRef,
    pub sender: Sender<Result<Bytes, String>>,
}

impl Server {
    pub(super) async fn new(builder: ServerBuilder) -> Result<Self> {
        let ServerBuilder {
            clut_path,
            clut_ram_bytes,
            batch_window_ms,
            clut_tti_secs,
            slug_update_interval_secs,
            clut_index_interval_secs,
            ram_entry_capacity,
            clut_data_multiplier,
            patch_ref_multiplier,
            storage_directory,
            storage_file,
            storage_capacity_bytes,
            max_concurrent_downloads,
            #[cfg(feature = "prometheus")]
            prometheus_registry,
        } = builder;

        // Create mpsc channel for batching patch data requests
        let (patch_batch_tx, patch_batch_rx) = tokio::sync::mpsc::unbounded_channel();

        let cache = HybridCacheBuilder::new()
            .with_name("xiv-dl-cache")
            .with_flush_on_close(true);

        #[cfg(feature = "prometheus")]
        let cache = match prometheus_registry {
            Some(prometheus_registry) => {
                use mixtrics::registry::prometheus::PrometheusMetricsRegistry;

                cache.with_metrics_registry(Box::new(PrometheusMetricsRegistry::new(
                    prometheus_registry,
                )))
            }
            None => cache,
        };

        let mut cache = cache
            .memory(ram_entry_capacity)
            .with_weighter(move |k, _| match k {
                CacheKey::SlugList => 1,
                CacheKey::Slug(..) => 1,
                CacheKey::ClutFile(..) => clut_data_multiplier,
                CacheKey::PatchData(..) => patch_ref_multiplier,
            })
            .storage();

        let device: Option<Arc<dyn Device>> = if let Some(storage_file) = &storage_file {
            let mut builder = FileDeviceBuilder::new(storage_file);
            if let Some(bytes) = storage_capacity_bytes.or_else(|| existing_size(storage_file)) {
                builder = builder.with_capacity(bytes);
            }
            Some(builder.build()?)
        } else if let Some(storage_directory) = &storage_directory {
            let mut builder = FsDeviceBuilder::new(storage_directory);
            if let Some(bytes) = storage_capacity_bytes {
                builder = builder.with_capacity(bytes);
            }
            Some(builder.build()?)
        } else {
            None
        };

        if let Some(device) = device {
            cache = cache
                .with_io_engine_config(select_io_engine_config())
                .with_engine_config(BlockEngineConfig::new(device))
        } else {
            cache = cache
                .with_io_engine_config(Box::new(NoopIoEngineConfig) as Box<dyn IoEngineConfig>);
        }

        let cache = cache.with_compression(Compression::Zstd).build().await?;

        let http_client = Client::builder()
            .user_agent(format!("{}/{}", build::PROJECT_NAME, build::PKG_VERSION))
            .build()
            .context("Failed to create HTTP client")?;

        let downloader = Downloader::new(max_concurrent_downloads)?;

        let clut_cache = Cache::builder()
            .max_capacity(clut_ram_bytes)
            .weigher(|_, clut: &Arc<LazyClut>| clut.resident_size().try_into().unwrap_or(u32::MAX))
            .time_to_idle(std::time::Duration::from_secs(clut_tti_secs))
            .build();

        let this = Self(Arc::new(ServerImpl {
            cache,
            http_client,
            downloader,
            clut_path,
            clut_cache,
            clut_index: RwLock::default(),
            clut_index_interval: Duration::from_secs(clut_index_interval_secs),
            slugs: RwLock::default(),
            slug_updater_thread: OnceLock::new(),
            slug_updater_token: CancellationToken::new(),
            patch_batch_tx,
        }));

        // Start background task for batching patch data requests
        {
            let batching_server = this.clone();
            let mut rx = patch_batch_rx;
            tokio::spawn(async move {
                let batch_interval = std::time::Duration::from_millis(batch_window_ms);
                loop {
                    let mut batch = Vec::new();
                    // Wait for at least one request or timeout
                    match rx.recv().await {
                        Some(req) => batch.push(req),
                        None => {
                            log::warn!("Patch batch channel closed, stopping batching thread");
                            break;
                        }
                    }

                    // Collect requests until timeout
                    let end_time = tokio::time::Instant::now() + batch_interval;
                    while let Ok(req) = tokio::time::timeout_at(end_time, rx.recv()).await {
                        if let Some(req) = req {
                            batch.push(req);
                        } else {
                            log::warn!("Patch batch channel closed, stopping batching thread");
                            break;
                        }
                    }

                    if !batch.is_empty() {
                        // Group by slug for batch processing
                        let mut by_slug: HashMap<_, Vec<_>> = HashMap::new();
                        for req in batch {
                            by_slug.entry(req.slug.clone()).or_default().push((
                                req.patch_version,
                                req.patch_ref,
                                SafeSender::new(req.sender),
                            ));
                        }
                        for (slug, reqs) in by_slug {
                            let batching_server = batching_server.clone();
                            let mut cache_misses: HashMap<_, Vec<_>> = HashMap::new();
                            for (ver, r, sender) in reqs {
                                cache_misses.entry((ver, r)).or_default().push(sender);
                            }

                            tokio::spawn(async move {
                                batching_server
                                    .download_patch_data(slug, cache_misses)
                                    .await
                            });
                        }
                    }
                }
            });
        }

        this.0
            .slug_updater_thread
            .set({
                let cancellation_token = this.0.slug_updater_token.clone();
                let this = Arc::downgrade(&this.0);
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                    slug_update_interval_secs,
                ));
                interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
                tokio::spawn(async move {
                    log::info!("Starting slug updater thread");
                    loop {
                        select! {
                            biased;
                            _ = cancellation_token.cancelled() => {
                                return;
                            }
                            _ = interval.tick() => {
                                let this = match this.upgrade() {
                                    Some(this) => this,
                                    None => return, // Server has been dropped
                                };
                                if let Err(e) = Self(this).update_slugs().await {
                                    log::error!("Failed to update slugs: {e:?}");
                                }
                            }
                        }
                    }
                })
            })
            .map_err(|_| anyhow::anyhow!("Failed to initialize slug updater thread"))?;

        Ok(this)
    }

    /// Relist the CLUTs each repository has, if the last listing has aged out.
    async fn refresh_clut_index(&self, slugs: &[Slug]) {
        if self
            .0
            .clut_index
            .read()
            .unwrap()
            .listed
            .is_some_and(|at| at.elapsed() < self.0.clut_index_interval)
        {
            return;
        }

        let listings: HashMap<Slug, Vec<GameVersion>> = slugs
            .iter()
            .filter_map(|slug| contents_api(&self.0.clut_path, *slug).map(|url| (*slug, url)))
            .map(|(slug, url)| async move {
                match self.list_cluts(&url).await {
                    Ok(versions) => Some((slug, versions)),
                    Err(e) => {
                        log::warn!("Failed to list CLUTs for {slug}: {e:?}");
                        None
                    }
                }
            })
            .collect::<FuturesUnordered<_>>()
            .filter_map(ready)
            .collect()
            .await;

        if listings.is_empty() {
            return;
        }
        let mut index = self.0.clut_index.write().unwrap();
        index.listed = Some(Instant::now());
        index.versions.extend(listings);
    }

    async fn list_cluts(&self, url: &str) -> Result<Vec<GameVersion>> {
        let entries: Vec<ContentEntry> = self
            .0
            .http_client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut versions: Vec<GameVersion> = entries
            .iter()
            .filter_map(|entry| entry.name.strip_suffix(".clut"))
            .filter_map(|name| GameVersion::new(name).ok())
            .collect();
        versions.sort();
        Ok(versions)
    }

    pub async fn update_slugs(&self) -> Result<()> {
        let repos = get_all_repositories(&self.0.http_client).await?;

        let known = repos
            .iter()
            .map(|repo| Slug::from_str(&repo.slug))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.refresh_clut_index(&known).await;

        let mut slugs = Vec::new();
        for repo in repos {
            let slug = Slug::from_str(&repo.slug)?;
            let latest_patch =
                repo.latest_version.patches.first().ok_or_else(|| {
                    anyhow::anyhow!("No patches found for repository: {}", repo.slug)
                })?;
            let base_patch_url = {
                let mut patch_url = latest_patch.url.parse::<Url>()?;
                patch_url
                    .path_segments_mut()
                    .map_err(|_| {
                        anyhow::anyhow!("Failed to parse patch URL: {}", latest_patch.url)
                    })?
                    .pop();
                patch_url.to_string()
            };
            let latest_version = GameVersion::new(&repo.latest_version.version_string)?;
            let served = self
                .0
                .clut_index
                .read()
                .unwrap()
                .versions
                .get(&slug)
                .cloned();
            // A version can only be read if a CLUT was built for it, which covers the
            // lineages the current patch chain has left behind as well as the one it is
            // on. Without a listing to go by, fall back to what the chain still offers.
            let versions = match served {
                Some(served) => served,
                None => {
                    let mut versions = repo
                        .versions
                        .into_iter()
                        .filter(|v| v.is_active)
                        .map(|v| {
                            GameVersion::new(&v.version_string)
                                .context(format!("Invalid version string: {}", v.version_string))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    versions.sort();
                    versions
                }
            };

            let slug_data = SlugData {
                base_patch_url,
                repository: repo.name,
                versions,
                latest_version,
            };

            self.0
                .cache
                .insert(CacheKey::Slug(slug), CacheValue::Slug(slug_data.clone()));
            slugs.push((slug, slug_data));
        }

        self.0.cache.insert(
            CacheKey::SlugList,
            CacheValue::SlugList(slugs.iter().map(|(slug, _)| *slug).collect()),
        );
        *self.0.slugs.write().unwrap() = slugs;

        Ok(())
    }

    fn resident_slug(&self, slug: Slug) -> Option<SlugData> {
        let slugs = self.0.slugs.read().unwrap();
        slugs
            .iter()
            .find(|(s, _)| *s == slug)
            .map(|(_, data)| data.clone())
    }

    pub async fn get_slug_list(&self) -> Result<Vec<Slug>> {
        let resident: Vec<Slug> = self
            .0
            .slugs
            .read()
            .unwrap()
            .iter()
            .map(|(slug, _)| *slug)
            .collect();
        if !resident.is_empty() {
            return Ok(resident);
        }
        if let Some(CacheValue::SlugList(slugs)) =
            self.0.cache.get(&CacheKey::SlugList).await?.as_deref()
        {
            Ok(slugs.clone())
        } else {
            bail!("Slug list not found in cache")
        }
    }

    pub async fn get_slug(&self, slug: Slug) -> Result<SlugData> {
        if let Some(slug_data) = self.resident_slug(slug) {
            return Ok(slug_data);
        }
        if let Some(CacheValue::Slug(slug_data)) =
            self.0.cache.get(&CacheKey::Slug(slug)).await?.as_deref()
        {
            Ok(slug_data.clone())
        } else {
            bail!("Slug {slug} not found in cache");
        }
    }

    pub async fn get_patch_data<'a>(
        &self,
        slug: Slug,
        refs: impl Iterator<Item = (&'a PatchVersion, &'a PatchRef)>,
    ) -> Result<impl Stream<Item = Result<((&'a PatchVersion, &'a PatchRef), Bytes)>> + Send> {
        let cache_futures: Vec<_> = refs
            .map(|(patch_ver, patch_ref)| {
                let cache = &self.0.cache;
                let patch_key =
                    CacheKey::PatchData(slug, (*patch_ver).clone(), (*patch_ref).clone());
                let patch_batch_tx = self.0.patch_batch_tx.clone();
                async move {
                    if let Some(CacheValue::PatchData(data)) =
                        cache.get(&patch_key).await?.as_deref().cloned()
                    {
                        Ok::<_, anyhow::Error>(
                            ready(Ok(((patch_ver, patch_ref), Bytes::from(data)))).boxed(),
                        )
                    } else {
                        let (sender, receiver) = oneshot::channel();
                        // Enqueue the batch request
                        let req = BatchPatchRequest {
                            slug,
                            patch_version: (*patch_ver).clone(),
                            patch_ref: (*patch_ref).clone(),
                            sender,
                        };
                        // Ignore send error (if channel closed, request is dropped)
                        if let Err(e) = patch_batch_tx.send(req) {
                            _ =
                                e.0.sender
                                    .send(Err("Failed to send out batch request".to_string()));
                        }
                        Ok(async move {
                            let r = receiver.await.context("In-flight download channel closed");
                            let r = match r {
                                Ok(data) => data,
                                Err(e) => {
                                    bail!(e);
                                }
                            };
                            r.map(|bytes| ((patch_ver, patch_ref), bytes))
                                .map_err(|err_str| anyhow::anyhow!(err_str))
                        }
                        .boxed())
                    }
                }
            })
            .collect::<FuturesUnordered<_>>()
            .try_collect()
            .await?;

        Ok(FuturesUnordered::from_iter(cache_futures.into_iter()))
    }

    async fn download_patch_data(
        &self,
        slug: Slug,
        mut cache_misses: HashMap<(PatchVersion, PatchRef), Vec<SafeSender<Result<Bytes, String>>>>,
    ) {
        let mut errors = Vec::new();

        let base_patch_url = self
            .get_slug(slug)
            .await
            .map(|slug_data| slug_data.base_patch_url);

        match base_patch_url {
            Ok(base_patch_url) => {
                // Use downloader to fetch missing patch data with batching
                // Collect the keys first to avoid borrowing conflicts
                let keys_to_download: Vec<(PatchVersion, PatchRef)> =
                    cache_misses.keys().cloned().collect();
                let download_refs: Vec<_> = keys_to_download.iter().map(|(v, r)| (v, r)).collect();
                let download_stream = self
                    .0
                    .downloader
                    .get_patch_data(&base_patch_url, download_refs.into_iter());

                download_stream
                    .for_each_concurrent(None, |result| {
                        match result {
                            Ok((patch_ref, bytes)) => {
                                // Store the downloaded data in the cache
                                self.0.cache.insert(
                                    CacheKey::PatchData(
                                        slug,
                                        patch_ref.0.as_ref().clone(),
                                        patch_ref.1.clone(),
                                    ),
                                    CacheValue::PatchData(bytes.to_vec()),
                                );

                                // Create the key to look up in cache_misses
                                let lookup_key =
                                    (patch_ref.0.as_ref().clone(), patch_ref.1.clone());

                                // Notify the subscriber of the download result
                                if let Some(senders) = cache_misses.remove(&lookup_key) {
                                    for sender in senders {
                                        _ = sender.send(Ok(bytes.clone()));
                                    }
                                } else {
                                    log::warn!(
                                        "No sender found for patch data {} {:?}",
                                        patch_ref.0,
                                        patch_ref.1
                                    );
                                }
                            }
                            Err(e) => {
                                errors.push(e);
                            }
                        }
                        ready(())
                    })
                    .await;
            }
            Err(e) => {
                errors.push(e);
            }
        }

        if !cache_misses.is_empty() {
            let errors = errors
                .into_iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            cache_misses.drain().for_each(|(_, senders)| {
                for sender in senders {
                    _ = sender.send(Err(errors.clone()));
                }
            });
        }

        assert!(
            cache_misses.is_empty(),
            "Some patch data requests were not fulfilled"
        );
    }

    async fn fetch_clut_bytes(&self, slug: Slug, version: GameVersion) -> Result<Vec<u8>> {
        let cache_result = self
            .0
            .cache
            .get_or_fetch(&CacheKey::ClutFile(slug, version.clone()), || {
                let this = self.clone();
                let version = version.clone();
                async move {
                    let clut_url = format!("{}/{}/{}.clut", this.0.clut_path, slug, version);
                    log::debug!("Fetching CLUT file for slug: {slug}, version: {version}");
                    let clut_bytes = this
                        .0
                        .http_client
                        .get(&clut_url)
                        .send()
                        .await?
                        .error_for_status()?
                        .bytes()
                        .await?;
                    Ok::<_, reqwest::Error>(CacheValue::ClutFile(clut_bytes.to_vec()))
                }
            })
            .await?;

        match cache_result.value() {
            CacheValue::ClutFile(bytes) => Ok(bytes.clone()),
            other => bail!("Invalid cache value for CLUT file: expected ClutFile, found {other:?}"),
        }
    }

    /// Fetch a CLUT ready to serve reads. The per-file `DataRef`s are decoded on
    /// demand rather than up front.
    pub async fn get_clut(&self, slug: Slug, version: GameVersion) -> Result<Arc<LazyClut>> {
        log::debug!("Requesting CLUT {version}");
        self.0
            .clut_cache
            .try_get_with((slug, version.clone()), async {
                let bytes = self.fetch_clut_bytes(slug, version.clone()).await?;
                let clut = LazyClut::read(Cursor::new(&bytes))?;
                Self::check_clut(&clut, slug, &version)?;
                Ok::<_, anyhow::Error>(Arc::new(clut))
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get CLUT: {e:?}"))
    }

    fn check_clut(clut: &LazyClut, slug: Slug, version: &GameVersion) -> Result<()> {
        if clut.header.repository != slug {
            bail!(
                "Invalid CLUT file: expected repository {}, found {}",
                slug,
                clut.header.repository
            );
        }
        if &clut.header.version != version {
            bail!(
                "Invalid CLUT file: expected version {}, found {}",
                version,
                clut.header.version
            );
        }
        Ok(())
    }

    /// Like [`get_clut`](Self::get_clut) but yields only the folder set and each
    /// file's size. For an indexed CLUT the sizes come out of the index, so no ref is
    /// decoded. The result is not cached here; cache it in the caller if needed.
    pub async fn get_clut_index(&self, slug: Slug, version: GameVersion) -> Result<ClutIndex> {
        // Parsing an indexed CLUT is cheap, but re-reading its bytes is not, and a
        // browse usually follows a read of the same version.
        if let Some(clut) = self.0.clut_cache.get(&(slug, version.clone())).await {
            return Ok(clut.index());
        }
        let bytes = self.fetch_clut_bytes(slug, version.clone()).await?;
        let clut = LazyClut::read(Cursor::new(&bytes))?;
        Self::check_clut(&clut, slug, &version)?;
        Ok(clut.index())
    }

    pub async fn close(&self) -> Result<()> {
        self.0.slug_updater_token.cancel();
        self.0.cache.close().await?;
        Ok(())
    }
}

impl Drop for ServerImpl {
    fn drop(&mut self) {
        self.slug_updater_token.cancel();
    }
}
