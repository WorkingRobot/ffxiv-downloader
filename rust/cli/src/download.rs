use crate::{
    build,
    cache::CacheMetadata,
    diff::ClutDiff,
    ops::{FilteredFileOperations, PersistentFileOperations},
    patcher::ClutPatcher,
};
use anyhow::{Context, Result};
use clap::Args;
use regex::Regex;
use reqwest::Client;
use std::io::Cursor;
use std::{path::PathBuf, sync::Arc};
use url::Url;
use xiv_core::{
    file::{clut::Clut, version::GameVersion},
    thaliak::get_repository_metadata,
};

#[derive(Args, Debug, Clone)]
pub struct DownloadConfigArgs {
    /// Repository slug to download from (default: ffxiv global)
    #[arg(short, long, value_name = "SLUG", default_value = "4e9a232b")]
    pub slug: String,
    /// Base URL or path for CLUT files
    #[arg(
        short,
        long,
        value_name = "URL",
        default_value = "https://raw.githubusercontent.com/WorkingRobot/ffxiv-lut/refs/heads/main/cluts"
    )]
    pub clut_path: String,
    /// Specific version to download (default: latest)
    #[arg(short, long, value_name = "VERSION")]
    pub version: Option<String>,
    /// Output directory for downloaded files (default: current directory)
    #[arg(short, long, value_name = "DIR")]
    pub output_path: Option<PathBuf>,
    /// Regex pattern for files to download (default: all files)
    #[arg(short, long = "files", value_name = "REGEX")]
    pub file_patterns: Vec<String>,
    /// Number of parallel downloads (default: number of CPU cores)
    #[arg(short, long, value_name = "NUM")]
    pub parallelism: Option<usize>,
    /// Queue depth of the writing queue (default: parallelism * 4)
    #[arg(short, long, value_name = "DEPTH")]
    pub queue_depth: Option<usize>,
    /// Keep a .cachemeta.json file to track already downloaded versions
    /// instead of always starting from scratch
    #[arg(short, long, value_name = "BOOL", default_value_t = false, default_missing_value = "true", num_args = 0..=1)]
    pub use_cache: bool,
}

#[derive(Debug, Clone)]
pub struct DownloadConfig {
    pub slug: String,
    pub clut_path: String,
    pub version: Option<String>,
    pub output_path: PathBuf,
    pub file_patterns: Vec<String>,
    pub parallelism: usize,
    pub queue_depth: usize,
    pub use_cache: bool,
}

impl From<DownloadConfigArgs> for DownloadConfig {
    fn from(args: DownloadConfigArgs) -> Self {
        let parallelism = args.parallelism.unwrap_or(num_cpus::get());
        Self {
            slug: args.slug,
            clut_path: args.clut_path,
            version: args.version.filter(|v| !v.is_empty()),
            output_path: args
                .output_path
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
            file_patterns: args.file_patterns,
            parallelism,
            queue_depth: args.queue_depth.unwrap_or(parallelism * 4),
            use_cache: args.use_cache,
        }
    }
}

/// Main download command implementation
pub struct DownloadCommand {
    client: Client,
    config: DownloadConfig,
    regexes: Arc<Vec<Regex>>,
    cache: Option<CacheMetadata>,
}

impl DownloadCommand {
    pub fn new(config: DownloadConfig) -> Result<Self> {
        // Compile regex patterns
        let regexes = config
            .file_patterns
            .iter()
            .map(|pattern| {
                Regex::new(pattern).with_context(|| format!("Invalid regex pattern: {pattern}"))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            client: Client::builder()
                .user_agent(format!("{}/{}", build::PROJECT_NAME, build::PKG_VERSION))
                .build()
                .context("Failed to create HTTP client")?,
            regexes: Arc::new(regexes),
            cache: if config.use_cache {
                let mut cache = CacheMetadata::load(&config.output_path)?;
                // Handle invariant early
                if cache.slug.is_empty() {
                    cache.slug = config.slug.clone();
                }
                Some(cache)
            } else {
                None
            },
            config,
        })
    }

    fn regex_matches(regexes: &[Regex], path: &str) -> bool {
        if regexes.is_empty() {
            return true; // If no patterns specified, match all files
        }
        regexes.iter().any(|regex| regex.is_match(path))
    }

