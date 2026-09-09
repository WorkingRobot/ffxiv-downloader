use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::file::version::GameVersion;
use crate::patch::{Patch, Step};

pub const SCHEMA_BASE: &str =
    "https://raw.githubusercontent.com/WorkingRobot/ffxiv-patches/main/schema";
pub const REPOSITORY_SCHEMA: &str =
    "https://raw.githubusercontent.com/WorkingRobot/ffxiv-patches/main/schema/repository.schema.json";
pub const REGISTRY_SCHEMA: &str =
    "https://raw.githubusercontent.com/WorkingRobot/ffxiv-patches/main/schema/repositories.schema.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    Global,
    Korea,
    China,
    Taiwan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatchType {
    #[serde(rename = "DIFF")]
    Diff,
    #[serde(rename = "HIST")]
    Hist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Cdn,
    ArchiveOrg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Alive,
    Dead,
    Unchecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMethod {
    VersionCheck,
    CdnSweep,
    MaintenanceScrape,
    ThaliakSeed,
    ArchiveIndex,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyMethod {
    ChainCoverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub version: u8,
    pub patch_type: PatchType,
    pub add_commands: Option<u32>,
    pub file_commands: Option<u32>,
}

impl Header {
    pub fn is_install(&self) -> bool {
        matches!((self.add_commands, self.file_commands), (Some(0), Some(files)) if files > 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub url: String,
    pub status: Status,
    pub checked: Timestamp,
}

pub fn now() -> Timestamp {
    Timestamp::now()
        .round(jiff::Unit::Second)
        .unwrap_or_else(|_| Timestamp::now())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovered {
    pub at: Timestamp,
    pub method: DiscoveryMethod,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verified {
    pub at: Timestamp,
    pub method: VerifyMethod,
    pub unsupplied: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchEntry {
    pub prev: Option<GameVersion>,
    pub size: i64,
    pub header: Option<Header>,
    pub sources: BTreeMap<Origin, Source>,
    pub discovered: Discovered,
    pub verified: Option<Verified>,
}

impl PatchEntry {
    pub fn preferred(&self) -> Option<&Source> {
        self.sources
            .values()
            .find(|source| source.status != Status::Dead)
            .or_else(|| self.sources.values().next())
    }

    pub fn patch(&self, version: &GameVersion) -> Result<Patch> {
        let source = self
            .preferred()
            .with_context(|| format!("version {version} lists no source"))?;
        Ok(Patch {
            url: source.url.clone(),
            size: self.size,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub slug: String,
    pub name: String,
    pub region: Region,
    pub latest: GameVersion,
    pub patches: BTreeMap<GameVersion, PatchEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRef {
    pub slug: String,
    pub name: String,
    pub region: Region,
    pub latest: GameVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub repositories: Vec<RepositoryRef>,
}

impl Repository {
    pub fn chain(&self, version: &GameVersion) -> Result<Vec<(GameVersion, Patch)>> {
        let mut chain = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut current = Some(version.clone());
        while let Some(version) = current {
            ensure!(
                seen.insert(version.clone()),
                "{} version {version} is its own ancestor",
                self.slug
            );
            let entry = self
                .patches
                .get(&version)
                .with_context(|| format!("{} has no version {version}", self.slug))?;
            chain.push((version.clone(), entry.patch(&version)?));
            current = entry.prev.clone();
        }
        chain.reverse();
        Ok(chain)
    }

    pub fn forest(&self) -> Result<Vec<Step>> {
        self.patches
            .iter()
            .map(|(version, entry)| {
                Ok(Step {
                    version: version.clone(),
                    patch: entry.patch(version)?,
                    parent: entry.prev.clone(),
                })
            })
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == REPOSITORY_SCHEMA,
            "{} declares schema {}, expected {REPOSITORY_SCHEMA}",
            self.slug,
            self.schema
        );

        for (version, entry) in &self.patches {
            if let Some(prev) = &entry.prev {
                ensure!(
                    self.patches.contains_key(prev),
                    "{} version {version} names missing predecessor {prev}",
                    self.slug
                );
                ensure!(
                    prev < version,
                    "{} version {version} names later predecessor {prev}",
                    self.slug
                );
            }
            ensure!(
                !entry.sources.is_empty(),
                "{} version {version} lists no source",
                self.slug
            );
        }

        ensure!(
            self.patches.contains_key(&self.latest),
            "{} names latest {} which it does not list",
            self.slug,
            self.latest
        );

        Ok(())
    }
}
