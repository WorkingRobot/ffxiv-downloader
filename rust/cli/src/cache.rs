use std::{collections::HashSet, path::Path};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use xiv_core::file::version::GameVersion;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetadata {
    pub slug: String,
    pub version: GameVersion,
    pub filtered_files: HashSet<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CacheMetadataOld {
    pub installed_versions: Vec<String>,
    pub filtered_files: HashSet<String>,
}

impl CacheMetadata {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().join(".cachemeta.json");
        if path.exists() {
            let data = std::fs::read_to_string(&path)?;
            let meta = if let Ok(meta) = serde_json::from_str(&data) {
                meta
            } else {
                let old_meta: CacheMetadataOld = serde_json::from_str(&data)?;
                Self {
                    slug: String::new(),
                    version: old_meta
                        .installed_versions
                        .into_iter()
                        .filter_map(|v| GameVersion::new(&v).ok())
                        .max()
                        .unwrap_or_else(GameVersion::epoch),
                    filtered_files: old_meta.filtered_files,
                }
            };
            Ok(meta)
        } else {
            Ok(Self {
                slug: String::new(),
                version: GameVersion::epoch(),
                filtered_files: HashSet::new(),
            })
        }
    }

    pub fn store(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref().join(".cachemeta.json");
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }
}
