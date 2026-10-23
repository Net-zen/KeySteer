//! AppKit display-synchronised frame delivery for continuous pointer movement.
//!
//! macOS 14's `NSView.displayLinkWithTarget:selector:` tracks the display that
//! contains the view. The backend rebinds it to the cursor's display panel when
//! the cursor crosses screens, without querying refresh rates. The
//! paused link is retained between gestures. Shared monotonic timing excludes
//! idle time and preserves elapsed time across delayed main-loop turns.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use crate::platform::common::frame_clock::FrameClockState;

use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::NSObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{NSEventTrackingRunLoopMode, NSModalPanelRunLoopMode, NSView};
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::CADisplayLink;

struct FrameTargetIvars {
    frames: RefCell<FrameClockState>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeySteerDisplayLinkTarget"]
    #[ivars = FrameTargetIvars]
    struct FrameTarget;

    impl FrameTarget {
        #[unsafe(method(displayFrame:))]
        fn display_frame(&self, _link: &CADisplayLink) {
            let mut frames = self.ivars().frames.borrow_mut();
            if let Some(generation) = frames.generation() {
                frames.publish(generation, Instant::now());
            }
        }
    }
);

impl FrameTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this: Allocated<Self> = mtm.alloc();
        let this = this.set_ivars(FrameTargetIvars {
            frames: RefCell::new(FrameClockState::default()),
        });
        // SAFETY: The NSObject superclass is initialized after Rust ivars.
        unsafe { msg_send![super(this), init] }
    }

    fn start(&self) {
        self.ivars().frames.borrow_mut().start(Instant::now());
    }

    fn pause(&self) {
        self.ivars().frames.borrow_mut().pause();
    }

    fn is_running(&self) -> bool {
        self.ivars().frames.borrow().generation().is_some()
    }

    fn take_elapsed(&self) -> Option<Duration> {
        self.ivars().frames.borrow_mut().take_elapsed()
    }
}

pub struct DisplayFrameClock {
    target: Retained<FrameTarget>,
    link: Option<Retained<CADisplayLink>>,
    source: Option<Retained<NSView>>,
}

impl DisplayFrameClock {
    pub fn new(mtm: MainThreadMarker) -> Self {
        Self {
            target: FrameTarget::new(mtm),
            link: None,
            source: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.target.is_running()
    }

    /// Create a paused link once its overlay view exists, before a held input.
    pub fn prepare(&mut self, source: &NSView) {
        if self
            .source
            .as_deref()
            .is_some_and(|current| std::ptr::eq(current, source))
        {
            return;
        }
        if let Some(link) = self.link.take() {
            // Preserve accumulated elapsed time while changing display cadence.
            link.invalidate();
        }
        self.source = Some(source.retain());
        // SAFETY: `displayFrame:` has the required `(CADisplayLink *)`
        // callback signature, and both target and source stay retained.
        let link =
            unsafe { source.displayLinkWithTarget_selector(&self.target, sel!(displayFrame:)) };
        link.setPaused(!self.is_running());
        // Match the runtime's native modes so all frame-driven modes continue
        // during menus, even when tracking is not in AppKit's common-mode set.
        // SAFETY: link, main loop, and process-lifetime mode tokens remain live.
        unsafe {
            for mode in [
                NSRunLoopCommonModes,
                NSEventTrackingRunLoopMode,
                NSModalPanelRunLoopMode,
            ] {
                link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), mode);
            }
        }
        self.link = Some(link);
    }

    pub fn start(&mut self, source: &NSView) {
        self.prepare(source);
        if !self.is_running() {
            self.target.start();
            if let Some(link) = &self.link {
                link.setPaused(false);
            }
        }
    }

    pub fn stop(&mut self) {
        if self.is_running() {
            self.target.pause();
            if let Some(link) = &self.link {
                link.setPaused(true);
            }
        }
    }

    /// Run the main run loop until AppKit delivers a display frame or the
    /// engine's own deadline expires. The deadline is a blocking bound, not a
    /// periodic movement timer.
    pub fn next(&self, timeout: Duration) -> Option<Duration> {
        autoreleasepool(|_| self.next_inner(timeout))
    }

    /// Read an already delivered frame without running AppKit or allocating.
    pub fn try_next(&self) -> Option<Duration> {
        self.target.take_elapsed()
    }

    fn next_inner(&self, timeout: Duration) -> Option<Duration> {
        if let Some(elapsed) = self.target.take_elapsed() {
            return Some(elapsed);
        }

        super::super::event_loop::wait_for_app_event(timeout);
        // Any non-frame AppKit wake returns control to Backend::poll so a
        // synchronously waiting hook is checked before another VBlank wait.
        self.target.take_elapsed()
    }
}

impl Drop for DisplayFrameClock {
    fn drop(&mut self) {
        if let Some(link) = self.link.take() {
            link.invalidate();
        }
    }
}
