use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use anyhow::{Context, Result, bail, ensure};
use binrw::{BinRead, BinWrite, Endian};

use crate::file::types::PlatformId;
use crate::file::utils::NetString;

/// A chunk's kind as stored in a LUT. The numbering is part of the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(repr = u8)]
#[repr(u8)]
pub enum ChunkType {
    AddDirectory = 0,
    ApplyFreeSpace = 1,
    ApplyOption = 2,
    DeleteDirectory = 3,
    EndOfFile = 4,
    FileHeader = 5,
    Xxxx = 6,

    SqpkAddData = 32,
    SqpkDeleteData = 33,
    SqpkExpandData = 34,
    SqpkHeader = 35,
    SqpkIndex = 36,
    SqpkPatchInfo = 37,
    SqpkTargetInfo = 38,

    SqpkFileAdd = 64,
    SqpkFileDelExpac = 65,
    SqpkFileDelete = 66,
    SqpkFileMkdir = 67,
}

/// The header of a `SqpkFileAdd` data block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedBlock {
    pub compressed_size: i32,
    pub data_size: i32,
    /// Absolute offset of the block's data within the patch file.
    pub patch_offset: i64,
}

impl CompressedBlock {
    /// A sentinel rather than a flag: an uncompressed block reports this exact size.
    pub fn is_compressed(&self) -> bool {
        self.compressed_size != 32000
    }

    fn read(reader: &mut Cursor<&[u8]>, base_offset: i64) -> Result<Self> {
        let start = reader.position();

        let header_size = i32::read_le(reader)?;
        let _pad = u32::read_le(reader)?;
        let compressed_size = i32::read_le(reader)?;
        let data_size = i32::read_le(reader)?;
        // The header may be longer than the four fields above.
        reader.seek(SeekFrom::Start(start + header_size as u64))?;

        let block = Self {
            compressed_size,
            data_size,
            patch_offset: base_offset + reader.position() as i64,
        };
        let payload = if block.is_compressed() {
            compressed_size
        } else {
            data_size
        };
        reader.seek(SeekFrom::Current(payload.into()))?;

        // Blocks are padded to a 128-byte multiple measured from the block's own
        // start.
        let read = reader.position() - start;
        reader.seek(SeekFrom::Start(start + read.next_multiple_of(128)))?;
        Ok(block)
    }

    fn write_lut<W: Write + Seek>(&self, writer: &mut W) -> Result<()> {
        self.compressed_size
            .write_options(writer, Endian::Little, ())?;
        self.data_size.write_options(writer, Endian::Little, ())?;
        self.patch_offset
            .write_options(writer, Endian::Little, ())?;
        Ok(())
    }

    fn read_lut<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        Ok(Self {
            compressed_size: i32::read_le(reader)?,
            data_size: i32::read_le(reader)?,
            patch_offset: i64::read_le(reader)?,
        })
    }
}

/// The `FHDR` chunk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileHeader {
    pub version: u8,
    pub patch_type: String,
    pub entry_files: u32,
    pub add_directories: u32,
    pub delete_directories: u32,
    pub delete_data_size: i64,
    pub minor_version: u32,
    pub repository_name: u32,
    pub commands: u32,
    pub sqpk_add_commands: u32,
    pub sqpk_delete_commands: u32,
    pub sqpk_expand_commands: u32,
    pub sqpk_header_commands: u32,
    pub sqpk_file_commands: u32,
}

/// One ZiPatch instruction, holding exactly what a LUT preserves: for data-bearing
/// chunks that is the target path and the offsets of the bytes inside the patch, never
/// the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    FileHeader(FileHeader),
    ApplyOption {
        kind: u8,
        value: bool,
    },
    ApplyFreeSpace {
        unknown_a: i64,
        unknown_b: i64,
    },
    AddDirectory(String),
    DeleteDirectory(String),
    EndOfFile,
    Xxxx,

    SqpkAddData {
        target: String,
        block_offset: i64,
        block_number: i64,
        block_delete_number: i64,
        patch_offset: i64,
    },
    SqpkDeleteData {
        target: String,
        block_offset: i64,
        block_number: i64,
    },
    SqpkExpandData {
        target: String,
        block_offset: i64,
        block_number: i64,
    },
    SqpkHeader {
        target: String,
        header_kind: u8,
        patch_offset: i64,
    },
    SqpkIndex,
    SqpkPatchInfo {
        status: u8,
        version: u8,
        install_size: u64,
    },
    SqpkTargetInfo {
        platform: u16,
        region: i16,
        is_debug: bool,
        version: u16,
        deleted_data_size: u64,
        seek_count: u64,
    },

    SqpkFileAdd {
        target: String,
        file_offset: i64,
        blocks: Vec<CompressedBlock>,
    },
    SqpkFileDelExpac {
        expansion_id: u16,
    },
    SqpkFileDelete {
        target: String,
    },
    SqpkFileMkdir {
        target: String,
    },
}

