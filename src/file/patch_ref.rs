use crate::file::utils::VarUInt32;

use super::utils::VarInt64;
use binrw::{BinRead, BinWrite};

/// Reference to patch data within a patch file
/// Fields match exactly what the C# ClutPatchRef auto-properties expose
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PatchRef {
    /// Absolute offset in the patch file (computed from delta during deserialization)
    pub offset: u64,

    /// Size of the patch data (extracted from VarInt32 during deserialization)
    pub size: u32,

    /// Whether the patch data is compressed (converted from u8 during deserialization)
    pub is_compressed: bool,
}

impl BinRead for PatchRef {
    type Args<'a> = (&'a mut u64,); // previous offset for delta decoding

    fn read_options<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        (prev_offset,): Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let offset_delta = VarInt64::read_options(reader, endian, ())?;
        let size_varint = VarUInt32::read_options(reader, endian, ())?;
        let is_compressed_byte = u8::read_options(reader, endian, ())?;

        // Convert from disk format to C# struct format
        let absolute_offset = prev_offset
            .checked_add_signed(offset_delta.0)
            .ok_or_else(|| binrw::Error::Custom {
                pos: 0,
                err: Box::new("Offset overflow".to_string()),
            })?;
        *prev_offset = absolute_offset; // Update for next delta calculation

        Ok(PatchRef {
            offset: absolute_offset,
            size: size_varint.0,
            is_compressed: is_compressed_byte != 0,
        })
    }
}

impl BinWrite for PatchRef {
    type Args<'a> = (&'a mut u64,); // previous offset for delta encoding

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        (prev_offset,): Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        // Convert from C# struct format back to disk format
        let offset_delta = self.offset.wrapping_sub(*prev_offset) as i64;
        *prev_offset = self.offset; // Update for next delta calculation

        VarInt64(offset_delta).write_options(writer, endian, ())?;
        VarUInt32(self.size).write_options(writer, endian, ())?;
        (self.is_compressed as u8).write_options(writer, endian, ())?;
        Ok(())
    }
}

impl PatchRef {}
