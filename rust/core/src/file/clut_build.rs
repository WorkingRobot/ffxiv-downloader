use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use anyhow::{Context, Result, ensure};
use binrw::{BinWrite, Endian};

use super::clut::{Clut, compress_chunk};
use super::data_ref::DataRef;
use super::file_data::{FileData, PatchIndex};
use super::header::Header;
use super::types::{CompressType, Version};
use super::utils::NetString;
use super::version::PatchVersion;
use crate::zipatch::chunk::{
    Chunk, HEADER_SIZE, expansion_folder, normalize_path, resolve_platform,
};

/// Expansion data lives under these two trees, so removing an expansion means
/// clearing both.
const EXPANSION_ROOTS: [&str; 2] = ["sqpack", "movie"];

/// Files kept even when their expansion is removed.
const KEEP_ON_REMOVE: [&str; 5] = [".var", "00000.bk2", "00001.bk2", "00002.bk2", "00003.bk2"];

/// A CLUT under construction: the state of an install after some prefix of a patch
/// chain has been applied.
#[derive(Debug, Clone)]
pub struct ClutBuilder {
    pub header: Header,
    pub folders: BTreeSet<String>,
    pub files: BTreeMap<String, Vec<DataRef>>,
    /// Files whose references have changed since they were last collapsed.
    dirty: BTreeSet<String>,
}

impl ClutBuilder {
    pub fn new(header: Header) -> Self {
        Self {
            header,
            folders: BTreeSet::new(),
            files: BTreeMap::new(),
            dirty: BTreeSet::new(),
        }
    }

    /// Resume from an already-built CLUT. Its files start dirty: nothing here can
    /// prove the references in an arbitrary CLUT are already disjoint.
    pub fn from_clut(clut: &Clut) -> Self {
        Self {
            header: clut.header.clone(),
            folders: clut.folders.iter().cloned().collect(),
            files: clut
                .files
                .iter()
                .map(|(path, refs)| (path.clone(), refs.as_ref().clone()))
                .collect(),
            dirty: clut.files.keys().cloned().collect(),
        }
    }

    /// Fold one patch's chunk into the install. Chunks that describe no file content
    /// (headers, options, index commands) are ignored, as the patcher ignores them.
    pub fn apply(&mut self, patch: &PatchVersion, chunk: &Chunk) -> Result<()> {
        let platform = self.header.platform;
        let resolve = |target: &str| resolve_platform(target, platform);

        match chunk {
            Chunk::AddDirectory(dir) => {
                self.folders.insert(normalize_path(dir));
            }
            Chunk::DeleteDirectory(dir) => {
                self.folders.remove(&normalize_path(dir));
            }
            Chunk::SqpkFileMkdir { target } => {
                self.folders.insert(resolve(target));
            }
            Chunk::SqpkFileDelete { target } => {
                self.files.remove(&resolve(target));
            }

            Chunk::SqpkHeader {
                target,
                header_kind,
                patch_offset,
            } => {
                let offset = if Chunk::is_version_header(*header_kind) {
                    0
                } else {
                    HEADER_SIZE as u64
                };
                self.file(resolve(target))
                    .push(DataRef::from_raw_patch_data(
                        patch.clone(),
                        *patch_offset as u64,
                        offset,
                        HEADER_SIZE as u32,
                    ));
            }

            Chunk::SqpkAddData {
                target,
                block_offset,
                block_number,
                block_delete_number,
                patch_offset,
            } => {
                let refs = self.file(resolve(target));
                if *block_number > 0 {
                    refs.push(DataRef::from_raw_patch_data(
                        patch.clone(),
                        *patch_offset as u64,
                        *block_offset as u64,
                        u32::try_from(*block_number)?,
                    ));
                }
                if *block_delete_number > 0 {
                    refs.push(DataRef::from_zeros(
                        patch.clone(),
                        (*block_offset + *block_number) as u64,
                        u32::try_from(*block_delete_number)?,
                    ));
                }
            }

            // Both commands leave the region as a single empty sqpack block.
            Chunk::SqpkDeleteData {
                target,
                block_offset,
                block_number,
            }
            | Chunk::SqpkExpandData {
                target,
                block_offset,
                block_number,
            } => {
                let (empty, zero) = DataRef::from_empty(
                    patch.clone(),
                    *block_offset as u64,
                    u32::try_from(*block_number)?,
                )?;
                let refs = self.file(resolve(target));
                refs.push(empty);
                refs.extend(zero);
            }

            Chunk::SqpkFileAdd {
                target,
                file_offset,
                blocks,
            } => {
                let path = resolve(target);
                let refs = self.file(path.clone());
                // Writing from the start replaces the file rather than patching it.
                if *file_offset == 0 {
                    log::info!("Clearing file {path} ({} Blocks)", refs.len());
                    refs.clear();
                }
                let mut offset = *file_offset as u64;
                for block in blocks {
                    let data_size = u32::try_from(block.data_size)?;
                    refs.push(if block.is_compressed() {
                        DataRef::from_compressed_patch_data(
                            patch.clone(),
                            block.patch_offset as u64,
                            offset,
                            u32::try_from(block.compressed_size)?,
                            data_size,
                        )
                    } else {
                        DataRef::from_raw_patch_data(
                            patch.clone(),
                            block.patch_offset as u64,
                            offset,
                            data_size,
                        )
                    });
                    offset += u64::from(data_size);
                }
            }

            Chunk::SqpkFileDelExpac { expansion_id } => {
                let expansion = expansion_folder(*expansion_id);
                for root in EXPANSION_ROOTS {
                    let dir = format!("{root}/{expansion}");
                    self.files.retain(|path, _| {
                        !path.starts_with(&dir)
                            || KEEP_ON_REMOVE.iter().any(|keep| path.ends_with(keep))
                    });
                }
            }

            Chunk::FileHeader(_)
            | Chunk::ApplyOption { .. }
            | Chunk::ApplyFreeSpace { .. }
            | Chunk::EndOfFile
            | Chunk::Xxxx
            | Chunk::SqpkIndex
            | Chunk::SqpkPatchInfo { .. }
            | Chunk::SqpkTargetInfo { .. } => {}
        }

        Ok(())
    }

