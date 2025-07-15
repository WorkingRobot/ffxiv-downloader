use super::file_data::FileData;
use super::header::Header;
use super::types::{CompressType, PatchVersion};
use super::utils::NetString;
use binrw::BinRead;
use brotli::Decompressor;
use flate2::read::ZlibDecoder;
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

/// Complete CLUT file structure containing header, folders, and file data
#[derive(Debug, Clone)]
pub struct Clut {
    /// File header with metadata
    pub header: Header,

    /// Set of folder paths
    pub folders: HashSet<String>,

    /// Map of file paths to their data
    pub files: HashMap<String, FileData>,
}

impl Clut {
    /// Create a new empty CLUT file
    pub fn new() -> Self {
        Self {
            header: Header::default(),
            folders: HashSet::new(),
            files: HashMap::new(),
        }
    }

    /// Read a CLUT file from a binary reader
    pub fn read<R: Read + std::io::Seek>(mut reader: R) -> anyhow::Result<Self> {
        // Read header
        let header = Header::read_options(&mut reader, binrw::Endian::Little, ())?;

        // Read compressed data
        let compressed_size = header.get_compressed_size();
        let mut compressed_data = vec![0u8; compressed_size as usize];
        reader.read_exact(&mut compressed_data)?;

        // Decompress data if needed
        let decompressed_data = match header.compression {
            CompressType::None => compressed_data,
            CompressType::Zlib => {
                let mut decoder = ZlibDecoder::new(&compressed_data[..]);
                let mut decompressed = Vec::new();
                decoder.read_to_end(&mut decompressed)?;

                if decompressed.len() != header.decompressed_size as usize {
                    return Err(anyhow::anyhow!(
                        "Decompressed size mismatch: expected {}, got {}",
                        header.decompressed_size,
                        decompressed.len()
                    ));
                }
                decompressed
            }
            CompressType::Brotli => {
                let mut decompressed = Vec::new();
                let mut decoder = Decompressor::new(&compressed_data[..], 8192);
                decoder.read_to_end(&mut decompressed)?;

                if decompressed.len() != header.decompressed_size as usize {
                    return Err(anyhow::anyhow!(
                        "Brotli decompressed size mismatch: expected {}, got {}",
                        header.decompressed_size,
                        decompressed.len()
                    ));
                }
                decompressed
            }
        };

        // Parse decompressed data
        let mut cursor = Cursor::new(&decompressed_data);
        Self::read_decompressed_data(header, &mut cursor)
    }

    /// Read the decompressed data portion of a CLUT file
    fn read_decompressed_data<R: Read + std::io::Seek>(
        header: Header,
        reader: &mut R,
    ) -> anyhow::Result<Self> {
        use binrw::Endian;

        // Read patch versions
        let patch_len = i32::read_options(reader, Endian::Little, ())?;
        let mut patch_versions = Vec::with_capacity(patch_len as usize);
        for _i in 0..patch_len {
            let patch_str = NetString::read_options(reader, Endian::Little, ())?.0;
            patch_versions.push(PatchVersion::new(&patch_str)?);
        }

        // Read folders
        let folder_len = i32::read_options(reader, Endian::Little, ())?;
        let mut folders = HashSet::with_capacity(folder_len as usize);
        for _ in 0..folder_len {
            let folder = NetString::read_options(reader, Endian::Little, ())?.0;
            folders.insert(folder);
        }

        // Read file names
        let file_len = i32::read_options(reader, Endian::Little, ())?;
        let mut file_names = Vec::with_capacity(file_len as usize);
        for _ in 0..file_len {
            let file_name = NetString::read_options(reader, Endian::Little, ())?.0;
            file_names.push(file_name);
        }

        // Read file data
        let mut files = HashMap::with_capacity(file_len as usize);
        for file_name in file_names {
            let file_data = FileData::read_with_patches(reader, &patch_versions)?;
            files.insert(file_name, file_data);
        }

        Ok(Clut {
            header,
            folders,
            files,
        })
    }

    /// Get statistics about the CLUT file
    pub fn stats(&self) -> ClutStats {
        let total_data_refs = self
            .files
            .values()
            .map(|file_data| file_data.data.len())
            .sum();

        let unique_patches: HashSet<_> = self
            .files
            .values()
            .flat_map(|file_data| &file_data.data)
            .map(|data_ref| data_ref.applied_version())
            .collect();

        ClutStats {
            folder_count: self.folders.len(),
            file_count: self.files.len(),
            total_data_refs,
            unique_patch_count: unique_patches.len(),
        }
    }
}

impl Default for Clut {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about a CLUT file
#[derive(Debug, Clone)]
pub struct ClutStats {
    pub folder_count: usize,
    pub file_count: usize,
    pub total_data_refs: usize,
    pub unique_patch_count: usize,
}

impl std::fmt::Display for ClutStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CLUT Stats: {} folders, {} files, {} data references, {} unique patches",
            self.folder_count, self.file_count, self.total_data_refs, self.unique_patch_count
        )
    }
}
