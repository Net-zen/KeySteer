//! Drive the engine from both ordinary AppKit dispatch and nested menu tracking.
//!
//! Dispatching an NSEvent can enter a tracking loop for an arbitrary duration.
//! No engine/backend borrow is held across that dispatch. A common-mode observer
//! processes producer wakeups and display frames; a reusable timer honors the
//! engine's next deadline even when there are no native events.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use core_foundation::base::{TCFType, kCFAllocatorDefault};
use core_foundation::date::CFAbsoluteTimeGetCurrent;
use core_foundation::runloop::{
    CFRunLoop, CFRunLoopActivity, CFRunLoopObserver, CFRunLoopObserverContext,
    CFRunLoopObserverCreate, CFRunLoopObserverInvalidate, CFRunLoopObserverRef, CFRunLoopTimer,
    CFRunLoopTimerContext, CFRunLoopTimerCreate, CFRunLoopTimerInvalidate, CFRunLoopTimerRef,
    CFRunLoopTimerSetNextFireDate, kCFRunLoopAfterWaiting, kCFRunLoopBeforeWaiting,
    kCFRunLoopCommonModes,
};
use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{
    NSApplication, NSEventMask, NSEventTrackingRunLoopMode, NSMenu, NSModalPanelRunLoopMode,
};
use objc2_foundation::{NSDate, NSRunLoop};

const MAX_APP_EVENTS_PER_POLL: usize = 64;

/// Wake AppKit's main run loop after a backend producer queues an event.
/// A Rust channel wake alone does not commit pending NSWindow/NSView updates.
pub(super) fn wake_main_run_loop() {
    if let Some(run_loop) = objc2_core_foundation::CFRunLoop::main() {
        run_loop.wake_up();
    }
}

/// Let AppKit process native sources until an event producer wakes the loop or
/// the engine deadline expires. This is a blocking bound, not a periodic timer.
pub(super) fn wait_for_app_event(timeout: Duration) {
    if timeout.is_zero() || MainThreadMarker::new().is_none() {
        return;
    }
    autoreleasepool(|_| {
        let deadline = NSDate::dateWithTimeIntervalSinceNow(timeout.as_secs_f64());
        NSRunLoop::mainRunLoop().runMode_beforeDate(
            super::native::default_run_loop_modes().foundation,
            &deadline,
        );
    });
}

/// Service already-ready sources without waiting or querying workspace state.
pub(super) fn pump_ready_sources() {
    autoreleasepool(|_| {
        NSRunLoop::mainRunLoop().runMode_beforeDate(
            super::native::default_run_loop_modes().foundation,
            &NSDate::distantPast(),
        );
    });
}

/// Dispatch a bounded batch of AppKit events on the backend's main thread.
///
/// `Engine::run` remains platform-independent: macOS owns its AppKit event
/// integration here, while the fixed budget prevents a native event burst
/// from starving synchronous input disposition or shutdown.
pub(super) fn pump_app_events() {
    autoreleasepool(|_| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let application = NSApplication::sharedApplication(mtm);
        let expiration = NSDate::distantPast();
        for _ in 0..MAX_APP_EVENTS_PER_POLL {
            let Some(event) = application.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&expiration),
                super::native::default_run_loop_modes().foundation,
                true,
            ) else {
                break;
            };
            application.sendEvent(&event);
        }
    });
}

type Turn<'a> = dyn FnMut() -> Result<Option<Duration>, String> + 'a;

struct Driver<'a> {
    turn: RefCell<&'a mut Turn<'a>>,
    result: RefCell<Option<Result<(), String>>>,
    timer: Cell<CFRunLoopTimerRef>,
    menu: Option<&'a NSMenu>,
    run_loop: CFRunLoop,
}

