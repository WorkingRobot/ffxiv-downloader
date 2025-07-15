use crate::binary::VarInt32;
use crate::clut_patch_ref::ClutPatchRef;
use crate::types::PatchVersion;
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

/// Reference to data within a file, either from a patch or as zero/empty blocks
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClutDataRef {
    /// Type of reference
    pub ref_type: RefType,

    /// Index into the patch version array
    pub applied_version_index: i32,

    /// Number of blocks (only for EmptyBlock type)
    pub block_count: Option<i32>,

    /// Reference to patch data (only for Patch type)
    pub patch: Option<ClutPatchRef>,

    /// Patch offset (only for non-FullPatch type)
    pub patch_offset: Option<i32>,

    /// File offset (read separately)
    pub offset: i64,

    /// Data length (read separately)
    pub length: i32,
}

impl BinRead for ClutDataRef {
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
        let applied_version_index =
            if version_index >= 0 && (version_index as usize) < patch_versions.len() {
                version_index
            } else {
                return Err(binrw::Error::Custom {
                    pos: 0,
                    err: Box::new(format!("Invalid patch version index: {}", version_index)),
                });
            };

        let block_count = if ref_type == RefType::EmptyBlock {
            Some(i32::read_options(reader, endian, ())?)
        } else {
            None
        };

        let patch = if ref_type == RefType::Patch {
            Some(ClutPatchRef::read_options(reader, endian, ())?)
        } else {
            None
        };

        // Update patch offset tracker if we have patch data
        if let Some(ref patch_ref) = patch {
            *patch_offset_tracker = patch_ref.absolute_offset(*patch_offset_tracker);
        }

        let patch_offset = if raw_type == 0 {
            Some(VarInt32::read_options(reader, endian, ())?.0)
        } else {
            None
        };

        Ok(ClutDataRef {
            ref_type,
            applied_version_index,
            block_count,
            patch,
            patch_offset,
            offset: 0, // Will be read separately
            length: 0, // Will be read separately
        })
    }
}

impl BinWrite for ClutDataRef {
    type Args<'a> = (&'a mut i64,); // patch_offset tracker

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        (patch_offset_tracker,): Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        let type_to_write =
            if self.ref_type == RefType::Patch && self.patch_offset.unwrap_or(0) == 0 {
                0xFF
            } else {
                self.ref_type as u8
            };

        type_to_write.write_options(writer, endian, ())?;
        VarInt32(self.applied_version_index).write_options(writer, endian, ())?;

        if self.ref_type == RefType::EmptyBlock {
            self.block_count
                .unwrap()
                .write_options(writer, endian, ())?;
        }

        if self.ref_type == RefType::Patch {
            if let Some(ref patch_ref) = self.patch {
                patch_ref.write_options(writer, endian, ())?;
                *patch_offset_tracker = patch_ref.absolute_offset(*patch_offset_tracker);
            }
        }

        if type_to_write == 0 {
            VarInt32(self.patch_offset.unwrap_or(0)).write_options(writer, endian, ())?;
        }

        Ok(())
    }
}
