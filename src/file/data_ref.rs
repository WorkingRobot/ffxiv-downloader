use super::patch_ref::PatchRef;
use super::types::PatchVersion;
use super::utils::VarInt32;
use binrw::{BinRead, BinWrite};

/// Type of data reference in a CLUT file
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
pub enum RefType {
    Patch = 0,
    Zero = 1,
    EmptyBlock = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DataRefType {
    /// Patch data reference
    Patch {
        /// Reference to patch data
        patch: PatchRef,
        /// Patch offset (only for non-FullPatch type)
        patch_offset: Option<i32>,
    },
    /// Zero-filled blocks
    Zero {},
    /// Empty blocks
    EmptyBlock { block_count: i32 },
}

/// Reference to data within a file, represented as an enum matching C# ClutDataRef variants
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRef {
    /// Patch version where this data reference is from
    applied_version: PatchVersion,
    /// File offset (read separately in phase 2)
    offset: i64,
    /// Data length (read separately in phase 3)
    length: i32,
    /// Typed Data
    ref_type: DataRefType,
}

impl DataRef {
    /// Get the applied version index for any variant
    pub fn applied_version(&self) -> &PatchVersion {
        return &self.applied_version;
    }

    /// Get the file offset for any variant
    pub fn offset(&self) -> i64 {
        return self.offset;
    }

    /// Get the data length for any variant
    pub fn length(&self) -> i32 {
        return self.length;
    }

    /// Set the offset for any variant (used during phase 2 reading)
    pub fn set_offset(&mut self, new_offset: i64) {
        self.offset = new_offset;
    }

    /// Set the length for any variant (used during phase 3 reading)
    pub fn set_length(&mut self, new_length: i32) {
        self.length = new_length;
    }
}

impl BinRead for DataRef {
    type Args<'a> = (&'a mut i64, &'a [PatchVersion]); // patch_offset tracker, patch versions

    fn read_options<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        (patch_offset_tracker, patch_versions): Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let raw_type = u8::read_options(reader, endian, ())?;

        let ref_type = match raw_type {
            0 | 0xFF => RefType::Patch,
            1 => RefType::Zero,
            2 => RefType::EmptyBlock,
            _ => {
                return Err(binrw::Error::Custom {
                    pos: 0,
                    err: Box::new(format!("Invalid ref type: {}", raw_type)),
                });
            }
        };

        let version_index_varint = VarInt32::read_options(reader, endian, ())?;
        let version_index = version_index_varint.0;

        // Look up the actual patch version from the index
        let applied_version =
            if version_index >= 0 && (version_index as usize) < patch_versions.len() {
                patch_versions[version_index as usize].clone()
            } else {
                return Err(binrw::Error::Custom {
                    pos: 0,
                    err: Box::new(format!("Invalid patch version index: {}", version_index)),
                });
            };

        let mut ret = DataRef {
            applied_version,
            offset: 0,                      // Will be set in phase 2
            length: 0,                      // Will be set in phase 3
            ref_type: DataRefType::Zero {}, // Default, will be overwritten
        };

        ret.ref_type = match ref_type {
            RefType::Patch => {
                let patch = PatchRef::read_options(reader, endian, (patch_offset_tracker,))?;
                let patch_offset = if raw_type == 0 {
                    Some(VarInt32::read_options(reader, endian, ())?.0)
                } else {
                    None
                };

                DataRefType::Patch {
                    patch,
                    patch_offset,
                }
            }
            RefType::Zero => DataRefType::Zero {},
            RefType::EmptyBlock => {
                let block_count = i32::read_options(reader, endian, ())?;
                DataRefType::EmptyBlock { block_count }
            }
        };
        Ok(ret)
    }
}

impl BinWrite for DataRef {
    type Args<'a> = (&'a mut i64, &'a [PatchVersion]); // patch_offset tracker

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        (patch_offset_tracker, patch_versions): Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        let type_to_write = match self.ref_type {
            DataRefType::Patch { patch_offset, .. } => {
                if patch_offset.unwrap_or(0) == 0 {
                    0xFF // FullPatch
                } else {
                    0 // Non-FullPatch
                }
            }
            DataRefType::Zero {} => RefType::Zero as u8,
            DataRefType::EmptyBlock { .. } => RefType::EmptyBlock as u8,
        };
        type_to_write.write_options(writer, endian, ())?;

        let version_index = patch_versions
            .iter()
            .position(|v| v == &self.applied_version)
            .ok_or_else(|| binrw::Error::Custom {
                pos: 0,
                err: Box::new(format!("Patch version not found in versions list")),
            })? as i32;

        VarInt32(version_index).write_options(writer, endian, ())?;
        match &self.ref_type {
            DataRefType::Patch {
                patch,
                patch_offset,
            } => {
                patch.write_options(writer, endian, (patch_offset_tracker,))?;
                if type_to_write == 0 {
                    VarInt32(patch_offset.unwrap_or(0)).write_options(writer, endian, ())?;
                }
            }
            DataRefType::Zero {} => {}
            DataRefType::EmptyBlock { block_count } => {
                block_count.write_options(writer, endian, ())?;
            }
        }
        Ok(())
    }
}
