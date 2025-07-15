use crate::binary::NetString;
use crate::types::{ClutVersion, CompressType, GameVersion, PatchVersion, PlatformId};
use binrw::{BinRead, BinWrite};

/// CLUT file header containing metadata and compression information
#[derive(Debug, Clone, BinRead, BinWrite)]
#[br(little)]
#[bw(little)]
pub struct ClutHeader {
    /// Magic bytes (0xDF23)
    #[br(assert(magic == 0xDF23, "Invalid magic bytes: expected 0xDF23, got {:#X}", magic))]
    pub magic: u16,

    /// File version
    #[br(assert(file_version == ClutVersion::SeparateVersioning, "Unsupported version: {:?}", file_version))]
    pub file_version: ClutVersion,

    /// Compression type used for the data
    pub compression: CompressType,

    /// Target platform
    pub platform: PlatformId,

    /// Repository name
    pub repository: NetString,

    /// Game version
    pub version: NetString,

    /// Patch version
    pub patch_version: NetString,

    /// Base patch URL (can be empty)
    pub base_patch_url: NetString,

    /// Size of decompressed data
    pub decompressed_size: i32,

    /// Size of compressed data (only present if compression != None)
    #[br(if(compression != CompressType::None))]
    #[bw(if(*compression != CompressType::None))]
    pub compressed_size: Option<i32>,
}

impl ClutHeader {
    /// Parse the version strings into structured types
    pub fn parse_versions(&self) -> anyhow::Result<(GameVersion, PatchVersion)> {
        let game_version = GameVersion::new(&self.version.0)?;
        let patch_version = PatchVersion::new(&self.patch_version.0)?;
        Ok((game_version, patch_version))
    }

    /// Get the actual compressed size (falls back to decompressed size if no compression)
    pub fn get_compressed_size(&self) -> i32 {
        self.compressed_size.unwrap_or(self.decompressed_size)
    }

    /// Check if the base patch URL is meaningful (not empty or whitespace)
    pub fn has_base_patch_url(&self) -> bool {
        !self.base_patch_url.0.trim().is_empty()
    }
}

impl Default for ClutHeader {
    fn default() -> Self {
        Self {
            magic: 0xDF23,
            file_version: ClutVersion::SeparateVersioning,
            compression: CompressType::None,
            platform: PlatformId::Win32,
            repository: NetString("UNKNOWN".to_string()),
            version: NetString(GameVersion::epoch().to_string()),
            patch_version: NetString(PatchVersion::epoch().to_string()),
            base_patch_url: NetString(String::new()),
            decompressed_size: 0,
            compressed_size: None,
        }
    }
}
