//! Native top-status-item controls.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSBackingStoreType, NSButton, NSCellImagePosition,
    NSControlStateValueOff, NSControlStateValueOn, NSFont, NSImage, NSImageView, NSMenu,
    NSMenuItem, NSPanel, NSScreenSaverWindowLevel, NSSquareStatusItemLength, NSStatusBar,
    NSStatusItem, NSTextField, NSView, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSData, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL};

use crate::api::Autostart;
use crate::api::backend::{BackendEvent, UpdateCheckResult, UpdateProgress};

use super::EventSender;

static SENDER: OnceLock<Mutex<Option<EventSender>>> = OnceLock::new();

const STATUS_ICON_ATTACH_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const STATUS_ICON_ATTACH_RETRY_ATTEMPTS: u8 = 120;
const STATUS_ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/keysteer-icon.png");
const STATUS_ICON_SIZE: f64 = 18.0;

struct StatusTargetIvars {
    terminating: Cell<bool>,
    note: RefCell<Option<NotePanel>>,
    cached_note: RefCell<Option<NotePanel>>,
    update_alert: RefCell<Option<Retained<NSPanel>>>,
    downloaded_update: RefCell<Option<PathBuf>>,
}

struct NotePanel {
    id: u64,
    panel: Retained<NSPanel>,
    field: Retained<NSTextField>,
    live: bool,
    request: crate::api::window_presets::TextPrompt,
    previous: Option<Retained<objc2_app_kit::NSRunningApplication>>,
}