    pub async fn run(&mut self) -> Result<(GameVersion, bool)> {
        std::fs::create_dir_all(&self.config.output_path).with_context(|| {
            format!(
                "Failed to create output directory: {}",
                self.config.output_path.display()
            )
        })?;
        log::info!("Output Path: {}", self.config.output_path.display());
        if !self.config.file_patterns.is_empty() {
            log::info!("File Filter: {:?}", self.config.file_patterns);
        }

        let meta = get_repository_metadata(&self.client, &self.config.slug).await?;
        let latest_version = GameVersion::new(&meta.latest_version.version_string)?;
        log::info!("Repository:");
        log::info!("  Slug: {}", self.config.slug);
        log::info!("  Name: {}", meta.name);
        log::info!("  Description: {}", meta.description.unwrap_or_default());
        log::info!("  Latest Version: {latest_version}");

        let target_version = if let Some(ref version) = self.config.version {
            GameVersion::new(version)
                .with_context(|| format!("Invalid version specified: {version}"))?
        } else {
            latest_version
        };

        if let Some(cache) = &self.cache
            && cache.slug == self.config.slug
            && cache.version == target_version
        {
            log::info!("Version {target_version} is already downloaded. Skipping download.");
            return Ok((target_version, false));
        }

        log::info!("Downloading version {target_version}");

        let target_clut = self.download_clut(&target_version).await?;
        let source_clut = {
            if let Some(cache) = &self.cache
                && cache.version != GameVersion::epoch()
            {
                if cache.slug == self.config.slug {
                    log::info!("Using cached version {}", cache.version);

                    let filter_invalidated = cache
                        .filtered_files
                        .iter()
                        .any(|f| Self::regex_matches(&self.regexes, f));
                    if filter_invalidated {
                        log::warn!(
                            "Cache contains files not matching the specified patterns. Re-downloading."
                        );
                        None
                    } else {
                        let cached_clut = self.download_clut(&cache.version).await?;
                        Some(cached_clut)
                    }
                } else {
                    log::warn!(
                        "Cache slug mismatch: expected {}, found {}",
                        cache.slug,
                        self.config.slug
                    );
                    None
                }
            } else {
                None
            }
        };

        let mut diff = if let Some(source_clut) = source_clut {
            ClutDiff::new(&target_clut, &source_clut)
                .with_context(|| "Failed to create CLUT diff")?
        } else {
            ClutDiff::from(target_clut)
        };

        if let Some(patch) = meta.latest_version.patches.first() {
            let mut patch_url = patch.url.parse::<Url>()?;
            patch_url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("Failed to parse patch URL: {}", patch.url))?
                .pop();
            diff.provide_base_patch_url(&patch_url);
        }
        let diff_filtered_files =
            diff.filter_files(|path| Self::regex_matches(&self.regexes, path));

        let regexes = self.regexes.clone();
        let patcher = ClutPatcher::new(
            diff,
            FilteredFileOperations::new(
                PersistentFileOperations::new(&self.config.output_path),
                move |path| Self::regex_matches(&regexes, path),
            ),
            self.config.parallelism,
            self.config.queue_depth,
        )?;

        patcher.apply_diff().await?;

        if let Some(cache) = &mut self.cache {
            let patcher_filtered_files = patcher.operations().filtered_files().await;

            cache.slug = self.config.slug.clone();
            cache.version = target_version.clone();
            cache.filtered_files = patcher_filtered_files
                .into_iter()
                .chain(diff_filtered_files.into_iter())
                .collect();

            cache.store(&self.config.output_path)?;
            log::debug!("Cache metadata saved successfully.");
        }

        log::info!("Download completed successfully.");

        Ok((target_version, true))
    }

    async fn download_clut(&self, version: &GameVersion) -> Result<Clut> {
        let clut_url = format!(
            "{}/{}/{}.clut",
            self.config.clut_path, self.config.slug, version
        );
        let clut_bytes = self.client.get(&clut_url).send().await?.bytes().await?;
        Clut::read(Cursor::new(clut_bytes))
            .with_context(|| format!("Failed to read CLUT from {clut_url}"))
    }
}
