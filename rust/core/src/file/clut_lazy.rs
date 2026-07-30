use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Cursor, Read, Seek, Write};
use std::ops::Range;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, ensure};
use binrw::{BinRead, BinWrite, Endian};

use super::clut::{Clut, ClutIndex, compress_chunk, decompress_chunk};
use super::data_ref::DataRef;
use super::file_data::{FileData, PatchIndex};
use super::header::Header;
use super::types::{CompressType, Version};
use super::utils::{NetString, VarInt64, VarUInt32};
use super::version::PatchVersion;

/// Refs between decode checkpoints. Each costs 28 bytes, so a 3.4M-ref CLUT spends
/// ~24 KB on them while bounding a windowed decode to one stride.
const STRIDE: u32 = 4096;

/// Chunks are grown to about this size before being closed, so the many tiny file
/// blocks (the median is a few KB) share a chunk instead of each paying a frame
/// header and losing compression context. A block larger than this gets its own
/// chunk: splitting one would gain nothing, since a file's refs are delta-encoded
/// and must be decoded from the start of its block regardless.
const TARGET_CHUNK: usize = 1 << 20;

/// Decompressed chunks kept per CLUT. Reads walk a file sequentially, so a single
/// entry covers the common case; two keeps interleaved readers from thrashing.
const CHUNK_CACHE: usize = 2;

/// A CLUT that decodes a file's [`DataRef`]s only when a read asks for them, and (for
/// [`Version::Indexed`]) decompresses only the chunk holding them.
///
/// The eager [`Clut`] inflates every varint into an 88-byte `DataRef` up front: 3.4M
/// refs for the base game, ~375 MB resident, to serve reads that each need a few
/// hundred. This holds the compressed payload instead and expands slices of it.
pub struct LazyClut {
    pub header: Header,
    pub folders: HashSet<String>,
    versions: Vec<PatchVersion>,
    stride: u32,
    /// Payload order, which the index section is positional against.
    files: Vec<(String, Block)>,
    by_name: HashMap<String, usize>,
    payload: Payload,
}

enum Payload {
    /// Pre-v3: one stream with no chunk table, so it is expanded up front. Blocks
    /// address it directly, as though it were a single chunk.
    Whole(Arc<[u8]>),
    /// v3: chunks expanded on demand.
    Chunked {
        compressed: Box<[u8]>,
        chunks: Vec<ChunkSpan>,
        recent: Mutex<VecDeque<(u32, Arc<[u8]>)>>,
    },
}

/// One chunk's extent in the payload.
#[derive(Clone, Copy, Debug)]
pub struct ChunkSpan {
    pub compressed_offset: u32,
    pub compressed_len: u32,
    pub decompressed_len: u32,
}

impl ChunkSpan {
    pub(crate) fn compressed_range(&self) -> Range<usize> {
        let start = self.compressed_offset as usize;
        start..start + self.compressed_len as usize
    }
}

/// One file's ref regions.
struct Block {
    ref_count: u32,
    /// Reconstructed file length (last ref's end offset).
    size: u64,
    /// Refs ascend by offset, so a window can be located by binary search. The
    /// format permits a negative delta; such a file is decoded in full instead.
    ascending: bool,
    /// Which chunk holds this block. Checkpoints are relative to its start.
    chunk: u32,
    checkpoints: Box<[Checkpoint]>,
}

/// Decoder state entering a ref. A file's refs are stored as three consecutive
/// regions (structs, offset deltas, lengths) decoded in lockstep, so resuming
/// mid-file needs a position in each, plus the two delta accumulators. Lengths are
/// absolute and carry no state.
#[derive(Clone, Copy)]
struct Checkpoint {
    structs: u32,
    offsets: u32,
    lengths: u32,
    patch_offset: u64,
    file_offset: u64,
}

/// The section between the header and the payload.
pub(crate) struct Index {
    stride: u32,
    pub(crate) chunks: Vec<ChunkSpan>,
    files: Vec<Block>,
}

