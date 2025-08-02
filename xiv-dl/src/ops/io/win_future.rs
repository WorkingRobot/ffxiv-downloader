use std::{
    pin::Pin,
    task::{Context, Poll, Waker},
};

use either::Either;
use windows::{
    Win32::{
        Foundation::{ERROR_IO_PENDING, HANDLE, NTSTATUS},
        System::IO::{BindIoCompletionCallback, OVERLAPPED},
    },
    core::{Error, Result},
};

#[repr(C)]
struct OverlappedWrap {
    o: OVERLAPPED,
    data: Either<Option<Waker>, Result<u32>>,
}

// SAFETY: Rust says OVERLAPPED is not Send, but only because it contains a HANDLE.
unsafe impl Send for OverlappedWrap {}

impl OverlappedWrap {
    unsafe extern "system" fn waker_callback(
        dwerrorcode: u32,
        lpnumberofbytestransferred: u32,
        lpoverlapped: *mut OVERLAPPED,
    ) {
        let wrap_ptr = lpoverlapped.cast::<OverlappedWrap>();
        let wrap = unsafe { wrap_ptr.as_mut() }.expect("pointer is null");

        let result = if dwerrorcode == 0 {
            Ok(lpnumberofbytestransferred)
        } else {
            Err(Error::from_hresult(
                NTSTATUS(dwerrorcode as i32).to_hresult(),
            ))
        };

        let waker = match &mut wrap.data {
            Either::Left(waker_opt) => waker_opt.take(),
            Either::Right(_) => None,
        };

        wrap.data = Either::Right(result);

        if let Some(waker) = waker {
            waker.wake();
        }
    }

    #[inline]
    pub unsafe fn bind_handle(handle: HANDLE) -> Result<()> {
        unsafe { BindIoCompletionCallback(handle, Some(Self::waker_callback), 0) }
    }
}

impl Default for OverlappedWrap {
    fn default() -> Self {
        Self {
            o: OVERLAPPED::default(),
            data: Either::Left(None),
        }
    }
}

pub struct FileFuture {
    overlapped: Pin<Box<OverlappedWrap>>,
}

impl FileFuture {
    pub fn new(mut caller: impl FnMut(&mut OVERLAPPED) -> Result<()>) -> Result<Self> {
        let mut overlapped = Box::pin(OverlappedWrap::default());
        let overlapped_ref = &mut overlapped.as_mut().get_mut().o;
        let err =
            caller(overlapped_ref).expect_err("Caller should return an ERROR_PENDING on success");
        if err.code() != ERROR_IO_PENDING.into() {
            return Err(err);
        }
        Ok(Self { overlapped })
    }

    #[inline]
    pub unsafe fn bind_handle(handle: HANDLE) -> Result<()> {
        unsafe { OverlappedWrap::bind_handle(handle) }
    }
}

impl Future for FileFuture {
    type Output = Result<u32>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.get_mut().overlapped.data {
            Either::Left(waker_opt) => {
                if let Some(waker) = waker_opt {
                    waker.clone_from(cx.waker());
                } else {
                    *waker_opt = Some(cx.waker().clone());
                }
                Poll::Pending
            }
            Either::Right(result) => Poll::Ready(result.clone()),
        }
    }
}
