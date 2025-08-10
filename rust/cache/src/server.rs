use std::{
    collections::HashMap,
    io::Cursor,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use foyer::{
    Compression, DirectFsDeviceOptions, Engine, HybridCache, HybridCacheBuilder, RuntimeOptions,
    TokioRuntimeOptions,
};
use futures::{
    FutureExt, Stream, StreamExt, TryStreamExt, future::ready, stream::FuturesUnordered,
};
use mini_moka::sync::Cache;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::{
    select,
    sync::{
        Mutex,
        broadcast::{self, Sender},
    },
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

use crate::build;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlugData {
    pub base_patch_url: String,
    pub repository: String,
    pub versions: Vec<GameVersion>,
    pub latest_version: GameVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum CacheKey {
    Slug(Slug),
    ClutFile(Slug, GameVersion),
    PatchData(Slug, PatchVersion, PatchRef),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum CacheValue {
    Slug(SlugData),
    ClutFile(Vec<u8>),
    PatchData(Vec<u8>),
}

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct Server(Arc<ServerImpl>);

type InFlightResult = Result<Bytes, String>;

#[derive(Debug)]
struct ServerImpl {
    cache: HybridCache<CacheKey, CacheValue>,
    http_client: Client,

    downloader: Downloader,
    // Track in-flight downloads to avoid duplicate requests
    in_flight_downloads: Arc<Mutex<HashMap<CacheKey, Sender<InFlightResult>>>>,

    clut_path: String,
    clut_cache: Cache<(Slug, GameVersion), Arc<Clut>>,

    slug_updater_thread: OnceLock<JoinHandle<()>>,
    slug_updater_token: CancellationToken,
}

pub struct ServerBuilder {
    clut_path: String,
    clut_ram_capacity: u64,
    clut_tti_secs: u64,
    slug_update_interval_secs: u64,
    ram_entry_capacity: usize,
    clut_data_multiplier: usize,
    patch_ref_multiplier: usize,
    storage_directory: Option<PathBuf>,
    storage_capacity_bytes: usize,
    max_concurrent_downloads: usize,
}

impl ServerBuilder {
    pub fn new() -> Self {
        Self {
            clut_path:
                "https://raw.githubusercontent.com/WorkingRobot/ffxiv-lut/refs/heads/main/cluts"
                    .to_string(),
            clut_ram_capacity: 8,          // 8 CLUTs in RAM
            clut_tti_secs: 5 * 60,         // 5 minutes
            slug_update_interval_secs: 60, // 1 minute
            ram_entry_capacity: 16384,     // 16k "entries" in RAM
            clut_data_multiplier: 1024,    // 1024x multiplier for CLUT data in RAM cache
            patch_ref_multiplier: 8,       // 8x multiplier for patch references in RAM cache
            storage_directory: None,
            storage_capacity_bytes: 10 * 1024 * 1024 * 1024, // 10 GiB
            max_concurrent_downloads: 16,
        }
    }

    pub fn clut_path(mut self, path: impl Into<String>) -> Self {
        self.clut_path = path.into();
        self
    }

    /// Maximum number of parsed CLUTs to keep in RAM
    pub fn clut_ram_capacity(mut self, capacity: u64) -> Self {
        self.clut_ram_capacity = capacity;
        self
    }

    /// Time to idle for CLUTs in RAM before eviction
    pub fn clut_tti_secs(mut self, secs: u64) -> Self {
        self.clut_tti_secs = secs;
        self
    }

    /// Interval in seconds to update repositories from Thaliak
    pub fn slug_update_interval_secs(mut self, secs: u64) -> Self {
        self.slug_update_interval_secs = secs;
        self
    }

    /// Maximum number of entries in RAM cache
    pub fn ram_entry_capacity(mut self, capacity: usize) -> Self {
        self.ram_entry_capacity = capacity;
        self
    }

    /// Multiplier for CLUT data in RAM cache
    pub fn clut_data_multiplier(mut self, multiplier: usize) -> Self {
        self.clut_data_multiplier = multiplier;
        self
    }

    /// Multiplier for patch references in RAM cache
    pub fn patch_ref_multiplier(mut self, multiplier: usize) -> Self {
        self.patch_ref_multiplier = multiplier;
        self
    }

    /// Directory to store cache files
    pub fn storage_directory(mut self, dir: impl Into<PathBuf>) -> Self {
        self.storage_directory = Some(dir.into());
        self
    }

    /// Maximum size of the storage in bytes
    pub fn storage_capacity_bytes(mut self, bytes: usize) -> Self {
        self.storage_capacity_bytes = bytes;
        self
    }

    /// Maximum number of concurrent connections to download patches
    pub fn max_concurrent_downloads(mut self, count: usize) -> Self {
        self.max_concurrent_downloads = count;
        self
    }

    pub async fn build(self) -> Result<Server> {
        Server::new(self).await
    }
}

impl Server {
    async fn new(builder: ServerBuilder) -> Result<Self> {
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
        } = builder;

        let cache = HybridCacheBuilder::new()
            .with_name("xiv-dl-cache")
            .memory(ram_entry_capacity)
            .with_weighter(move |k, _| match k {
                CacheKey::Slug(..) => 1,
                CacheKey::ClutFile(..) => clut_data_multiplier,
                CacheKey::PatchData(..) => patch_ref_multiplier,
            })
            .storage(Engine::large())
            .with_compression(Compression::Zstd)
            .with_device_options(
                DirectFsDeviceOptions::new(
                    storage_directory.ok_or_else(|| anyhow!("No storage backend provided"))?,
                )
                .with_capacity(storage_capacity_bytes),
            )
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
            in_flight_downloads: Arc::default(),
            clut_path,
            clut_cache,
            slug_updater_thread: OnceLock::new(),
            slug_updater_token: CancellationToken::new(),
        }));

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
                    loop {
                        select! {
                            biased;
                            _ = cancellation_token.cancelled() => {
                                log::debug!("Slug updater thread cancelled");
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

    async fn update_slugs(&self) -> Result<()> {
        let repos = get_all_repositories(&self.0.http_client).await?;

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

            self.0
                .cache
                .insert(CacheKey::Slug(slug), CacheValue::Slug(slug_data));
        }
        Ok(())
    }

    pub async fn get_patch_data<'a>(
        &self,
        slug: Slug,
        refs: impl Iterator<Item = (&'a PatchVersion, &'a PatchRef)>,
    ) -> Result<impl Stream<Item = Result<((&'a PatchVersion, &'a PatchRef), Bytes)>>> {
        let pending_downloads = Mutex::new(HashMap::new());

        let cache_futures: Vec<_> = refs
            .map(|(patch_ver, patch_ref)| async move {
                Ok::<_, anyhow::Error>((
                    (patch_ver, patch_ref),
                    self.0
                        .cache
                        .obtain(CacheKey::PatchData(
                            slug,
                            (*patch_ver).clone(),
                            (*patch_ref).clone(),
                        ))
                        .await?,
                ))
            })
            .collect::<FuturesUnordered<_>>()
            .and_then(async |((patch_ver, patch_ref), cache_entry)| {
                match cache_entry.as_deref().cloned() {
                    Some(CacheValue::PatchData(data)) => {
                        Ok(ready(Ok(((patch_ver, patch_ref), Bytes::from(data)))).boxed_local())
                    }
                    Some(_) => Err(anyhow::anyhow!("Invalid cache value for patch data")),
                    None => {
                        let mut receiver = {
                            let key =
                                CacheKey::PatchData(slug, patch_ver.clone(), patch_ref.clone());
                            let mut in_flight = self.0.in_flight_downloads.lock().await;
                            if let Some(sender) = in_flight.get(&key) {
                                // Download is already in progress, subscribe to it
                                sender.subscribe()
                            } else {
                                let (sender, receiver) =
                                    broadcast::channel::<Result<Bytes, String>>(1);
                                in_flight.insert(key, sender.clone());
                                drop(in_flight); // Prevent any possible deadlock
                                pending_downloads
                                    .lock()
                                    .await
                                    .insert((patch_ver.clone(), patch_ref.clone()), sender);
                                receiver
                            }
                        };

                        Ok(async move {
                            receiver
                                .recv()
                                .await
                                .context("In-flight download channel closed")?
                                .map(|bytes| ((patch_ver, patch_ref), bytes))
                                .map_err(|err_str| anyhow::anyhow!(err_str))
                        }
                        .boxed_local())
                    }
                }
            })
            .try_collect()
            .await?;

        let pending_downloads = pending_downloads.into_inner();
        if !pending_downloads.is_empty() {
            self.download_patch_data(slug, pending_downloads).await;
        }

        Ok(FuturesUnordered::from_iter(cache_futures.into_iter()))
    }

    async fn download_patch_data(
        &self,
        slug: Slug,
        mut cache_misses: HashMap<(PatchVersion, PatchRef), Sender<Result<Bytes, String>>>,
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
                let mut download_stream = self
                    .0
                    .downloader
                    .get_patch_data(&base_patch_url, download_refs.into_iter());

                while let Some(result) = download_stream.next().await {
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
                            let lookup_key = (patch_ref.0.as_ref().clone(), patch_ref.1.clone());

                            // Notify the subscriber of the download result
                            if let Some(sender) = cache_misses.remove(&lookup_key) {
                                if let Err(e) = sender.send(Ok(bytes)) {
                                    log::error!(
                                        "Failed to send patch data to receiver for {} {:?}: {}",
                                        patch_ref.0,
                                        patch_ref.1,
                                        e
                                    );
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
                }
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
            cache_misses.drain().for_each(|(_, sender)| {
                if let Err(e) = sender.send(Err(errors.clone())) {
                    log::error!("Failed to send error to receiver: {e}");
                }
            });
        }
    }

    pub async fn get_clut(&self, slug: Slug, version: GameVersion) -> Result<Arc<Clut>> {
        if let Some(clut) = self.0.clut_cache.get(&(slug, version.clone())) {
            return Ok(clut);
        }

        let cache_result = self
            .0
            .cache
            .fetch(CacheKey::ClutFile(slug, version.clone()), || {
                let this = self.clone();
                let version = version.clone();
                async move {
                    let clut_url = format!("{}/{}/{}.clut", this.0.clut_path, slug, version);
                    async {
                        let clut_bytes = this
                            .0
                            .http_client
                            .get(&clut_url)
                            .send()
                            .await?
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
            let clut = Arc::new(clut);
            self.0.clut_cache.insert((slug, version), clut.clone());

            Ok(clut)
        } else {
            bail!(
                "Invalid cache value for CLUT file: expected ClutFile, found {:?}",
                cache_result.value()
            );
        }
    }
}

impl Drop for ServerImpl {
    fn drop(&mut self) {
        self.slug_updater_token.cancel();
    }
}
