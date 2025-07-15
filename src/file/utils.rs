use binrw::{BinRead, BinResult, BinWrite, Endian};
use std::io::{Read, Write};

/// Custom parser for .NET 7-bit encoded integers (BinaryReader.Read7BitEncodedInt)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarInt32(pub i32);

impl BinRead for VarInt32 {
    type Args<'a> = ();

    fn read_options<R: Read + std::io::Seek>(
        reader: &mut R,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<Self> {
        let mut result = 0i32;
        let mut shift = 0;

        loop {
            let byte = u8::read_le(reader)?;
            result |= ((byte & 0x7F) as i32) << shift;

            if (byte & 0x80) == 0 {
                break;
            }

            shift += 7;
            if shift >= 32 {
                return Err(binrw::Error::Custom {
                    pos: 0,
                    err: Box::new("VarInt32 overflow"),
                });
            }
        }

        Ok(VarInt32(result))
    }
}

impl BinWrite for VarInt32 {
    type Args<'a> = ();

    fn write_options<W: Write + std::io::Seek>(
        &self,
        writer: &mut W,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<()> {
        let mut value = self.0 as u32;

        while value >= 0x80 {
            writer.write_all(&[(value as u8) | 0x80])?;
            value >>= 7;
        }

        writer.write_all(&[value as u8])?;
        Ok(())
    }
}

/// Custom parser for .NET 7-bit encoded 64-bit integers (BinaryReader.Read7BitEncodedInt64)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarInt64(pub i64);

impl BinRead for VarInt64 {
    type Args<'a> = ();

    fn read_options<R: Read + std::io::Seek>(
        reader: &mut R,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<Self> {
        let mut result = 0i64;
        let mut shift = 0;

        loop {
            let byte = u8::read_le(reader)?;
            result |= ((byte & 0x7F) as i64) << shift;

            if (byte & 0x80) == 0 {
                break;
            }

            shift += 7;
            if shift >= 64 {
                return Err(binrw::Error::Custom {
                    pos: 0,
                    err: Box::new("VarInt64 overflow"),
                });
            }
        }

        Ok(VarInt64(result))
    }
}

impl BinWrite for VarInt64 {
    type Args<'a> = ();

    fn write_options<W: Write + std::io::Seek>(
        &self,
        writer: &mut W,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> BinResult<()> {
        let mut value = self.0 as u64;

        while value >= 0x80 {
            writer.write_all(&[(value as u8) | 0x80])?;
            value >>= 7;
        }

        writer.write_all(&[value as u8])?;
        Ok(())
    }
}

/// Custom parser for .NET BinaryReader strings (length-prefixed with 7-bit encoded length)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetString(pub String);

impl BinRead for NetString {
    type Args<'a> = ();

    fn read_options<R: Read + std::io::Seek>(
        reader: &mut R,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> BinResult<Self> {
        let length = VarInt32::read_options(reader, endian, args)?.0;
        if length < 0 {
            return Err(binrw::Error::Custom {
                pos: 0,
                err: Box::new("Negative string length"),
            });
        }

        let mut bytes = vec![0u8; length as usize];
        reader.read_exact(&mut bytes)?;

        let string = String::from_utf8(bytes).map_err(|e| binrw::Error::Custom {
            pos: 0,
            err: Box::new(format!("Invalid UTF-8: {}", e)),
        })?;

        Ok(NetString(string))
    }
}

impl BinWrite for NetString {
    type Args<'a> = ();

    fn write_options<W: Write + std::io::Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        args: Self::Args<'_>,
    ) -> BinResult<()> {
        let bytes = self.0.as_bytes();
        VarInt32(bytes.len() as i32).write_options(writer, endian, args)?;
        writer.write_all(bytes)?;
        Ok(())
    }
}
