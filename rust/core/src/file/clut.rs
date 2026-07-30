use crate::file::{data_ref::DataRef, version::PatchVersion};

use super::clut_lazy::{ChunkSpan, Index};
use super::file_data::FileData;
use super::header::Header;
use super::types::{CompressType, Version};
use super::utils::NetString;
use anyhow::{Context, ensure};
use binrw::BinRead;
use brotli::Decompressor;
use flate2::read::ZlibDecoder;
use std::io::{Cursor, Read};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Decompress one chunk (or a whole single-stream payload) to its known length.
pub(crate) fn decompress_chunk(
    compression: CompressType,
    src: &[u8],
    expected: usize,
) -> anyhow::Result<Vec<u8>> {
    let out = match compression {
        CompressType::None => src.to_vec(),
        CompressType::Zlib => {
            let mut out = Vec::with_capacity(expected);
            ZlibDecoder::new(src).read_to_end(&mut out)?;
            out
        }
        CompressType::Brotli => {
            let mut out = Vec::with_capacity(expected);
            Decompressor::new(src, 8192).read_to_end(&mut out)?;
            out
        }
        CompressType::Zstd => zstd::bulk::decompress(src, expected)?,
    };

    ensure!(
        out.len() == expected,
        "{compression:?} decompressed size mismatch: expected {expected}, got {}",
        out.len()
    );
    Ok(out)
}

/// Compress one chunk with the given codec.
pub(crate) fn compress_chunk(compression: CompressType, src: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(match compression {
        CompressType::None => src.to_vec(),
        CompressType::Zlib => {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
            std::io::Write::write_all(&mut encoder, src)?;
            encoder.finish()?
        }
        CompressType::Brotli => {
            let mut out = Vec::new();
            let params = brotli::enc::BrotliEncoderParams {
                quality: 11,
                ..Default::default()
            };
            brotli::BrotliCompress(&mut Cursor::new(src), &mut out, &params)?;
            out
        }
        CompressType::Zstd => zstd::bulk::compress(src, ZSTD_LEVEL)?,
    })
}

const ZSTD_LEVEL: i32 = 19;

/// Complete CLUT file structure containing header, folders, and file data
#[derive(Debug, Default, Clone)]
pub struct Clut {
    /// File header with metadata
    pub header: Header,

    /// Set of folder paths
    pub folders: HashSet<String>,

    /// Map of file paths to their data
    pub files: HashMap<String, Arc<Vec<DataRef>>>,
}

/// A CLUT parsed for metadata only.
#[derive(Debug, Default, Clone)]
pub struct ClutIndex {
    pub folders: HashSet<String>,
    pub files: HashMap<String, u64>,
}

impl Clut {
    /// Read a CLUT file from a binary reader
    pub fn read<R: Read + std::io::Seek>(reader: R) -> anyhow::Result<Self> {
        let (header, decompressed_data) = Self::decompress(reader)?;
        let mut cursor = Cursor::new(&decompressed_data);
        Self::read_decompressed_data(header, &mut cursor)
    }

    /// Read only the folder set and per-file sizes, without retaining the
    /// per-file `DataRef` lists.
    pub fn read_index<R: Read + std::io::Seek>(reader: R) -> anyhow::Result<ClutIndex> {
        let (_header, decompressed_data) = Self::decompress(reader)?;
        let mut reader = Cursor::new(&decompressed_data);

        let (patch_versions, folders, file_names) = Self::read_strings(&mut reader)?;

        let mut files = HashMap::with_capacity(file_names.len());
        for file_name in file_names {
            // The per-file refs are read and dropped immediately; only the
            // reconstructed length (last ref's end offset) is kept.
            let refs = FileData::read_with_patches(&mut reader, &patch_versions)?;
            let size = refs.last().map_or(0, |r| r.offset() + r.len() as u64);
            files.insert(file_name, size);
        }

        Ok(ClutIndex { folders, files })
    }

    /// Read the patch version, folder and file name sections that precede the
    /// per-file data in the decompressed payload.
    pub(crate) fn read_strings<R: Read + std::io::Seek>(
        reader: &mut R,
    ) -> anyhow::Result<(Vec<PatchVersion>, HashSet<String>, Vec<String>)> {
        use binrw::Endian;

        let patch_len = i32::read_options(reader, Endian::Little, ())?;
        let mut patch_versions = Vec::with_capacity(patch_len as usize);
        for _ in 0..patch_len {
            let patch_str = NetString::read_options(reader, Endian::Little, ())?.0;
            patch_versions.push(PatchVersion::new(&patch_str)?);
        }

        let folder_len = i32::read_options(reader, Endian::Little, ())?;
        let mut folders = HashSet::with_capacity(folder_len as usize);
        for _ in 0..folder_len {
            folders.insert(NetString::read_options(reader, Endian::Little, ())?.0);
        }

        let file_len = i32::read_options(reader, Endian::Little, ())?;
        let mut file_names = Vec::with_capacity(file_len as usize);
        for _ in 0..file_len {
            file_names.push(NetString::read_options(reader, Endian::Little, ())?.0);
        }

        Ok((patch_versions, folders, file_names))
    }

