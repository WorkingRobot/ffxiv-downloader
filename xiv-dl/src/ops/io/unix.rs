use libc::{ENOSYS, EOPNOTSUPP};
use std::os::unix::io::AsRawFd;
use std::{fs::OpenOptions, io, path::Path};
use tokio::task;

#[cfg(target_os = "linux")]
mod imp {
    pub type Offset = libc::off64_t;

    pub unsafe fn setup(fd: i32) -> i32 {
        unsafe { libc::posix_fadvise64(fd, 0, 0, libc::POSIX_FADV_RANDOM) }
    }

    pub unsafe fn pread(fd: i32, buf: *mut libc::c_void, len: usize, offset: Offset) -> isize {
        unsafe { libc::pread64(fd, buf, len, offset) }
    }

    pub unsafe fn pwrite(fd: i32, buf: *const libc::c_void, len: usize, offset: Offset) -> isize {
        unsafe { libc::pwrite64(fd, buf, len, offset) }
    }

    pub unsafe fn ftruncate(fd: i32, size: Offset) -> i32 {
        unsafe { libc::ftruncate64(fd, size) }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub type Offset = libc::off_t;

    #[cfg(any(
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "solaris",
        target_os = "illumos"
    ))]
    pub unsafe fn setup(fd: i32) -> i32 {
        unsafe { libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_RANDOM) }
    }

    #[cfg(not(any(
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "solaris",
        target_os = "illumos"
    )))]
    pub unsafe fn setup(_fd: i32) -> i32 {
        0
    }

    pub unsafe fn pread(fd: i32, buf: *mut libc::c_void, len: usize, offset: Offset) -> isize {
        unsafe { libc::pread(fd, buf, len, offset) }
    }

    pub unsafe fn pwrite(fd: i32, buf: *const libc::c_void, len: usize, offset: Offset) -> isize {
        unsafe { libc::pwrite(fd, buf, len, offset) }
    }

    pub unsafe fn ftruncate(fd: i32, size: Offset) -> i32 {
        unsafe { libc::ftruncate(fd, size) }
    }
}

pub struct AsyncFile {
    file: std::fs::File,
}

impl AsyncFile {
    pub fn open(opts: &OpenOptions, path: impl AsRef<Path>) -> io::Result<Self> {
        let file = opts.open(path)?;
        let instance = Self { file };
        instance.setup()?;
        Ok(instance)
    }

    fn setup(&self) -> io::Result<()> {
        let fd = self.file.as_raw_fd();

        match unsafe { imp::setup(fd) } {
            ENOSYS | EOPNOTSUPP => {
                // If the function is not implemented or not supported, we ignore it
                Ok(())
            }
            e if e < 0 => Err(io::Error::last_os_error()),
            _ => Ok(()),
        }
    }

    pub async fn pread(&self, offset: usize, buf: &mut [u8]) -> io::Result<usize> {
        let fd = self.file.as_raw_fd();

        let offset: imp::Offset = offset
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "offset too large"))?;

        let buf_len = buf.len();
        // SAFETY: We cast the raw pointer to usize to make it Send across thread boundaries.
        // This is safe because:
        // 1. The buffer reference `buf` remains alive for the entire duration of this function
        // 2. The spawn_blocking task will complete before this function returns
        // 3. No other code can invalidate or move the buffer while we hold the mutable reference
        // 4. The pointer is immediately cast back to the correct type in the unsafe block
        // 5. The libc::pread64 call will not outlive the buffer's lifetime
        let buf_ptr = buf.as_mut_ptr() as usize;

        let bytes_read = task::spawn_blocking(move || {
            // SAFETY: Cast back from usize to pointer. This is safe because:
            // - The original pointer was valid and the buffer is still alive
            // - We're not dereferencing through Rust, only passing to libc
            // - libc::pread64/pread is expected to handle raw pointers correctly
            let result = unsafe { imp::pread(fd, buf_ptr as *mut libc::c_void, buf_len, offset) };
            if result < 0 {
                Err(io::Error::last_os_error())
            } else {
                let bytes_read: usize = result
                    .try_into()
                    .map_err(|_| io::Error::other("invalid return value"))?;
                Ok(bytes_read)
            }
        })
        .await
        .map_err(|_| io::Error::other("spawn_blocking failed"))??;

        Ok(bytes_read)
    }

    pub async fn pwrite(&self, offset: usize, data: &[u8]) -> io::Result<usize> {
        let fd = self.file.as_raw_fd();
        let offset: i64 = offset
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "offset too large"))?;

        let data_len = data.len();
        // SAFETY: We cast the raw pointer to usize to make it Send across thread boundaries.
        // This is safe because:
        // 1. The data slice reference `data` remains alive for the entire duration of this function
        // 2. The spawn_blocking task will complete before this function returns
        // 3. The data is immutable (shared reference), so no mutations can occur
        // 4. The pointer is immediately cast back to the correct type in the unsafe block
        // 5. The pwrite call will not outlive the data's lifetime
        let data_ptr = data.as_ptr() as usize;

        task::spawn_blocking(move || {
            // SAFETY: Cast back from usize to pointer. This is safe because:
            // - The original pointer was valid and the data slice is still alive
            // - We're not dereferencing through Rust, only passing to libc
            // - pwrite is expected to handle raw pointers correctly
            // - The data is read-only, so no race conditions with mutations
            let result =
                unsafe { imp::pwrite(fd, data_ptr as *const libc::c_void, data_len, offset) };
            if result < 0 {
                Err(io::Error::last_os_error())
            } else {
                let bytes_written: usize = result
                    .try_into()
                    .map_err(|_| io::Error::other("invalid return value"))?;
                Ok(bytes_written)
            }
        })
        .await
        .map_err(|_| io::Error::other("spawn_blocking failed"))?
    }

    pub fn truncate(&self, size: usize) -> io::Result<()> {
        let fd = self.file.as_raw_fd();
        let size: i64 = size
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "size too large"))?;

        if unsafe { imp::ftruncate(fd, size) } < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