define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeySteerInlineInputPanel"]
    struct InlineInputPanel;
    impl InlineInputPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool { true }

        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &objc2_app_kit::NSEvent) -> bool {
            use objc2_app_kit::NSEventModifierFlags as Flags;
            let modifiers = event.modifierFlags() & (Flags::Command | Flags::Control | Flags::Option | Flags::Shift);
            let action = if modifiers == Flags::Command || modifiers == (Flags::Command | Flags::Shift) {
                event.charactersIgnoringModifiers().and_then(|text| {
                    let key = text.to_string().to_lowercase();
                    match (key.as_str(), modifiers.contains(Flags::Shift)) {
                        ("a", false) => Some(sel!(selectAll:)), ("c", false) => Some(sel!(copy:)),
                        ("v", false) => Some(sel!(paste:)), ("x", false) => Some(sel!(cut:)),
                        ("z", false) => Some(sel!(undo:)), ("z", true) => Some(sel!(redo:)), _ => None,
                    }
                })
            } else { None };
            // SAFETY: standard AppKit action signatures route through the live
            // first responder. Unhandled events delegate to the NSPanel superclass.
            unsafe {
                if let Some(action) = action
                    && NSApplication::sharedApplication(self.mtm()).sendAction_to_from(action, None, Some(self)) {
                    true
                } else {
                    msg_send![super(self), performKeyEquivalent: event]
                }
            }
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeySteerStatusTarget"]
    #[ivars = StatusTargetIvars]
    struct StatusTarget;

    // SAFETY: NSObject is initialized in StatusTarget::new.
    unsafe impl NSObjectProtocol for StatusTarget {}

    // SAFETY: The delegate is main-thread-only and retained for the application's lifetime.
    unsafe impl NSApplicationDelegate for StatusTarget {
        #[unsafe(method(applicationDidBecomeActive:))]
        fn application_did_become_active(&self, _notification: &objc2_foundation::NSNotification) {
            // Activation is asynchronous. Re-establish the editor after AppKit
            // delivers activation, without holding a borrow through callbacks.
            let editor = self.ivars().note.borrow().as_ref()
                .map(|note| (note.id, note.panel.clone(), note.field.clone()));
            if let Some((id, panel, field)) = editor {
                panel.makeKeyAndOrderFront(None);
                if !panel.makeFirstResponder(Some(&field)) {
                    let note = self.ivars().note.borrow_mut().take();
                    if let Some(note) = note { note.panel.close(); }
                    emit(BackendEvent::TextPromptResult { id, value: Err("Text input rejected first responder after activation".into()) });
                }
            }
        }
        #[unsafe(method(applicationShouldTerminate:))]
        fn application_should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            self.ivars().terminating.set(true);
            emit(BackendEvent::Quit);
            NSApplicationTerminateReply::TerminateLater
        }
    }

    impl StatusTarget {
        #[unsafe(method(controlTextDidChange:))]
        fn search_text_changed(&self, _notification: &AnyObject) {
            let editor = self.ivars().note.borrow().as_ref().filter(|note| note.live)
                .map(|note| (note.id, note.field.clone(), note.request.max_chars));
            if let Some((id, field, max_chars)) = editor {
                emit(BackendEvent::TextPromptChanged { id, text: crate::api::window_presets::bounded_text(field.stringValue().to_string(), max_chars) });
            }
        }
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn search_edit_command(&self, _control: &AnyObject, _view: &AnyObject, command: objc2::runtime::Sel) -> bool {
            if command == sel!(cancelOperation:) {
                self.finish_note(false);
                true
            } else { false }
        }
        #[unsafe(method(saveLayoutNote:))]
        fn save_layout_note(&self, _sender: Option<&AnyObject>) { self.finish_note(true); }
        #[unsafe(method(cancelLayoutNote:))]
        fn cancel_layout_note(&self, _sender: Option<&AnyObject>) { self.finish_note(false); }
        #[unsafe(method(toggleEnabled:))]
        fn toggle_enabled(&self, _sender: Option<&AnyObject>) {
            emit(BackendEvent::ToggleEnabled);
        }

        #[unsafe(method(reloadConfig:))]
        fn reload_config(&self, _sender: Option<&AnyObject>) {
            emit(BackendEvent::ReloadConfig);
        }

        #[unsafe(method(openConfigSimulator:))]
        fn open_config_simulator(&self, _sender: Option<&AnyObject>) {
            emit(BackendEvent::OpenConfigSimulator);
        }

        #[unsafe(method(toggleAutostart:))]
        fn toggle_autostart(&self, _sender: Option<&AnyObject>) {
            emit(BackendEvent::ToggleAutostart);
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: Option<&AnyObject>) {
            emit(BackendEvent::CheckForUpdates);
        }

        #[unsafe(method(showAbout:))]
        fn show_about(&self, _sender: Option<&AnyObject>) {
            if let Err(error) = show_panel(
                self.mtm(),
                self,
                "About KeySteer",
                &crate::platform::common::app_info::details(),
                PanelAction::OpenRepository,
            ) {
                crate::support::logging::report_error("macos-about", error);
            }
        }

        #[unsafe(method(openRepository:))]
        fn open_repository(&self, _sender: Option<&AnyObject>) {
            if let Err(error) = open_https_url(crate::platform::common::app_info::REPOSITORY_URL) {
                crate::support::logging::report_error("macos-about", error);
            }
        }

        #[unsafe(method(dismissUpdateAlert:))]
        fn dismiss_update_alert_action(&self, _sender: Option<&AnyObject>) {
            self.dismiss_update_alert();
        }

        #[unsafe(method(showDownloadedUpdate:))]
        fn show_downloaded_update(&self, _sender: Option<&AnyObject>) {
            let downloaded_update = self.ivars().downloaded_update.borrow();
            let Some(path) = downloaded_update.as_deref() else {
                return;
            };
            let full_path = NSString::from_str(&path.to_string_lossy());
            let root_path = NSString::from_str(
                &path
                    .parent()
                    .unwrap_or(path)
                    .to_string_lossy(),
            );
            if !NSWorkspace::sharedWorkspace()
                .selectFile_inFileViewerRootedAtPath(Some(&full_path), &root_path)
            {
                crate::support::logging::report_error(
                    "macos-update",
                    format!("Finder could not reveal {}", path.display()),
                );
            }
        }

        #[unsafe(method(quitApplication:))]
        fn quit_application(&self, _sender: Option<&AnyObject>) {
            emit(BackendEvent::Quit);
        }
    }
);

