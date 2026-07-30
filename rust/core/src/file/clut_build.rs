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

#[cfg(test)]
mod tests {
    use super::*;

    fn patch() -> PatchVersion {
        PatchVersion::new("D2025.01.01.0000.0000").unwrap()
    }

    /// Raw patch data, so slicing is allowed and the patch offset tracks the trim.
    fn raw(offset: u64, length: u32) -> DataRef {
        DataRef::from_raw_patch_data(patch(), 1_000_000 + offset, offset, length)
    }

    fn extents(segments: &[DataRef]) -> Vec<(u64, u64)> {
        segments.iter().map(|r| (r.offset(), r.end())).collect()
    }

    fn try_overlay(intervals: &[DataRef]) -> Result<Vec<DataRef>> {
        let mut out = BTreeMap::new();
        for interval in intervals {
            apply_overlay(&mut out, interval)?;
        }
        Ok(out.into_values().collect())
    }

    fn overlay(intervals: &[DataRef]) -> Vec<DataRef> {
        try_overlay(intervals).unwrap()
    }

    #[test]
    fn later_writes_win_and_leave_no_overlap() {
        // A middle write splits the first interval in two.
        let merged = overlay(&[raw(0, 100), raw(40, 20)]);
        assert_eq!(extents(&merged), [(0, 40), (40, 60), (60, 100)]);

        // The surviving tail must still point at the right patch bytes.
        assert_eq!(merged[2].patch().unwrap().offset, 1_000_000);
        assert_eq!(merged[2].patch_offset(), Some(60));
    }

    #[test]
    fn a_write_can_swallow_several_intervals() {
        let merged = overlay(&[raw(0, 10), raw(10, 10), raw(20, 10), raw(5, 20)]);
        assert_eq!(extents(&merged), [(0, 5), (5, 25), (25, 30)]);
    }

    #[test]
    fn overlapping_ends_are_trimmed_on_both_sides() {
        assert_eq!(
            extents(&overlay(&[raw(0, 50), raw(100, 50), raw(25, 100)])),
            [(0, 25), (25, 125), (125, 150)]
        );
        // An exact replacement leaves one interval.
        assert_eq!(extents(&overlay(&[raw(0, 50), raw(0, 50)])), [(0, 50)]);
    }

    /// An empty-block header describes a whole region, so a write landing inside one
    /// has no correct answer. Failing loudly beats emitting half a header.
    #[test]
    fn slicing_an_empty_block_is_an_error() {
        let (empty, zero) = DataRef::from_empty(patch(), 0, 1024).unwrap();
        let err = try_overlay(&[empty.clone(), zero.unwrap(), raw(8, 16)])
            .expect_err("a write inside an empty block must not be silently accepted");
        assert!(err.to_string().contains("EmptyBlock"), "{err}");

        // A write that starts exactly where the header ends is fine.
        assert_eq!(extents(&overlay(&[empty, raw(24, 16)])), [(0, 24), (24, 40)]);
    }

    #[test]
    fn intervals_come_out_sorted_and_disjoint() {
        // Interleaved and repeatedly overlapping writes.
        let merged = overlay(
            &[
                (500u64, 100u32),
                (0, 200),
                (150, 400),
                (600, 50),
                (100, 25),
                (0, 1000),
                (300, 10),
            ]
            .map(|(offset, len)| raw(offset, len)),
        );
        for pair in merged.windows(2) {
            assert!(
                pair[0].end() <= pair[1].offset(),
                "overlap between {:?} and {:?}",
                extents(&pair[..1]),
                extents(&pair[1..])
            );
        }
        assert_eq!(extents(&merged), [(0, 300), (300, 310), (310, 1000)]);
    }

    #[test]
    fn removing_an_expansion_keeps_the_filtered_files() {
        let mut builder = ClutBuilder::new(Header::default());
        for path in [
            "sqpack/ex4/0a0000.win32.dat0",
            "sqpack/ex4/0a0000.win32.index",
            "movie/ex4/00000.bk2",
            "movie/ex4/00004.bk2",
            "sqpack/ex4/somefile.var",
            "sqpack/ffxiv/0a0000.win32.dat0",
        ] {
            builder.files.insert(path.to_string(), Vec::new());
        }
        builder
            .apply(&patch(), &Chunk::SqpkFileDelExpac { expansion_id: 4 })
            .unwrap();

        let kept: Vec<_> = builder.files.keys().map(String::as_str).collect();
        assert_eq!(
            kept,
            [
                "movie/ex4/00000.bk2",
                "sqpack/ex4/somefile.var",
                "sqpack/ffxiv/0a0000.win32.dat0",
            ]
        );
    }

    /// The same builder state must serialize to the same bytes every time, so a
    /// regenerated corpus does not churn.
    #[test]
    fn payload_is_reproducible() {
        let mut builder = ClutBuilder::new(Header::default());
        for (path, offset) in [("b.dat", 0u64), ("a.dat", 100), ("c.dat", 200)] {
            builder.files.insert(path.to_string(), vec![raw(offset, 50)]);
            builder.folders.insert(format!("dir/{path}"));
        }
        assert_eq!(builder.payload().unwrap(), builder.payload().unwrap());
    }
}