pub const HEADER_SIZE: i64 = 1024;

const VERSION_HEADER: u8 = b'V';

impl Chunk {
    pub(super) fn read(fourcc: &str, body: &[u8], base_offset: i64) -> Result<Self> {
        let reader = &mut Cursor::new(body);
        match fourcc {
            "FHDR" => Self::read_file_header(reader),
            "APLY" => {
                let kind = u32::read_be(reader)?;
                let mut pad = [0u8; 4];
                reader.read_exact(&mut pad)?;
                let value = u32::read_be(reader)? != 0;
                Ok(Self::ApplyOption {
                    kind: kind as u8,
                    // Anything other than IgnoreMissing or IgnoreOldMismatch is off.
                    value: matches!(kind, 1 | 2) && value,
                })
            }
            "APFS" => Ok(Self::ApplyFreeSpace {
                unknown_a: i64::read_be(reader)?,
                unknown_b: i64::read_be(reader)?,
            }),
            "ADIR" => Ok(Self::AddDirectory(read_be_string(reader)?)),
            "DELD" => Ok(Self::DeleteDirectory(read_be_string(reader)?)),
            "EOF_" => Ok(Self::EndOfFile),
            "XXXX" => Ok(Self::Xxxx),
            "SQPK" => Self::read_sqpk(reader, body.len(), base_offset),
            other => bail!("Invalid chunk type {other}"),
        }
    }

    fn read_file_header(reader: &mut Cursor<&[u8]>) -> Result<Self> {
        // The only little-endian field in a ZiPatch.
        let version = (u32::read_le(reader)? >> 16) as u8;
        let mut header = FileHeader {
            version,
            patch_type: read_ascii(reader, 4)?,
            entry_files: u32::read_be(reader)?,
            ..Default::default()
        };

        if version == 3 {
            header.add_directories = u32::read_be(reader)?;
            header.delete_directories = u32::read_be(reader)?;
            header.delete_data_size =
                i64::from(u32::read_be(reader)?) | (i64::from(u32::read_be(reader)?) << 32);
            header.minor_version = u32::read_be(reader)?;
            header.repository_name = u32::read_be(reader)?;
            header.commands = u32::read_be(reader)?;
            header.sqpk_add_commands = u32::read_be(reader)?;
            header.sqpk_delete_commands = u32::read_be(reader)?;
            header.sqpk_expand_commands = u32::read_be(reader)?;
            header.sqpk_header_commands = u32::read_be(reader)?;
            header.sqpk_file_commands = u32::read_be(reader)?;
        }

        Ok(Self::FileHeader(header))
    }

    fn read_sqpk(reader: &mut Cursor<&[u8]>, body_len: usize, base_offset: i64) -> Result<Self> {
        let inner_size = i32::read_be(reader)?;
        ensure!(
            inner_size as usize == body_len,
            "Sqpk size mismatch: {inner_size} != {body_len}"
        );

        let mut command = [0u8; 1];
        reader.read_exact(&mut command)?;
        match command[0] {
            b'A' => {
                skip(reader, 3)?;
                let target = read_dat_path(reader)?;
                let block_offset = read_block_count(reader)?;
                let block_number = read_block_count(reader)?;
                let block_delete_number = read_block_count(reader)?;
                Ok(Self::SqpkAddData {
                    target,
                    block_offset,
                    block_number,
                    block_delete_number,
                    patch_offset: base_offset + reader.position() as i64,
                })
            }
            b'D' => {
                skip(reader, 3)?;
                Ok(Self::SqpkDeleteData {
                    target: read_dat_path(reader)?,
                    block_offset: read_block_count(reader)?,
                    block_number: read_block_count(reader)?,
                })
            }
            b'E' => {
                skip(reader, 3)?;
                Ok(Self::SqpkExpandData {
                    target: read_dat_path(reader)?,
                    block_offset: read_block_count(reader)?,
                    block_number: read_block_count(reader)?,
                })
            }
            b'H' => {
                let file_kind = read_u8(reader)?;
                let header_kind = read_u8(reader)?;
                skip(reader, 1)?;
                let target = if file_kind == b'D' {
                    read_dat_path(reader)?
                } else {
                    read_index_path(reader)?
                };
                Ok(Self::SqpkHeader {
                    target,
                    header_kind,
                    patch_offset: base_offset + reader.position() as i64,
                })
            }
            b'I' => Ok(Self::SqpkIndex),
            b'X' => {
                let status = read_u8(reader)?;
                let version = read_u8(reader)?;
                skip(reader, 1)?;
                Ok(Self::SqpkPatchInfo {
                    status,
                    version,
                    install_size: u64::read_be(reader)?,
                })
            }
            b'T' => {
                skip(reader, 3)?;
                Ok(Self::SqpkTargetInfo {
                    platform: u16::read_be(reader)?,
                    region: i16::read_be(reader)?,
                    is_debug: i16::read_be(reader)? != 0,
                    version: u16::read_be(reader)?,
                    deleted_data_size: u64::read_le(reader)?,
                    seek_count: u64::read_le(reader)?,
                })
            }
            b'F' => Self::read_sqpk_file(reader, body_len, base_offset),
            other => bail!("Invalid sqpk command {}", other as char),
        }
    }

