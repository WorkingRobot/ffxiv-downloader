use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};

use crate::file::{clut::Clut, data_ref::DataRef, header::Header, version::GameVersion};

pub struct ClutDiff {
    pub repository: String,
    pub base_patch_url: String,
    pub version_from: GameVersion,
    pub version_to: GameVersion,

    pub removed_folders: HashSet<String>,
    pub removed_files: HashSet<String>,

    pub added_folders: HashSet<String>,
    pub added_files: HashMap<String, Vec<DataRef>>,
}

impl ClutDiff {
    pub fn new(from: &Clut, to: &Clut) -> Result<Self> {
        if from.header.repository != to.header.repository {
            bail!(
                "Cannot diff CLUTs from different repositories: {} vs {}",
                from.header.repository,
                to.header.repository
            );
        }
        if from.header.version >= to.header.version {
            bail!(
                "Cannot diff CLUTs with from version {} >= to version {}",
                from.header.version,
                to.header.version
            );
        }

        let removed_folders = to.folders.difference(&from.folders).cloned().collect();
        let removed_files = to
            .files
            .keys()
            .collect::<HashSet<_>>()
            .difference(&from.files.keys().collect::<HashSet<_>>())
            .map(|&s| s.clone())
            .collect();
        let added_folders = from.folders.difference(&to.folders).cloned().collect();

        let mut added_files = HashMap::new();
        for (path, data_refs) in &to.files {
            // If a brand new file is added, add it whole.
            if !from.files.contains_key(path) {
                added_files.insert(path.clone(), data_refs.data.clone());
            }

            let new_data: Vec<DataRef> = data_refs
                .data
                .iter()
                .filter(|d| *d.applied_version() > from.header.patch_version)
                .cloned()
                .collect();
            if new_data.len() > 0 {
                added_files.insert(path.clone(), new_data);
            }
        }

        Ok(Self {
            repository: from.header.repository.clone(),
            base_patch_url: if to.header.has_base_patch_url() {
                &to.header.base_patch_url
            } else {
                &from.header.base_patch_url
            }
            .trim_end_matches('/')
            .to_string(),
            version_from: from.header.version.clone(),
            version_to: to.header.version.clone(),
            removed_folders,
            removed_files,
            added_folders,
            added_files,
        })
    }

    pub fn provide_base_patch_url(&mut self, url: impl AsRef<str>) {
        let url = url.as_ref().trim_end_matches('/');
        if !url.is_empty() {
            self.base_patch_url = url.to_string();
        }
    }

    pub fn filter_files(&mut self, filter: impl Fn(&str) -> bool) -> HashSet<String> {
        let mut filtered_out = HashSet::new();

        let mut filter = |path: &String| {
            if filter(path) {
                true
            } else {
                filtered_out.insert(path.clone());
                false
            }
        };

        self.added_files.retain(|path, _| filter(path));
        self.removed_files.retain(|path| filter(path));

        filtered_out
    }
}

impl From<Clut> for ClutDiff {
    fn from(value: Clut) -> Self {
        let from = Clut {
            header: Header {
                repository: value.header.repository.clone(),
                version: GameVersion::epoch(),
                ..Default::default()
            },
            ..Default::default()
        };
        Self::new(&from, &value).unwrap()
    }
}