impl StatusTarget {
    fn finish_note(&self, save: bool) {
        let note = self.ivars().note.borrow_mut().take();
        if let Some(note) = note {
            let id = note.id;
            let value = save.then(|| {
                crate::api::window_presets::bounded_text(
                    note.field.stringValue().to_string(),
                    note.request.max_chars,
                )
            });
            if note.live {
                note.panel.orderOut(None);
                if NSApplication::sharedApplication(self.mtm()).isActive()
                    && let Some(previous) = &note.previous
                {
                    #[allow(deprecated)]
                    previous.activateWithOptions(
                        objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps,
                    );
                }
                *self.ivars().cached_note.borrow_mut() = Some(note);
            } else {
                note.panel.close();
            }
            emit(BackendEvent::TextPromptResult {
                id,
                value: Ok(value),
            });
        }
    }
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this: Allocated<Self> = mtm.alloc();
        let this = this.set_ivars(StatusTargetIvars {
            terminating: Cell::new(false),
            note: RefCell::new(None),
            cached_note: RefCell::new(None),
            update_alert: RefCell::new(None),
            downloaded_update: RefCell::new(None),
        });
        // SAFETY: The NSObject superclass is initialized after Rust ivars.
        unsafe { msg_send![super(this), init] }
    }

    fn show_update_alert(&self, alert: Retained<NSPanel>, downloaded_update: Option<PathBuf>) {
        self.dismiss_update_alert();
        *self.ivars().downloaded_update.borrow_mut() = downloaded_update;
        *self.ivars().update_alert.borrow_mut() = Some(alert);
        let alert = self.ivars().update_alert.borrow();
        if let Some(window) = alert.as_ref() {
            window.center();
            NSApplication::sharedApplication(self.mtm()).activate();
            window.makeKeyAndOrderFront(None);
        }
    }

    fn dismiss_update_alert(&self) {
        self.ivars().downloaded_update.borrow_mut().take();
        if let Some(alert) = self.ivars().update_alert.borrow_mut().take() {
            alert.close();
        }
    }
}

pub struct StatusItem {
    item: Retained<NSStatusItem>,
    _menu: Retained<NSMenu>,
    _icon: Option<Retained<NSImage>>,
    _target: Retained<StatusTarget>,
    toggle_item: Retained<NSMenuItem>,
    autostart_item: Retained<NSMenuItem>,
    update_item: Retained<NSMenuItem>,
    enabled: bool,
    icon_attach_retry: Option<IconAttachRetry>,
}

struct IconAttachRetry {
    next_attempt: Instant,
    attempts_remaining: u8,
}

/// Finish AppKit startup on the main thread before creating the top status
/// item. The backend owns subsequent event dispatch, so the portable runtime
/// remains unaware of NSApplication.
pub(super) fn prepare_application(mtm: MainThreadMarker) -> Result<(), String> {
    let application = NSApplication::sharedApplication(mtm);
    if !application.setActivationPolicy(NSApplicationActivationPolicy::Accessory)
        && application.activationPolicy() != NSApplicationActivationPolicy::Accessory
    {
        return Err("AppKit rejected the accessory activation policy".into());
    }
    application.finishLaunching();
    Ok(())
}