    /// The header and the payload behind it, with a chunked payload reassembled into
    /// the single stream an unchunked one holds.
    pub fn decompress<R: Read + std::io::Seek>(
        mut reader: R,
    ) -> anyhow::Result<(Header, Vec<u8>)> {
        // Read header
        let header = Header::read_options(&mut reader, binrw::Endian::Little, ())?;

        // A v3 payload is split into independently compressed chunks, so the eager
        // reader needs the chunk table to reassemble it. The rest of the index only
        // matters to readers decoding one file at a time.
        let chunks = (header.file_version == Version::Indexed)
            .then(|| Index::read(&mut reader).map(|index| index.chunks))
            .transpose()?;

        let decompressed_data = Self::decompress_payload(&header, chunks.as_deref(), &mut reader)?;
        Ok((header, decompressed_data))
    }

    /// Decompress the payload following the header (and, for v3, the index). With a
    /// chunk table the chunks are decompressed and concatenated.
    pub(crate) fn decompress_payload<R: Read + std::io::Seek>(
        header: &Header,
        chunks: Option<&[ChunkSpan]>,
        reader: &mut R,
    ) -> anyhow::Result<Vec<u8>> {
        let mut compressed_data = vec![0u8; header.get_compressed_size() as usize];
        reader.read_exact(&mut compressed_data)?;

        let Some(chunks) = chunks else {
            return decompress_chunk(
                header.compression,
                &compressed_data,
                header.decompressed_size as usize,
            );
        };

        let mut out = Vec::with_capacity(header.decompressed_size as usize);
        for chunk in chunks {
            let src = compressed_data
                .get(chunk.compressed_range())
                .context("CLUT chunk runs past the payload")?;
            out.extend_from_slice(&decompress_chunk(
                header.compression,
                src,
                chunk.decompressed_len as usize,
            )?);
        }
        ensure!(
            out.len() == header.decompressed_size as usize,
            "CLUT chunks decompress to {}, header says {}",
            out.len(),
            header.decompressed_size
        );
        Ok(out)
    }

    /// Read the decompressed data portion of a CLUT file
    fn read_decompressed_data<R: Read + std::io::Seek>(
        header: Header,
        reader: &mut R,
    ) -> anyhow::Result<Self> {
        use binrw::Endian;

        // Read patch versions
        let patch_len = i32::read_options(reader, Endian::Little, ())?;
        let mut patch_versions = Vec::with_capacity(patch_len as usize);
        for _i in 0..patch_len {
            let patch_str = NetString::read_options(reader, Endian::Little, ())?.0;
            patch_versions.push(PatchVersion::new(&patch_str)?);
        }

        // Read folders
        let folder_len = i32::read_options(reader, Endian::Little, ())?;
        let mut folders = HashSet::with_capacity(folder_len as usize);
        for _ in 0..folder_len {
            let folder = NetString::read_options(reader, Endian::Little, ())?.0;
            folders.insert(folder);
        }

        // Read file names
        let file_len = i32::read_options(reader, Endian::Little, ())?;
        let mut file_names = Vec::with_capacity(file_len as usize);
        for _ in 0..file_len {
            let file_name = NetString::read_options(reader, Endian::Little, ())?.0;
            file_names.push(file_name);
        }

        // Read file data
        let mut files = HashMap::with_capacity(file_len as usize);
        for file_name in file_names {
            let file_data = FileData::read_with_patches(reader, &patch_versions)?;
            files.insert(file_name, Arc::new(file_data));
        }

        Ok(Clut {
            header,
            folders,
            files,
        })
    }

    /// Get statistics about the CLUT file
    pub fn stats(&self) -> ClutStats {
        let total_data_refs = self.files.values().map(|file_data| file_data.len()).sum();

        let unique_patches: HashSet<_> = self
            .files
            .values()
            .flat_map(|file_data| file_data.as_slice())
            .map(|data_ref| data_ref.applied_version())
            .collect();

        ClutStats {
            folder_count: self.folders.len(),
            file_count: self.files.len(),
            total_data_refs,
            unique_patch_count: unique_patches.len(),
        }
    }
}

/// Statistics about a CLUT file
#[derive(Debug, Clone)]
pub struct ClutStats {
    pub folder_count: usize,
    pub file_count: usize,
    pub total_data_refs: usize,
    pub unique_patch_count: usize,
}

impl std::fmt::Display for ClutStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CLUT Stats: {} folders, {} files, {} data references, {} unique patches",
            self.folder_count, self.file_count, self.total_data_refs, self.unique_patch_count
        )
    }
}
