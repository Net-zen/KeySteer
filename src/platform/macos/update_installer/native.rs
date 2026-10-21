//! Owned, bounded process-exit notification and same-volume bundle exchange.
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) struct ProcessExit(OwnedFd);

impl ProcessExit {
    pub(super) fn watch(pid: u32) -> Result<Self, String> {
        // SAFETY: kqueue takes no pointers and returns a fresh descriptor.
        let fd = unsafe { libc::kqueue() };
        if fd < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        // SAFETY: fd is valid and its sole owner is transferred to OwnedFd.
        let queue = Self(unsafe { OwnedFd::from_raw_fd(fd) });
        let event = libc::kevent {
            ident: pid as usize,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        queue.call(Some(&event), None, Duration::ZERO)?;
        Ok(queue)
    }

    fn call(
        &self,
        change: Option<&libc::kevent>,
        event: Option<&mut libc::kevent>,
        timeout: Duration,
    ) -> Result<i32, String> {
        let input_count = i32::from(change.is_some());
        let output_count = i32::from(event.is_some());
        let limit = libc::timespec {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as libc::c_long,
        };
        // SAFETY: the owned queue and optional single input/output records live
        // for this call. kevent neither retains the pointers nor closes the fd.
        let result = unsafe {
            libc::kevent(
                self.0.as_raw_fd(),
                change.map_or(std::ptr::null(), std::ptr::from_ref),
                input_count,
                event.map_or(std::ptr::null_mut(), std::ptr::from_mut),
                output_count,
                &limit,
            )
        };
        if result < 0 {
            Err(io::Error::last_os_error().to_string())
        } else {
            Ok(result)
        }
    }

    pub(super) fn wait(&self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut event = libc::kevent {
                ident: 0,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            match self.call(
                None,
                Some(&mut event),
                deadline.saturating_duration_since(Instant::now()),
            ) {
                Ok(1)
                    if event.flags & libc::EV_ERROR == 0 && event.fflags & libc::NOTE_EXIT != 0 =>
                {
                    return Ok(());
                }
                Ok(_) => {
                    return Err(
                        "KeySteer did not finish shutting down; the application was not replaced"
                            .into(),
                    );
                }
                Err(_)
                    if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
                        && Instant::now() < deadline => {}
                Err(error) => return Err(format!("cannot wait for KeySteer to exit: {error}")),
            }
        }
    }
}

pub(super) fn exchange(left: &Path, right: &Path) -> Result<(), String> {
    let left = CString::new(left.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let right = CString::new(right.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // SAFETY: both NUL-terminated paths live through the synchronous call.
    // RENAME_SWAP exchanges complete existing bundles on the same volume; it
    // never edits a running executable's inode or discards the previous bundle.
    if unsafe { libc::renamex_np(left.as_ptr(), right.as_ptr(), libc::RENAME_SWAP) } != 0 {
        Err(io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
