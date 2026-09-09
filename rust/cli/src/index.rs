use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use futures::stream::{self, StreamExt};
use reqwest::Client;
use reqwest::header::{HeaderValue, USER_AGENT};
use xiv_core::file::version::GameVersion;
use xiv_core::index::{Header, Origin, PatchType, Region, Repository, Source, Status, now};

const PATCHER_AGENT: &str = "FFXIV PATCH CLIENT";
const MAGIC: [u8; 12] = *b"\x91ZIPATCH\r\n\x1a\n";
const HEADER_BYTES: usize = 80;

#[derive(Args, Debug, Clone)]
pub struct IndexArgs {
    #[command(subcommand)]
    pub command: IndexCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum IndexCommand {
    /// Check committed repository files against the schema
    Check(CheckArgs),
    /// Check every recorded source and record whether it still downloads
    Liveness(LivenessArgs),
    /// Attach an archive.org item as a fallback source
    Archive(ArchiveArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ArchiveArgs {
    /// Directory holding repository files
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub path: PathBuf,
    /// archive.org item to read
    #[arg(long, value_name = "ITEM")]
    pub item: String,
    /// Directory inside the item to map onto a slug, in the form DIR=SLUG, or a bare SLUG for every file
    #[arg(long, value_name = "DIR=SLUG", num_args = 1..)]
    pub map: Vec<String>,
}

#[derive(Args, Debug, Clone)]
pub struct LivenessArgs {
    /// Directory holding repository files
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub path: PathBuf,
    /// Number of requests to keep in flight
    #[arg(long, value_name = "NUM", default_value_t = 8)]
    pub parallelism: usize,
}

#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    /// Directory holding repository files
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub path: PathBuf,
}

pub async fn run(args: IndexArgs, client: &Client) -> Result<()> {
    match args.command {
        IndexCommand::Check(args) => check(args),
        IndexCommand::Liveness(args) => liveness(args, client).await,
        IndexCommand::Archive(args) => archive(args, client).await,
    }
}

async fn archive(args: ArchiveArgs, client: &Client) -> Result<()> {
    let mut owners: BTreeMap<String, String> = BTreeMap::new();
    let mut fallback: Option<String> = None;
    for spec in &args.map {
        match spec.split_once('=') {
            Some((dir, slug)) => {
                owners.insert(dir.to_string(), slug.to_string());
            }
            None => fallback = Some(spec.clone()),
        }
    }

    let metadata: serde_json::Value = client
        .get(format!("https://archive.org/metadata/{}", args.item))
        .send()
        .await?
        .json()
        .await?;
    let files = metadata
        .get("files")
        .and_then(|files| files.as_array())
        .with_context(|| format!("{} lists no files", args.item))?;

    let mut wanted: BTreeMap<String, BTreeMap<GameVersion, String>> = BTreeMap::new();
    for file in files {
        let Some(name) = file.get("name").and_then(|name| name.as_str()) else {
            continue;
        };
        let Some(stem) = name.rsplit('/').next().and_then(|f| f.strip_suffix(".patch")) else {
            continue;
        };
        let Ok(version) = GameVersion::new(stem) else {
            continue;
        };
        let Some(slug) = name
            .split('/')
            .find_map(|part| owners.get(part))
            .or(fallback.as_ref())
        else {
            continue;
        };
        let url = format!(
            "https://archive.org/download/{}/{}",
            args.item,
            name.replace(' ', "%20")
        );
        wanted.entry(slug.clone()).or_default().insert(version, url);
    }

    let stamp = now();
    for path in repository_files(&args.path)? {
        let text = std::fs::read_to_string(&path)?;
        let mut repository: Repository = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        let Some(available) = wanted.get(&repository.slug) else {
            continue;
        };
        let mut added = 0;
        for (version, entry) in repository.patches.iter_mut() {
            let Some(url) = available.get(version) else {
                continue;
            };
            if entry.sources.contains_key(&Origin::ArchiveOrg) {
                continue;
            }
            entry.sources.insert(
                Origin::ArchiveOrg,
                Source {
                    url: url.clone(),
                    status: Status::Unchecked,
                    checked: stamp,
                },
            );
            added += 1;
        }
        if added == 0 {
            continue;
        }
        repository.validate()?;
        let mut text = serde_json::to_string_pretty(&repository)?;
        text.push('\n');
        std::fs::write(&path, text)?;
        log::info!("{}: {added} archive sources added", repository.slug);
    }
    Ok(())
}

async fn liveness(args: LivenessArgs, client: &Client) -> Result<()> {
    for path in repository_files(&args.path)? {
        let text = std::fs::read_to_string(&path)?;
        let mut repository: Repository = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", path.display()))?;

        let urls: Vec<String> = repository
            .patches
            .values()
            .flat_map(|entry| entry.sources.values().map(|source| source.url.clone()))
            .collect();
        let alive: BTreeMap<String, bool> = stream::iter(urls.into_iter().map(|url| async move {
            let ok = client
                .head(&url)
                .header(USER_AGENT, HeaderValue::from_static(PATCHER_AGENT))
                .send()
                .await
                .map(|response| response.status().is_success())
                .unwrap_or(false);
            (url, ok)
        }))
        .buffer_unordered(args.parallelism)
        .collect()
        .await;

        let stamp = now();
        let mut changed = 0;
        for entry in repository.patches.values_mut() {
            for source in entry.sources.values_mut() {
                let status = match alive.get(&source.url) {
                    Some(true) => Status::Alive,
                    Some(false) => Status::Dead,
                    None => continue,
                };
                if source.status == status {
                    continue;
                }
                source.status = status;
                source.checked = stamp;
                changed += 1;
            }
        }
        repository.validate()?;
        if changed > 0 {
            let mut text = serde_json::to_string_pretty(&repository)?;
            text.push('\n');
            std::fs::write(&path, text)?;
        }
        let dead = repository
            .patches
            .iter()
            .filter(|(_, entry)| entry.sources.values().all(|s| s.status == Status::Dead))
            .count();
        log::info!(
            "{}: {changed} changed, {dead} of {} unreachable",
            repository.slug,
            repository.patches.len()
        );
    }
    Ok(())
}

pub fn parse_header(body: &[u8]) -> Option<Header> {
    if body.len() < HEADER_BYTES || body[..MAGIC.len()] != MAGIC {
        return None;
    }
    let be = |offset: usize| {
        u32::from_be_bytes([
            body[offset],
            body[offset + 1],
            body[offset + 2],
            body[offset + 3],
        ])
    };
    let patch_type = match &body[24..28] {
        b"DIFF" => PatchType::Diff,
        b"HIST" => PatchType::Hist,
        _ => return None,
    };
    let version = (u32::from_le_bytes([body[20], body[21], body[22], body[23]]) >> 16) as u8;
    let counted = version == 3;
    Some(Header {
        version,
        patch_type,
        add_commands: counted.then(|| be(60)),
        file_commands: counted.then(|| be(76)),
    })
}

fn check(args: CheckArgs) -> Result<()> {
    let mut checked = 0;
    for path in repository_files(&args.path)? {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let repository: Repository = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        repository
            .validate()
            .with_context(|| format!("validating {}", path.display()))?;
        checked += 1;
    }
    log::info!("{checked} repositories valid");
    Ok(())
}

pub fn repository_files(root: &std::path::Path) -> Result<Vec<PathBuf>> {
    let path = if root.join("repos").is_dir() {
        root.join("repos")
    } else {
        root.to_path_buf()
    };
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&path).with_context(|| format!("reading {}", path.display()))? {
        let path = entry?.path();
        if is_repository_file(&path) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub fn repos_dir(root: &std::path::Path) -> Result<PathBuf> {
    let dir = root.join("repos");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

pub fn is_repository_file(path: &std::path::Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.len() == 8 && stem.chars().all(|c| c.is_ascii_hexdigit()))
        && path.extension().is_some_and(|ext| ext == "json")
}

pub fn parse_region(region: &str) -> Result<Region> {
    Ok(match region {
        "global" => Region::Global,
        "korea" => Region::Korea,
        "china" => Region::China,
        "taiwan" => Region::Taiwan,
        other => anyhow::bail!("unknown region {other}"),
    })
}

pub fn parse_slug(spec: &str) -> Result<(String, Region)> {
    let (slug, region) = spec
        .split_once('=')
        .with_context(|| format!("expected slug=region, got {spec}"))?;
    Ok((slug.to_string(), parse_region(region)?))
}
