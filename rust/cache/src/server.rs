use std::{fmt::Display, path::Path, str::FromStr, sync::Arc};

use anyhow::{Context, Result};
use bytes::Bytes;
use foyer::{
    Compression, DirectFsDeviceOptions, Engine, HybridCache, HybridCacheBuilder, RuntimeOptions,
    TokioRuntimeOptions,
};
use futures::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;
use xiv_core::{
    downloader::Downloader,
    file::{
        patch_ref::PatchRef,
        version::{GameVersion, PatchVersion},
    },
    thaliak::get_all_repositories,
};

use crate::build;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Slug(u32);

impl FromStr for Slug {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let slug = u32::from_str_radix(s, 16).context(format!("Invalid slug format: {}", s))?;
        Ok(Slug(slug))
    }
}

impl Display for Slug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

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

pub struct Server {
    cache: HybridCache<CacheKey, CacheValue>,
    downloader: Downloader,
    update_client: Client,
}

impl Server {
    pub async fn new(
        ram_entry_capacity: usize,
        patch_ref_multiplier: usize,
        storage_directory: impl AsRef<Path>,
        storage_capacity_bytes: usize,
        max_concurrent_downloads: usize,
    ) -> Result<Self> {
        let cache = HybridCacheBuilder::new()
            .with_name("xiv-dl-cache")
            .memory(ram_entry_capacity)
            .with_weighter(move |k, _| {
                if matches!(k, CacheKey::PatchData(..)) {
                    patch_ref_multiplier
                } else {
                    1
                }
            })
            .storage(Engine::large())
            .with_compression(Compression::Zstd)
            .with_device_options(
                DirectFsDeviceOptions::new(storage_directory).with_capacity(storage_capacity_bytes),
            )
            .with_runtime_options(RuntimeOptions::Unified(TokioRuntimeOptions::default()))
            .build()
            .await?;

        let downloader = Downloader::new(max_concurrent_downloads)?;

        let client = Client::builder()
            .user_agent(format!("{}/{}", build::PROJECT_NAME, build::PKG_VERSION))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            cache,
            downloader,
            update_client: client,
        })
    }

    pub async fn update_slugs(&self) -> Result<()> {
        let repos = get_all_repositories(&self.update_client).await?;

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
                base_patch_url: base_patch_url,
                repository: repo.name,
                versions,
                latest_version,
            };

            self.cache
                .insert(CacheKey::Slug(slug), CacheValue::Slug(slug_data));
        }
        // Implementation for updating slugs in the cache
        Ok(())
    }

    pub async fn get_patch_data(
        &self,
        slug: Slug,
        refs: Vec<(&PatchVersion, &PatchRef)>,
    ) -> impl Stream<Item = Result<((Arc<PatchVersion>, PatchRef), Bytes)>> {
    }
}
