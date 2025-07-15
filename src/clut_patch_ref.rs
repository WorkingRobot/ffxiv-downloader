use crate::binary::{VarInt32, VarInt64};
use binrw::{BinRead, BinWrite};

/// Reference to patch data within a patch file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClutPatchRef {
    /// Offset in the patch file (delta-encoded)
    pub offset_delta: VarInt64,

    /// Size of the patch data
    pub size: VarInt32,

    /// Whether the patch data is compressed
    pub is_compressed: u8, // Using u8 instead of bool for binrw compatibility
}

impl BinRead for ClutPatchRef {
    type Args<'a> = ();

    fn read_options<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let offset_delta = VarInt64::read_options(reader, endian, ())?;
        let size = VarInt32::read_options(reader, endian, ())?;
        let is_compressed = u8::read_options(reader, endian, ())?;

        Ok(ClutPatchRef {
            offset_delta,
            size,
            is_compressed,
        })
    }
}

impl BinWrite for ClutPatchRef {
    type Args<'a> = ();

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        self.offset_delta.write_options(writer, endian, ())?;
        self.size.write_options(writer, endian, ())?;
        self.is_compressed.write_options(writer, endian, ())?;
        Ok(())
    }
}

impl ClutPatchRef {
    /// Calculate the absolute offset given the previous offset
    pub fn absolute_offset(&self, prev_offset: i64) -> i64 {
        prev_offset + self.offset_delta.0
    }

    /// Get the end offset given the previous offset
    pub fn end_offset(&self, prev_offset: i64) -> i64 {
        self.absolute_offset(prev_offset) + self.size.0 as i64
    }

    /// Check if the patch data is compressed
    pub fn is_compressed(&self) -> bool {
        self.is_compressed != 0
    }
}
