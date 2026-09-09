use anyhow::{Result, ensure};

use crate::file::version::{GameVersion, PatchVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub url: String,
    pub size: i64,
}

impl Patch {
    pub fn version(&self) -> Result<PatchVersion> {
        let name = self
            .url
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        ensure!(!stem.is_empty(), "no version in patch url {}", self.url);
        PatchVersion::new(stem)
    }
}

/// A version's place in the graph: the patch that reaches it, and the version that
/// patch is applied on top of.
#[derive(Debug, Clone)]
pub struct Step {
    pub version: GameVersion,
    pub patch: Patch,
    pub parent: Option<GameVersion>,
}
