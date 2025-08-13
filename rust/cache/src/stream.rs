use std::{
    cmp::min,
    io::{self, Error, ErrorKind, SeekFrom},
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};

use crate::file::CacheFile;

pub struct CacheFileStream {
    file: CacheFile,
    pos: u64,
    // In-flight read future so we can implement `poll_read` without borrowing the user's buffer across await.
    in_flight: Option<Pin<Box<dyn Future<Output = io::Result<Vec<u8>>> + Send>>>,
}

impl CacheFileStream {
    pub fn new(file: CacheFile) -> Self {
        Self {
            file,
            pos: 0,
            in_flight: None,
        }
    }

    pub fn position(&self) -> u64 {
        self.pos
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
        // If we know the total length, clamp the request so we don't read past EOF.
        let want = buf.remaining();
        if want == 0 {
            return Poll::Ready(Ok(()));
        }

        if self.pos >= self.len() {
            // At EOF.
            return Poll::Ready(Ok(()));
        }

        // Determine how much to try this time.
        let to_read = {
            let remaining_in_file = (self.len() - self.pos) as usize;
            min(want, remaining_in_file)
        };

        if to_read == 0 {
            return Poll::Ready(Ok(()));
        }

        // Ensure we have an in-flight future (can't hold `buf` across await).
        if self.in_flight.is_none() {
            let offset = self.pos;
            let to_read = to_read;
            let src = self.file.clone();

            // Build the future that performs the actual `pread`.
            let fut = async move {
                let mut tmp = vec![0u8; to_read];
                src.pread(offset, &mut tmp).await?;
                Ok::<Vec<u8>, io::Error>(tmp)
            };

            self.in_flight = Some(Box::pin(fut));
        }

        // Poll the in-flight read.
        match self.in_flight.as_mut().unwrap().as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(res) => {
                self.in_flight = None; // clear for next call
                match res {
                    Ok(tmp) => {
                        let n = tmp.len();
                        // Safety: data is initialized by `pread`.
                        buf.put_slice(&tmp);
                        self.pos = self.pos.saturating_add(n as u64);
                        Poll::Ready(Ok(()))
                    }
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
        }
    }
}

impl AsyncSeek for CacheFileStream {
    fn start_seek(mut self: Pin<&mut Self>, pos: SeekFrom) -> io::Result<()> {
        let new_pos: i128 = match pos {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::Current(delta) => self.pos as i128 + delta as i128,
            SeekFrom::End(delta) => self.len() as i128 + delta as i128,
        };
        if new_pos < 0 {
            return Err(Error::new(ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = new_pos as u64;
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.pos))
    }
}
