use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Args;
use futures::TryStreamExt;
use reqwest::Client;
use tokio_util::io::SyncIoBridge;
use xiv_core::file::lut::Lut;
use xiv_core::file::version::{GameVersion, PatchVersion};
use xiv_core::thaliak::chain::{Patch, get_patch_chain};
use xiv_core::thaliak::get_repository_metadata;
use xiv_core::zipatch::ZiPatch;

use crate::Compression;
use crate::resource::Fetcher;

#[derive(Args, Debug, Clone)]
pub struct LutArgs {
    /// Repository slug to build LUTs for
    #[arg(short, long, value_name = "SLUG")]
    pub slug: String,
    /// Version to walk back from (default: latest)
    #[arg(short, long, value_name = "VERSION")]
    pub version: Option<String>,
    /// Patch URLs or paths to use instead of a Thaliak chain
    #[arg(long, value_name = "URL", num_args = 1..)]
    pub urls: Vec<String>,
    /// Number of patches to process at once (default: number of CPU cores)
    #[arg(short, long, value_name = "NUM")]
    pub parallelism: Option<usize>,
    /// Directory to write LUTs to (default: current directory)
    #[arg(short, long, value_name = "DIR")]
    pub output_path: Option<PathBuf>,
    /// Compression for the LUT payload
    #[arg(short, long, value_name = "TYPE", default_value_t = Compression::Brotli)]
    pub compression: Compression,
    /// Rebuild LUTs that already exist
    #[arg(long)]
    pub force: bool,
}

pub async fn run(args: LutArgs, fetcher: Arc<Fetcher>, client: &Client) -> Result<()> {
    let output = args
        .output_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&output).with_context(|| format!("creating {}", output.display()))?;
    log::info!("Output Path: {}", output.display());

    let mut chain = resolve_chain(client, &args).await?;

    if !args.force {
        chain.retain(|(_, patch)| {
            let Ok(version) = patch.version() else {
                return true;
            };
            let exists = output.join(format!("{version}.lut")).exists();
            if exists {
                log::info!("Skipping patch {version}");
            }
            !exists
        });
    }

    let unknown = chain.iter().any(|(_, patch)| patch.size == 0);
    let total: i64 = chain.iter().map(|(_, patch)| patch.size).sum();
    log::info!(
        "Total Size: {}{:.2} GiB",
        if unknown { ">" } else { "" },
        total as f64 / (1 << 30) as f64
    );

    let parallelism = args.parallelism.unwrap_or_else(num_cpus::get);
    let compression = args.compression.into();
    let slug = &args.slug;
    let output = &output;

    futures::stream::iter(chain.into_iter().map(Ok))
        .try_for_each_concurrent(parallelism, |(_, patch)| {
            let fetcher = fetcher.clone();
            async move {
                let version = patch.version()?;
                log::info!("Downloading patch {version}");
                log::debug!("  URL: {}", patch.url);
                if patch.size != 0 {
                    log::debug!("  Size: {:.2} MiB", patch.size as f64 / (1 << 20) as f64);
                }

                let chunks = read_chunks(&fetcher, &patch, &version).await?;
                let lut = Lut {
                    compression,
                    repository: slug.clone(),
                    version: version.clone(),
                    chunks,
                };

                let name = format!("{version}.lut");
                log::debug!("Writing to {name}");
                let bytes = tokio::task::spawn_blocking(move || lut.write()).await??;
                tokio::fs::write(output.join(&name), &bytes).await?;
                log::info!(
                    "Finished {version} ({:.2} KiB)",
                    bytes.len() as f64 / (1 << 10) as f64
                );
                Ok::<_, anyhow::Error>(())
            }
        })
        .await
}

/// Parse a patch as it downloads.
async fn read_chunks(
    fetcher: &Fetcher,
    patch: &Patch,
    version: &PatchVersion,
) -> Result<Vec<xiv_core::zipatch::Chunk>> {
    let stream = fetcher
        .stream(&patch.url, &version.to_string(), "patch")
        .await?;
    tokio::task::spawn_blocking(move || {
        let mut patch = ZiPatch::new(BufReader::with_capacity(1 << 20, SyncIoBridge::new(stream)))?;
        let mut chunks = Vec::new();
        while let Some(chunk) = patch.next_chunk()? {
            chunks.push(chunk);
        }
        Ok(chunks)
    })
    .await?
}

/// The patches to process, either the explicit list or the chain leading to a version.
pub async fn resolve_chain(client: &Client, args: &LutArgs) -> Result<Vec<(GameVersion, Patch)>> {
    if !args.urls.is_empty() {
        return args
            .urls
            .iter()
            .map(|url| {
                let stem = url
                    .rsplit(['/', '\\'])
                    .next()
                    .and_then(|name| name.rsplit_once('.').map(|(stem, _)| stem))
                    .with_context(|| format!("no version in {url}"))?;
                let version = GameVersion::new(stem)?;
                log::warn!("Patch version {version} may not line up with the game version.");
                Ok((
                    version,
                    Patch {
                        url: url.clone(),
                        size: std::fs::metadata(url).map_or(0, |m| m.len() as i64),
                    },
                ))
            })
            .collect();
    }

    let meta = get_repository_metadata(client, &args.slug).await?;
    log::debug!("Repository:");
    log::debug!("  Slug: {}", args.slug);
    log::debug!("  Name: {}", meta.name);
    log::debug!("  Description: {}", meta.description.unwrap_or_default());
    log::debug!("  Latest Version: {}", meta.latest_version.version_string);

    let version = match &args.version {
        Some(version) if !version.is_empty() => GameVersion::new(version)?,
        _ => GameVersion::new(&meta.latest_version.version_string)?,
    };
    log::info!("Using version {version}");

    log::debug!("Downloading patch chain");
    get_patch_chain(client, &args.slug, &version).await
}