/// Summarized rather than derived: the payload is megabytes.
impl std::fmt::Debug for LazyClut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyClut")
            .field("repository", &self.header.repository)
            .field("version", &self.header.version)
            .field("folders", &self.folders.len())
            .field("files", &self.files.len())
            .field("chunked", &matches!(self.payload, Payload::Chunked { .. }))
            .finish()
    }
}

impl LazyClut {
    pub fn read<R: Read + Seek>(mut reader: R) -> Result<Self> {
        let header = Header::read_options(&mut reader, Endian::Little, ())?;
        let index = (header.file_version == Version::Indexed)
            .then(|| Index::read(&mut reader))
            .transpose()?;

        match index {
            // Only chunk 0 (the string sections) is expanded here; file blocks wait
            // until a read needs them.
            Some(index) => {
                let mut compressed =
                    vec![0u8; header.get_compressed_size() as usize].into_boxed_slice();
                reader.read_exact(&mut compressed)?;

                let first = index.chunks.first().context("CLUT index has no chunks")?;
                let strings = decompress_chunk(
                    header.compression,
                    compressed
                        .get(first.compressed_range())
                        .context("CLUT chunk runs past the payload")?,
                    first.decompressed_len as usize,
                )?;
                let (versions, folders, names) = Clut::read_strings(&mut Cursor::new(&strings))?;

                Self::build(
                    header,
                    folders,
                    versions,
                    index.stride,
                    names,
                    index.files,
                    Payload::Chunked {
                        compressed,
                        chunks: index.chunks,
                        recent: Mutex::default(),
                    },
                )
            }
            // No index, so block boundaries have to be walked out of the payload.
            None => {
                let blob: Arc<[u8]> =
                    Clut::decompress_payload(&header, None, &mut reader)?.into();
                let mut cursor = Cursor::new(&blob[..]);
                let (versions, folders, names) = Clut::read_strings(&mut cursor)?;
                let blocks = names
                    .iter()
                    .map(|_| Block::scan(&mut cursor, &versions, STRIDE, 0))
                    .collect::<Result<Vec<_>>>()?;

                Self::build(
                    header,
                    folders,
                    versions,
                    STRIDE,
                    names,
                    blocks,
                    Payload::Whole(blob),
                )
            }
        }
    }

