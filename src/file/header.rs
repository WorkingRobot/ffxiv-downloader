use super::types::{CompressType, GameVersion, PatchVersion, PlatformId, Version};
use super::utils::NetString;
use binrw::{BinRead, BinWrite};

/// CLUT file header containing metadata and compression information
/// Fields match exactly what the C# ClutHeader auto-properties expose
#[derive(Debug, Clone)]
pub struct Header {
    /// Magic bytes (0xDF23)
    pub magic: u16,

    /// File version
    pub file_version: Version,

    /// Compression type used for the data
    pub compression: CompressType,

    /// Target platform
    pub platform: PlatformId,

    /// Repository name (converted from NetString during deserialization)
    pub repository: String,

    /// Game version (converted from NetString during deserialization)
    pub version: GameVersion,

    /// Patch version (converted from NetString during deserialization)
    pub patch_version: PatchVersion,

    /// Base patch URL (converted from NetString during deserialization, can be empty)
    pub base_patch_url: String,

    /// Size of decompressed data
    pub decompressed_size: i32,

    /// Size of compressed data (computed from compression type and stored value)
    pub compressed_size: i32,
}

impl Header {
    /// Get the actual compressed size (already computed during deserialization)
    pub fn get_compressed_size(&self) -> i32 {
        self.compressed_size
    }

    /// Check if the base patch URL is meaningful (not empty or whitespace)
    pub fn has_base_patch_url(&self) -> bool {
        !self.base_patch_url.trim().is_empty()
    }
}

impl Default for Header {
    fn default() -> Self {
        Self {
            magic: 0xDF23,
            file_version: Version::SeparateVersioning,
            compression: CompressType::None,
            platform: PlatformId::Win32,
            repository: "UNKNOWN".to_string(),
            version: GameVersion::epoch(),
            patch_version: PatchVersion::epoch(),
            base_patch_url: String::new(),
            decompressed_size: 0,
            compressed_size: 0,
        }
    }
}

impl BinRead for Header {
    type Args<'a> = ();

    fn read_options<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        // Read disk format
        let magic = u16::read_options(reader, endian, ())?;
        if magic != 0xDF23 {
            return Err(binrw::Error::Custom {
                pos: 0,
                err: Box::new(format!(
                    "Invalid magic bytes: expected 0xDF23, got {:#X}",
                    magic
                )),
            });
        }

        let file_version = Version::read_options(reader, endian, ())?;
        if file_version != Version::SeparateVersioning {
            return Err(binrw::Error::Custom {
                pos: 0,
                err: Box::new(format!("Unsupported version: {:?}", file_version)),
            });
        }

        let compression = CompressType::read_options(reader, endian, ())?;
        let platform = PlatformId::read_options(reader, endian, ())?;

        // Read NetStrings and convert to regular strings
        let repository = NetString::read_options(reader, endian, ())?.0;
        let version = NetString::read_options(reader, endian, ())?.0;
        let version = GameVersion::new(&version).map_err(|e| binrw::Error::Custom {
            pos: 0,
            err: Box::new(e),
        })?;
        let patch_version = NetString::read_options(reader, endian, ())?.0;
        let patch_version =
            PatchVersion::new(&patch_version).map_err(|e| binrw::Error::Custom {
                pos: 0,
                err: Box::new(e),
            })?;
        let base_patch_url = NetString::read_options(reader, endian, ())?.0;

        let decompressed_size = i32::read_options(reader, endian, ())?;

        // Read compressed size conditionally and convert to non-optional
        let compressed_size = if compression != CompressType::None {
            i32::read_options(reader, endian, ())?
        } else {
            decompressed_size // Use decompressed size when no compression
        };

        Ok(Header {
            magic,
            file_version,
            compression,
            platform,
            repository,
            version,
            patch_version,
            base_patch_url,
            decompressed_size,
            compressed_size,
        })
    }
}

impl BinWrite for Header {
    type Args<'a> = ();

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        // Write disk format
        self.magic.write_options(writer, endian, ())?;
        self.file_version.write_options(writer, endian, ())?;
        self.compression.write_options(writer, endian, ())?;
        self.platform.write_options(writer, endian, ())?;

        // Convert strings back to NetStrings for writing
        NetString(self.repository.clone()).write_options(writer, endian, ())?;
        NetString(self.version.to_string()).write_options(writer, endian, ())?;
        NetString(self.patch_version.to_string()).write_options(writer, endian, ())?;
        NetString(self.base_patch_url.clone()).write_options(writer, endian, ())?;

        self.decompressed_size.write_options(writer, endian, ())?;

        // Write compressed size only if compression is used
        if self.compression != CompressType::None {
            self.compressed_size.write_options(writer, endian, ())?;
        }

        Ok(())
    }
}
