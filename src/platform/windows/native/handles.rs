//! Owned kernel handles and child-process containment.
use windows::Win32::Foundation::HANDLE;

/// A process or thread handle that is closed exactly once.
#[repr(transparent)]
pub(super) struct OwnedHandle(HANDLE);

impl OwnedHandle {
    pub(super) fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    pub(super) fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    #[inline(always)]
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;

        // SAFETY: this wrapper is created only from an owned successful handle
        // and Drop is its sole close path.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            crate::report_error!("windows-native", "CloseHandle failed: {error}");
        }
    }
}