    fn build(
        header: Header,
        folders: HashSet<String>,
        versions: Vec<PatchVersion>,
        stride: u32,
        names: Vec<String>,
        blocks: Vec<Block>,
        payload: Payload,
    ) -> Result<Self> {
        ensure!(
            blocks.len() == names.len(),
            "CLUT index covers {} files, payload has {}",
            blocks.len(),
            names.len()
        );
        let by_name = names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), i))
            .collect();

        Ok(Self {
            header,
            folders,
            versions,
            stride,
            files: names.into_iter().zip(blocks).collect(),
            by_name,
            payload,
        })
    }

    pub fn contains(&self, path: &str) -> bool {
        self.by_name.contains_key(path)
    }

    pub fn file_size(&self, path: &str) -> Option<u64> {
        self.block(path).ok().map(|b| b.size)
    }

    /// Folder set plus each file's size. For an indexed CLUT the sizes come straight
    /// from the index, so nothing is decoded and no file block is even decompressed.
    pub fn index(&self) -> ClutIndex {
        ClutIndex {
            folders: self.folders.clone(),
            files: self
                .files
                .iter()
                .map(|(name, block)| (name.clone(), block.size))
                .collect(),
        }
    }

    /// Decode every ref for one file.
    pub fn file_refs(&self, path: &str) -> Result<Vec<DataRef>> {
        self.file_refs_range(path, 0, u64::MAX)
    }

    /// Decode only the refs overlapping `[start, end)`, in file order.
    pub fn file_refs_range(&self, path: &str, start: u64, end: u64) -> Result<Vec<DataRef>> {
        let block = self.block(path)?;
        if block.ref_count == 0 {
            return Ok(Vec::new());
        }

        // The last checkpoint at or before `start` is the earliest that can hold an
        // overlapping ref. Without the ascending guarantee there is nothing to
        // bisect, so decode from the top.
        let first = if block.ascending {
            block
                .checkpoints
                .partition_point(|c| c.file_offset <= start)
                .saturating_sub(1)
        } else {
            0
        };
        let checkpoint = block.checkpoints[first];
        let chunk = self.chunk(block.chunk)?;

        let mut structs = Cursor::new(&chunk[..]);
        let mut offsets = Cursor::new(&chunk[..]);
        let mut lengths = Cursor::new(&chunk[..]);
        structs.set_position(checkpoint.structs.into());
        offsets.set_position(checkpoint.offsets.into());
        lengths.set_position(checkpoint.lengths.into());

        let mut patch_offset = checkpoint.patch_offset;
        let mut offset = checkpoint.file_offset;
        let mut out = Vec::new();
        for _ in (first as u32 * self.stride)..block.ref_count {
            let mut data_ref = DataRef::read_options(
                &mut structs,
                Endian::Little,
                (&mut patch_offset, &self.versions),
            )?;
            offset = offset
                .checked_add_signed(VarInt64::read_options(&mut offsets, Endian::Little, ())?.0)
                .context("file offset overflow")?;
            if block.ascending && offset >= end {
                break;
            }
            data_ref.set_offset(offset);
            data_ref.set_len(VarUInt32::read_options(&mut lengths, Endian::Little, ())?.0);
            if offset < end && offset.saturating_add(data_ref.len().into()) > start {
                out.push(data_ref);
            }
        }
        Ok(out)
    }

    fn block(&self, path: &str) -> Result<&Block> {
        let i = *self
            .by_name
            .get(path)
            .with_context(|| format!("file not in CLUT: {path}"))?;
        Ok(&self.files[i].1)
    }

    /// The decompressed bytes of one chunk, from the small cache when possible.
    fn chunk(&self, index: u32) -> Result<Arc<[u8]>> {
        let (compressed, chunks, recent) = match &self.payload {
            Payload::Whole(blob) => return Ok(blob.clone()),
            Payload::Chunked {
                compressed,
                chunks,
                recent,
            } => (compressed, chunks, recent),
        };

        {
            let mut recent = recent.lock().unwrap();
            if let Some(pos) = recent.iter().position(|(i, _)| *i == index) {
                let entry = recent.remove(pos).unwrap();
                let bytes = entry.1.clone();
                recent.push_front(entry);
                return Ok(bytes);
            }
        }

        let span = chunks
            .get(index as usize)
            .with_context(|| format!("CLUT chunk {index} out of range"))?;
        // Decompressed outside the lock; a racing reader may duplicate the work,
        // which is cheaper than serializing every read behind it.
        let bytes: Arc<[u8]> = decompress_chunk(
            self.header.compression,
            compressed
                .get(span.compressed_range())
                .context("CLUT chunk runs past the payload")?,
            span.decompressed_len as usize,
        )?
        .into();

        let mut recent = recent.lock().unwrap();
        if !recent.iter().any(|(i, _)| *i == index) {
            recent.push_front((index, bytes.clone()));
            recent.truncate(CHUNK_CACHE);
        }
        Ok(bytes)
    }

    /// Rewrite as an indexed, chunked CLUT. Refs are re-encoded a file at a time, so
    /// this never holds more than one file's worth of them.
    pub fn rewrite(&self, compression: CompressType) -> Result<Vec<u8>> {
        // Chunk 0 is the string sections alone: a reader that only wants the file
        // list and sizes then expands nothing else.
        let mut strings = Cursor::new(Vec::new());
        (self.versions.len() as i32).write_options(&mut strings, Endian::Little, ())?;
        for version in &self.versions {
            NetString(format!("{version}")).write_options(&mut strings, Endian::Little, ())?;
        }
        (self.folders.len() as i32).write_options(&mut strings, Endian::Little, ())?;
        // Sorted, because a `HashSet` iterates in an order that differs between
        // processes and would make the output unreproducible. No published CLUT has
        // any folders, so this has never shown up.
        let mut folders: Vec<&String> = self.folders.iter().collect();
        folders.sort();
        for folder in folders {
            NetString(folder.clone()).write_options(&mut strings, Endian::Little, ())?;
        }
        (self.files.len() as i32).write_options(&mut strings, Endian::Little, ())?;
        for (name, _) in &self.files {
            NetString(name.clone()).write_options(&mut strings, Endian::Little, ())?;
        }

        let patches = PatchIndex::new(&self.versions);
        let mut raw_chunks = vec![strings.into_inner()];
        // Each file records the chunk it lands in; blocks are appended until the
        // chunk reaches TARGET_CHUNK, so checkpoints are recovered per chunk below.
        let mut chunk_of = Vec::with_capacity(self.files.len());
        let mut current = Vec::new();
        let mut per_chunk = vec![0usize];
        for (name, _) in &self.files {
            let refs = self.file_refs(name)?;
            let mut block = Cursor::new(Vec::new());
            FileData::write_with_patches(&refs, &mut block, &patches)?;
            let block = block.into_inner();

            if !current.is_empty() && current.len() + block.len() > TARGET_CHUNK {
                raw_chunks.push(std::mem::take(&mut current));
                per_chunk.push(0);
            }
            current.extend_from_slice(&block);
            chunk_of.push(raw_chunks.len() as u32);
            *per_chunk.last_mut().unwrap() += 1;
            if current.len() >= TARGET_CHUNK {
                raw_chunks.push(std::mem::take(&mut current));
                per_chunk.push(0);
            }
        }
        if !current.is_empty() {
            raw_chunks.push(current);
        } else {
            per_chunk.pop();
        }

        // Recover the checkpoints by walking each chunk exactly as a reader will,
        // which keeps one implementation of the layout rather than two.
        let mut blocks = Vec::with_capacity(self.files.len());
        for (chunk, count) in raw_chunks.iter().skip(1).zip(&per_chunk) {
            let mut cursor = Cursor::new(&chunk[..]);
            let index = blocks.len();
            for i in 0..*count {
                blocks.push(Block::scan(
                    &mut cursor,
                    &self.versions,
                    STRIDE,
                    chunk_of[index + i],
                )?);
            }
        }

        let mut payload = Vec::new();
        let mut chunks = Vec::with_capacity(raw_chunks.len());
        for raw in &raw_chunks {
            let compressed = compress_chunk(compression, raw)?;
            chunks.push(ChunkSpan {
                compressed_offset: payload.len() as u32,
                compressed_len: compressed.len() as u32,
                decompressed_len: raw.len() as u32,
            });
            payload.extend_from_slice(&compressed);
        }

        let mut header = self.header.clone();
        header.file_version = Version::Indexed;
        header.compression = compression;
        header.decompressed_size = raw_chunks.iter().map(|c| c.len() as i32).sum();
        header.compressed_size = payload.len() as i32;

        let section = Index {
            stride: STRIDE,
            chunks,
            files: blocks,
        }
        .write()?;

        let mut out = Cursor::new(Vec::with_capacity(payload.len() + section.len() + 256));
        header.write_options(&mut out, Endian::Little, ())?;
        i32::try_from(section.len())
            .context("CLUT index section too large")?
            .write_options(&mut out, Endian::Little, ())?;
        out.write_all(&section)?;
        out.write_all(&payload)?;
        Ok(out.into_inner())
    }
}