impl StatusItem {
    pub(super) fn release_text_prompt(&self) {
        self._target.finish_note(false);
        let cached = self._target.ivars().cached_note.borrow_mut().take();
        if let Some(note) = cached {
            note.panel.close();
        }
    }
    pub(super) fn cancel_text_prompt(&self, id: u64) {
        if self
            ._target
            .ivars()
            .note
            .borrow()
            .as_ref()
            .is_some_and(|n| n.id == id)
        {
            self._target.finish_note(false);
        }
    }
    pub(super) fn request_text_prompt(
        &self,
        request: crate::api::window_presets::TextPrompt,
    ) -> Result<(), String> {
        let target = &self._target;
        target.finish_note(false);
        let mtm = target.mtm();
        let previous = NSWorkspace::sharedWorkspace().frontmostApplication();
        // AppKit calls below can synchronously re-enter finish_note. A temporary
        // borrow in an if-let scrutinee would remain live across those calls.
        let cached = target.ivars().cached_note.borrow_mut().take();
        if let Some(mut note) = cached {
            if request.live_style.is_some()
                && note.request.live_style == request.live_style
                && note.request.bounds == request.bounds
            {
                note.id = request.id;
                note.request = request;
                note.previous = previous;
                note.field.setStringValue(&NSString::from_str(""));
                let panel = note.panel.clone();
                let field = note.field.clone();
                *target.ivars().note.borrow_mut() = Some(note);
                return focus_text_prompt(target, &panel, &field);
            }
            note.panel.close();
        }
        let screens = super::screens::list_screens()?;
        let primary = crate::api::Screen::primary(&screens).ok_or("No display for layout input")?;
        let live = request.live_style.is_some();
        let bounds = request.live_style.as_ref().map_or(request.bounds, |style| {
            request.bounds.inset(
                style.padding_x + style.border_width,
                style.padding_y + style.border_width,
            )
        });
        let rect = NSRect::new(
            NSPoint::new(bounds.x, primary.bounds.bottom() - bounds.bottom()),
            NSSize::new(bounds.width, bounds.height),
        );
        let allocated = InlineInputPanel::alloc(mtm).set_ivars(());
        // SAFETY: initializes this retained NSPanel subclass on the AppKit thread.
        let panel: Retained<InlineInputPanel> = unsafe {
            msg_send![super(allocated),
            initWithContentRect: rect, styleMask: NSWindowStyleMask::Borderless,
            backing: NSBackingStoreType::Buffered, defer: false]
        };
        if panel.isReleasedWhenClosed() {
            return Err("Note panel has ambiguous ownership".into());
        }
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(false);
        panel.setTitle(&NSString::from_str(&request.title));
        // The shared search shell lives in the screen-saver-level overlay.
        // Its native text/caret must be above that surface, including fullscreen.
        panel.setLevel(NSScreenSaverWindowLevel + 1);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.setHasShadow(false);
        let content = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), rect.size),
        );
        let label =
            NSTextField::wrappingLabelWithString(&NSString::from_str(&request.message), mtm);
        label.setFrame(NSRect::new(
            NSPoint::new(12.0, 62.0),
            NSSize::new(bounds.width - 24.0, 28.0),
        ));
        if !live {
            content.addSubview(&label);
        }
        let field = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
        field.setPlaceholderString(Some(&NSString::from_str(&request.placeholder)));
        field.setFrame(NSRect::new(
            NSPoint::new(12.0, 20.0),
            NSSize::new((bounds.width - 204.0).max(40.0), 30.0),
        ));
        if let Some(style) = &request.live_style {
            field.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), rect.size));
            field.setBezeled(false);
            field.setBordered(false);
            let color = |c: crate::api::Color| {
                objc2_app_kit::NSColor::colorWithSRGBRed_green_blue_alpha(
                    f64::from(c.r) / 255.0,
                    f64::from(c.g) / 255.0,
                    f64::from(c.b) / 255.0,
                    f64::from(c.a) / 255.0,
                )
            };
            field.setBackgroundColor(Some(&color(style.background)));
            field.setTextColor(Some(&color(style.text_color)));
            panel.setBackgroundColor(Some(&color(style.background)));
            let font = if style.font_family.is_empty() {
                None
            } else {
                NSFont::fontWithName_size(&NSString::from_str(&style.font_family), style.font_size)
            }
            .unwrap_or_else(|| NSFont::systemFontOfSize(style.font_size));
            field.setFont(Some(&font));
            // SAFETY: the retained status target implements these action/delegate selectors
            // and outlives the field; no Rust borrows cross a callback.
            unsafe {
                field.setTarget(Some(&**target));
                field.setAction(Some(sel!(saveLayoutNote:)));
                let _: () = msg_send![&*field, setDelegate: &**target];
            }
        }
        content.addSubview(&field);
        for (title, selector, x, key) in [
            ("Save", sel!(saveLayoutNote:), bounds.width - 180.0, "\r"),
            (
                "Cancel",
                sel!(cancelLayoutNote:),
                bounds.width - 90.0,
                "\u{1b}",
            ),
        ] {
            if live {
                continue;
            }
            // SAFETY: selectors are implemented above and the retained status target
            // outlives all buttons. AppKit retains no temporary Rust references.
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str(title),
                    Some(&**target),
                    Some(selector),
                    mtm,
                )
            };
            button.setFrame(NSRect::new(NSPoint::new(x, 20.0), NSSize::new(86.0, 30.0)));
            button.setKeyEquivalent(&NSString::from_str(key));
            content.addSubview(&button);
        }
        panel.setContentView(Some(&content));
        *target.ivars().note.borrow_mut() = Some(NotePanel {
            live,
            request: request.clone(),
            previous,
            id: request.id,
            panel: Retained::into_super(panel.clone()),
            field: field.clone(),
        });
        focus_text_prompt(target, &panel, &field)
    }
    pub(super) fn new(mtm: MainThreadMarker, sender: EventSender) -> Self {
        *SENDER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(sender);

        let target = StatusTarget::new(mtm);
        NSApplication::sharedApplication(mtm).setDelegate(Some(ProtocolObject::from_ref(&*target)));
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);

        let toggle_item = menu_item(mtm, "Pause", sel!(toggleEnabled:), &target);
        let reload_item = menu_item(mtm, "Reload Configuration", sel!(reloadConfig:), &target);
        let simulator_item = menu_item(
            mtm,
            "Configuration & Simulator...",
            sel!(openConfigSimulator:),
            &target,
        );
        let autostart_item = menu_item(mtm, "Start at Login", sel!(toggleAutostart:), &target);
        let update_item = menu_item(mtm, "Check for Updates...", sel!(checkForUpdates:), &target);
        let about_item = menu_item(mtm, "About KeySteer...", sel!(showAbout:), &target);
        let autostart_enabled = match super::autostart::MacosAutostart::new().is_enabled() {
            Ok(enabled) => enabled,
            Err(error) => {
                crate::support::logging::report_error("macos-autostart", error);
                false
            }
        };
        set_checked(&autostart_item, autostart_enabled);
        let quit_item = menu_item(mtm, "Quit KeySteer", sel!(quitApplication:), &target);
        menu.addItem(&toggle_item);
        menu.addItem(&reload_item);
        menu.addItem(&simulator_item);
        menu.addItem(&autostart_item);
        menu.addItem(&update_item);
        menu.addItem(&about_item);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&quit_item);

        let icon = status_icon(STATUS_ICON_SIZE);
        let (item, icon_attached) = create_native_item(mtm, &menu, icon.as_deref());

        Self {
            item,
            _menu: menu,
            _icon: icon,
            _target: target,
            toggle_item,
            autostart_item,
            update_item,
            enabled: true,
            icon_attach_retry: (!icon_attached).then(|| IconAttachRetry {
                next_attempt: Instant::now(),
                attempts_remaining: STATUS_ICON_ATTACH_RETRY_ATTEMPTS,
            }),
        }
    }

    /// Complete icon attachment when a cold login creates the status item
    /// before its native button is hosted. This reuses Backend::poll and never
    /// creates a timer, thread, item replacement, or visibility loop.
    pub(super) fn maintain_icon_attachment(&mut self) {
        let Some(retry) = self.icon_attach_retry.as_mut() else {
            return;
        };
        let now = Instant::now();
        if now < retry.next_attempt {
            return;
        }
        if let Some(button) = self.item.button(self._target.mtm()) {
            configure_status_button(&button, self._icon.as_deref());
            self.icon_attach_retry = None;
            return;
        }
        retry.attempts_remaining = retry.attempts_remaining.saturating_sub(1);
        if retry.attempts_remaining == 0 {
            self.icon_attach_retry = None;
            crate::support::logging::report_error(
                "macos-status-item",
                "AppKit did not attach a button to the top status item after login",
            );
        } else {
            retry.next_attempt = now + STATUS_ICON_ATTACH_RETRY_INTERVAL;
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.toggle_item.setTitle(&NSString::from_str(if enabled {
            "Pause"
        } else {
            "Resume"
        }));
    }

    pub(super) fn set_autostart_enabled(&mut self, enabled: bool) {
        set_checked(&self.autostart_item, enabled);
    }

    pub(super) fn present_update_progress(&mut self, progress: &UpdateProgress) {
        let title = match progress {
            UpdateProgress::Checking => "Checking for Updates...".to_string(),
            UpdateProgress::Downloading { latest, percent } => {
                format!("Downloading KeySteer {latest}... {}%", (*percent).min(100))
            }
        };
        self.set_update_menu(&title, false);
    }

    pub(super) fn present_update_result(
        &mut self,
        result: &UpdateCheckResult,
    ) -> Result<(), String> {
        let title = match result {
            UpdateCheckResult::UpdateDownloaded { latest, .. } => {
                format!("KeySteer {latest} Downloaded")
            }
            UpdateCheckResult::UpToDate { current } => {
                format!("KeySteer {current} Is Up to Date")
            }
            UpdateCheckResult::Failed(_) => "Update Check Failed - Retry...".to_string(),
        };
        self.set_update_menu(&title, true);
        match result {
            UpdateCheckResult::UpdateDownloaded {
                current,
                latest,
                path,
            } => self.show_alert(
                "KeySteer update downloaded",
                &format!(
                    "KeySteer {latest} was saved to {}. Quit KeySteer, extract the ZIP, then move the new app to Applications to replace version {current}.",
                    path.display()
                ),
                Some(path),
            ),
            UpdateCheckResult::UpToDate { current } => self.show_alert(
                "KeySteer is up to date",
                &format!("KeySteer {current} is already the latest version."),
                None,
            ),
            UpdateCheckResult::Failed(error) => {
                self.show_alert("Could not check for updates", error, None)
            }
        }
    }

    fn set_update_menu(&self, title: &str, enabled: bool) {
        self.update_item.setTitle(&NSString::from_str(title));
        self.update_item.setEnabled(enabled);
    }

    fn show_alert(
        &self,
        title: &str,
        details: &str,
        downloaded_update: Option<&Path>,
    ) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            "update result must be presented on the macOS main thread".to_string()
        })?;
        let action = downloaded_update
            .map(PanelAction::RevealInFinder)
            .unwrap_or(PanelAction::None);
        show_panel(mtm, &self._target, title, details, action)
    }
}