impl Driver<'_> {
    fn advance(&self) {
        if self.result.borrow().is_some() {
            return;
        }
        // An AppKit method called by a turn can itself service native sources.
        // Skip that nested callback instead of aliasing engine/backend &mut.
        let Ok(mut turn) = self.turn.try_borrow_mut() else {
            return;
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| autoreleasepool(|_| turn())));
        drop(turn);
        let outcome = outcome.unwrap_or_else(|_| Err("macOS runtime turn panicked".into()));
        match outcome {
            Ok(Some(delay)) => {
                // SAFETY: the registration owns this live timer on the main
                // thread, and invalidates it before dropping the Driver.
                unsafe {
                    CFRunLoopTimerSetNextFireDate(
                        self.timer.get(),
                        CFAbsoluteTimeGetCurrent() + delay.as_secs_f64(),
                    );
                }
            }
            outcome => {
                *self.result.borrow_mut() = Some(outcome.map(|_| ()));
                // Quit/errors must also unwind a menu's nested tracking loop,
                // before backend shutdown releases any native resources.
                if let Some(menu) = self.menu {
                    menu.cancelTrackingWithoutAnimation();
                }
                self.run_loop.stop();
            }
        }
    }
}

extern "C" fn observer_callback(
    _observer: CFRunLoopObserverRef,
    _activity: CFRunLoopActivity,
    context: *mut c_void,
) {
    // SAFETY: Registration installs exactly this Driver pointer on the main
    // loop and removes all callbacks before its stack lifetime ends.
    unsafe { &*context.cast::<Driver<'_>>() }.advance();
}

extern "C" fn timer_callback(_timer: CFRunLoopTimerRef, context: *mut c_void) {
    // SAFETY: identical scoped/main-thread context contract as the observer.
    unsafe { &*context.cast::<Driver<'_>>() }.advance();
}

struct Registration<'a> {
    observer: CFRunLoopObserver,
    timer: CFRunLoopTimer,
    _driver: &'a Driver<'a>,
}

impl<'a> Registration<'a> {
    fn new(driver: &'a Driver<'a>) -> Result<Self, String> {
        let context = std::ptr::from_ref(driver).cast_mut().cast::<c_void>();
        let mut observer_context = CFRunLoopObserverContext {
            version: 0,
            info: context,
            retain: None,
            release: None,
            copyDescription: None,
        };
        let mut timer_context = CFRunLoopTimerContext {
            version: 0,
            info: context,
            retain: None,
            release: None,
            copyDescription: None,
        };
        // SAFETY: the scoped Driver outlives this registration. Core Foundation
        // copies context records, not their pointed-to data. Both callbacks are
        // registered only on the main loop; Drop invalidates them synchronously.
        unsafe {
            let observer = CFRunLoopObserverCreate(
                kCFAllocatorDefault,
                kCFRunLoopBeforeWaiting | kCFRunLoopAfterWaiting,
                1,
                0,
                observer_callback,
                &mut observer_context,
            );
            if observer.is_null() {
                return Err("cannot create the macOS runtime observer".into());
            }
            let observer = CFRunLoopObserver::wrap_under_create_rule(observer);
            // Repeating keeps the timer valid when fired; each completed turn
            // sets its actual next deadline, including immediate bounded work.
            let timer = CFRunLoopTimerCreate(
                kCFAllocatorDefault,
                CFAbsoluteTimeGetCurrent(),
                86_400.0,
                0,
                0,
                timer_callback,
                &mut timer_context,
            );
            if timer.is_null() {
                return Err("cannot create the macOS runtime deadline timer".into());
            }
            let timer = CFRunLoopTimer::wrap_under_create_rule(timer);
            driver.timer.set(timer.as_concrete_TypeRef());
            let registration = Self {
                observer,
                timer,
                _driver: driver,
            };
            let main = &driver.run_loop;
            // NSString and CFString are toll-free bridged. Explicitly register
            // tracking/modal modes as well as common modes; do not depend on
            // which modes AppKit adds to the common set on a given OS release.
            for mode in [
                kCFRunLoopCommonModes,
                std::ptr::from_ref(NSEventTrackingRunLoopMode).cast(),
                std::ptr::from_ref(NSModalPanelRunLoopMode).cast(),
            ] {
                main.add_observer(&registration.observer, mode);
                main.add_timer(&registration.timer, mode);
            }
            Ok(registration)
        }
    }
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        // SAFETY: both owned objects are live; invalidation removes every mode
        // registration before the borrowed Driver/callback is allowed to drop.
        unsafe {
            CFRunLoopObserverInvalidate(self.observer.as_concrete_TypeRef());
            CFRunLoopTimerInvalidate(self.timer.as_concrete_TypeRef());
        }
    }
}