impl Index {
    pub(crate) fn read<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let size = i32::read_options(reader, Endian::Little, ())?;
        ensure!(size >= 0, "negative CLUT index size");
        let mut section = vec![0u8; size as usize];
        reader.read_exact(&mut section)?;
        let mut cursor = Cursor::new(&section[..]);

        let stride = u32::read_options(&mut cursor, Endian::Little, ())?;
        ensure!(stride > 0, "CLUT index stride is zero");

        let chunk_count = u32::read_options(&mut cursor, Endian::Little, ())?;
        let mut chunks = Vec::with_capacity(chunk_count as usize);
        let mut offset = 0u32;
        for _ in 0..chunk_count {
            let compressed_len = u32::read_options(&mut cursor, Endian::Little, ())?;
            chunks.push(ChunkSpan {
                compressed_offset: offset,
                compressed_len,
                decompressed_len: u32::read_options(&mut cursor, Endian::Little, ())?,
            });
            offset = offset
                .checked_add(compressed_len)
                .context("CLUT chunk table overflows the payload")?;
        }

        let file_count = u32::read_options(&mut cursor, Endian::Little, ())?;
        let files = (0..file_count)
            .map(|_| Block::read(&mut cursor, stride, chunk_count))
            .collect::<Result<_>>()?;

