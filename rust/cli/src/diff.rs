use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};
use xiv_core::file::{clut_lazy::LazyClut, data_ref::DataRef, slug::Slug, version::GameVersion};

/// What has to change on disk to turn one installed version into another.
pub struct ClutDiff {
    #[allow(dead_code)]
    pub repository: Slug,
    pub base_patch_url: String,
    #[allow(dead_code)]
    pub version_from: GameVersion,
    #[allow(dead_code)]
    pub version_to: GameVersion,

    pub removed_folders: HashSet<String>,
    pub removed_files: HashSet<String>,

    pub added_folders: HashSet<String>,
    pub added_files: HashMap<String, Vec<DataRef>>,
    pub file_sizes: HashMap<String, u64>,
    pub filtered_files: HashSet<String>,
}

impl ClutDiff {
    /// Diff `to` against `from`, or against an empty install when `from` is `None`.
    pub fn new(
        from: Option<&LazyClut>,
        to: &LazyClut,
        keep: &dyn Fn(&str) -> bool,
    ) -> Result<Self> {
        if let Some(from) = from {
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
        }

        let empty = HashSet::new();
        let from_folders = from.map_or(&empty, |from| &from.folders);
        let added_folders = to.folders.difference(from_folders).cloned().collect();
        let removed_folders = from_folders.difference(&to.folders).cloned().collect();

        let removed_files = from
            .into_iter()
            .flat_map(LazyClut::files)
            .filter(|path| !to.contains(path))
            .map(str::to_string)
            .collect();

        let mut added_files = HashMap::new();
        let mut file_sizes = HashMap::new();
        let mut filtered_files = HashSet::new();
        for path in to.files() {
            if !keep(path) {
                filtered_files.insert(path.to_string());
                continue;
            }
            if let Some(size) = to.file_size(path) {
                file_sizes.insert(path.to_string(), size);
            }
            let refs = to.file_refs(path)?;
            // A file the older install already has only needs what later patches wrote
            // to it; one it does not have is taken whole.
            let refs = match from {
                Some(from) if from.contains(path) => refs
                    .into_iter()
                    .filter(|d| *d.applied_version() > from.header.patch_version)
                    .collect(),
                _ => refs,
            };
            if !refs.is_empty() {
                added_files.insert(path.to_string(), refs);
            }
        }

        let base_patch_url = match from {
            Some(from) if !to.header.has_base_patch_url() => &from.header.base_patch_url,
            _ => &to.header.base_patch_url,
        };

        Ok(Self {
            repository: to.header.repository,
            base_patch_url: base_patch_url.trim_end_matches('/').to_string(),
            version_from: from.map_or_else(GameVersion::epoch, |f| f.header.version.clone()),
            version_to: to.header.version.clone(),
            removed_folders,
            removed_files,
            added_folders,
            added_files,
            file_sizes,
            filtered_files,
        })
    }

    pub fn provide_base_patch_url(&mut self, url: impl AsRef<str>) {
        let url = url.as_ref().trim_end_matches('/');
        if !url.is_empty() {
            self.base_patch_url = url.to_string();
        }
    }
}
