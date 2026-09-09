use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use futures::stream::{self, StreamExt};
use jiff::civil::Date;
use reqwest::Client;
use reqwest::header::{HeaderValue, USER_AGENT};
use xiv_core::file::version::GameVersion;
use std::collections::BTreeMap;

use xiv_core::index::{
    Discovered, DiscoveryMethod, Header, Origin, PatchEntry, Repository, REPOSITORY_SCHEMA, Source, Status, now,
};

const AGENT: &str = "FFXIV PATCH CLIENT";
const REVISIONS: u32 = 3;

#[derive(Args, Debug, Clone)]
pub struct DiscoverArgs {
    /// Repository slug to sweep, in the form slug=region
    #[arg(short, long, value_name = "SLUG=REGION")]
    pub slug: String,
    /// Directory holding repository files
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub index_path: PathBuf,
    /// Repository name, required when the repository file does not exist yet
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// Patch url directory, required when the repository file does not exist yet
    #[arg(long, value_name = "URL")]
    pub base: Option<String>,
    /// First date to probe
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub from: Date,
    /// Last date to probe
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub to: Date,
    /// Highest part field to probe
    #[arg(long, value_name = "NUM", default_value_t = 2)]
    pub parts: u32,
    /// Number of requests to keep in flight
    #[arg(short, long, value_name = "NUM", default_value_t = 8)]
    pub parallelism: usize,
    /// Stop after this many requests
    #[arg(long, value_name = "NUM", default_value_t = 100_000)]
    pub budget: usize,
}

pub async fn run(args: DiscoverArgs, client: &Client) -> Result<()> {
    let (slug, region) = crate::index::parse_slug(&args.slug)?;
    let path = crate::index::repos_dir(&args.index_path)?.join(format!("{slug}.json"));
    let mut repository = match load(&path)? {
        Some(repository) => repository,
        None => Repository {
            schema: REPOSITORY_SCHEMA.to_string(),
            slug: slug.clone(),
            name: args
                .name
                .clone()
                .with_context(|| format!("{slug} has no repository file, so --name is required"))?,
            region,
            latest: GameVersion::new("2012.01.01.0000.0000")?,
            patches: BTreeMap::new(),
        },
    };

    let base = match &args.base {
        Some(base) => base.trim_end_matches('/').to_string(),
        None => repository
            .patches
            .values()
            .next()
            .and_then(|entry| entry.sources.values().next())
            .and_then(|source| source.url.rsplit_once('/').map(|(head, _)| head.to_string()))
            .with_context(|| format!("{slug} has no source to take a url prefix from"))?,
    };

    let mut names = Vec::new();
    let mut day = args.from;
    while day <= args.to {
        let stamp = format!("{:04}.{:02}.{:02}", day.year(), day.month(), day.day());
        for part in 0..=args.parts {
            for revision in 0..REVISIONS {
                names.push(format!("D{stamp}.{part:04}.{revision:04}"));
                names.push(format!("H{stamp}.{part:04}.{revision:04}a"));
            }
        }
        day = day.tomorrow()?;
    }
    names.truncate(args.budget);

    log::info!("{slug}: probing {} names", names.len());
    let found: Vec<(String, i64, Option<Header>)> = stream::iter(names.into_iter().map(|name| {
        let url = format!("{base}/{name}.patch");
        async move {
            probe(client, &url)
                .await
                .map(|(size, header)| (name, size, header))
        }
    }))
    .buffer_unordered(args.parallelism)
    .filter_map(|found| async move { found })
    .collect()
    .await;

    let mut found = found;
    let mut queue: Vec<String> = found
        .iter()
        .filter_map(|(name, _, _)| next_letter(name))
        .collect();
    while !queue.is_empty() {
        let batch: Vec<(String, i64, Option<Header>)> =
            stream::iter(queue.drain(..).map(|name| {
                let url = format!("{base}/{name}.patch");
                async move {
                    probe(client, &url)
                        .await
                        .map(|(size, header)| (name, size, header))
                }
            }))
            .buffer_unordered(args.parallelism)
            .filter_map(|found| async move { found })
            .collect()
            .await;
        queue.extend(batch.iter().filter_map(|(name, _, _)| next_letter(name)));
        found.extend(batch);
    }

    let stamp = now();
    let mut added = 0;
    for (name, size, header) in found {
        let version = GameVersion::new(&name)?;
        if repository.patches.contains_key(&version) {
            continue;
        }
        repository.patches.insert(
            version,
            PatchEntry {
                prev: None,
                size,
                header,
                sources: BTreeMap::from([(
                    Origin::Cdn,
                    Source {
                        url: format!("{base}/{name}.patch"),
                        status: Status::Alive,
                        checked: stamp,
                    },
                )]),
                discovered: Discovered {
                    at: stamp,
                    method: DiscoveryMethod::CdnSweep,
                },
                verified: None,
            },
        );
        added += 1;
    }

    relink(&mut repository);
    if let Some(last) = repository.patches.keys().next_back() {
        repository.latest = last.clone();
    }
    repository.validate()?;

    let mut text = serde_json::to_string_pretty(&repository)?;
    text.push('\n');
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    log::info!(
        "{slug}: {added} new, {} total, latest {}",
        repository.patches.len(),
        repository.latest
    );
    Ok(())
}

fn relink(repository: &mut Repository) {
    let mut previous: Option<GameVersion> = None;
    for (version, entry) in repository.patches.iter_mut() {
        if entry.discovered.method == DiscoveryMethod::CdnSweep {
            let install = entry.header.as_ref().is_some_and(|header| header.is_install());
            entry.prev = if install && previous.is_none() {
                None
            } else {
                previous.clone()
            };
        }
        previous = Some(version.clone());
    }
}

fn next_letter(name: &str) -> Option<String> {
    let stem = name.strip_prefix('H')?;
    let split = stem.rfind(|c: char| c.is_ascii_digit())? + 1;
    let (version, section) = stem.split_at(split);
    let mut letters: Vec<u8> = section.bytes().collect();
    if letters.is_empty() {
        return None;
    }
    let mut index = letters.len();
    loop {
        if index == 0 {
            letters.insert(0, b'a');
            break;
        }
        index -= 1;
        if letters[index] == b'z' {
            letters[index] = b'a';
        } else {
            letters[index] += 1;
            break;
        }
    }
    Some(format!("H{version}{}", String::from_utf8(letters).ok()?))
}

async fn probe(client: &Client, url: &str) -> Option<(i64, Option<Header>)> {
    let response = client
        .get(url)
        .header(USER_AGENT, HeaderValue::from_static(AGENT))
        .header(reqwest::header::RANGE, HeaderValue::from_static("bytes=0-79"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let size = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit('/').next().and_then(|len| len.parse().ok()))
        .unwrap_or(0);
    let body = response.bytes().await.ok()?;
    Some((size, crate::index::parse_header(&body)))
}

fn load(path: &Path) -> Result<Option<Repository>> {
    if !path.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let repository =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(repository))
}
