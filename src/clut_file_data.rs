use binrw::{BinRead, BinWrite};
use crate::binary::{VarInt32, VarInt64};
use crate::clut_data_ref::ClutDataRef;
use crate::types::PatchVersion;

/// File data within a CLUT, containing references to data blocks
#[derive(Debug, Clone)]
pub struct ClutFileData {
    /// List of data references for this file
    pub data: Vec<ClutDataRef>,
}

impl ClutFileData {
    /// Create a new empty file data
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
        }
    }
    
    /// Read file data with patch version mapping
    pub fn read_with_patches<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        patch_versions: &[PatchVersion],
    ) -> binrw::BinResult<Self> {
        use binrw::Endian;
        
        let data_size = i32::read_options(reader, Endian::Little, ())?;
        
        let mut data = Vec::with_capacity(data_size as usize);
        let mut patch_offset_tracker = 0i64;
        
        // Phase 1: Read all data reference structures (without offsets/lengths)
        for _ in 0..data_size {
            let data_ref = ClutDataRef::read_options(
                reader, 
                Endian::Little, 
                (&mut patch_offset_tracker, patch_versions)
            )?;
            data.push(data_ref);
        }
        
        // Phase 2: Read offsets (delta-encoded, 7-bit encoded)
        let mut last_offset = 0i64;
        for data_ref in &mut data {
            let offset_delta_varint = VarInt64::read_options(reader, Endian::Little, ())?;
            let offset_delta = offset_delta_varint.0;
            let absolute_offset = last_offset + offset_delta;
            data_ref.offset = absolute_offset;
            last_offset = absolute_offset;
        }
        
        // Phase 3: Read lengths (7-bit encoded)
        for data_ref in &mut data {
            let length_varint = VarInt32::read_options(reader, Endian::Little, ())?;
            data_ref.length = length_varint.0;
        }
        
        Ok(ClutFileData { data })
    }
    
    /// Write file data with patch version mapping
    #[allow(dead_code)]
    pub fn write_with_patches<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        _patch_map: &std::collections::HashMap<PatchVersion, i32>,
    ) -> binrw::BinResult<()> {
        use binrw::Endian;
        
        VarInt32(self.data.len() as i32).write_options(writer, Endian::Little, ())?;
        
        let mut patch_offset_tracker = 0i64;
        
        // Write all data references
        for data_ref in &self.data {
            data_ref.write_options(
                writer, 
                Endian::Little, 
                (&mut patch_offset_tracker,)
            )?;
        }
        
        // Write offsets (delta-encoded)
        let mut last_offset = 0i64;
        for data_ref in &self.data {
            let offset_delta = data_ref.offset - last_offset;
            VarInt64(offset_delta).write_options(writer, Endian::Little, ())?;
            last_offset = data_ref.offset;
        }
        
        // Write lengths
        for data_ref in &self.data {
            VarInt32(data_ref.length).write_options(writer, Endian::Little, ())?;
        }
        
        Ok(())
    }
}

impl Default for ClutFileData {
    fn default() -> Self {
        Self::new()
    }
}
