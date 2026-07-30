use std::collections::BTreeSet;
use std::io::{Cursor, Read, Seek, Write};

use anyhow::{Context, Result, ensure};
use binrw::{BinRead, BinWrite, Endian};

use super::clut::compress_chunk;
use super::types::CompressType;
use super::utils::NetString;
use super::version::PatchVersion;
use crate::zipatch::{Chunk, ChunkType};

const MAGIC: u16 = 0xDE22;

/// LUT file version. Unlike a CLUT there is only one readable version.
const VERSION: u16 = 2;

/// One patch's chunks, with the data they reference left in the patch file.
#[derive(Debug, Clone)]
pub struct Lut {
    pub compression: CompressType,
    pub repository: String,
    pub version: PatchVersion,
    pub chunks: Vec<Chunk>,
}

impl Lut {
    pub fn read<R: Read + Seek>(mut reader: R) -> Result<Self> {
        let magic = u16::read_le(&mut reader)?;
        ensure!(magic == MAGIC, "Invalid magic: {magic:04X}");
        let version = u16::read_le(&mut reader)?;
        ensure!(version == VERSION, "Unsupported version: {version}");

        let compression = CompressType::read_le(&mut reader)?;
        let repository = NetString::read_le(&mut reader)?.0;
        let version = PatchVersion::new(&NetString::read_le(&mut reader)?.0)?;

        // A LUT header records no payload size, so the rest of the stream is the
        // payload and its decompressed length is unknown until it is inflated.
        let mut compressed = Vec::new();
        reader.read_to_end(&mut compressed)?;
        let payload = inflate(compression, &compressed)?;

        Ok(Self {
            compression,
            repository,
            version,
            chunks: Self::read_chunks(&payload)?,
        })
    }

    fn read_chunks(payload: &[u8]) -> Result<Vec<Chunk>> {
        let reader = &mut Cursor::new(payload);

        let name_count = i32::read_le(reader)?;
        ensure!(name_count >= 0, "negative name count");
        let names = (0..name_count)
            .map(|_| Ok(NetString::read_le(reader)?.0))
            .collect::<Result<Vec<_>>>()?;

        let chunk_count = i32::read_le(reader)?;
        ensure!(chunk_count >= 0, "negative chunk count");
        (0..chunk_count)
            .map(|_| {
                let kind = ChunkType::read_le(reader)?;
                let index_count = i32::read_le(reader)?;
                ensure!(index_count >= 0, "negative name index count");
                let chunk_names = (0..index_count)
                    .map(|_| {
                        let index = i32::read_le(reader)?;
                        names
                            .get(usize::try_from(index)?)
                            .cloned()
                            .with_context(|| format!("name index {index} out of range"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let data_len = i32::read_le(reader)?;
                ensure!(data_len >= 0, "negative chunk data length");
                let mut data = vec![0u8; data_len as usize];
                reader.read_exact(&mut data)?;
                Chunk::read_lut(kind, &chunk_names, &data)
            })
            .collect()
    }

    pub fn write(&self) -> Result<Vec<u8>> {
        let payload = self.write_chunks()?;
        let mut out = Cursor::new(Vec::new());
        MAGIC.write_options(&mut out, Endian::Little, ())?;
        VERSION.write_options(&mut out, Endian::Little, ())?;
        self.compression
            .write_options(&mut out, Endian::Little, ())?;
        NetString(self.repository.clone()).write_options(&mut out, Endian::Little, ())?;
        NetString(self.version.to_string()).write_options(&mut out, Endian::Little, ())?;
        out.write_all(&compress_chunk(self.compression, &payload)?)?;
        Ok(out.into_inner())
    }

    fn write_chunks(&self) -> Result<Vec<u8>> {
        let encoded = self
            .chunks
            .iter()
            .map(Chunk::write_lut)
            .collect::<Result<Vec<_>>>()?;

        // Chunks reference names by index into this sorted table.
        let names: Vec<&String> = encoded
            .iter()
            .flat_map(|(_, names, _)| names)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let mut out = Cursor::new(Vec::new());
        let le = Endian::Little;
        (names.len() as i32).write_options(&mut out, le, ())?;
        for name in &names {
            NetString((*name).clone()).write_options(&mut out, le, ())?;
        }

        (encoded.len() as i32).write_options(&mut out, le, ())?;
        for (kind, chunk_names, data) in &encoded {
            kind.write_options(&mut out, le, ())?;
            (chunk_names.len() as i32).write_options(&mut out, le, ())?;
            for name in chunk_names {
                let index = names
                    .binary_search(&name)
                    .expect("every chunk name is in the table");
                (index as i32).write_options(&mut out, le, ())?;
            }
            (data.len() as i32).write_options(&mut out, le, ())?;
            out.write_all(data)?;
        }
        Ok(out.into_inner())
    }
}

/// Decompress a payload of unknown length.
fn inflate(compression: CompressType, src: &[u8]) -> Result<Vec<u8>> {
    Ok(match compression {
        CompressType::None => src.to_vec(),
        CompressType::Zlib => {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(src).read_to_end(&mut out)?;
            out
        }
        CompressType::Brotli => {
            let mut out = Vec::new();
            brotli::Decompressor::new(src, 8192).read_to_end(&mut out)?;
            out
        }
        CompressType::Zstd => {
            let mut out = Vec::new();
            zstd::Decoder::new(src)?.read_to_end(&mut out)?;
            out
        }
    })
}
