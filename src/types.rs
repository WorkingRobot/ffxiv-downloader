use binrw::{BinRead, BinWrite};
use std::fmt;

/// Compression types for CLUT files
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
pub enum CompressType {
    None = 0,
    Zlib = 1,
    Brotli = 2,
}

/// CLUT file version
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[br(repr = u16)]
#[bw(repr = u16)]
#[repr(u16)]
pub enum ClutVersion {
    Initial = 1,
    SeparateVersioning = 2,
}

/// Platform ID for FFXIV installations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformId {
    Win32,
    Ps3,
    Ps4,
    Ps5,
    Lys,
    Placeholder,
    Unknown,
}

impl binrw::BinRead for PlatformId {
    type Args<'a> = ();

    fn read_options<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let value = u8::read_options(reader, endian, ())?;
        Ok(match value {
            0 => PlatformId::Win32,
            1 => PlatformId::Ps3,
            2 => PlatformId::Ps4,
            3 => PlatformId::Ps5,
            4 => PlatformId::Lys,
            254 => PlatformId::Placeholder, // ushort.MaxValue - 1 = 65534, but if it's a byte it would be 254
            255 => PlatformId::Unknown, // ushort.MaxValue = 65535, but if it's a byte it would be 255
            other => {
                return Err(binrw::Error::Custom {
                    pos: 0,
                    err: Box::new(format!(
                        "Unknown PlatformId value: {} (0x{:02X})",
                        other, other
                    )),
                });
            }
        })
    }
}

impl binrw::BinWrite for PlatformId {
    type Args<'a> = ();

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        let value = match self {
            PlatformId::Win32 => 0,
            PlatformId::Ps3 => 1,
            PlatformId::Ps4 => 2,
            PlatformId::Ps5 => 3,
            PlatformId::Lys => 4,
            PlatformId::Placeholder => 254,
            PlatformId::Unknown => 255,
        };
        value.write_options(writer, endian, ())
    }
}

/// Game version structure that matches FFXIV's versioning scheme
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameVersion {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub part: i32,
    pub revision: i32,
    pub is_historic: bool,
    pub section: Option<String>,
}

impl GameVersion {
    pub fn new(version_string: &str) -> anyhow::Result<Self> {
        let mut version_string = version_string.to_string();
        let mut is_historic = false;

        if version_string.starts_with('H') {
            is_historic = true;
            version_string = version_string[1..].to_string();
        } else if version_string.starts_with('D') {
            is_historic = false;
            version_string = version_string[1..].to_string();
        }

        let mut section = None;
        while version_string
            .chars()
            .last()
            .map_or(false, |c| c.is_ascii_lowercase())
        {
            let last_char = version_string.pop().unwrap();
            match &mut section {
                Some(s) => *s = format!("{}{}", last_char, s),
                None => section = Some(last_char.to_string()),
            }
        }

        let parts: Vec<&str> = version_string.split('.').collect();
        if parts.len() != 5 {
            return Err(anyhow::anyhow!(
                "Invalid version string: {}",
                version_string
            ));
        }

        Ok(GameVersion {
            year: parts[0].parse()?,
            month: parts[1].parse()?,
            day: parts[2].parse()?,
            part: parts[3].parse()?,
            revision: parts[4].parse()?,
            is_historic,
            section,
        })
    }

    pub fn epoch() -> Self {
        Self::new("2012.01.01.0000.0000").unwrap()
    }
}

impl fmt::Display for GameVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = if self.is_historic { "H" } else { "" };
        let section = self.section.as_deref().unwrap_or("");
        write!(
            f,
            "{}{:04}.{:02}.{:02}.{:04}.{:04}{}",
            prefix, self.year, self.month, self.day, self.part, self.revision, section
        )
    }
}

/// Patch version structure that matches FFXIV's patch versioning scheme
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PatchVersion {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub part: i32,
    pub revision: i32,
    pub is_historic: bool,
    pub section: Option<String>,
}

impl PatchVersion {
    pub fn new(version_string: &str) -> anyhow::Result<Self> {
        let mut version_string = version_string.to_string();
        let mut is_historic = false;

        if version_string.starts_with('H') {
            is_historic = true;
            version_string = version_string[1..].to_string();
        } else if version_string.starts_with('D') {
            is_historic = false;
            version_string = version_string[1..].to_string();
        }

        let mut section = None;
        while version_string
            .chars()
            .last()
            .map_or(false, |c| c.is_ascii_lowercase())
        {
            let last_char = version_string.pop().unwrap();
            match &mut section {
                Some(s) => *s = format!("{}{}", last_char, s),
                None => section = Some(last_char.to_string()),
            }
        }

        let parts: Vec<&str> = version_string.split('.').collect();
        if parts.len() != 5 {
            return Err(anyhow::anyhow!(
                "Invalid version string: {}",
                version_string
            ));
        }

        Ok(PatchVersion {
            year: parts[0].parse()?,
            month: parts[1].parse()?,
            day: parts[2].parse()?,
            part: parts[3].parse()?,
            revision: parts[4].parse()?,
            is_historic,
            section,
        })
    }

    pub fn epoch() -> Self {
        Self::new("2012.01.01.0000.0000").unwrap()
    }
}

impl fmt::Display for PatchVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = if self.is_historic { "H" } else { "D" };
        let section = self.section.as_deref().unwrap_or("");
        write!(
            f,
            "{}{:04}.{:02}.{:02}.{:04}.{:04}{}",
            prefix, self.year, self.month, self.day, self.part, self.revision, section
        )
    }
}
