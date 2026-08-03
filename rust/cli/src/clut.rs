use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Args;
use reqwest::Client;
use xiv_core::file::clut::decompress;
use xiv_core::file::clut_build::ClutBuilder;
use xiv_core::file::clut_lazy::LazyClut;
use xiv_core::file::header::Header;
use xiv_core::file::lut::Lut;
use xiv_core::file::types::{CompressType, PlatformId, Version};
use xiv_core::file::version::GameVersion;
use xiv_core::thaliak::chain::{Patch, Step, get_patch_forest};

use crate::Compression;
use crate::lut::{LutArgs, resolve_chain};
use crate::resource::{Fetcher, join};

#[derive(Args, Debug, Clone)]
pub struct ClutArgs {
    /// Repository slug to build CLUTs for
    #[arg(short, long, value_name = "SLUG")]
    pub slug: String,
    /// Version to walk back from (default: latest)
    #[arg(short, long, value_name = "VERSION")]
    pub version: Option<String>,
    /// Build a CLUT for every version the repository offers, not only the chain to one
    #[arg(long, conflicts_with_all = ["version", "urls", "base_clut"])]
    pub all_versions: bool,
    /// Directory or URL prefix holding the LUT files
    #[arg(short, long, value_name = "PATH")]
    pub base_path: Option<String>,
    /// Base URL recorded in the CLUT for consumers to resolve patch URLs against
    #[arg(long, value_name = "URL")]
    pub base_patch_url: Option<String>,
    /// LUT URLs or paths to use instead of a Thaliak chain
    #[arg(long, value_name = "URL", num_args = 1..)]
    pub urls: Vec<String>,
    /// CLUT to start from, instead of an empty install
    #[arg(long, value_name = "URL")]
    pub base_clut: Option<String>,
    /// Directory to write CLUTs to (default: current directory)
    #[arg(short, long, value_name = "DIR")]
    pub output_path: Option<PathBuf>,
    /// Compression for the CLUT payload
    #[arg(short, long, value_name = "TYPE", default_value_t = Compression::Zstd)]
    pub compression: Compression,
    /// CLUT format version. 3 splits the payload into independently compressed chunks
    /// and adds an index, so a reader decodes one file without expanding the rest; 2 is
    /// a single stream every reader has to inflate whole.
    #[arg(long, value_name = "N", default_value_t = 3)]
    pub clut_version: u16,
    /// Rewrite CLUTs whose content is already on disk unchanged
    #[arg(long)]
    pub force: bool,
}

pub async fn run(args: ClutArgs, fetcher: Arc<Fetcher>, client: &Client) -> Result<()> {
    let format = match args.clut_version {
        2 => Version::SeparateVersioning,
        3 => Version::Indexed,
        other => bail!("Unsupported CLUT version {other}; expected 2 or 3"),
    };
    let compression: CompressType = args.compression.into();
    if compression == CompressType::Zstd && format != Version::Indexed {
        bail!("Zstd is only defined for CLUT version 3");
    }

    let output = args
        .output_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&output).with_context(|| format!("creating {}", output.display()))?;
    log::info!("Output Path: {}", output.display());

    let fold = Fold {
        fetcher: &fetcher,
        base_path: args.base_path.as_deref(),
        output: &output,
        format,
        compression,
        force: args.force,
    };

    let mut header = Header {
        platform: PlatformId::Win32,
        repository: args.slug.parse()?,
        ..Default::default()
    };
    if let Some(url) = &args.base_patch_url
        && !url.trim().is_empty()
    {
        header.base_patch_url = url.clone();
    }

    if args.all_versions {
        let steps = get_patch_forest(client, &args.slug).await?;
        return fold.walk(header, steps).await;
    }

    let chain = resolve_chain(
        client,
        &LutArgs {
            slug: args.slug.clone(),
            version: args.version.clone(),
            all_versions: false,
            max_patch_bytes: None,
            urls: args.urls.clone(),
            parallelism: None,
            output_path: None,
            compression: args.compression,
            force: false,
        },
    )
    .await?;

    let mut builder = match &args.base_clut {
        Some(location) => {
            let bytes = fetcher
                .bytes(location, &GameVersion::epoch().to_string(), "clut")
                .await?;
            let mut builder = ClutBuilder::from_clut(&LazyClut::read(Cursor::new(bytes))?)?;
            builder.header.repository = header.repository;
            if !header.base_patch_url.is_empty() {
                builder.header.base_patch_url = header.base_patch_url;
            }
            builder
        }
        None => ClutBuilder::new(header),
    };

    for (game_version, patch) in chain {
        fold.step(&mut builder, &game_version, &patch).await?;
    }

    Ok(())
}

/// Where the LUTs come from and how the CLUTs they fold into are packed.
struct Fold<'a> {
    fetcher: &'a Fetcher,
    base_path: Option<&'a str>,
    output: &'a Path,
    format: Version,
    compression: CompressType,
    force: bool,
}