    fn file(&mut self, path: String) -> &mut Vec<DataRef> {
        self.dirty.insert(path.clone());
        self.files.entry(path).or_default()
    }

    pub fn remove_overlaps(&mut self) -> Result<()> {
        for path in std::mem::take(&mut self.dirty) {
            let Some(refs) = self.files.get_mut(&path) else {
                continue;
            };
            let mut merged = BTreeMap::new();
            // Replayed in list order, not sorted order: the order is what encodes
            // which patch wrote a byte last.
            for interval in refs.iter() {
                apply_overlay(&mut merged, interval)
                    .with_context(|| format!("collapsing references for {path}"))?;
            }
            *refs = merged.into_values().collect();
        }
        Ok(())
    }

    pub fn write(&self, compression: CompressType) -> Result<Vec<u8>> {
        self.wrap(&self.payload()?, compression)
    }

    /// The uncompressed payload: the version table, the folder and file names, then
    /// each file's references. This is the whole of a CLUT's content, so two CLUTs
    /// with equal payloads differ only in how they were packed.
    pub fn payload(&self) -> Result<Vec<u8>> {
        let versions: Vec<PatchVersion> = self
            .files
            .values()
            .flatten()
            .map(DataRef::applied_version)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let patches = PatchIndex::new(&versions);

        let mut payload = Cursor::new(Vec::new());
        let le = Endian::Little;
        (versions.len() as i32).write_options(&mut payload, le, ())?;
        for version in &versions {
            NetString(version.to_string()).write_options(&mut payload, le, ())?;
        }
        (self.folders.len() as i32).write_options(&mut payload, le, ())?;
        for folder in &self.folders {
            NetString(folder.clone()).write_options(&mut payload, le, ())?;
        }
        (self.files.len() as i32).write_options(&mut payload, le, ())?;
        for path in self.files.keys() {
            NetString(path.clone()).write_options(&mut payload, le, ())?;
        }
        for refs in self.files.values() {
            FileData::write_with_patches(refs, &mut payload, &patches)?;
        }
        Ok(payload.into_inner())
    }

    /// Compress a payload and put a header on it.
    pub fn wrap(&self, payload: &[u8], compression: CompressType) -> Result<Vec<u8>> {
        let compressed = compress_chunk(compression, payload)?;
        let mut header = self.header.clone();
        header.file_version = Version::SeparateVersioning;
        header.compression = compression;
        header.decompressed_size =
            i32::try_from(payload.len()).context("CLUT payload too large")?;
        header.compressed_size = i32::try_from(compressed.len())?;

        let mut out = Cursor::new(Vec::with_capacity(compressed.len() + 256));
        header.write_options(&mut out, Endian::Little, ())?;
        out.write_all(&compressed)?;
        Ok(out.into_inner())
    }
}

fn apply_overlay(segments: &mut BTreeMap<u64, DataRef>, new_segment: &DataRef) -> Result<()> {
    let new_start = new_segment.offset();
    let new_end = new_segment.end();
    ensure!(
        new_start < new_end,
        "zero-length reference at {new_start}, which has no place in a set keyed by offset"
    );

    let mut right = None;
    // The interval to the left may reach into, or straight past, the new one.
    if let Some((_, prev)) = segments.range(..new_start).next_back() {
        let prev = prev.clone();
        if prev.end() > new_start {
            if prev.end() > new_end {
                right = Some(prev.slice_interval(new_end, prev.end())?);
            }
            segments.insert(
                prev.offset(),
                prev.slice_interval(prev.offset(), new_start)?,
            );
        }
    }

    // Everything the new interval fully covers is dropped; the one interval it only
    // partly covers is trimmed instead. Intervals are disjoint, so at most the last
    // in range can reach past the end.
    let covered: Vec<u64> = segments
        .range(new_start..new_end)
        .map(|(k, _)| *k)
        .collect();
    for key in covered {
        let current = segments.remove(&key).expect("key came from this map");
        if current.end() > new_end {
            segments.insert(new_end, current.slice_interval(new_end, current.end())?);
            break;
        }
    }

    segments.insert(new_start, new_segment.clone());
    if let Some(right) = right {
        segments.insert(right.offset(), right);
    }
    Ok(())
}
