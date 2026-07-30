use crate::file::version::PatchVersion;

use super::clut_lazy::{ChunkSpan, Index};
use super::header::Header;
use super::types::{CompressType, Version};
use super::utils::NetString;
use anyhow::{Context, ensure};
use binrw::BinRead;
use brotli::Decompressor;
use flate2::read::ZlibDecoder;
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Seek};

/// A CLUT parsed for metadata only.
#[derive(Debug, Default, Clone)]
pub struct ClutIndex {
    pub folders: HashSet<String>,
    pub files: HashMap<String, u64>,
}

/// Chosen from a sweep over the base-game CLUT: at this level per-file chunking
/// costs 0.74% against a single zstd stream while cutting the worst-case read from
/// 99 ms to 3 ms. Levels below 19 lose several percent of ratio for no read win.
const ZSTD_LEVEL: i32 = 19;

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
pub(crate) fn compress_chunk(
    compression: CompressType,
    src: &[u8],
) -> anyhow::Result<Vec<u8>> {
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

/// Read the patch version, folder and file name sections that precede the per-file
/// data in the decompressed payload.
pub(crate) fn read_strings<R: Read + Seek>(
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

/// The header and the payload behind it, with a chunked payload reassembled into the
/// single stream an unchunked one holds.
pub fn decompress<R: Read + Seek>(mut reader: R) -> anyhow::Result<(Header, Vec<u8>)> {
    let header = Header::read_options(&mut reader, binrw::Endian::Little, ())?;

    // A v3 payload is split into independently compressed chunks, so reassembling it
    // needs the chunk table. The rest of the index only matters to readers decoding
    // one file at a time.
    let chunks = (header.file_version == Version::Indexed)
        .then(|| Index::read(&mut reader).map(|index| index.chunks))
        .transpose()?;

    let payload = decompress_payload(&header, chunks.as_deref(), &mut reader)?;
    Ok((header, payload))
}

/// Decompress the payload following the header (and, for v3, the index).
pub(crate) fn decompress_payload<R: Read + Seek>(
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
