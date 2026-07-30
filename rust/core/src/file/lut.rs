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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zipatch::{CompressedBlock, FileHeader};

    /// One of every chunk type, so the LUT round trip covers each payload layout.
    fn chunks() -> Vec<Chunk> {
        vec![
            Chunk::FileHeader(FileHeader {
                version: 3,
                patch_type: "HIST".to_string(),
                entry_files: 1,
                add_directories: 2,
                delete_directories: 3,
                delete_data_size: 0x1_0000_0007,
                minor_version: 5,
                repository_name: 0xdead_beef,
                commands: 7,
                sqpk_add_commands: 8,
                sqpk_delete_commands: 9,
                sqpk_expand_commands: 10,
                sqpk_header_commands: 11,
                sqpk_file_commands: 12,
            }),
            Chunk::ApplyOption { kind: 2, value: true },
            Chunk::ApplyFreeSpace { unknown_a: 0, unknown_b: 0 },
            Chunk::AddDirectory("movie/ffxiv".to_string()),
            Chunk::DeleteDirectory("sqpack/ex5".to_string()),
            Chunk::Xxxx,
            Chunk::SqpkAddData {
                target: "/sqpack/ffxiv/0a0000.%PLACEHOLDER%.dat0".to_string(),
                block_offset: 128,
                block_number: 256,
                block_delete_number: 384,
                patch_offset: 4096,
            },
            Chunk::SqpkDeleteData {
                target: "/sqpack/ex1/0a0000.%PLACEHOLDER%.dat1".to_string(),
                block_offset: 512,
                block_number: 128,
            },
            Chunk::SqpkExpandData {
                target: "/sqpack/ex2/0a0000.%PLACEHOLDER%.dat2".to_string(),
                block_offset: 640,
                block_number: 256,
            },
            Chunk::SqpkHeader {
                target: "/sqpack/ffxiv/000000.%PLACEHOLDER%.index".to_string(),
                header_kind: b'V',
                patch_offset: 8192,
            },
            Chunk::SqpkIndex,
            Chunk::SqpkPatchInfo { status: 1, version: 3, install_size: 1 << 40 },
            Chunk::SqpkTargetInfo {
                platform: 0,
                region: -1,
                is_debug: false,
                version: 2,
                deleted_data_size: 12345,
                seek_count: 678,
            },
            Chunk::SqpkFileAdd {
                target: "movie/ffxiv/00000.bk2".to_string(),
                file_offset: 0,
                blocks: vec![
                    // 32000 is the uncompressed sentinel; the second block is deflated.
                    CompressedBlock { compressed_size: 32000, data_size: 16000, patch_offset: 1024 },
                    CompressedBlock { compressed_size: 900, data_size: 16000, patch_offset: 17024 },
                ],
            },
            Chunk::SqpkFileDelExpac { expansion_id: 4 },
            Chunk::SqpkFileDelete {
                target: "sqpack/ex4/0a0000.%PLACEHOLDER%.dat0".to_string(),
            },
            Chunk::SqpkFileMkdir { target: "sqpack/ex5".to_string() },
            Chunk::EndOfFile,
        ]
    }

    #[test]
    fn every_chunk_type_round_trips_through_a_lut() {
        for compression in [
            CompressType::None,
            CompressType::Zlib,
            CompressType::Brotli,
            CompressType::Zstd,
        ] {
            let lut = Lut {
                compression,
                repository: "4e9a232b".to_string(),
                version: PatchVersion::new("D2025.11.03.0000.0001").unwrap(),
                chunks: chunks(),
            };
            let back = Lut::read(Cursor::new(lut.write().unwrap())).unwrap();

            assert_eq!(back.compression, compression);
            assert_eq!(back.repository, lut.repository);
            assert_eq!(back.version, lut.version);
            assert_eq!(back.chunks, lut.chunks, "{compression:?}");
        }
    }

    /// Names are shared across chunks and referenced by index, so a chunk must resolve
    /// its own name even when another chunk sorts between its entries.
    #[test]
    fn name_indices_survive_sorting() {
        let lut = Lut {
            compression: CompressType::None,
            repository: "4e9a232b".to_string(),
            version: PatchVersion::epoch(),
            chunks: vec![
                Chunk::AddDirectory("zzz".to_string()),
                Chunk::AddDirectory("aaa".to_string()),
                Chunk::SqpkFileMkdir { target: "mmm".to_string() },
                Chunk::AddDirectory("aaa".to_string()),
            ],
        };
        let back = Lut::read(Cursor::new(lut.write().unwrap())).unwrap();
        assert_eq!(back.chunks, lut.chunks);
    }
}
