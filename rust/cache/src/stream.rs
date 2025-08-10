use std::{
    io::{self, SeekFrom},
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncSeek, BufReader, ReadBuf};

use crate::file::CacheFile;

pub struct CacheFileStream {
    file: CacheFile,
    position: u64,
}

impl CacheFileStream {
    pub fn new(file: CacheFile) -> Self {
        Self { file, position: 0 }
    }

    pub fn buffered(file: CacheFile) -> BufReader<Self> {
        BufReader::new(Self::new(file))
    }

    pub fn buffered_with_capacity(file: CacheFile, capacity: usize) -> BufReader<Self> {
        BufReader::with_capacity(capacity, Self::new(file))
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    pub fn len(&self) -> u64 {
        self.file.len()
    }

    pub fn is_empty(&self) -> bool {
        self.file.is_empty()
    }
}

impl AsyncRead for CacheFileStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        // Check if we've reached the end of the file
        if self.position >= self.file.len() {
            return Poll::Ready(Ok(()));
        }

        let to_read = buf
            .remaining()
            .min((self.file.len() - self.position) as usize);
        if to_read == 0 {
            return Poll::Ready(Ok(()));
        }

        // Create buffer for pread
        let mut read_buffer = vec![0u8; to_read];
        let position = self.position;
        let file = self.file.clone();

        let mut read_future = Box::pin(async move {
            file.pread(position, &mut read_buffer)
                .await
                .map_err(io::Error::other)?;
            Ok(read_buffer)
        });

        match read_future.as_mut().poll(cx) {
            Poll::Ready(Ok(data)) => {
                let bytes_to_copy = data.len().min(buf.remaining());
                buf.put_slice(&data[..bytes_to_copy]);
                self.position += bytes_to_copy as u64;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncSeek for CacheFileStream {
    fn start_seek(mut self: Pin<&mut Self>, position: SeekFrom) -> io::Result<()> {
        let new_pos = match position {
            SeekFrom::Start(pos) => pos,
            SeekFrom::End(offset) => {
                let file_len = self.file.len();
                if offset >= 0 {
                    file_len + offset as u64
                } else {
                    file_len.saturating_sub((-offset) as u64)
                }
            }
            SeekFrom::Current(offset) => {
                if offset >= 0 {
                    self.position + offset as u64
                } else {
                    self.position.saturating_sub((-offset) as u64)
                }
            }
        };

        self.position = new_pos;
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.position))
    }
}

pub fn blocking_reader(file: CacheFile) -> BlockingCacheFileReader {
    BlockingCacheFileReader::new(CacheFileStream::new(file))
}

pub struct BlockingCacheFileReader {
    stream: CacheFileStream,
}

impl BlockingCacheFileReader {
    fn new(stream: CacheFileStream) -> Self {
        Self { stream }
    }

    pub fn position(&self) -> u64 {
        self.stream.position()
    }

    pub fn len(&self) -> u64 {
        self.stream.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stream.is_empty()
    }
}

impl io::Read for BlockingCacheFileReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        tokio::task::block_in_place(|| {
            let handle = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;

            handle.block_on(async {
                let mut read_buf = ReadBuf::new(buf);
                let initial_filled = read_buf.filled().len();

                futures::future::poll_fn(|cx| {
                    Pin::new(&mut self.stream).poll_read(cx, &mut read_buf)
                })
                .await?;

                Ok(read_buf.filled().len() - initial_filled)
            })
        })
    }
}

impl io::Seek for BlockingCacheFileReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        tokio::task::block_in_place(|| {
            let handle = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;

            handle.block_on(async {
                Pin::new(&mut self.stream).start_seek(pos)?;
                futures::future::poll_fn(|cx| Pin::new(&mut self.stream).poll_complete(cx)).await
            })
        })
    }
}
