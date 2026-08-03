use std::path::PathBuf;

use serde::Deserialize;

use crate::server::Server;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerBuilder {
    pub(super) clut_path: String,
    pub(super) clut_ram_bytes: u64,
    pub(super) batch_window_ms: u64,
    pub(super) clut_tti_secs: u64,
    pub(super) slug_update_interval_secs: u64,
    pub(super) clut_index_interval_secs: u64,
    pub(super) ram_entry_capacity: usize,
    pub(super) clut_data_multiplier: usize,
    pub(super) patch_ref_multiplier: usize,
    pub(super) storage_directory: Option<PathBuf>,
    pub(super) storage_capacity_bytes: usize,
    pub(super) max_concurrent_downloads: usize,
    #[cfg(feature = "prometheus")]
    #[serde(skip)]
    pub(super) prometheus_registry: Option<prometheus::Registry>,
}

impl ServerBuilder {
    pub fn clut_path(mut self, path: impl Into<String>) -> Self {
        self.clut_path = path.into();
        self
    }

    /// How many bytes of parsed CLUTs to keep in RAM.
    pub fn clut_ram_bytes(mut self, bytes: u64) -> Self {
        self.clut_ram_bytes = bytes;
        self
    }

    /// How long to gather patch-data requests before issuing them as one ranged
    /// request. A read submits all of its references at once, so this only needs to be
    /// long enough to merge concurrent readers; every miss waits this long.
    pub fn batch_window_ms(mut self, ms: u64) -> Self {
        self.batch_window_ms = ms;
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

    /// Interval in seconds to relist the CLUTs each repository has. One listing per
    /// repository is one API request, so this is kept well clear of GitHub's hourly
    /// allowance rather than following the Thaliak tick.
    pub fn clut_index_interval_secs(mut self, secs: u64) -> Self {
        self.clut_index_interval_secs = secs;
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

    #[cfg(feature = "prometheus")]
    /// Prometheus registry for metrics
    pub fn prometheus_registry(mut self, registry: prometheus::Registry) -> Self {
        self.prometheus_registry = Some(registry);
        self
    }

    pub async fn build(self) -> anyhow::Result<Server> {
        Server::new(self).await
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self {
            clut_path:
                "https://raw.githubusercontent.com/WorkingRobot/ffxiv-lut/refs/heads/main/cluts"
                    .to_string(),
            clut_ram_bytes: 512 << 20, // 512 MiB of parsed CLUTs
            batch_window_ms: 20,
            clut_tti_secs: 30 * 60,            // 30 minutes
            slug_update_interval_secs: 60,     // 1 minute
            clut_index_interval_secs: 60 * 60, // 1 hour
            ram_entry_capacity: 16384,         // 16k "entries" in RAM
            clut_data_multiplier: 1024,        // 1024x multiplier for CLUT data in RAM cache
            patch_ref_multiplier: 8,           // 8x multiplier for patch references in RAM cache
            storage_directory: None,
            storage_capacity_bytes: 10 * 1024 * 1024 * 1024, // 10 GiB
            max_concurrent_downloads: 16,
            #[cfg(feature = "prometheus")]
            prometheus_registry: None,
        }
    }
}