        Ok(Self {
            stride,
            chunks,
            files,
        })
    }

    fn write(&self) -> Result<Vec<u8>> {
        let mut out = Cursor::new(Vec::new());
        self.stride.write_options(&mut out, Endian::Little, ())?;
        (self.chunks.len() as u32).write_options(&mut out, Endian::Little, ())?;
        for chunk in &self.chunks {
            chunk
                .compressed_len
                .write_options(&mut out, Endian::Little, ())?;
            chunk
                .decompressed_len
                .write_options(&mut out, Endian::Little, ())?;
        }
        (self.files.len() as u32).write_options(&mut out, Endian::Little, ())?;
        for block in &self.files {
            block.write(&mut out)?;
        }
        Ok(out.into_inner())
    }
}

impl Block {
    /// Walk one file's three ref regions, recording a checkpoint every `stride`
    /// refs. Leaves the cursor at the next file's block.
    fn scan(
        cursor: &mut Cursor<&[u8]>,
        versions: &[PatchVersion],
        stride: u32,
        chunk: u32,
    ) -> Result<Self> {
        let ref_count = i32::read_options(cursor, Endian::Little, ())?;
        ensure!(ref_count >= 0, "negative CLUT ref count");
        let ref_count = ref_count as u32;
        let checkpoints = ref_count.div_ceil(stride) as usize;

        let mut structs = Vec::with_capacity(checkpoints);
        let mut patch_offsets = Vec::with_capacity(checkpoints);
        let mut patch_offset = 0u64;
        for i in 0..ref_count {
            if i % stride == 0 {
                structs.push(cursor.position() as u32);
                patch_offsets.push(patch_offset);
            }
            DataRef::read_options(cursor, Endian::Little, (&mut patch_offset, versions))?;
        }

        let mut offsets = Vec::with_capacity(checkpoints);
        let mut file_offsets = Vec::with_capacity(checkpoints);
        let mut offset = 0u64;
        let mut ascending = true;
        for i in 0..ref_count {
            if i % stride == 0 {
                offsets.push(cursor.position() as u32);
                file_offsets.push(offset);
            }
            let delta = VarInt64::read_options(cursor, Endian::Little, ())?.0;
            ascending &= delta >= 0;
            offset = offset
                .checked_add_signed(delta)
                .context("file offset overflow")?;
        }

        let mut lengths = Vec::with_capacity(checkpoints);
        let mut len = 0u32;
        for i in 0..ref_count {
            if i % stride == 0 {
                lengths.push(cursor.position() as u32);
            }
            len = VarUInt32::read_options(cursor, Endian::Little, ())?.0;
        }

        Ok(Self {
            ref_count,
            size: offset + u64::from(len),
            ascending,
            chunk,
            checkpoints: (0..checkpoints)
                .map(|i| Checkpoint {
                    structs: structs[i],
                    offsets: offsets[i],
                    lengths: lengths[i],
                    patch_offset: patch_offsets[i],
                    file_offset: file_offsets[i],
                })
                .collect(),
        })
    }

    fn read<R: Read + Seek>(reader: &mut R, stride: u32, chunk_count: u32) -> Result<Self> {
        let ref_count = u32::read_options(reader, Endian::Little, ())?;
        let size = u64::read_options(reader, Endian::Little, ())?;
        let ascending = u8::read_options(reader, Endian::Little, ())? != 0;
        let chunk = u32::read_options(reader, Endian::Little, ())?;
        ensure!(
            chunk < chunk_count,
            "CLUT index points a file at chunk {chunk} of {chunk_count}"
        );
        let count = u32::read_options(reader, Endian::Little, ())?;
        // The decode loop derives a checkpoint's first ref index as `i * stride`, so
        // a table of the wrong length would silently decode the wrong refs.
        ensure!(
            count == ref_count.div_ceil(stride),
            "CLUT index has {count} checkpoints for {ref_count} refs at stride {stride}"
        );

        let checkpoints = (0..count)
            .map(|_| {
                Ok(Checkpoint {
                    structs: u32::read_options(reader, Endian::Little, ())?,
                    offsets: u32::read_options(reader, Endian::Little, ())?,
                    lengths: u32::read_options(reader, Endian::Little, ())?,
                    patch_offset: u64::read_options(reader, Endian::Little, ())?,
                    file_offset: u64::read_options(reader, Endian::Little, ())?,
                })
            })
            .collect::<binrw::BinResult<_>>()?;

        Ok(Self {
            ref_count,
            size,
            ascending,
            chunk,
            checkpoints,
        })
    }

