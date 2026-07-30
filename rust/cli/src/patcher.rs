use crate::{
    diff::ClutDiff,
    ops::{FileOperations, TargetFile, TargetFileExt},
};
use anyhow::Result;
use bytes::Bytes;
use either::Either;
use futures::{
    StreamExt, TryStreamExt,
    stream::{self, PollNext},
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use xiv_core::{
    downloader::Downloader,
    file::{data_ref::DataRef, patch_ref::PatchRef, version::PatchVersion},
};

/// Reference to a `DataRef` by its indices
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DataReference {
    pub file_name_index: usize,
    pub data_ref_index: usize,
}

/// Reference to a `PatchRef` by its indices
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PatchReference(DataReference);

/// The main CLUT patcher that handles downloading and applying patches
pub struct ClutPatcher<F: FileOperations> {
    downloader: Downloader,
    diff: ClutDiff,
    file_names: Vec<String>,
    ops: Arc<F>,
    max_concurrent_writes: usize,
}

impl<F: FileOperations> ClutPatcher<F> {
    /// Create a new CLUT patcher instance
    pub fn new(
        diff: ClutDiff,
        ops: F,
        max_concurrent_downloads: usize,
        max_concurrent_writes: usize,
    ) -> Result<Self> {
        Ok(Self {
            downloader: Downloader::new(max_concurrent_downloads)?,
            file_names: diff.added_files.keys().cloned().collect(),
            diff,
            ops: Arc::new(ops),
            max_concurrent_writes,
        })
    }

    pub fn operations(&self) -> Arc<F> {
        self.ops.clone()
    }

    pub async fn apply_diff(&self) -> Result<()> {
        for removed_file in &self.diff.removed_files {
            self.ops.delete_file(removed_file).await?;
        }

        for removed_dir in &self.diff.removed_folders {
            self.ops.delete_directory(removed_dir).await?;
        }

        for added_dir in &self.diff.added_folders {
            self.ops.create_directory(added_dir).await?;
        }

        self.process_data_refs().await?;

        // After the references are in place, cut each file to the length the target
        // version says it has; one that shrank still holds the old version's tail.
        for (path, size) in &self.diff.file_sizes {
            self.ops.open_file(path).await?.truncate(*size).await?;
        }

        Ok(())
    }

    async fn process_data_refs(&self) -> Result<()> {
        let all_refs = self.file_names.iter().enumerate().flat_map(|(idx, name)| {
            (0..self.diff.added_files[name].len()).map(move |data_ref_idx| DataReference {
                file_name_index: idx,
                data_ref_index: data_ref_idx,
            })
        });

        let plain_refs: Vec<_> = all_refs
            .clone()
            .filter(|data_ref| !self.get_data_ref(data_ref).is_patch())
            .collect();
        let plain_refs_len = plain_refs.len();

        let mut patch_refs_len = 0usize;
        let mut patch_refs = HashMap::new();
        for data_ref in all_refs
            .clone()
            .filter(|data_ref| self.get_data_ref(data_ref).is_patch())
        {
            let patch_ref = PatchReference(data_ref.clone());
            patch_refs
                .entry(self.get_patch_ref(&patch_ref))
                .or_insert_with(Vec::new)
                .push(patch_ref);
            patch_refs_len += 1;
        }

        let write_size = all_refs.fold(0usize, |acc, data_ref| {
            acc + self.get_data_ref(&data_ref).len() as usize
        });
        let download_size = patch_refs
            .keys()
            .fold(0usize, |acc, (_, patch_ref)| acc + patch_ref.size as usize);

        log::info!(
            "Total write size: {:.2} MiB",
            write_size as f64 / (1 << 20) as f64
        );
        log::info!(
            "Approx. download size: {:.2} MiB",
            download_size as f64 / (1 << 20) as f64
        );

        let plain_stream = stream::iter(plain_refs.into_iter().map(|r| Ok(Either::Left(r))));
        let download_stream = self
            .downloader
            .get_patch_data(&self.diff.base_patch_url, patch_refs.keys().copied())
            .map(|r| r.map(Either::Right));
        let full_stream =
            stream::select_with_strategy(plain_stream, download_stream, |()| PollNext::Right);

        let plain_refs_seen = AtomicUsize::new(0);
        let patch_refs_seen = AtomicUsize::new(0);

        full_stream
            .try_for_each_concurrent(Some(self.max_concurrent_writes), |operation| async {
                match operation {
                    Either::Left(data_ref) => {
                        plain_refs_seen.fetch_add(1, Ordering::Relaxed);
                        self.process_op_plain(&data_ref).await?;
                    }
                    Either::Right(((version, patch_ref), bytes)) => {
                        let refs =
                            patch_refs
                                .get(&(version.as_ref(), &patch_ref))
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "No patch references found for version {:?} and patch {:?}",
                                        version,
                                        patch_ref
                                    )
                                })?;
                        assert!(!refs.is_empty(), "Patch references should not be empty");
                        for patch_ref in refs {
                            patch_refs_seen.fetch_add(1, Ordering::Relaxed);
                            self.process_op_patch(patch_ref, &bytes).await?;
                        }
                    }
                }

                Ok(())
            })
            .await?;

        let plain_refs_seen = plain_refs_seen.load(Ordering::SeqCst);
        let patch_refs_seen = patch_refs_seen.load(Ordering::SeqCst);
        assert_eq!(
            plain_refs_seen, plain_refs_len,
            "Expected to process {plain_refs_len} plain references, but got {plain_refs_seen}",
        );
        assert_eq!(
            patch_refs_seen, patch_refs_len,
            "Expected to process {patch_refs_len} patch references, but got {patch_refs_seen}",
        );

        Ok(())
    }

    async fn process_op_plain(&self, data_ref: &DataReference) -> Result<()> {
        let file = self.ops.open_file(self.get_data_ref_name(data_ref)).await?;
        let data_ref = self.get_data_ref(data_ref);
        assert!(!data_ref.is_patch(), "Expected plain DataRef, got patch");
        if data_ref.is_zero() {
            file.wipe(data_ref.offset(), data_ref.len()).await?;
        } else if data_ref.is_empty_block() {
            file.write_empty_file_block(data_ref.offset(), data_ref.block_count().unwrap())
                .await?;
        } else {
            unreachable!()
        }
        Ok(())
    }

    async fn process_op_patch(&self, patch_ref: &PatchReference, bytes: &Bytes) -> Result<()> {
        let file = self
            .ops
            .open_file(self.get_data_ref_name(&patch_ref.0))
            .await?;
        let data_ref = self.get_data_ref(&patch_ref.0);
        assert!(data_ref.is_patch(), "Expected patch DataRef, got plain");
        let patch_offset = data_ref.patch_offset().unwrap() as usize;
        let data = bytes.slice(patch_offset..patch_offset + data_ref.len() as usize);
        file.write_at(&data, data_ref.offset()).await?;
        Ok(())
    }

    fn get_data_ref_name(&self, data_ref: &DataReference) -> &str {
        &self.file_names[data_ref.file_name_index]
    }

    /// Get data reference by indices
    fn get_data_ref(&self, data_ref: &DataReference) -> &DataRef {
        &self.diff.added_files[self.get_data_ref_name(data_ref)][data_ref.data_ref_index]
    }

    /// Get patch reference by indices
    fn get_patch_ref(&self, patch_ref: &PatchReference) -> (&PatchVersion, &PatchRef) {
        let data_ref = self.get_data_ref(&patch_ref.0);
        (data_ref.applied_version(), data_ref.patch().unwrap())
    }
}
