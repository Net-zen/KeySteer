//! Modeless, IME-capable note entry owned by the existing tray UI thread.
use crate::api::{backend::BackendEvent, window_presets::TextPrompt};
use std::cell::RefCell;
use std::sync::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

pub(super) const MESSAGE: u32 = WM_APP + 0x51;
enum Job {
    Show(Box<TextPrompt>),
    Cancel(u64),
    Release,
}
static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());
struct Active {
    request: TextPrompt,
    hwnd: HWND,
    _font: Option<super::native::OwnedFont>,
    previous: HWND,
    text_buffer: Vec<u16>,
}
impl Drop for Active {
    fn drop(&mut self) {
        // SAFETY: Active is created and dropped only on the owning tray thread.
        if let Err(error) = unsafe { DestroyWindow(self.hwnd) } {
            crate::report_error!("windows-dialog", "Cannot destroy layout input: {error}");
        }
    }
}
thread_local! { static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) }; }
thread_local! { static CACHED: RefCell<Option<Active>> = const { RefCell::new(None) }; }

pub(super) fn request(owner: HWND, prompt: TextPrompt) -> Result<(), String> {
    enqueue(owner, Job::Show(Box::new(prompt)))
}
pub(super) fn cancel(owner: HWND, id: u64) -> Result<(), String> {
    enqueue(owner, Job::Cancel(id))
}
pub(super) fn release(owner: HWND) -> Result<(), String> {
    enqueue(owner, Job::Release)
}
fn enqueue(owner: HWND, job: Job) -> Result<(), String> {
    let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    jobs.push(job);
    // SAFETY: the live tray owner receives a payload-free wake; jobs own their strings.
    if let Err(error) = unsafe { PostMessageW(Some(owner), MESSAGE, WPARAM(0), LPARAM(0)) } {
        jobs.pop();
        return Err(format!("Cannot wake layout note dialog: {error}"));
    }
    Ok(())
}
pub(super) fn process() {
    let jobs = std::mem::take(&mut *JOBS.lock().unwrap_or_else(|e| e.into_inner()));
    for job in jobs {
        match job {
            Job::Release => {
                close(None);
                CACHED.with(|cached| cached.borrow_mut().take());
            }
            Job::Show(prompt) => {
                close(None);
                let id = prompt.id;
                if let Err(error) = show(*prompt) {
                    ACTIVE.with(|a| a.borrow_mut().take());
                    super::status_item::emit(BackendEvent::TextPromptResult {
                        id,
                        value: Err(error),
                    });
                }
            }
            Job::Cancel(id) => {
                if ACTIVE.with(|a| a.borrow().as_ref().is_some_and(|a| a.request.id == id)) {
                    close(None);
                }
            }
        }
    }
}
pub(super) fn dispatch(message: &MSG) -> bool {
    let hwnd = ACTIVE.with(|a| a.borrow().as_ref().map(|a| a.hwnd));
    if message.message == WM_KEYDOWN
        && matches!(message.wParam.0, 13 | 27)
        && ACTIVE.with(|a| {
            a.borrow()
                .as_ref()
                .is_some_and(|a| a.request.live_style.is_some())
        })
    {
        use windows::Win32::UI::Input::Ime::*;
        // SAFETY: this message belongs to the tray thread's native edit. Acquire
        // and release its IME context in the same turn, without retaining it.
        let composing = unsafe {
            let context = ImmGetContext(message.hwnd);
            if context.is_invalid() {
                false
            } else {
                let composing = ImmGetCompositionStringW(context, GCS_COMPSTR, None, 0) > 0;
                let _ = ImmReleaseContext(message.hwnd, context);
                composing
            }
        };
        if !composing && let Some(hwnd) = hwnd {
            dialog_proc(
                hwnd,
                WM_COMMAND,
                WPARAM(if message.wParam.0 == 13 { 1 } else { 2 }),
                LPARAM(0),
            );
            return true;
        }
        if composing {
            return false;
        }
    }
    // SAFETY: dialog and message belong to this thread. No RefCell borrow spans callbacks.
    hwnd.is_some_and(|hwnd| unsafe { IsDialogMessageW(hwnd, message).as_bool() })
}
pub(super) fn shutdown() {
    JOBS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    close(None);
    CACHED.with(|cached| cached.borrow_mut().take());
}
fn close(value: Option<String>) {
    if let Some(active) = ACTIVE.with(|a| a.borrow_mut().take()) {
        let id = active.request.id;
        if active.request.live_style.is_some() {
            // SAFETY: the tray thread owns this editor; hiding retains resources for the same mode session.
            unsafe {
                let restore = GetForegroundWindow() == active.hwnd;
                let _ = ShowWindow(active.hwnd, SW_HIDE);
                if restore {
                    let _ = SetForegroundWindow(active.previous);
                }
            }
            CACHED.with(|cached| *cached.borrow_mut() = Some(active));
        } else {
            drop(active);
        }
        super::status_item::emit(BackendEvent::TextPromptResult {
            id,
            value: Ok(value),
        });
    }
}
fn show(request: TextPrompt) -> Result<(), String> {
    let live = request.live_style.is_some();
    // SAFETY: reading the foreground HWND has no ownership transfer.
    let previous = unsafe { GetForegroundWindow() };
    if let Some(mut cached) = CACHED.with(|cached| cached.borrow_mut().take())
        && live
        && cached.request.live_style == request.live_style
        && cached.request.bounds == request.bounds
    {
        cached.previous = previous;
        cached.request = request;
        let hwnd = cached.hwnd;
        // SAFETY: the hidden editor remains owned by this tray thread. Clear
        // before installing the new request so no old text event is emitted.
        unsafe {
            let _ = SetDlgItemTextW(hwnd, 101, windows::core::w!(""));
        }
        ACTIVE.with(|active| *active.borrow_mut() = Some(cached));
        // SAFETY: reuse the same live modeless dialog and edit child.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        focus_editor(hwnd)?;
        return Ok(());
    }
    let mut words = Vec::<u16>::new();
    fn dword(words: &mut Vec<u16>, value: u32) {
        words.extend([value as u16, (value >> 16) as u16]);
    }
    fn string(words: &mut Vec<u16>, value: &str) {
        words.extend(value.encode_utf16());
        words.push(0);
    }
    dword(&mut words, WS_POPUP.0 | DS_SETFONT as u32);
    dword(&mut words, (WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0);
    words.extend([if live { 1 } else { 4 }, 0, 0, 300, 100, 0, 0]);
    string(&mut words, &request.title);
    words.push(9);
    string(&mut words, "Segoe UI");
    for (id, class, text, x, y, width, height, extra) in [
        (100u16, 0x82, request.message.as_str(), 10, 8, 280, 26, 0),
        (
            101,
            0x81,
            "",
            10,
            37,
            280,
            16,
            ES_AUTOHSCROLL as u32 | WS_BORDER.0 | WS_TABSTOP.0,
        ),
        (
            1,
            0x80,
            "Save",
            176,
            72,
            54,
            18,
            BS_DEFPUSHBUTTON as u32 | WS_TABSTOP.0,
        ),
        (2, 0x80, "Cancel", 236, 72, 54, 18, WS_TABSTOP.0),
    ] {
        if live && id != 101 {
            continue;
        }
        if !words.len().is_multiple_of(2) {
            words.push(0);
        }
        dword(
            &mut words,
            (WS_CHILD | WS_VISIBLE).0
                | if live {
                    ES_AUTOHSCROLL as u32 | WS_TABSTOP.0
                } else {
                    extra
                },
        );
        dword(&mut words, 0);
        words.extend([x, y, width, height, id, 0xffff, class]);
        string(&mut words, text);
        words.push(0);
    }
    if !words.len().is_multiple_of(2) {
        words.push(0);
    }
    let aligned: Vec<u32> = words
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u32::from(p[0]) | (u32::from(p[1]) << 16))
        .collect();
    let mut bounds = request.bounds;
    let font = if let Some(style) = &request.live_style {
        let scale = bounds.height / (style.font_size * 1.8 + style.padding_y * 2.0);
        bounds = bounds.inset(
            (style.padding_x + style.border_width) * scale,
            (style.padding_y + style.border_width) * scale,
        );
        Some(super::native::OwnedFont::new(
            if style.font_family.is_empty() {
                "Segoe UI"
            } else {
                &style.font_family
            },
            (style.font_size * scale).round() as i32,
            style.bold,
        )?)
    } else {
        None
    };
    // SAFETY: DWORD-aligned DLGTEMPLATE and all variable entries remain live during
    // synchronous creation. The system copies them; callback retains no pointers.
    let hwnd = unsafe {
        CreateDialogIndirectParamW(
            None,
            aligned.as_ptr().cast(),
            None,
            Some(dialog_proc),
            LPARAM(0),
        )
    }
    .map_err(|e| format!("Cannot create layout note dialog: {e}"))?;
    let placeholder: Vec<u16> = request
        .placeholder
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let max_units = request.max_chars.saturating_mul(2);
    let font_handle = font.as_ref().map(|font| font.raw());
    ACTIVE.with(|a| {
        *a.borrow_mut() = Some(Active {
            request,
            hwnd,
            _font: font,
            previous,
            text_buffer: vec![0; max_units.saturating_add(1)],
        })
    });
    // SAFETY: this thread owns the newly created dialog and its edit child.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            bounds.x.round() as i32,
            bounds.y.round() as i32,
            bounds.width.round() as i32,
            bounds.height.round() as i32,
            SWP_NOACTIVATE,
        )
        .map_err(|e| format!("Cannot position layout input: {e}"))?;
        let unit = bounds.height / 100.0;
        for (id, x, y, width, height) in [
            (100, 12.0, 8.0, bounds.width / unit - 24.0, 28.0),
            (101, 12.0, 46.0, bounds.width / unit - 204.0, 34.0),
            (1, bounds.width / unit - 180.0, 46.0, 78.0, 34.0),
            (2, bounds.width / unit - 90.0, 46.0, 78.0, 34.0),
        ] {
            if live && id != 101 {
                continue;
            }
            let child = GetDlgItem(Some(hwnd), id)
                .map_err(|e| format!("Cannot locate layout input: {e}"))?;
            MoveWindow(
                child,
                if live { 0 } else { (x * unit) as i32 },
                if live { 0 } else { (y * unit) as i32 },
                if live {
                    bounds.width as i32
                } else {
                    (width.max(1.0) * unit) as i32
                },
                if live {
                    bounds.height as i32
                } else {
                    (height * unit) as i32
                },
                true,
            )
            .map_err(|e| format!("Cannot size layout input: {e}"))?;
        }
        if let Some(font) = font_handle {
            SendDlgItemMessageW(hwnd, 101, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
        SendDlgItemMessageW(
            hwnd,
            101,
            windows::Win32::UI::Controls::EM_LIMITTEXT,
            WPARAM(max_units),
            LPARAM(0),
        );
        SendDlgItemMessageW(
            hwnd,
            101,
            windows::Win32::UI::Controls::EM_SETCUEBANNER,
            WPARAM(0),
            LPARAM(placeholder.as_ptr() as isize),
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    focus_editor(hwnd)?;
    Ok(())
}

fn focus_editor(hwnd: HWND) -> Result<(), String> {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, GetFocus, SetFocus};
    // SAFETY: called on the dialog's owning thread after creation or reuse.
    // A rejected foreground request uses a synchronous input-queue handoff.
    // Every successful attachment is detached before any return.
    unsafe {
        let edit =
            GetDlgItem(Some(hwnd), 101).map_err(|e| format!("Cannot find search input: {e}"))?;
        let current = GetCurrentThreadId();
        let foreground = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let mut attached = false;
        if !SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() != hwnd {
            attached = foreground != 0
                && foreground != current
                && AttachThreadInput(current, foreground, true).as_bool();
            if attached {
                let _ = SetForegroundWindow(hwnd);
            }
        }
        let _ = SetFocus(Some(edit));
        if attached {
            let _ = AttachThreadInput(current, foreground, false);
        }
        // Windows can report no foreground while activation is transitioning
        // (and on a noninteractive desktop). Our queue must still own both
        // the active dialog and its edit; a different foreground is a failure.
        let foreground_window = GetForegroundWindow();
        if (!foreground_window.is_invalid() && foreground_window != hwnd)
            || GetActiveWindow() != hwnd
            || GetFocus() != edit
        {
            return Err(format!(
                "Cannot focus search input (foreground={:?}, dialog={hwnd:?}, focus={:?}, edit={edit:?}, attached={attached})",
                GetForegroundWindow(),
                GetFocus()
            ));
        }
    }
    Ok(())
}

extern "system" fn dialog_proc(hwnd: HWND, message: u32, wparam: WPARAM, _: LPARAM) -> isize {
    match message {
        WM_CTLCOLOREDIT | WM_CTLCOLORDLG => {
            use windows::Win32::Graphics::Gdi::*;
            ACTIVE.with(|active| {
                let active = active.borrow();
                let Some(style) = active
                    .as_ref()
                    .and_then(|active| active.request.live_style.as_ref())
                else {
                    return 0;
                };
                let color = |c: crate::api::Color| {
                    windows::Win32::Foundation::COLORREF(
                        u32::from(c.r) | u32::from(c.g) << 8 | u32::from(c.b) << 16,
                    )
                };
                // SAFETY: WM_CTLCOLOR supplies this live paint DC. DC_BRUSH is a system-owned stock object.
                unsafe {
                    let dc = HDC(wparam.0 as *mut _);
                    SetTextColor(dc, color(style.text_color));
                    SetBkColor(dc, color(style.background));
                    SetDCBrushColor(dc, color(style.background));
                    GetStockObject(DC_BRUSH).0 as isize
                }
            })
        }
        WM_COMMAND if wparam.0 & 0xffff == 101 && (wparam.0 >> 16) as u32 == EN_CHANGE => {
            let live = ACTIVE.with(|active| {
                active
                    .borrow()
                    .as_ref()
                    .filter(|a| a.request.live_style.is_some())
                    .map(|a| (a.request.id, a.request.max_chars))
            });
            if live.is_some()
                && let Some((id, text)) = read_editor_text(hwnd)
            {
                super::status_item::emit(BackendEvent::TextPromptChanged { id, text });
            }
            1
        }
        WM_INITDIALOG => 1,
        WM_CLOSE => {
            close(None);
            1
        }
        WM_COMMAND if wparam.0 & 0xffff == 2 => {
            close(None);
            1
        }
        WM_COMMAND if wparam.0 & 0xffff == 1 => {
            if let Some((_, text)) = read_editor_text(hwnd) {
                close(Some(text));
            }
            1
        }
        _ => 0,
    }
}

fn read_editor_text(hwnd: HWND) -> Option<(u64, String)> {
    let (id, max, mut buffer) = ACTIVE.with(|active| {
        active.borrow_mut().as_mut().map(|a| {
            (
                a.request.id,
                a.request.max_chars,
                std::mem::take(&mut a.text_buffer),
            )
        })
    })?;
    buffer.resize(max.saturating_mul(2).saturating_add(1), 0);
    // SAFETY: dialog owns edit 101 and the UTF-16 buffer. No RefCell borrow
    // spans the synchronous native read; capacity is returned to its request.
    let len = unsafe { GetDlgItemTextW(hwnd, 101, &mut buffer) } as usize;
    let text =
        crate::api::window_presets::bounded_text(String::from_utf16_lossy(&buffer[..len]), max);
    ACTIVE.with(|active| {
        if let Some(a) = active.borrow_mut().as_mut().filter(|a| a.request.id == id) {
            a.text_buffer = buffer;
        }
    });
    Some((id, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Creates disposable native note dialogs; run explicitly on an interactive desktop"]
    fn native_note_dialog_preserves_unicode_and_cancels_owned_windows() {
        let (sender, receiver) = crate::platform::common::event_queue::channel();
        *super::super::status_item::SENDER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap() = Some(super::super::EventSender::without_wake(sender));
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                shutdown();
                *super::super::status_item::SENDER
                    .get()
                    .unwrap()
                    .lock()
                    .unwrap() = None;
            }
        }
        let _cleanup = Cleanup;
        let prompt = TextPrompt {
            edit_keys: Default::default(),
            live_style: None,
            copy_keys: Default::default(),
            bounds: crate::api::Rect::new(80.0, 600.0, 720.0, 100.0),
            id: 1,
            title: "KeySteer disposable note probe".into(),
            message: "Native note test".into(),
            placeholder: "Optional note".into(),
            max_chars: 80,
        };
        show(prompt.clone()).unwrap();
        let hwnd = ACTIVE.with(|a| a.borrow().as_ref().unwrap().hwnd);
        let expected = "中文备注 · Coding 🦀";
        // SAFETY: this ignored probe owns the same-thread dialog and edit child;
        // the temporary UTF-16 string lives for the synchronous text copy.
        unsafe {
            assert_eq!(
                GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION.0,
                0,
                "inline input has no dialog title bar"
            );
            let mut bounds = windows::Win32::Foundation::RECT::default();
            GetWindowRect(hwnd, &mut bounds).unwrap();
            assert_eq!(
                (bounds.left, bounds.top, bounds.right, bounds.bottom),
                (80, 600, 800, 700)
            );
            SetDlgItemTextW(hwnd, 101, &windows::core::HSTRING::from(expected)).unwrap();
        }
        dialog_proc(hwnd, WM_COMMAND, WPARAM(1), LPARAM(0));
        assert!(ACTIVE.with(|a| a.borrow().is_none()));
        assert!(
            matches!(receiver.recv_timeout(std::time::Duration::from_secs(1)).unwrap(), BackendEvent::TextPromptResult { id: 1, value: Ok(Some(text)) } if text == expected)
        );
        show(TextPrompt { id: 2, ..prompt }).unwrap();
        shutdown();
        assert!(matches!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            BackendEvent::TextPromptResult {
                id: 2,
                value: Ok(None)
            }
        ));
        assert!(ACTIVE.with(|a| a.borrow().is_none()));
    }

    #[test]
    #[ignore = "native live editor probe; creates a disposable editor"]
    fn native_search_editor_reuses_then_releases_window_and_unicode_state() {
        let (sender, receiver) = crate::platform::common::event_queue::channel();
        *super::super::status_item::SENDER
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .unwrap() = Some(super::super::EventSender::without_wake(sender));
        let mut window = None;
        for id in 10..20 {
            show(TextPrompt {
                edit_keys: Default::default(),
                id,
                bounds: crate::api::Rect::new(80.0, 600.0, 420.0, 48.0),
                live_style: Some(crate::api::overlay::LabelStyle::default().into()),
                copy_keys: Default::default(),
                title: "Search test".into(),
                message: String::new(),
                placeholder: String::new(),
                max_chars: 4096,
            })
            .unwrap();
            let hwnd = ACTIVE.with(|a| a.borrow().as_ref().unwrap().hwnd);
            if let Some(window) = window {
                assert_eq!(hwnd, window);
            }
            window = Some(hwnd);
            // SAFETY: this explicit probe owns this dialog and its native edit child.
            unsafe {
                let foreground = GetForegroundWindow();
                assert!(foreground.is_invalid() || foreground == hwnd);
                assert_eq!(
                    windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow(),
                    hwnd
                );
                assert_eq!(
                    windows::Win32::UI::Input::KeyboardAndMouse::GetFocus(),
                    GetDlgItem(Some(hwnd), 101).unwrap()
                );
                SetDlgItemTextW(hwnd, 101, windows::core::w!("复制 Paste 🦀 /标签")).unwrap();
            }
            let changed = receiver
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            assert!(
                matches!(changed, BackendEvent::TextPromptChanged { id: actual, text } if actual == id && text == "复制 Paste 🦀 /标签")
            );
            // SAFETY: the probe's dialog still owns edit 101 on this thread.
            let edit = unsafe { GetDlgItem(Some(hwnd), 101) }.unwrap();
            let message = MSG {
                hwnd: edit,
                message: WM_KEYDOWN,
                wParam: WPARAM(13),
                ..Default::default()
            };
            assert!(dispatch(&message));
            assert!(
                matches!(receiver.recv_timeout(std::time::Duration::from_secs(1)).unwrap(), BackendEvent::TextPromptResult { id: actual, value: Ok(Some(text)) } if actual == id && text == "复制 Paste 🦀 /标签")
            );
            assert!(ACTIVE.with(|a| a.borrow().is_none()));
            assert!(CACHED.with(|a| a.borrow().is_some()));
        }
        shutdown();
        assert!(CACHED.with(|a| a.borrow().is_none()));
        // SAFETY: IsWindow only queries the former handle; it does not dereference it.
        assert!(!unsafe { IsWindow(window) }.as_bool());
        *super::super::status_item::SENDER
            .get()
            .unwrap()
            .lock()
            .unwrap() = None;
    }
}