#[derive(Clone, Copy)]
enum PanelAction<'a> {
    None,
    OpenRepository,
    RevealInFinder(&'a Path),
}

fn show_panel(
    mtm: MainThreadMarker,
    target: &StatusTarget,
    title: &str,
    details: &str,
    action: PanelAction<'_>,
) -> Result<(), String> {
    autoreleasepool(|_| {
        const WIDTH: f64 = 520.0;
        const HEIGHT: f64 = 210.0;
        let content_rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT));
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            content_rect,
            NSWindowStyleMask::Titled,
            NSBackingStoreType::Buffered,
            false,
        );
        if panel.isReleasedWhenClosed() {
            return Err(
                "macOS panel unexpectedly releases itself when closed; refusing ambiguous ownership"
                    .into(),
            );
        }

        panel.setTitle(&NSString::from_str("KeySteer"));
        panel.setFloatingPanel(true);
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(false);

        let content = NSView::initWithFrame(NSView::alloc(mtm), content_rect);
        if let Some(icon) = status_icon(64.0) {
            let image_view = NSImageView::imageViewWithImage(&icon, mtm);
            image_view.setFrame(NSRect::new(
                NSPoint::new(28.0, 118.0),
                NSSize::new(64.0, 64.0),
            ));
            content.addSubview(&image_view);
        }

        let title_label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
        title_label.setFont(Some(&NSFont::boldSystemFontOfSize(17.0)));
        title_label.setFrame(NSRect::new(
            NSPoint::new(112.0, 158.0),
            NSSize::new(380.0, 24.0),
        ));
        content.addSubview(&title_label);

        let details_label = NSTextField::wrappingLabelWithString(&NSString::from_str(details), mtm);
        details_label.setFrame(NSRect::new(
            NSPoint::new(112.0, 58.0),
            NSSize::new(380.0, 88.0),
        ));
        content.addSubview(&details_label);

        // SAFETY: both selectors are implemented by the retained target with
        // matching Objective-C signatures; AppKit retains no Rust borrow.
        let (button, secondary_button) = unsafe {
            let button = NSButton::buttonWithTitle_target_action(
                &NSString::from_str("OK"),
                Some(target),
                Some(sel!(dismissUpdateAlert:)),
                mtm,
            );
            let secondary_button = match action {
                PanelAction::None => None,
                PanelAction::OpenRepository => Some(NSButton::buttonWithTitle_target_action(
                    &NSString::from_str("KeySteer"),
                    Some(target),
                    Some(sel!(openRepository:)),
                    mtm,
                )),
                PanelAction::RevealInFinder(_) => Some(NSButton::buttonWithTitle_target_action(
                    &NSString::from_str("Show in Finder"),
                    Some(target),
                    Some(sel!(showDownloadedUpdate:)),
                    mtm,
                )),
            };
            (button, secondary_button)
        };
        button.setFrame(NSRect::new(
            NSPoint::new(412.0, 16.0),
            NSSize::new(80.0, 32.0),
        ));
        button.setKeyEquivalent(&NSString::from_str("\r"));
        content.addSubview(&button);

        if let Some(secondary_button) = secondary_button {
            secondary_button.setFrame(NSRect::new(
                NSPoint::new(276.0, 16.0),
                NSSize::new(124.0, 32.0),
            ));
            content.addSubview(&secondary_button);
        }

        panel.setContentView(Some(&content));
        let downloaded_update = match action {
            PanelAction::RevealInFinder(path) => Some(path.to_path_buf()),
            PanelAction::None | PanelAction::OpenRepository => None,
        };
        target.show_update_alert(panel, downloaded_update);
        Ok(())
    })
}

