use std::{fs::OpenOptions, io, path::Path};

mod open_options;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;
#[cfg(windows)]
mod win_future;

#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
use win as imp;

pub use open_options::OpenOptionsExt;

#[repr(transparent)]
pub struct PositionedFile(imp::AsyncFile);

impl PositionedFile {
    #[inline]
    pub fn new(opts: &OpenOptions, path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self(imp::AsyncFile::open(opts, path)?))
    }

    #[inline]
    pub async fn pwrite(&self, offset: usize, data: &[u8]) -> io::Result<usize> {
        self.0.pwrite(offset, data).await
    }

    #[inline]
    pub async fn pread(&self, offset: usize, buf: &mut [u8]) -> io::Result<usize> {
        self.0.pread(offset, buf).await
    }

    #[inline]
    pub fn truncate(&self, size: usize) -> io::Result<()> {
        self.0.truncate(size)
    }

    pub async fn pwrite_all(&self, offset: usize, data: &[u8]) -> io::Result<()> {
        let mut written = 0;
        while written < data.len() {
            let bytes_written = self.pwrite(offset + written, &data[written..]).await?;
            if bytes_written == 0 {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "wrote zero bytes"));
            }
            assert!(written + bytes_written <= data.len());
            written += bytes_written;
        }
        Ok(())
    }

    pub async fn pread_all(&self, offset: usize, buf: &mut [u8]) -> io::Result<()> {
        let mut read = 0;
        while read < buf.len() {
            let bytes_read = self.pread(offset + read, &mut buf[read..]).await?;
            if bytes_read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "read zero bytes",
                ));
            }
            read += bytes_read;
        }
        Ok(())
    }
}
