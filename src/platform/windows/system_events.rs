//! Foreground and console notifications routed into the engine event loop.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::Win32::Foundation::{FALSE, HWND, TRUE};
use windows::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
};
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::WindowsAndMessaging::WM_QUIT;
use windows::core::BOOL;

use super::WAKE_MESSAGE;

static FOCUS_CHANGED: AtomicBool = AtomicBool::new(false);
static WAKE_FAILED: AtomicBool = AtomicBool::new(false);
static OWNER_THREAD: AtomicU32 = AtomicU32::new(0);

pub struct ForegroundWatcher(HWINEVENTHOOK);

impl ForegroundWatcher {
    pub fn new(owner_thread: u32) -> Result<Self, String> {
        OWNER_THREAD.store(owner_thread, Ordering::Release);
        let hook = super::native::install_foreground_event_hook(Some(win_event));
        if hook.is_invalid() {
            OWNER_THREAD.store(0, Ordering::Release);
            Err("SetWinEventHook(EVENT_SYSTEM_FOREGROUND) failed".into())
        } else {
            Ok(Self(hook))
        }
    }

    pub fn take_changed(&self) -> bool {
        FOCUS_CHANGED.swap(false, Ordering::AcqRel)
    }

    pub fn take_wake_failure(&self) -> bool {
        WAKE_FAILED.swap(false, Ordering::AcqRel)
    }
}

impl Drop for ForegroundWatcher {
    fn drop(&mut self) {
        if !super::native::uninstall_event_hook(self.0) {
            crate::report_error!(
                "windows-events",
                "UnhookWinEvent failed while stopping foreground notifications"
            );
        }
        OWNER_THREAD.store(0, Ordering::Release);
    }
}

extern "system" fn win_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object_id: i32,
    _child_id: i32,
    _thread_id: u32,
    _time: u32,
) {
    FOCUS_CHANGED.store(true, Ordering::Release);
    let owner = OWNER_THREAD.load(Ordering::Acquire);
    if owner != 0
        && let Err(error) = super::native::post_thread_message(owner, WAKE_MESSAGE, 0)
    {
        let _ = error;
        WAKE_FAILED.store(true, Ordering::Release);
    }
}

static ENGINE_THREAD: AtomicU32 = AtomicU32::new(0);
static SHUTDOWN_COMPLETE: AtomicBool = AtomicBool::new(false);

/// Converts console control events into the engine's normal quit path.
pub struct ConsoleControl;

impl ConsoleControl {
    pub fn new(engine_thread: u32) -> Result<Self, String> {
        SHUTDOWN_COMPLETE.store(false, Ordering::Release);
        ENGINE_THREAD.store(engine_thread, Ordering::Release);
        if let Err(error) = super::native::set_console_control_handler(Some(handler), true) {
            ENGINE_THREAD.store(0, Ordering::Release);
            return Err(format!("SetConsoleCtrlHandler failed: {error}"));
        }
        Ok(Self)
    }

    pub fn mark_shutdown_complete(&self) {
        SHUTDOWN_COMPLETE.store(true, Ordering::Release);
    }
}

impl Drop for ConsoleControl {
    fn drop(&mut self) {
        ENGINE_THREAD.store(0, Ordering::Release);
        if let Err(error) = super::native::set_console_control_handler(Some(handler), false) {
            crate::report_error!(
                "windows-console",
                "cannot unregister console control handler: {error}"
            );
        }
    }
}

extern "system" fn handler(control: u32) -> BOOL {
    if !matches!(
        control,
        CTRL_C_EVENT
            | CTRL_BREAK_EVENT
            | CTRL_CLOSE_EVENT
            | CTRL_LOGOFF_EVENT
            | CTRL_SHUTDOWN_EVENT
    ) {
        return FALSE;
    }
    let thread = ENGINE_THREAD.load(Ordering::Acquire);
    if thread == 0 {
        return FALSE;
    }
    if super::native::post_thread_message(thread, WM_QUIT, 0).is_err() {
        return FALSE;
    }
    if matches!(
        control,
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT
    ) {
        // Windows may terminate the process shortly after this handler returns.
        // Give the engine a bounded window to release Hook/COM/HWND resources.
        for _ in 0..400 {
            if SHUTDOWN_COMPLETE.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    TRUE
}