pub(super) fn open_https_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") || url.contains('\0') {
        return Err("macOS refused an invalid HTTPS URL".into());
    }
    let text = NSString::from_str(url);
    let url = NSURL::URLWithString(&text)
        .ok_or_else(|| "macOS could not parse the HTTPS URL".to_string())?;
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err("macOS could not open the default browser".into())
    }
}

fn create_native_item(
    mtm: MainThreadMarker,
    menu: &NSMenu,
    icon: Option<&NSImage>,
) -> (Retained<NSStatusItem>, bool) {
    let status_bar = NSStatusBar::systemStatusBar();
    let item = status_bar.statusItemWithLength(NSSquareStatusItemLength);
    // This is the status item's pop-up control menu, not NSApplication's main
    // menu. AppKit presents it when the user clicks the top status icon.
    item.setMenu(Some(menu));
    item.setVisible(true);
    let icon_attached = if let Some(button) = item.button(mtm) {
        configure_status_button(&button, icon);
        true
    } else {
        false
    };
    (item, icon_attached)
}

fn configure_status_button(button: &objc2_app_kit::NSStatusBarButton, icon: Option<&NSImage>) {
    button.setImagePosition(NSCellImagePosition::ImageOnly);
    if let Some(image) = icon {
        button.setImage(Some(image));
        button.setTitle(&NSString::from_str(""));
    } else if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str("cursorarrow.motionlines"),
        Some(&NSString::from_str("KeySteer")),
    ) {
        button.setImage(Some(&image));
        button.setTitle(&NSString::from_str(""));
    } else {
        button.setTitle(&NSString::from_str("KeySteer"));
    }
}