    fn write<W: Write + Seek>(&self, writer: &mut W) -> binrw::BinResult<()> {
        self.ref_count.write_options(writer, Endian::Little, ())?;
        self.size.write_options(writer, Endian::Little, ())?;
        u8::from(self.ascending).write_options(writer, Endian::Little, ())?;
        self.chunk.write_options(writer, Endian::Little, ())?;
        (self.checkpoints.len() as u32).write_options(writer, Endian::Little, ())?;
        for checkpoint in &self.checkpoints {
            checkpoint.structs.write_options(writer, Endian::Little, ())?;
            checkpoint.offsets.write_options(writer, Endian::Little, ())?;
            checkpoint.lengths.write_options(writer, Endian::Little, ())?;
            checkpoint
                .patch_offset
                .write_options(writer, Endian::Little, ())?;
            checkpoint
                .file_offset
                .write_options(writer, Endian::Little, ())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::types::PlatformId;
    use crate::file::utils::VarInt32;
    use crate::file::version::GameVersion;

    const PATCHES: [&str; 3] = [
        "D2023.01.01.0000.0000",
        "D2024.06.02.0000.0000",
        "D2025.11.03.0000.0001",
    ];

    /// Build an uncompressed pre-v3 CLUT whose files have the given ref counts,
    /// cycling through every ref type so the decoders agree on each variant's
    /// layout. Refs are laid out contiguously from offset 0, as a real CLUT's are.
    fn synth(files: &[(&str, u32)]) -> Vec<u8> {
        let mut blob = Cursor::new(Vec::new());
        let le = Endian::Little;

        (PATCHES.len() as i32).write_options(&mut blob, le, ()).unwrap();
        for patch in PATCHES {
            NetString(patch.to_string()).write_options(&mut blob, le, ()).unwrap();
        }
        (1i32).write_options(&mut blob, le, ()).unwrap();
        NetString("sqpack/ffxiv".to_string()).write_options(&mut blob, le, ()).unwrap();
        (files.len() as i32).write_options(&mut blob, le, ()).unwrap();
        for (name, _) in files {
            NetString(name.to_string()).write_options(&mut blob, le, ()).unwrap();
        }

        for (_, ref_count) in files {
            (*ref_count as i32).write_options(&mut blob, le, ()).unwrap();

            let mut patch_offset = 0u64;
            for i in 0..*ref_count {
                let kind = i % 4;
                let raw_type: u8 = match kind {
                    0 => 0xFF, // patch, no patch offset
                    1 => 0,    // patch, with patch offset
                    2 => 1,    // zero fill
                    _ => 2,    // empty block
                };
                raw_type.write_options(&mut blob, le, ()).unwrap();
                VarInt32((i % PATCHES.len() as u32) as i32)
                    .write_options(&mut blob, le, ())
                    .unwrap();
                match kind {
                    0 | 1 => {
                        let target = patch_offset + 4096 + u64::from(i);
                        VarInt64(target.wrapping_sub(patch_offset) as i64)
                            .write_options(&mut blob, le, ())
                            .unwrap();
                        patch_offset = target;
                        VarUInt32(ref_len(i)).write_options(&mut blob, le, ()).unwrap();
                        u8::from(i % 8 == 0).write_options(&mut blob, le, ()).unwrap();
                        if raw_type == 0 {
                            VarUInt32(i % 512).write_options(&mut blob, le, ()).unwrap();
                        }
                    }
                    2 => {}
                    _ => (1i32 + (i % 3) as i32).write_options(&mut blob, le, ()).unwrap(),
                }
            }

            for i in 0..*ref_count {
                let delta = if i == 0 { 0 } else { i64::from(ref_len(i - 1)) };
                VarInt64(delta).write_options(&mut blob, le, ()).unwrap();
            }

            for i in 0..*ref_count {
                VarUInt32(ref_len(i)).write_options(&mut blob, le, ()).unwrap();
            }
        }

        let blob = blob.into_inner();
        let header = Header {
            compression: CompressType::None,
            platform: PlatformId::Win32,
            version: GameVersion::new("2025.11.03.0000.0000").unwrap(),
            patch_version: PatchVersion::new(PATCHES[0]).unwrap(),
            decompressed_size: blob.len() as i32,
            compressed_size: blob.len() as i32,
            ..Default::default()
        };

        let mut out = Cursor::new(Vec::new());
        header.write_options(&mut out, Endian::Little, ()).unwrap();
        out.write_all(&blob).unwrap();
        out.into_inner()
    }

    /// Ref lengths vary so a window boundary can fall mid-ref.
    fn ref_len(i: u32) -> u32 {
        100 + (i % 13) * 7
    }

    fn eager(bytes: &[u8]) -> Clut {
        Clut::read(Cursor::new(bytes)).unwrap()
    }

    #[test]
    fn matches_eager_parser() {
        // One file spans several strides so checkpoint resume is exercised.
        let bytes = synth(&[("a.dat", 0), ("b.dat", 1), ("c.dat", 9000), ("d.dat", 4096)]);
        let eager = eager(&bytes);
        let lazy = LazyClut::read(Cursor::new(&bytes)).unwrap();

        assert_eq!(lazy.folders, eager.folders);
        for (name, expected) in &eager.files {
            assert_eq!(&lazy.file_refs(name).unwrap(), expected.as_ref(), "{name}");
            let size = expected.last().map_or(0, |r| r.offset() + u64::from(r.len()));
            assert_eq!(lazy.file_size(name), Some(size), "{name}");
            assert_eq!(lazy.index().files.get(name), Some(&size), "{name}");
            assert!(lazy.contains(name));
        }
        assert!(!lazy.contains("nope.dat"));
        assert!(lazy.file_refs("nope.dat").is_err());
    }

    /// The browse path switched from `Clut::read_index` to `LazyClut::index`, so the
    /// two must agree on the folder set as well as the sizes.
    #[test]
    fn index_matches_read_index() {
        let spec = &[("a.dat", 0), ("b.dat", 1), ("c.dat", 9000)];
        let bytes = synth(spec);
        let scanned = LazyClut::read(Cursor::new(&bytes)).unwrap();
        let expected = Clut::read_index(Cursor::new(&bytes)).unwrap();

        assert_eq!(scanned.index().folders, expected.folders);
        assert_eq!(scanned.index().files, expected.files);
        assert!(!expected.folders.is_empty(), "fixture has no folders to compare");

        // And through a chunked rewrite, where sizes come from the index instead.
        let rewritten = scanned.rewrite(CompressType::Zstd).unwrap();
        let indexed = LazyClut::read(Cursor::new(&rewritten)).unwrap();
        assert_eq!(indexed.index().folders, expected.folders);
        assert_eq!(indexed.index().files, expected.files);
    }

    #[test]
    fn windowed_decode_matches_full_decode() {
        let bytes = synth(&[("c.dat", 9000)]);
        let lazy = LazyClut::read(Cursor::new(&bytes)).unwrap();
        let all = lazy.file_refs("c.dat").unwrap();
        let size = lazy.file_size("c.dat").unwrap();

        // Windows that start mid-ref, cross stride boundaries, and run past EOF.
        for (start, end) in [
            (0, 1),
            (0, size),
            (1, 2),
            (50, 51),
            (size - 1, size),
            (size / 2, size / 2 + 8192),
            (size - 10, size + 4096),
            (size, size + 1),
        ] {
            let expected: Vec<_> = all
                .iter()
                .filter(|r| r.offset() < end && r.offset() + u64::from(r.len()) > start)
                .cloned()
                .collect();
            let got = lazy.file_refs_range("c.dat", start, end).unwrap();
            assert_eq!(got, expected, "window {start}..{end}");
        }
    }

    /// Enough refs that the block alone exceeds TARGET_CHUNK, forcing its own chunk.
    const OVERSIZED: u32 = 150_000;

    #[test]
    fn chunked_rewrite_matches_scan() {
        // Many small blocks group into one chunk; the oversized one stands alone.
        let mut spec: Vec<(String, u32)> = (0..40).map(|i| (format!("small{i}.dat"), 10)).collect();
        spec.push(("big.dat".to_string(), OVERSIZED));
        spec.push(("tail.dat".to_string(), 7));
        let spec: Vec<(&str, u32)> = spec.iter().map(|(n, c)| (n.as_str(), *c)).collect();

        let bytes = synth(&spec);
        let scanned = LazyClut::read(Cursor::new(&bytes)).unwrap();

        // Brotli at q11 over a megabyte is slow in a debug build, and the codec is
        // orthogonal to chunk layout; every codec is covered on a small input below.
        for compression in [CompressType::None, CompressType::Zstd] {
            let rewritten = scanned.rewrite(compression).unwrap();
            let indexed = LazyClut::read(Cursor::new(&rewritten)).unwrap();

            assert_eq!(indexed.header.file_version, Version::Indexed);
            assert_eq!(indexed.header.compression, compression);
            assert_eq!(indexed.folders, scanned.folders);
            assert_eq!(indexed.index().files, scanned.index().files);

            // More than one chunk, and the oversized block is not sharing one.
            let Payload::Chunked { chunks, .. } = &indexed.payload else {
                panic!("rewrite produced an unchunked payload");
            };
            assert!(chunks.len() >= 3, "expected several chunks, got {}", chunks.len());
            assert_ne!(
                indexed.block("big.dat").unwrap().chunk,
                indexed.block("small0.dat").unwrap().chunk
            );

            for (name, _) in &spec {
                assert_eq!(
                    indexed.file_refs(name).unwrap(),
                    scanned.file_refs(name).unwrap(),
                    "{compression:?} refs for {name}"
                );
                let size = scanned.file_size(name).unwrap();
                assert_eq!(
                    indexed.file_refs_range(name, size / 3, size / 3 + 4096).unwrap(),
                    scanned.file_refs_range(name, size / 3, size / 3 + 4096).unwrap(),
                    "{compression:?} window for {name}"
                );
            }

            // The eager reader must reassemble the chunks into the same payload.
            assert_eq!(eager(&rewritten).files, eager(&bytes).files);
            // And re-chunking an already-chunked CLUT stays consistent.
            let again = indexed.rewrite(compression).unwrap();
            let twice = LazyClut::read(Cursor::new(&again)).unwrap();
            assert_eq!(twice.file_refs("big.dat").unwrap(), scanned.file_refs("big.dat").unwrap());
        }
    }

    #[test]
    fn every_codec_roundtrips_and_compresses() {
        let bytes = synth(&[("a.dat", 20_000), ("b.dat", 9), ("c.dat", 20_000)]);
        let scanned = LazyClut::read(Cursor::new(&bytes)).unwrap();

        for compression in [
            CompressType::None,
            CompressType::Zlib,
            CompressType::Brotli,
            CompressType::Zstd,
        ] {
            let rewritten = scanned.rewrite(compression).unwrap();
            let indexed = LazyClut::read(Cursor::new(&rewritten)).unwrap();
            for name in ["a.dat", "b.dat", "c.dat"] {
                assert_eq!(
                    indexed.file_refs(name).unwrap(),
                    scanned.file_refs(name).unwrap(),
                    "{compression:?} refs for {name}"
                );
            }
            if compression != CompressType::None {
                assert!(
                    rewritten.len() < bytes.len(),
                    "{compression:?} rewrite {} not smaller than raw {}",
                    rewritten.len(),
                    bytes.len()
                );
            }
        }
    }
}