    fn read_sqpk_file(
        reader: &mut Cursor<&[u8]>,
        body_len: usize,
        base_offset: i64,
    ) -> Result<Self> {
        let operation = read_u8(reader)?;
        skip(reader, 2)?;

        let file_offset = i64::read_be(reader)?;
        let _file_size = i64::read_be(reader)?;
        let path_len = i32::read_be(reader)?;
        let expansion_id = u16::read_be(reader)?;
        skip(reader, 2)?;
        let target = read_ascii(reader, path_len as usize)?;

        match operation {
            b'A' => {
                let mut blocks = Vec::new();
                while (reader.position() as usize) < body_len {
                    blocks.push(CompressedBlock::read(reader, base_offset)?);
                }
                Ok(Self::SqpkFileAdd {
                    target,
                    file_offset,
                    blocks,
                })
            }
            b'R' => Ok(Self::SqpkFileDelExpac { expansion_id }),
            b'D' => Ok(Self::SqpkFileDelete { target }),
            b'M' => Ok(Self::SqpkFileMkdir { target }),
            other => bail!("Operation {} is not supported.", other as char),
        }
    }

    pub fn write_lut(&self) -> Result<(ChunkType, Vec<String>, Vec<u8>)> {
        let mut names = Vec::new();
        let mut data = Cursor::new(Vec::new());
        let w = &mut data;
        let le = Endian::Little;

        let kind = match self {
            Self::AddDirectory(dir) => {
                names.push(dir.clone());
                ChunkType::AddDirectory
            }
            Self::DeleteDirectory(dir) => {
                names.push(dir.clone());
                ChunkType::DeleteDirectory
            }
            Self::ApplyFreeSpace { .. } => ChunkType::ApplyFreeSpace,
            Self::EndOfFile => ChunkType::EndOfFile,
            Self::Xxxx => ChunkType::Xxxx,
            Self::SqpkIndex => ChunkType::SqpkIndex,
            Self::ApplyOption { kind, value } => {
                kind.write_options(w, le, ())?;
                u8::from(*value).write_options(w, le, ())?;
                ChunkType::ApplyOption
            }
            Self::FileHeader(header) => {
                header.version.write_options(w, le, ())?;
                NetString(header.patch_type.clone()).write_options(w, le, ())?;
                for value in [
                    header.entry_files,
                    header.add_directories,
                    header.delete_directories,
                ] {
                    value.write_options(w, le, ())?;
                }
                header.delete_data_size.write_options(w, le, ())?;
                for value in [
                    header.minor_version,
                    header.repository_name,
                    header.commands,
                    header.sqpk_add_commands,
                    header.sqpk_delete_commands,
                    header.sqpk_expand_commands,
                    header.sqpk_header_commands,
                    header.sqpk_file_commands,
                ] {
                    value.write_options(w, le, ())?;
                }
                ChunkType::FileHeader
            }
            Self::SqpkAddData {
                target,
                block_offset,
                block_number,
                block_delete_number,
                patch_offset,
            } => {
                names.push(target.clone());
                for value in [
                    block_offset,
                    block_number,
                    block_delete_number,
                    patch_offset,
                ] {
                    value.write_options(w, le, ())?;
                }
                ChunkType::SqpkAddData
            }
            Self::SqpkDeleteData {
                target,
                block_offset,
                block_number,
            } => {
                names.push(target.clone());
                block_offset.write_options(w, le, ())?;
                block_number.write_options(w, le, ())?;
                ChunkType::SqpkDeleteData
            }
            Self::SqpkExpandData {
                target,
                block_offset,
                block_number,
            } => {
                names.push(target.clone());
                block_offset.write_options(w, le, ())?;
                block_number.write_options(w, le, ())?;
                ChunkType::SqpkExpandData
            }
            Self::SqpkHeader {
                target,
                header_kind,
                patch_offset,
            } => {
                names.push(target.clone());
                header_kind.write_options(w, le, ())?;
                patch_offset.write_options(w, le, ())?;
                ChunkType::SqpkHeader
            }
            Self::SqpkPatchInfo {
                status,
                version,
                install_size,
            } => {
                status.write_options(w, le, ())?;
                version.write_options(w, le, ())?;
                install_size.write_options(w, le, ())?;
                ChunkType::SqpkPatchInfo
            }
            Self::SqpkTargetInfo {
                platform,
                region,
                is_debug,
                version,
                deleted_data_size,
                seek_count,
            } => {
                platform.write_options(w, le, ())?;
                region.write_options(w, le, ())?;
                u8::from(*is_debug).write_options(w, le, ())?;
                version.write_options(w, le, ())?;
                deleted_data_size.write_options(w, le, ())?;
                seek_count.write_options(w, le, ())?;
                ChunkType::SqpkTargetInfo
            }
            Self::SqpkFileAdd {
                target,
                file_offset,
                blocks,
            } => {
                names.push(target.clone());
                file_offset.write_options(w, le, ())?;
                (blocks.len() as i32).write_options(w, le, ())?;
                for block in blocks {
                    block.write_lut(w)?;
                }
                ChunkType::SqpkFileAdd
            }
            Self::SqpkFileDelExpac { expansion_id } => {
                expansion_id.write_options(w, le, ())?;
                ChunkType::SqpkFileDelExpac
            }
            Self::SqpkFileDelete { target } => {
                names.push(target.clone());
                ChunkType::SqpkFileDelete
            }
            Self::SqpkFileMkdir { target } => {
                names.push(target.clone());
                ChunkType::SqpkFileMkdir
            }
        };

        Ok((kind, names, data.into_inner()))
    }

