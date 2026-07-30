pub mod chunk;

use std::io::Read;

use anyhow::{Result, bail, ensure};

pub use chunk::{Chunk, ChunkType, CompressedBlock, FileHeader};

const MAGIC: [u8; 12] = *b"\x91ZIPATCH\r\n\x1a\n";

/// Streams the chunks of a ZiPatch, tracking each one's absolute offset so a LUT can
/// record where the data it skips over lives.
pub struct ZiPatch<R> {
    reader: R,
    position: i64,
    /// Reused across chunks; grows to the largest one in the patch.
    buffer: Vec<u8>,
    finished: bool,
}

impl<R: Read> ZiPatch<R> {
    pub fn new(mut reader: R) -> Result<Self> {
        let mut magic = [0u8; MAGIC.len()];
        reader.read_exact(&mut magic)?;
        ensure!(magic == MAGIC, "Invalid magic");

        Ok(Self {
            reader,
            position: MAGIC.len() as i64,
            buffer: Vec::new(),
            finished: false,
        })
    }

    /// The next chunk, or `None` once the end-of-file chunk has been yielded. Bytes
    /// after that chunk are ignored, as the patcher ignores them.
    pub fn next_chunk(&mut self) -> Result<Option<Chunk>> {
        if self.finished {
            return Ok(None);
        }

        let mut size = [0u8; 4];
        self.reader.read_exact(&mut size)?;
        let size = u32::from_be_bytes(size) as usize;

        // The fourcc and body are checksummed together; the trailing CRC is not.
        self.buffer.clear();
        self.buffer.resize(size + 8, 0);
        self.reader.read_exact(&mut self.buffer)?;

        let (checked, expected) = self.buffer.split_at(size + 4);
        let expected = u32::from_be_bytes(expected.try_into().unwrap());
        let actual = crc32fast::hash(checked);
        let fourcc = ascii4(&checked[..4]);
        if expected != actual {
            bail!("Checksum mismatch {fourcc}: File: {expected:08X} != Calculated: {actual:08X}");
        }

        let body_offset = self.position + 8;
        self.position += size as i64 + 12;

        let chunk = Chunk::read(&fourcc, &checked[4..], body_offset)?;
        self.finished = matches!(chunk, Chunk::EndOfFile);
        Ok(Some(chunk))
    }

    /// The largest chunk seen so far, which is what this holds in memory.
    pub fn buffered(&self) -> usize {
        self.buffer.capacity()
    }
}

fn ascii4(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if b < 0x80 { b as char } else { '?' })
        .collect()
}
