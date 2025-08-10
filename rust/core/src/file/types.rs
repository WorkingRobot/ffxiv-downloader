use binrw::{BinRead, BinWrite};

/// Compression types for CLUT files
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(repr = u8)]
#[repr(u8)]
pub enum CompressType {
    None = 0,
    Zlib = 1,
    Brotli = 2,
}

/// CLUT file version
#[derive(Debug, Clone, Copy, PartialEq, Eq, BinRead, BinWrite)]
#[brw(repr = u16)]
#[repr(u16)]
pub enum Version {
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
                    err: Box::new(format!("Unknown PlatformId value: {other} (0x{other:02X})",)),
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