    pub fn read_lut(kind: ChunkType, names: &[String], data: &[u8]) -> Result<Self> {
        let reader = &mut Cursor::new(data);
        let name = |i: usize| -> Result<String> {
            names
                .get(i)
                .cloned()
                .with_context(|| format!("{kind:?} chunk is missing name {i}"))
        };

        Ok(match kind {
            ChunkType::AddDirectory => Self::AddDirectory(name(0)?),
            ChunkType::DeleteDirectory => Self::DeleteDirectory(name(0)?),
            ChunkType::ApplyFreeSpace => Self::ApplyFreeSpace {
                unknown_a: 0,
                unknown_b: 0,
            },
            ChunkType::EndOfFile => Self::EndOfFile,
            ChunkType::Xxxx => Self::Xxxx,
            ChunkType::SqpkIndex => Self::SqpkIndex,
            ChunkType::ApplyOption => Self::ApplyOption {
                kind: read_u8(reader)?,
                value: read_u8(reader)? != 0,
            },
            ChunkType::FileHeader => Self::FileHeader(FileHeader {
                version: read_u8(reader)?,
                patch_type: NetString::read_le(reader)?.0,
                entry_files: u32::read_le(reader)?,
                add_directories: u32::read_le(reader)?,
                delete_directories: u32::read_le(reader)?,
                delete_data_size: i64::read_le(reader)?,
                minor_version: u32::read_le(reader)?,
                repository_name: u32::read_le(reader)?,
                commands: u32::read_le(reader)?,
                sqpk_add_commands: u32::read_le(reader)?,
                sqpk_delete_commands: u32::read_le(reader)?,
                sqpk_expand_commands: u32::read_le(reader)?,
                sqpk_header_commands: u32::read_le(reader)?,
                sqpk_file_commands: u32::read_le(reader)?,
            }),
            ChunkType::SqpkAddData => Self::SqpkAddData {
                target: name(0)?,
                block_offset: i64::read_le(reader)?,
                block_number: i64::read_le(reader)?,
                block_delete_number: i64::read_le(reader)?,
                patch_offset: i64::read_le(reader)?,
            },
            ChunkType::SqpkDeleteData => Self::SqpkDeleteData {
                target: name(0)?,
                block_offset: i64::read_le(reader)?,
                block_number: i64::read_le(reader)?,
            },
            ChunkType::SqpkExpandData => Self::SqpkExpandData {
                target: name(0)?,
                block_offset: i64::read_le(reader)?,
                block_number: i64::read_le(reader)?,
            },
            ChunkType::SqpkHeader => Self::SqpkHeader {
                target: name(0)?,
                header_kind: read_u8(reader)?,
                patch_offset: i64::read_le(reader)?,
            },
            ChunkType::SqpkPatchInfo => Self::SqpkPatchInfo {
                status: read_u8(reader)?,
                version: read_u8(reader)?,
                install_size: u64::read_le(reader)?,
            },
            ChunkType::SqpkTargetInfo => Self::SqpkTargetInfo {
                platform: u16::read_le(reader)?,
                region: i16::read_le(reader)?,
                is_debug: read_u8(reader)? != 0,
                version: u16::read_le(reader)?,
                deleted_data_size: u64::read_le(reader)?,
                seek_count: u64::read_le(reader)?,
            },
            ChunkType::SqpkFileAdd => {
                let target = name(0)?;
                let file_offset = i64::read_le(reader)?;
                let count = i32::read_le(reader)?;
                ensure!(count >= 0, "negative block count");
                Self::SqpkFileAdd {
                    target,
                    file_offset,
                    blocks: (0..count)
                        .map(|_| CompressedBlock::read_lut(reader))
                        .collect::<Result<_>>()?,
                }
            }
            ChunkType::SqpkFileDelExpac => Self::SqpkFileDelExpac {
                expansion_id: u16::read_le(reader)?,
            },
            ChunkType::SqpkFileDelete => Self::SqpkFileDelete { target: name(0)? },
            ChunkType::SqpkFileMkdir => Self::SqpkFileMkdir { target: name(0)? },
        })
    }