pub(super) fn run(menu: Option<&NSMenu>, turn: &mut Turn<'_>) -> Result<(), String> {
    let _mtm = MainThreadMarker::new().ok_or("AppKit runtime must run on the main thread")?;
    let driver = Driver {
        turn: RefCell::new(turn),
        result: RefCell::new(None),
        timer: Cell::new(std::ptr::null_mut()),
        menu,
        run_loop: CFRunLoop::get_main(),
    };
    let registration = Registration::new(&driver)?;
    driver.advance();
    while driver.result.borrow().is_none() {
        // Critically, no engine/backend RefMut survives this dispatch. A menu
        // may keep sendEvent on the stack while observers continue engine turns.
        pump_app_events();
        if driver.result.borrow().is_none() {
            wait_for_app_event(Duration::from_secs(3600));
        }
    }
    drop(registration);
    driver.result.take().unwrap_or(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::runloop::{CFRunLoopMode, CFRunLoopRunResult, kCFRunLoopDefaultMode};

    #[test]
    fn app_event_dispatch_has_a_finite_budget() {
        assert!((1..=128).contains(&MAX_APP_EVENTS_PER_POLL));
    }

    fn modes() -> [CFRunLoopMode; 3] {
        // SAFETY: immutable framework-owned mode strings; the tests only run
        // Core Foundation loops on their own test threads, without AppKit UI.
        unsafe {
            [
                kCFRunLoopDefaultMode,
                std::ptr::from_ref(NSEventTrackingRunLoopMode).cast(),
                std::ptr::from_ref(NSModalPanelRunLoopMode).cast(),
            ]
        }
    }

    #[test]
    fn runtime_sources_deliver_turns_in_default_tracking_and_modal_modes() {
        for mode in modes() {
            let calls = Cell::new(0);
            let mut turn = || {
                calls.set(calls.get() + 1);
                Ok((calls.get() < 3).then_some(Duration::from_millis(2)))
            };
            let driver = Driver {
                turn: RefCell::new(&mut turn),
                result: RefCell::new(None),
                timer: Cell::new(std::ptr::null_mut()),
                menu: None,
                run_loop: CFRunLoop::get_current(),
            };
            let registration = Registration::new(&driver).unwrap();
            let observer = registration.observer.clone();
            let timer = registration.timer.clone();
            assert_eq!(
                CFRunLoop::run_in_mode(mode, Duration::from_secs(1), false),
                CFRunLoopRunResult::Stopped
            );
            assert_eq!(calls.get(), 3);
            assert_eq!(driver.result.take(), Some(Ok(())));
            drop(registration);
            for registered_mode in modes() {
                assert!(
                    !driver
                        .run_loop
                        .contains_observer(&observer, registered_mode)
                );
                assert!(!driver.run_loop.contains_timer(&timer, registered_mode));
            }
        }
    }

    #[test]
    fn nested_sources_cannot_reenter_a_turn_and_failure_stops_tracking() {
        let tracking = modes()[1];
        let calls = Cell::new(0);
        let mut turn = || {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                // Simulate a native operation servicing its run loop while it
                // still owns the engine borrow. The initial timer is ready.
                CFRunLoop::run_in_mode(tracking, Duration::ZERO, false);
                assert_eq!(calls.get(), 1);
                Ok(Some(Duration::ZERO))
            } else {
                Err("injected runtime failure".into())
            }
        };
        let driver = Driver {
            turn: RefCell::new(&mut turn),
            result: RefCell::new(None),
            timer: Cell::new(std::ptr::null_mut()),
            menu: None,
            run_loop: CFRunLoop::get_current(),
        };
        let _registration = Registration::new(&driver).unwrap();
        driver.advance();
        assert_eq!(calls.get(), 1);
        assert_eq!(
            CFRunLoop::run_in_mode(tracking, Duration::from_secs(1), false),
            CFRunLoopRunResult::Stopped
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(
            driver.result.take(),
            Some(Err("injected runtime failure".into()))
        );
    }
}
