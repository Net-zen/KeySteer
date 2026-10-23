//! Display snapshots, DPI, overlay rendering and frame timing.

pub(super) mod frame_clock;
pub(super) mod gpu_overlay;
pub(super) mod overlay;
pub(super) mod overlay_worker;

use std::cell::RefCell;
use std::sync::OnceLock;

use smallvec::SmallVec;

use windows::Win32::Foundation::{LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MONITORINFOEXW, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::BOOL;

use crate::api::geometry::{Rect, Screen};
use crate::api::overlay::{LabelStyle, LabelTextAnalysis};

#[inline]
fn label_text_offset_y(style: &LabelStyle, analysis: LabelTextAnalysis, fixed_line: bool) -> f64 {
    let units = if analysis.has_descender && !fixed_line {
        -2.0
    } else {
        -1.0
    };
    analysis.scaled_units(style.font_size, units)
}

/// Opt into per-monitor DPI awareness.
///
/// Without this, Windows lies about coordinates on scaled displays and every
/// overlay lands in the wrong place. Must run before any window is created.
pub fn enable_dpi_awareness() -> Result<(), String> {
    static RESULT: OnceLock<Result<(), String>> = OnceLock::new();
    RESULT.get_or_init(enable_dpi_awareness_once).clone()
}

fn enable_dpi_awareness_once() -> Result<(), String> {
    // SAFETY: the process context constant is valid and this is called before
    // KeySteer creates native windows.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
        .map_err(|error| format!("SetProcessDpiAwarenessContext failed: {error}"))
}

fn rect_to_api(r: RECT) -> Rect {
    let width = i64::from(r.right) - i64::from(r.left);
    let height = i64::from(r.bottom) - i64::from(r.top);
    Rect::new(r.left as f64, r.top as f64, width as f64, height as f64)
}

fn monitor_dpi(monitor: HMONITOR, dpi_x: &mut u32, dpi_y: &mut u32) -> windows::core::Result<()> {
    // SAFETY: callers retain the live monitor handle and both output pointers
    // refer to initialized writable integers for this synchronous query.
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, dpi_x, dpi_y) }
}

thread_local! {
    static SCREENS: RefCell<DisplaySnapshot> = RefCell::new(DisplaySnapshot::default());
}

#[derive(Default)]
pub(super) struct DisplaySnapshot {
    pub screens: Vec<Screen>,
    /// Borrowed monitor identities from the same enumeration, aligned by index.
    pub monitors: SmallVec<[isize; 4]>,
}

pub fn list_screens() -> Result<Vec<Screen>, String> {
    list_snapshot().map(|snapshot| snapshot.screens)
}

pub(super) fn list_snapshot() -> Result<DisplaySnapshot, String> {
    SCREENS.with_borrow_mut(|snapshot| {
        snapshot.screens.clear();
        snapshot.monitors.clear();
    });

    // SAFETY: the callback matches the expected signature and the pointer we
    // pass stays valid for the duration of the call.
    unsafe { EnumDisplayMonitors(None, None, Some(enum_callback), LPARAM(0)) }
        .ok()
        .map_err(|e| format!("EnumDisplayMonitors failed: {e}"))?;

    Ok(SCREENS.with_borrow_mut(std::mem::take))
}

extern "system" fn enum_callback(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    _data: LPARAM,
) -> BOOL {
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    // SAFETY: `info` is correctly sized via cbSize.
    let ok = unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast()) };
    if !ok.as_bool() {
        // Skip this monitor but keep enumerating the rest.
        crate::report_warning!(
            "windows-screen",
            "GetMonitorInfoW failed for monitor {monitor:?}"
        );
        return TRUE;
    }

    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    if let Err(error) = monitor_dpi(monitor, &mut dpi_x, &mut dpi_y) {
        crate::report_warning!(
            "windows-screen",
            "GetDpiForMonitor failed; using 96 DPI: {error}"
        );
    }

    SCREENS.with_borrow_mut(|snapshot| {
        snapshot.screens.push(Screen {
            bounds: rect_to_api(info.monitorInfo.rcMonitor),
            work_area: rect_to_api(info.monitorInfo.rcWork),
            is_primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            scale: dpi_x as f64 / 96.0,
            name: Some(
                String::from_utf16_lossy(&info.szDevice)
                    .trim_end_matches('\0')
                    .to_string(),
            ),
        });
        snapshot.monitors.push(monitor.0 as isize);
    });
    TRUE
}

/// DPI scale of the primary monitor.
///
/// Useful as a fallback before [`list_screens`] has succeeded, since overlay
/// sizing needs a scale factor even then.
#[allow(dead_code)]
pub fn primary_scale() -> f64 {
    // SAFETY: MonitorFromPoint with the primary fallback always yields a
    // monitor handle.
    let monitor = unsafe { MonitorFromPoint(Default::default(), MONITOR_DEFAULTTOPRIMARY) };
    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    if let Err(error) = monitor_dpi(monitor, &mut dpi_x, &mut dpi_y) {
        crate::report_warning!(
            "windows-screen",
            "primary GetDpiForMonitor failed; using 96 DPI: {error}"
        );
    }
    dpi_x as f64 / 96.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_text_position_keeps_the_existing_upward_optical_offsets() {
        let style = LabelStyle {
            font_size: 17.0,
            ..LabelStyle::default()
        };
        assert_eq!(
            label_text_offset_y(&style, LabelTextAnalysis::analyze("asa", 0), false),
            -1.0
        );
        assert_eq!(
            label_text_offset_y(&style, LabelTextAnalysis::analyze("aag", 0), false),
            -2.0
        );
        for text in ["", "asa", "asag", "jpyq"] {
            assert_eq!(
                label_text_offset_y(&style, LabelTextAnalysis::analyze(text, 0), true),
                -1.0
            );
        }
    }
}