impl Fold<'_> {
    /// Apply one version's LUT to `builder` and write the CLUT that results.
    async fn step(
        &self,
        builder: &mut ClutBuilder,
        game_version: &GameVersion,
        patch: &Patch,
    ) -> Result<()> {
        let patch_version = patch.version()?;
        // A chain from Thaliak names patch files; the LUTs derived from them sit
        // together under the base path.
        let location = if patch.url.ends_with(".patch") {
            join(self.base_path, &format!("{patch_version}.lut"))
        } else if patch.url.starts_with('/') {
            patch.url.clone()
        } else {
            join(self.base_path, &patch.url)
        };

        log::info!("Processing {game_version}");
        log::debug!("  URL: {location}");

        let bytes = self
            .fetcher
            .bytes(&location, &patch_version.to_string(), "lut")
            .await?;
        let lut =
            Lut::read(Cursor::new(bytes)).with_context(|| format!("reading LUT {location}"))?;

        builder.header.version = game_version.clone();
        builder.header.patch_version = patch_version.clone();
        for chunk in &lut.chunks {
            builder.apply(&patch_version, chunk)?;
        }

        log::debug!("Optimizing");
        let started = Instant::now();
        builder.remove_overlaps()?;
        log::debug!("Optimized in {:.2}s", started.elapsed().as_secs_f64());

        let name = format!("{game_version}.clut");
        let path = self.output.join(&name);
        let payload = builder.payload()?;

        if !self.force && holds(&path, &payload, self.format, self.compression) {
            log::info!("Skipping {name}");
            return Ok(());
        }

        log::debug!("Writing to {name}");
        let bytes = pack(builder, &payload, self.format, self.compression)?;
        std::fs::write(&path, &bytes)?;
        log::info!(
            "Finished {name} ({:.2} KiB)",
            bytes.len() as f64 / (1 << 10) as f64
        );
        Ok(())
    }

    /// The install a version's CLUT records, read back so another lineage can carry on
    /// from it.
    fn resume(&self, version: &GameVersion) -> Result<ClutBuilder> {
        let path = self.output.join(format!("{version}.clut"));
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        ClutBuilder::from_clut(&LazyClut::read(Cursor::new(bytes))?)
    }

    /// Fold every lineage, depth first. Where the graph forks, one branch carries the
    /// builder onward and the others resume from the CLUT already written for the fork,
    /// so memory holds one install rather than one per open branch. A version whose LUT
    /// is missing takes its successors down with it, leaving them to a later run.
    async fn walk(&self, header: Header, steps: Vec<Step>) -> Result<()> {
        let total = steps.len();
        let mut children: HashMap<GameVersion, Vec<Step>> = HashMap::new();
        let mut stack = Vec::new();
        for step in steps {
            match &step.parent {
                Some(parent) => children.entry(parent.clone()).or_default().push(step),
                None => stack.push(step),
            }
        }
        stack.reverse();

        let mut written = 0;
        let mut current: Option<(GameVersion, ClutBuilder)> = None;
        while let Some(step) = stack.pop() {
            let resumed = match &step.parent {
                None => Ok(ClutBuilder::new(header.clone())),
                Some(parent) => match current.take() {
                    Some((version, builder)) if version == *parent => Ok(builder),
                    _ => self.resume(parent),
                },
            };

            let outcome = match resumed {
                Ok(mut builder) => self
                    .step(&mut builder, &step.version, &step.patch)
                    .await
                    .map(|()| builder),
                Err(e) => Err(e),
            };

            match outcome {
                Ok(builder) => {
                    written += 1;
                    if let Some(kids) = children.remove(&step.version) {
                        stack.extend(kids.into_iter().rev());
                    }
                    current = Some((step.version, builder));
                }
                Err(e) => log::warn!("Skipping {} and its successors: {e:#}", step.version),
            }
        }

        log::info!("Wrote {written} of {total} versions");
        Ok(())
    }
}

/// Whether the CLUT already at `path` holds exactly this payload, packed this way.
fn holds(path: &Path, payload: &[u8], format: Version, compression: CompressType) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    match decompress(BufReader::new(file)) {
        Ok((header, existing)) => {
            header.file_version == format
                && header.compression == compression
                && existing == payload
        }
        // Unreadable or a format this build does not know: rewrite it.
        Err(_) => false,
    }
}

fn pack(
    builder: &ClutBuilder,
    payload: &[u8],
    format: Version,
    compression: CompressType,
) -> Result<Vec<u8>> {
    if format != Version::Indexed {
        return builder.wrap(payload, compression);
    }
    // The chunked writer works from a parsed CLUT, so the uncompressed v2 form is the
    // handover. Keeping one implementation of the chunk layout is worth the extra pass.
    let plain = builder.wrap(payload, CompressType::None)?;
    LazyClut::read(Cursor::new(plain))?.rewrite(compression)
}
