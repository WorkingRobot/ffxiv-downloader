use crate::file::utils::VarUInt32;

use super::data_ref::DataRef;
use super::utils::VarInt64;
use super::version::PatchVersion;
use binrw::{BinRead, BinWrite};

/// File data within a CLUT, containing references to data blocks
pub struct FileData;

impl FileData {
    /// Read file data with patch version mapping
    pub fn read_with_patches<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        patch_versions: &[PatchVersion],
    ) -> binrw::BinResult<Vec<DataRef>> {
        use binrw::Endian;

        let data_size = i32::read_options(reader, Endian::Little, ())?;

        let mut data = Vec::with_capacity(data_size as usize);
        let mut patch_offset_tracker = 0u64;

        // Phase 1: Read all data reference structures (without offsets/lengths)
        for _ in 0..data_size {
            let data_ref = DataRef::read_options(
                reader,
                Endian::Little,
                (&mut patch_offset_tracker, patch_versions),
            )?;
            data.push(data_ref);
        }

        // Phase 2: Read offsets (delta-encoded, 7-bit encoded)
        let mut last_offset = 0u64;
        for data_ref in &mut data {
            let offset_delta_varint = VarInt64::read_options(reader, Endian::Little, ())?;
            let offset_delta = offset_delta_varint.0;
            let absolute_offset =
                last_offset
                    .checked_add_signed(offset_delta)
                    .ok_or_else(|| binrw::Error::Custom {
                        pos: reader.stream_position().unwrap_or(0),
                        err: Box::new("Offset overflow".to_string()),
                    })?;
            data_ref.set_offset(absolute_offset);
            last_offset = absolute_offset;
        }

        // Phase 3: Read lengths (7-bit encoded)
        for data_ref in &mut data {
            let length_varint = VarUInt32::read_options(reader, Endian::Little, ())?;
            data_ref.set_len(length_varint.0);
        }

        Ok(data)
    }

    /// Write file data with patch version mapping
    pub fn write_with_patches<W: std::io::Write + std::io::Seek>(
        this: &[DataRef],
        writer: &mut W,
        patch_versions: &[PatchVersion],
    ) -> binrw::BinResult<()> {
        use binrw::Endian;

        (this.len() as i32).write_options(writer, Endian::Little, ())?;

        let mut patch_offset_tracker = 0u64;

        // Write all data references
        for data_ref in this {
            data_ref.write_options(
                writer,
                Endian::Little,
                (&mut patch_offset_tracker, patch_versions),
            )?;
        }

        // Write offsets (delta-encoded)
        let mut last_offset = 0u64;
        for data_ref in this {
            let offset_delta = data_ref.offset().wrapping_sub(last_offset) as i64;
            VarInt64(offset_delta).write_options(writer, Endian::Little, ())?;
            last_offset = data_ref.offset();
        }

        // Write lengths
        for data_ref in this {
            VarUInt32(data_ref.len()).write_options(writer, Endian::Little, ())?;
        }

        Ok(())
    }
}
