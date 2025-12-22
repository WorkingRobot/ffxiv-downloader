use std::{
    collections::HashMap,
    io::Cursor,
    str::FromStr,
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use foyer::{
    BlockEngineBuilder, Compression, DeviceBuilder, FsDeviceBuilder, HybridCache,
    HybridCacheBuilder, IoEngineBuilder, NoopIoEngineBuilder, RuntimeOptions, TokioRuntimeOptions,
};
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
        clut::Clut,
        patch_ref::PatchRef,
        slug::Slug,
        version::{GameVersion, PatchVersion},
    },
    thaliak::get_all_repositories,
};

// Doesn't work on Docker without seccomp changes, so let's just not touch it at all.
// #[cfg(target_os = "linux")]
// type FoyerIoEngineBuilder = foyer::UringIoEngineBuilder;

// #[cfg(not(target_os = "linux"))]
type FoyerIoEngineBuilder = foyer::PsyncIoEngineBuilder;

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
        assert!(
            self.tx.is_none(),
            "SafeSender was dropped without sending a message"
        );
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
    clut_cache: Cache<(Slug, GameVersion), Arc<Clut>>,

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
            clut_ram_capacity,
            clut_tti_secs,
            slug_update_interval_secs,
            ram_entry_capacity,
            clut_data_multiplier,
            patch_ref_multiplier,
            storage_directory,
            storage_capacity_bytes,
            max_concurrent_downloads,
            #[cfg(feature = "prometheus")]
            prometheus_registry,
        } = builder;

        // Create mpsc channel for batching patch data requests
        let (patch_batch_tx, patch_batch_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut cache = HybridCacheBuilder::new()
            .with_name("xiv-dl-cache")
            .with_flush_on_close(true);

        #[cfg(feature = "prometheus")]
        {
            use mixtrics::registry::prometheus::PrometheusMetricsRegistry;

            if let Some(prometheus_registry) = prometheus_registry {
                cache = cache.with_metrics_registry(Box::new(PrometheusMetricsRegistry::new(
                    prometheus_registry,
                )));
            }
        }

        let mut cache = cache
            .memory(ram_entry_capacity)
            .with_weighter(move |k, _| match k {
                CacheKey::SlugList => 1,
                CacheKey::Slug(..) => 1,
                CacheKey::ClutFile(..) => clut_data_multiplier,
                CacheKey::PatchData(..) => patch_ref_multiplier,
            })
            .storage();

        if let Some(storage_directory) = &storage_directory {
            cache = cache
                .with_io_engine(FoyerIoEngineBuilder::default().build().await?)
                .with_engine_config(BlockEngineBuilder::new(
                    FsDeviceBuilder::new(storage_directory)
                        .with_capacity(storage_capacity_bytes)
                        .build()?,
                ))
        } else {
            cache = cache.with_io_engine(NoopIoEngineBuilder::default().build().await?);
        }

        let cache = cache
            .with_compression(Compression::Zstd)
            .with_runtime_options(RuntimeOptions::Unified(TokioRuntimeOptions::default()))
            .build()
            .await?;

        let http_client = Client::builder()
            .user_agent(format!("{}/{}", build::PROJECT_NAME, build::PKG_VERSION))
            .build()
            .context("Failed to create HTTP client")?;

        let downloader = Downloader::new(max_concurrent_downloads)?;

        let clut_cache = Cache::builder()
            .max_capacity(clut_ram_capacity)
            .time_to_idle(std::time::Duration::from_secs(clut_tti_secs))
            .build();

        let this = Self(Arc::new(ServerImpl {
            cache,
            http_client,
            downloader,
            clut_path,
            clut_cache,
            slug_updater_thread: OnceLock::new(),
            slug_updater_token: CancellationToken::new(),
            patch_batch_tx,
        }));

        // Start background task for batching patch data requests
        {
            let batching_server = this.clone();
            let mut rx = patch_batch_rx;
            tokio::spawn(async move {
                let batch_interval = std::time::Duration::from_millis(500);
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

    pub async fn update_slugs(&self) -> Result<()> {
        let repos = get_all_repositories(&self.0.http_client).await?;

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
            let versions = repo
                .versions
                .into_iter()
                .filter(|v| v.is_active)
                .map(|v| {
                    GameVersion::new(&v.version_string)
                        .context(format!("Invalid version string: {}", v.version_string))
                })
                .collect::<Result<Vec<_>>>()?;

            let slug_data = SlugData {
                base_patch_url,
                repository: repo.name,
                versions,
                latest_version,
            };

            slugs.push(slug);
            self.0
                .cache
                .insert(CacheKey::Slug(slug), CacheValue::Slug(slug_data));
        }

        self.0
            .cache
            .insert(CacheKey::SlugList, CacheValue::SlugList(slugs));

        Ok(())
    }

    pub async fn get_slug_list(&self) -> Result<Vec<Slug>> {
        if let Some(CacheValue::SlugList(slugs)) =
            self.0.cache.obtain(CacheKey::SlugList).await?.as_deref()
        {
            Ok(slugs.clone())
        } else {
            bail!("Slug list not found in cache")
        }
    }

    pub async fn get_slug(&self, slug: Slug) -> Result<SlugData> {
        if let Some(CacheValue::Slug(slug_data)) =
            self.0.cache.obtain(CacheKey::Slug(slug)).await?.as_deref()
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
                    if cache.contains(&patch_key)
                        && let Some(CacheValue::PatchData(data)) =
                            cache.obtain(patch_key.clone()).await?.as_deref().cloned()
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

        let base_patch_url = async {
            // Get slug data to obtain base patch URL
            let slug_key = CacheKey::Slug(slug);
            let slug_data = self
                .0
                .cache
                .get(&slug_key)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Slug {} not found in cache", slug))?;
            let slug_data = match slug_data.value() {
                CacheValue::Slug(data) => data,
                _ => return Err(anyhow::anyhow!("Invalid slug data in cache")),
            };
            Ok(slug_data.base_patch_url.clone())
        }
        .await;

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

    pub async fn get_clut(&self, slug: Slug, version: GameVersion) -> Result<Arc<Clut>> {
        log::info!(
            "Requesting CLUT {version} ({} in cache)",
            self.0.clut_cache.entry_count()
        );
        self.0
            .clut_cache
            .try_get_with((slug, version.clone()), async {
                let cache_result = self
                    .0
                    .cache
                    .fetch(CacheKey::ClutFile(slug, version.clone()), || {
                        let this = self.clone();
                        let version = version.clone();
                        async move {
                            let clut_url =
                                format!("{}/{}/{}.clut", this.0.clut_path, slug, version);
                            async {
                                log::debug!(
                                    "Fetching CLUT file for slug: {}, version: {}",
                                    slug,
                                    version
                                );
                                let clut_bytes = this
                                    .0
                                    .http_client
                                    .get(&clut_url)
                                    .send()
                                    .await?
                                    .error_for_status()?
                                    .bytes()
                                    .await?;
                                Ok(CacheValue::ClutFile(clut_bytes.to_vec()))
                            }
                            .await
                            .map_err(foyer::Error::other::<reqwest::Error>)
                        }
                    })
                    .await?;

                if let CacheValue::ClutFile(bytes) = cache_result.value() {
                    let clut = Clut::read(Cursor::new(bytes))?;
                    if clut.header.repository != slug {
                        bail!(
                            "Invalid CLUT file: expected repository {}, found {}",
                            slug,
                            clut.header.repository
                        );
                    }
                    Ok(Arc::new(clut))
                } else {
                    bail!(
                        "Invalid cache value for CLUT file: expected ClutFile, found {:?}",
                        cache_result.value()
                    );
                }
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get CLUT: {e:?}"))
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