fn status_icon(size: f64) -> Option<Retained<NSImage>> {
    let data = NSData::with_bytes(STATUS_ICON_PNG);
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    image.setSize(NSSize::new(size, size));
    image.setTemplate(false);
    image.setAccessibilityDescription(Some(&NSString::from_str("KeySteer")));
    Some(image)
}

impl Drop for StatusItem {
    fn drop(&mut self) {
        self.release_text_prompt();
        self._target.dismiss_update_alert();
        if let Some(mutex) = SENDER.get() {
            *mutex.lock().unwrap_or_else(|error| error.into_inner()) = None;
        }
        if let Some(status_bar) = self.item.statusBar() {
            status_bar.removeStatusItem(&self.item);
        }
        let application = NSApplication::sharedApplication(self._target.mtm());
        application.setDelegate(None);
        if self._target.ivars().terminating.get() {
            // Engine flushes workspace statistics before shutting down the backend.
            application.replyToApplicationShouldTerminate(true);
        }
    }
}

fn focus_text_prompt(
    target: &StatusTarget,
    panel: &NSPanel,
    field: &NSTextField,
) -> Result<(), String> {
    panel.makeKeyAndOrderFront(None);
    // A global shortcut is an explicit activation request. AppKit can complete
    // it later; the application delegate restores the first responder then.
    #[allow(deprecated)]
    let accepted = objc2_app_kit::NSRunningApplication::currentApplication().activateWithOptions(
        objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps,
    );
    if accepted && panel.makeFirstResponder(Some(field)) {
        return Ok(());
    }
    // Do not leave a visible editor that would forward typing to another app.
    let note = target.ivars().note.borrow_mut().take();
    if let Some(note) = note {
        note.panel.close();
    }
    Err("Text input activation or first responder request was rejected".into())
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: objc2::runtime::Sel,
    target: &StatusTarget,
) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    item.setEnabled(true);
    // SAFETY: `action` names a method with the menu-item action signature and
    // `target` remains retained by StatusItem for the menu lifetime.
    unsafe {
        item.setAction(Some(action));
        item.setTarget(Some(target));
    }
    item
}

fn set_checked(item: &NSMenuItem, checked: bool) {
    item.setState(if checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

fn emit(event: BackendEvent) {
    let Some(sender) = SENDER
        .get()
        .and_then(|sender| sender.lock().ok())
        .and_then(|sender| sender.clone())
    else {
        return;
    };
    let _ = sender.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_actions_use_the_backend_event_channel() {
        let (sender, receiver) = crate::platform::common::event_queue::channel();
        *SENDER.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(EventSender::new(sender));
        emit(BackendEvent::ReloadConfig);
        assert!(matches!(
            receiver.recv().unwrap(),
            BackendEvent::ReloadConfig
        ));
        emit(BackendEvent::OpenConfigSimulator);
        assert!(matches!(
            receiver.recv().unwrap(),
            BackendEvent::OpenConfigSimulator
        ));
        emit(BackendEvent::CheckForUpdates);
        assert!(matches!(
            receiver.recv().unwrap(),
            BackendEvent::CheckForUpdates
        ));
        *SENDER.get().unwrap().lock().unwrap() = None;
    }
}