    pub fn is_version_header(header_kind: u8) -> bool {
        header_kind == VERSION_HEADER
    }
}

pub fn platform_name(platform: PlatformId) -> &'static str {
    match platform {
        PlatformId::Win32 => "win32",
        PlatformId::Ps3 => "ps3",
        PlatformId::Ps4 => "ps4",
        PlatformId::Ps5 => "ps5",
        PlatformId::Lys => "lys",
        PlatformId::Placeholder => PLACEHOLDER,
        PlatformId::Unknown => "unknown",
    }
}

const PLACEHOLDER: &str = "%PLACEHOLDER%";

pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_string()
}

pub fn resolve_platform(path: &str, platform: PlatformId) -> String {
    normalize_path(&path.replace(PLACEHOLDER, platform_name(platform)))
}

pub fn expansion_folder(expansion_id: u16) -> String {
    if expansion_id == 0 {
        "ffxiv".to_string()
    } else {
        format!("ex{expansion_id}")
    }
}

/// `/sqpack/<expansion>/<main><sub>.<platform>`, the stem both dat and index files
/// extend.
fn read_sqpack_stem(reader: &mut Cursor<&[u8]>) -> Result<(String, u32)> {
    let main_id = u16::read_be(reader)?;
    let sub_id = u16::read_be(reader)?;
    let file_id = u32::read_be(reader)?;
    let expansion = expansion_folder(sub_id >> 8);
    Ok((
        format!("/sqpack/{expansion}/{main_id:02x}{sub_id:04x}.{PLACEHOLDER}"),
        file_id,
    ))
}

fn read_dat_path(reader: &mut Cursor<&[u8]>) -> Result<String> {
    let (stem, file_id) = read_sqpack_stem(reader)?;
    Ok(format!("{stem}.dat{file_id}"))
}

fn read_index_path(reader: &mut Cursor<&[u8]>) -> Result<String> {
    let (stem, file_id) = read_sqpack_stem(reader)?;
    let suffix = if file_id == 0 {
        String::new()
    } else {
        file_id.to_string()
    };
    Ok(format!("{stem}.index{suffix}"))
}

fn read_block_count(reader: &mut Cursor<&[u8]>) -> Result<i64> {
    Ok(i64::from(u32::read_be(reader)?) << 7)
}

fn read_u8(reader: &mut Cursor<&[u8]>) -> Result<u8> {
    Ok(u8::read_le(reader)?)
}

fn skip(reader: &mut Cursor<&[u8]>, count: i64) -> Result<()> {
    reader.seek(SeekFrom::Current(count))?;
    Ok(())
}

fn read_be_string(reader: &mut Cursor<&[u8]>) -> Result<String> {
    let len = i32::read_be(reader)?;
    ensure!(len >= 0, "negative string length");
    read_ascii(reader, len as usize)
}

/// Fixed-length, null-padded, and ASCII: a byte above 0x7F becomes `?`, as the C#
/// implementation's decoder produces.
fn read_ascii(reader: &mut Cursor<&[u8]>, len: usize) -> Result<String> {
    let mut bytes = vec![0u8; len];
    reader.read_exact(&mut bytes)?;
    let end = bytes.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
    Ok(bytes[..end]
        .iter()
        .map(|&b| if b < 0x80 { b as char } else { '?' })
        .collect())
}
