use std::{
    fs::OpenOptions,
    io,
    os::windows::prelude::{IntoRawHandle, OpenOptionsExt},
    path::Path,
};
use windows::{
    Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FILE_END_OF_FILE_INFO, FILE_FLAG_OVERLAPPED, FILE_FLAG_RANDOM_ACCESS,
            FileEndOfFileInfo, ReadFile, SetFileInformationByHandle, WriteFile,
        },
        System::IO::OVERLAPPED,
    },
    core::Owned,
};

use crate::win_future::FileFuture;

pub struct AsyncFile {
    handle: Owned<HANDLE>,
}

impl AsyncFile {
    pub fn open(opts: &OpenOptions, path: impl AsRef<Path>) -> io::Result<Self> {
        let mut opts = opts.clone();
        let handle = opts
            .custom_flags(FILE_FLAG_OVERLAPPED.0 | FILE_FLAG_RANDOM_ACCESS.0)
            .open(path)?
            .into_raw_handle();
        let handle = unsafe { Owned::new(HANDLE(handle)) };
        unsafe { FileFuture::bind_handle(*handle) }?;
        Ok(Self { handle })
    }

    fn set_offset(overlapped: &mut OVERLAPPED, offset: u64) {
        let (offset_low, offset_high) = (offset as u32, (offset >> 32) as u32);
        overlapped.Anonymous.Anonymous.Offset = offset_low;
        overlapped.Anonymous.Anonymous.OffsetHigh = offset_high;
    }

    pub async fn pread(&self, offset: usize, buf: &mut [u8]) -> io::Result<usize> {
        let offset: u64 = offset
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Offset must fit in a u64"))?;
        let result = FileFuture::new(|overlapped| unsafe {
            Self::set_offset(overlapped, offset);
            ReadFile(*self.handle, Some(buf), None, Some(overlapped as *mut _))
        })?
        .await;

        result
            .map(|bytes| bytes as usize)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    pub async fn pwrite(&self, offset: usize, data: &[u8]) -> io::Result<usize> {
        let offset: u64 = offset
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Offset must fit in a u64"))?;
        let result = FileFuture::new(|overlapped| unsafe {
            Self::set_offset(overlapped, offset);
            WriteFile(*self.handle, Some(data), None, Some(overlapped as *mut _))
        })?
        .await;

        result
            .map(|bytes| bytes as usize)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    pub fn truncate(&self, size: usize) -> io::Result<()> {
        let size: i64 = size
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Size must fit in a i64"))?;

        let mut info = FILE_END_OF_FILE_INFO { EndOfFile: size };
        unsafe {
            SetFileInformationByHandle(
                *self.handle,
                FileEndOfFileInfo,
                (&raw mut info).cast(),
                std::mem::size_of::<FILE_END_OF_FILE_INFO>() as u32,
            )
        }
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}
