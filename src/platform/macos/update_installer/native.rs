//! Owned, bounded process-exit notification and same-volume bundle exchange.
use std::cell::Cell;
use std::ffi::{CString, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSString, NSURL};

pub(super) struct ProcessExit {
    queue: OwnedFd,
    exited: Cell<bool>,
}

impl ProcessExit {
    pub(super) fn watch(pid: u32) -> Result<Self, String> {
        // SAFETY: kqueue returns a fresh descriptor. Check it before immediately
        // transferring its sole ownership to OwnedFd, including on error paths.
        let queue = unsafe {
            let fd = libc::kqueue();
            if fd < 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            Self {
                queue: OwnedFd::from_raw_fd(fd),
                exited: Cell::new(false),
            }
        };
        let event = libc::kevent {
            ident: pid as usize,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        match queue.call(Some(&event), None, Duration::ZERO) {
            Ok(_) => {}
            // A LaunchServices app may exit before the completion is delivered.
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => queue.exited.set(true),
            Err(error) => return Err(error.to_string()),
        }
        Ok(queue)
    }

    fn call(
        &self,
        change: Option<&libc::kevent>,
        event: Option<&mut libc::kevent>,
        timeout: Duration,
    ) -> io::Result<i32> {
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
                self.queue.as_raw_fd(),
                change.map_or(std::ptr::null(), std::ptr::from_ref),
                input_count,
                event.map_or(std::ptr::null_mut(), std::ptr::from_mut),
                output_count,
                &limit,
            )
        };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(result)
        }
    }

    pub(super) fn wait(&self, timeout: Duration) -> Result<(), String> {
        if self.poll(timeout)? {
            Ok(())
        } else {
            Err("KeySteer did not finish shutting down in time".into())
        }
    }

    pub(super) fn poll(&self, timeout: Duration) -> Result<bool, String> {
        if self.exited.get() {
            return Ok(true);
        }
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
                    self.exited.set(true);
                    return Ok(true);
                }
                Ok(0) => return Ok(false),
                Ok(_) => return Err("invalid application exit notification".into()),
                Err(error)
                    if error.kind() == io::ErrorKind::Interrupted && Instant::now() < deadline => {}
                Err(error) => return Err(format!("cannot wait for KeySteer to exit: {error}")),
            }
        }
    }
}

/// A LaunchServices process is not our child. Keep its native identity and an
/// exit notification instead of treating /usr/bin/open as the application.
pub(super) struct Application {
    app: Retained<NSRunningApplication>,
    exited: ProcessExit,
}

impl Application {
    pub(super) fn has_exited(&self) -> Result<bool, String> {
        self.exited.poll(Duration::ZERO)
    }

    pub(super) fn stop(&self) -> Result<(), String> {
        if !self.has_exited()? {
            self.app.forceTerminate();
        }
        self.exited.wait(Duration::from_secs(5))
    }
}

pub(super) fn launch(
    bundle: &Path,
    args: &[OsString],
    ready_socket: Option<&Path>,
    timeout: Duration,
) -> Result<Application, super::StartError> {
    let url = NSURL::from_directory_path(bundle)
        .ok_or_else(|| super::StartError::stopped("invalid application bundle URL"))?;
    let arguments: Result<Vec<_>, _> = args
        .iter()
        .map(|arg| {
            arg.to_str().map(NSString::from_str).ok_or_else(|| {
                super::StartError::stopped("application arguments must be valid UTF-8")
            })
        })
        .collect();
    let configuration = NSWorkspaceOpenConfiguration::configuration();
    configuration.setArguments(&NSArray::from_retained_slice(&arguments?));
    configuration.setCreatesNewApplicationInstance(true);
    configuration.setAllowsRunningApplicationSubstitution(false);
    configuration.setActivates(false);
    configuration.setAddsToRecentItems(false);
    configuration.setPromptsUserIfNeeded(false);
    // LaunchServices supplies the normal application environment; never inherit
    // the old helper's responsibility chain or a stale startup acknowledgement.
    if let Some(socket) = ready_socket {
        let value = NSString::from_str(
            socket
                .to_str()
                .ok_or_else(|| super::StartError::stopped("invalid update socket path"))?,
        );
        configuration.setEnvironment(&NSDictionary::from_slices(
            &[&*NSString::from_str(super::READY_ENV)],
            &[&*value],
        ));
    }
    let (sender, receiver) = mpsc::channel();
    let completion = RcBlock::new(move |app: *mut NSRunningApplication, error: *mut NSError| {
        // SAFETY: AppKit supplies nullable objects valid for this callback.
        // Retain the application before crossing the channel; NSError is only
        // borrowed here and converted to an owned string. The framework's
        // NSRunningApplication is thread safe; no custom Send/Sync is needed.
        let result = unsafe {
            Retained::retain(app).ok_or_else(|| {
                error
                    .as_ref()
                    .map(|e| e.localizedDescription().to_string())
                    .unwrap_or_else(|| "LaunchServices did not return an application".into())
            })
        };
        if let Err(mpsc::SendError(Ok(app))) = sender.send(result) {
            // The helper timed out. Do not leave a late launch running if the
            // completion arrives while the helper is reporting that failure.
            app.forceTerminate();
        }
    });
    NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
        &url,
        &configuration,
        Some(&completion),
    );
    let app = receiver
        .recv_timeout(timeout)
        .map_err(|e| {
            super::StartError::uncertain(format!("application launch was not confirmed: {e}"))
        })?
        .map_err(super::StartError::stopped)?;
    let exited = ProcessExit::watch(app.processIdentifier() as u32).map_err(|error| {
        app.forceTerminate();
        super::StartError::uncertain(format!("cannot monitor the launched application: {error}"))
    })?;
    Ok(Application { app, exited })
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
