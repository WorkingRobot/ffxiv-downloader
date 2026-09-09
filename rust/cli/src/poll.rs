use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use reqwest::Client;
use reqwest::header::{HeaderValue, USER_AGENT};
use xiv_core::file::version::GameVersion;
use xiv_core::index::{
    Discovered, DiscoveryMethod, Origin, PatchEntry, Region, Repository, REPOSITORY_SCHEMA, Source, Status, now,
};

const AGENT: &str = "FFXIV_Patch";
pub const SENTINEL: &str = "2012.01.01.0000.0000";

#[derive(Args, Debug, Clone)]
pub struct PollArgs {
    /// Region to poll
    #[arg(short, long, value_name = "REGION")]
    pub region: String,
    /// Directory holding repository files
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub index_path: PathBuf,
    /// Repositories to create if absent, in the form slug=name
    #[arg(long, value_name = "SLUG=NAME", num_args = 0..)]
    pub repo: Vec<String>,
    /// Ask from the empty install rather than each repository's latest
    #[arg(long)]
    pub full: bool,
}

pub async fn run(args: PollArgs, client: &Client) -> Result<()> {
    let region = crate::index::parse_region(&args.region)?;
    let mut repositories = load_region(&args.index_path, region, &args.repo)?;
    if repositories.is_empty() {
        bail!("no repositories for {}", args.region);
    }

    let wanted = if region == Region::Global { "/boot" } else { "/game" };
    let game = repositories
        .iter()
        .position(|repository| repository.name.ends_with(wanted))
        .with_context(|| format!("{} has no {wanted} repository", args.region))?;

    let from = version_of(&repositories[game], args.full);
    let body = report(&repositories, game, args.full);
    let url = endpoint(region, &repositories[game].name, &from)?;

    let response = client
        .post(&url)
        .header(USER_AGENT, HeaderValue::from_static(AGENT))
        .header("X-Hash-Check", HeaderValue::from_static("enabled"))
        .body(body)
        .send()
        .await
        .with_context(|| format!("polling {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("{url} answered {status}");
    }
    let listed = parse_list(&response.text().await?);

    let stamp = now();
    let mut previous: BTreeMap<String, Option<GameVersion>> = repositories
        .iter()
        .map(|repository| {
            let last = if args.full {
                None
            } else {
                repository.patches.keys().next_back().cloned()
            };
            (repository.slug.clone(), last)
        })
        .collect();

    let mut added: BTreeMap<String, usize> = BTreeMap::new();
    for (version, size, patch_url) in listed {
        let Some(repository) = repositories
            .iter_mut()
            .find(|repository| patch_url.contains(&repository.slug))
        else {
            log::warn!("no repository owns {patch_url}");
            continue;
        };
        let version = GameVersion::new(&version)?;
        let prev = previous.get(&repository.slug).cloned().flatten();
        previous.insert(repository.slug.clone(), Some(version.clone()));

        if repository.patches.contains_key(&version) {
            continue;
        }
        repository.patches.insert(
            version,
            PatchEntry {
                prev,
                size,
                header: None,
                sources: BTreeMap::from([(
                    Origin::Cdn,
                    Source {
                        url: patch_url,
                        status: Status::Alive,
                        checked: stamp,
                    },
                )]),
                discovered: Discovered {
                    at: stamp,
                    method: DiscoveryMethod::VersionCheck,
                },
                verified: None,
            },
        );
        *added.entry(repository.slug.clone()).or_default() += 1;
    }

    for mut repository in repositories {
        if repository.patches.is_empty() {
            continue;
        }
        if let Some(last) = repository.patches.keys().next_back() {
            repository.latest = last.clone();
        }
        repository.validate()?;
        let path = crate::index::repos_dir(&args.index_path)?.join(format!("{}.json", repository.slug));
        let mut text = serde_json::to_string_pretty(&repository)?;
        text.push('\n');
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        log::info!(
            "{}: {} new, {} total, latest {}",
            repository.slug,
            added.get(&repository.slug).copied().unwrap_or(0),
            repository.patches.len(),
            repository.latest
        );
    }
    Ok(())
}

fn version_of(repository: &Repository, full: bool) -> GameVersion {
    if full {
        return GameVersion::new(SENTINEL).expect("sentinel parses");
    }
    repository
        .patches
        .keys()
        .next_back()
        .cloned()
        .unwrap_or_else(|| GameVersion::new(SENTINEL).expect("sentinel parses"))
}

fn report(repositories: &[Repository], game: usize, full: bool) -> String {
    let mut body = format!("{SENTINEL}=00000000000000000000000000000000");
    for (index, repository) in repositories.iter().enumerate() {
        if index == game {
            continue;
        }
        let Some(expansion) = repository.name.rsplit('/').next() else {
            continue;
        };
        body.push_str(&format!("\n{expansion}\t{}", version_of(repository, full)));
    }
    body
}

fn parse_list(body: &str) -> Vec<(String, i64, String)> {
    body.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 6 {
                return None;
            }
            let size = fields[0].parse().ok()?;
            let version = fields[4].to_string();
            let url = fields[fields.len() - 1].trim().to_string();
            if !url.starts_with("http") {
                return None;
            }
            Some((version, size, url))
        })
        .collect()
}

fn endpoint(region: Region, name: &str, from: &GameVersion) -> Result<String> {
    let parts: Vec<&str> = name.split('/').collect();
    let [product, platform, channel, repo] = parts.as_slice() else {
        bail!("repository name {name} is not product/platform/channel/repo");
    };
    let host = match (region, *repo) {
        (Region::Global, "boot") => "patch-bootver.ffxiv.com",
        (Region::Global, _) => bail!("the global game repository requires an authenticated session"),
        (Region::Korea, _) => "ngamever-live.ff14.co.kr",
        (Region::China, _) => "ffxivpatch01.ff14.sdo.com",
        (Region::Taiwan, _) => "patch-gamever.ffxiv.com.tw",
    };
    Ok(format!(
        "http://{host}/http/{platform}/{product}_{channel}_{repo}/{from}/"
    ))
}

fn load_region(path: &Path, region: Region, extra: &[String]) -> Result<Vec<Repository>> {
    let mut repositories: Vec<Repository> = Vec::new();
    if path.exists() {
        for file in crate::index::repository_files(path)? {
            let text = std::fs::read_to_string(&file)?;
            let repository: Repository = serde_json::from_str(&text)
                .with_context(|| format!("parsing {}", file.display()))?;
            if repository.region == region {
                repositories.push(repository);
            }
        }
    }
    for spec in extra {
        let (slug, name) = spec
            .split_once('=')
            .with_context(|| format!("expected slug=name, got {spec}"))?;
        if repositories.iter().any(|r| r.slug == slug) {
            continue;
        }
        repositories.push(Repository {
            schema: REPOSITORY_SCHEMA.to_string(),
            slug: slug.to_string(),
            name: name.to_string(),
            region,
            latest: GameVersion::new(SENTINEL)?,
            patches: BTreeMap::new(),
        });
    }
    repositories.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(repositories)
}
