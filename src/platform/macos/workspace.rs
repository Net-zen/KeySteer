//! Cached workspace state invalidated by native focus and appearance notices.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSWorkspace,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidDeactivateApplicationNotification,
    NSWorkspaceDidTerminateApplicationNotification,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions, NSNotification,
    NSNotificationCenter, NSObjectNSKeyValueObserverRegistration, NSString, ns_string,
};

use crate::api::backend::{Appearance, BackendEvent};
use crate::api::command::FocusedApp;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeySteerWorkspaceObserver"]
    #[ivars = AtomicBool]
    struct Observer;

    impl Observer {
        #[unsafe(method(workspaceChanged:))]
        fn workspace_changed(&self, _notification: &NSNotification) {
            self.invalidate();
        }

        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn appearance_changed(&self, _key: Option<&NSString>, _object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>, _context: *mut c_void) {
            self.invalidate();
        }
    }
);

impl Observer {
    fn invalidate(&self) {
        // Never call the engine from a notification; a turn may already own it.
        self.ivars().store(true, Ordering::Release);
        super::event_loop::wake_main_run_loop();
    }
}

pub struct Workspace {
    focused: Option<FocusedApp>,
    appearance: Appearance,
    observer: Retained<Observer>,
    center: Retained<NSNotificationCenter>,
    application: Retained<NSApplication>,
    appearances: Retained<NSArray<NSString>>,
}

impl Workspace {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let observer = mtm.alloc::<Observer>().set_ivars(AtomicBool::new(false));
        // SAFETY: initialize the NSObject superclass after setting Rust ivars.
        let observer: Retained<Observer> = unsafe { msg_send![super(observer), init] };
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let application = NSApplication::sharedApplication(mtm);
        // SAFETY: the retained main-thread observer implements these exact
        // selector/KVO signatures. Only NSApp.effectiveAppearance is observed,
        // with null context; Drop removes both registrations before release.
        // Appearance-name constants are immutable framework-owned strings.
        let appearances = unsafe {
            for name in [
                NSWorkspaceDidActivateApplicationNotification,
                NSWorkspaceDidDeactivateApplicationNotification,
                NSWorkspaceDidTerminateApplicationNotification,
            ] {
                center.addObserver_selector_name_object(
                    &observer,
                    sel!(workspaceChanged:),
                    Some(name),
                    None,
                );
            }
            let object: &NSObject = &application;
            object.addObserver_forKeyPath_options_context(
                &observer,
                ns_string!("effectiveAppearance"),
                NSKeyValueObservingOptions::empty(),
                std::ptr::null_mut(),
            );
            NSArray::from_slice(&[NSAppearanceNameDarkAqua, NSAppearanceNameAqua])
        };
        Self {
            focused: focused_app(),
            appearance: appearance(&application, &appearances),
            observer,
            center,
            application,
            appearances,
        }
    }

    pub fn focused_app(&self) -> Option<FocusedApp> {
        self.focused.clone()
    }

    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    pub fn refresh(&mut self) -> Vec<BackendEvent> {
        if !self.observer.ivars().swap(false, Ordering::AcqRel) {
            return Vec::new();
        }

        let mut events = Vec::new();
        let focused_pid = frontmost_process_id();
        if focused_pid != self.focused.as_ref().map(|app| app.process_id) {
            let focused = focused_app();
            if focused != self.focused {
                self.focused = focused.clone();
                events.push(BackendEvent::FocusChanged(focused));
            }
        }
        let appearance = appearance(&self.application, &self.appearances);
        if appearance != self.appearance {
            self.appearance = appearance;
            events.push(BackendEvent::AppearanceChanged(appearance));
        }
        events
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        // SAFETY: these are the exact live observer/object/key registrations
        // installed in new, removed on their owning main thread before release.
        unsafe {
            self.center.removeObserver(&self.observer);
            let object: &NSObject = &self.application;
            object.removeObserver_forKeyPath(&self.observer, ns_string!("effectiveAppearance"));
        }
    }
}

fn focused_app() -> Option<FocusedApp> {
    autoreleasepool(|_| {
        let application = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let bundle_id = application.bundleIdentifier()?.to_string();
        let process_id = u32::try_from(application.processIdentifier()).ok()?;
        Some(FocusedApp {
            bundle_id,
            window_title: String::new(),
            process_id,
        })
    })
}

fn appearance(application: &NSApplication, appearances: &NSArray<NSString>) -> Appearance {
    autoreleasepool(|_| {
        // Read the property that generated the notice, not a defaults cache
        // which may update at a different point in an automatic theme change.
        if application
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(appearances)
            .is_some_and(|style| style.isEqualToString(&appearances.objectAtIndex(0)))
        {
            Appearance::Dark
        } else {
            Appearance::Light
        }
    })
}

fn frontmost_process_id() -> Option<u32> {
    autoreleasepool(|_| {
        let application = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        u32::try_from(application.processIdentifier()).ok()
    })
}
