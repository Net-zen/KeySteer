//! A reusable, display-synchronised clock shared by movement and scrolling.
//!
//! The worker and its native output are prepared at startup and retained while
//! idle. Gesture release only pauses delivery; it never joins a native VBlank
//! wait on the engine thread. Shutdown alone interrupts and joins the worker.

use std::cell::Cell;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use smallvec::SmallVec;

use crate::api::{Point, Screen};
use crate::platform::common::display_target::DisplayTargetCache;
use crate::platform::common::frame_clock::FrameClockState;
use crate::support::worker::WorkerJoin;

use super::{WAKE_MESSAGE, native};

pub fn prefer_dynamic_vblank() {
    native::prefer_dynamic_vblank();
}

#[derive(Default)]
struct State {
    frames: FrameClockState,
    shutdown: bool,
    display: (isize, u64),
    // Protected by the same lock used to clear this token before its native
    // owner is released, so shutdown cannot signal a recycled HANDLE.
    compositor_stop: isize,
}

impl State {
    fn publish_frame(
        &mut self,
        generation: u64,
        output: Option<(isize, u64)>,
        now: Instant,
    ) -> bool {
        (output.is_none_or(|output| output == self.display)) && self.frames.publish(generation, now)
    }
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}

pub struct DisplayFrameClock {
    owner_thread: u32,
    shared: Arc<Shared>,
    display_target: DisplayTargetCache,
    monitors: SmallVec<[isize; 4]>,
    pointer: Cell<Option<Point>>,
    selected_monitor: Cell<isize>,
    worker: Option<WorkerJoin>,
}

const CLOCK_STOP_TIMEOUT: Duration = Duration::from_secs(2);

impl DisplayFrameClock {
    pub fn new(owner_thread: u32) -> Self {
        Self {
            owner_thread,
            shared: Arc::new(Shared::default()),
            display_target: DisplayTargetCache::default(),
            monitors: SmallVec::new(),
            pointer: Cell::new(None),
            selected_monitor: Cell::new(0),
            worker: None,
        }
    }

    /// Prepare native resources without starting a gesture or an idle timer.
    pub fn prepare(&mut self) -> Result<(), String> {
        if let Some(worker) = self.worker.as_mut() {
            if !worker.reap_finished()? {
                return Ok(());
            }
            self.worker.take();
        }
        let composition_enabled = native::dwm_composition_enabled()
            .map_err(|error| format!("cannot query DWM composition: {error}"))?;
        if !composition_enabled {
            return Err(
                "DWM composition is disabled; display-synchronised frames are unavailable".into(),
            );
        }
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.frames.pause();
            state.shutdown = false;
        }
        let shared = self.shared.clone();
        let owner_thread = self.owner_thread;
        self.worker = Some(WorkerJoin::spawn(
            "Windows display clock",
            std::thread::Builder::new().name("keysteer-display-clock".into()),
            move || run_clock(shared, owner_thread),
        )?);
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.is_running() {
            return Ok(());
        }
        self.prepare()?;
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.shutdown {
            return Err("Windows display clock is stopping".into());
        }
        state.frames.start(Instant::now());
        drop(state);
        self.shared.ready.notify_one();
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frames
            .pause();
        Ok(())
    }

    pub fn stop_until(&mut self, deadline: Instant) -> Result<(), String> {
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.frames.pause();
            state.shutdown = true;
            if state.compositor_stop != 0
                && !native::interrupt_compositor_clock(state.compositor_stop)
            {
                crate::report_error!(
                    "windows-frame-clock",
                    "cannot interrupt compositor frame wait"
                );
            }
        }
        self.shared.ready.notify_one();
        if let Some(worker) = self.worker.as_mut() {
            worker.join_until(deadline)?;
            self.worker.take();
        }
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frames
            .generation()
            .is_some()
    }

    /// Use coordinates already observed by the backend; never query the OS.
    pub fn retarget(&self, point: Point) {
        self.pointer
            .set(Some(Point::new(point.x.round(), point.y.round())));
        self.publish_target(false);
    }

    pub fn update_displays(&mut self, screens: &[Screen], monitors: &[isize]) {
        self.display_target
            .replace(screens.iter().map(|screen| screen.bounds));
        self.monitors.clear();
        self.monitors.extend_from_slice(monitors);
        self.publish_target(true);
    }

    fn publish_target(&self, invalidate: bool) {
        let monitor = self
            .display_target
            .select(self.pointer.get())
            .and_then(|index| self.monitors.get(index).copied())
            .unwrap_or_default();
        if !invalidate && self.selected_monitor.get() == monitor {
            return;
        }
        // Coordinate-only updates stay lock-free within one display. Crossing
        // or replacing the snapshot synchronizes with the worker's idle wait.
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.display.0 = monitor;
        if invalidate {
            state.display.1 = state.display.1.wrapping_add(1);
        }
        self.selected_monitor.set(monitor);
        drop(state);
        self.shared.ready.notify_one();
    }

    pub fn try_next(&self) -> Option<Duration> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frames
            .take_elapsed()
    }
}

impl Drop for DisplayFrameClock {
    fn drop(&mut self) {
        if self
            .worker
            .as_ref()
            .is_some_and(WorkerJoin::shutdown_failure_was_returned)
        {
            return;
        }
        let now = Instant::now();
        if let Err(error) = self.stop_until(now.checked_add(CLOCK_STOP_TIMEOUT).unwrap_or(now)) {
            crate::support::logging::report_error("windows-frame-clock", error);
        }
    }
}

struct CompositorBoost(bool);

impl CompositorBoost {
    fn request(enabled: bool) -> Self {
        Self(enabled && native::boost_compositor_clock(true))
    }

    fn disable(&mut self) {
        if self.0 {
            if !native::boost_compositor_clock(false) {
                crate::report_error!(
                    "windows-frame-clock",
                    "cannot release the compositor clock boost"
                );
            }
            self.0 = false;
        }
    }
}

impl Drop for CompositorBoost {
    fn drop(&mut self) {
        self.disable();
    }
}

fn run_clock(shared: Arc<Shared>, owner_thread: u32) {
    // This worker owns every native display resource. No native wait,
    // initialization, boost or output enumeration holds the delivery lock.
    let compositor_signal = native::CompositorClockSignal::try_new();
    let mut compositor_stop = compositor_signal
        .as_ref()
        .map(native::CompositorClockSignal::token);
    {
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.compositor_stop = compositor_stop.unwrap_or_default();
    }
    let mut selected_display = (isize::MIN, u64::MAX);
    let mut selected_output = None;
    let mut compositor_boost = CompositorBoost(false);
    let mut boosted_gesture = None;
    loop {
        let requested = {
            let state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.shutdown {
                break;
            }
            state.display
        };
        if compositor_stop.is_none() && requested != selected_display {
            selected_display = requested;
            selected_output = if requested.0 == 0 {
                None
            } else {
                match native::display_output_for_monitor(requested.0) {
                    Ok(output) => output,
                    Err(error) => {
                        crate::support::logging::report_error("windows-frame-clock", error);
                        None
                    }
                }
            };
        }
        let (generation, requested) = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.frames.generation().is_none() {
                drop(state);
                compositor_boost.disable();
                boosted_gesture = None;
                state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                while !state.shutdown
                    && state.frames.generation().is_none()
                    && (compositor_stop.is_some() || state.display == selected_display)
                {
                    state = shared.ready.wait(state).unwrap_or_else(|e| e.into_inner());
                }
            }
            if state.shutdown {
                break;
            }
            let Some(generation) = state.frames.generation() else {
                continue;
            };
            (generation, state.display)
        };
        // A retarget during the idle wait is prepared before the next VBlank.
        if compositor_stop.is_none() && selected_display != requested {
            continue;
        }
        if boosted_gesture != Some(generation) {
            if !compositor_boost.0 {
                compositor_boost = CompositorBoost::request(compositor_stop.is_some());
            }
            boosted_gesture = Some(generation);
        }
        let waited = if let Some(stop_event) = compositor_stop {
            match native::wait_for_compositor_frame(stop_event) {
                native::CompositorWait::Frame => Ok(()),
                native::CompositorWait::Interrupted => break,
                native::CompositorWait::Failed => {
                    crate::report_error!(
                        "windows-frame-clock",
                        "compositor clock failed; falling back to output VBlank"
                    );
                    compositor_boost.disable();
                    compositor_stop = None;
                    continue;
                }
            }
        } else if let Some(output) = selected_output.as_ref() {
            output.wait_for_vblank().map_err(|error| error.to_string())
        } else {
            native::wait_for_dwm_frame().map_err(|error| error.to_string())
        };
        if let Err(error) = waited {
            if selected_output.take().is_some() {
                crate::report_error!(
                    "windows-frame-clock",
                    "output VBlank wait failed; falling back to DWM: {error}"
                );
                continue;
            }
            crate::report_error!(
                "windows-frame-clock",
                "DwmFlush failed; frame delivery stopped: {error}"
            );
            break;
        }
        let wake = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            // An output wait can finish after a display snapshot replacement.
            // Keep its elapsed time for the next valid frame, but do not
            // deliver a frame from a retired native output.
            state.publish_frame(
                generation,
                compositor_stop.is_none().then_some(requested),
                Instant::now(),
            )
        };
        if wake && let Err(error) = native::post_thread_wake(owner_thread, WAKE_MESSAGE) {
            crate::report_error!(
                "windows-frame-clock",
                "cannot wake engine for display frame: {error}"
            );
            break;
        }
    }
    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    state.frames.pause();
    state.shutdown = true;
    state.compositor_stop = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn native_clock_resumes_after_cached_display_snapshot_replacement() -> Result<(), String> {
        // Backend startup now begins preparation before its display snapshot.
        let mut clock = DisplayFrameClock::new(native::prepare_thread_message_queue());
        clock.prepare()?;
        let snapshot = super::super::screens::list_snapshot()?;
        let first = snapshot.screens.first().ok_or("no interactive display")?;
        clock.retarget(first.bounds.center());
        clock.update_displays(&snapshot.screens, &snapshot.monitors);
        for round in 0..6 {
            if round == 2 {
                clock.update_displays(&[], &[]);
            } else {
                clock.update_displays(&snapshot.screens, &snapshot.monitors);
                clock.retarget(
                    snapshot.screens[round % snapshot.screens.len()]
                        .bounds
                        .center(),
                );
            }
            clock.start()?;
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if let Some(elapsed) = clock.try_next() {
                    assert!(!elapsed.is_zero());
                    break;
                }
                assert!(Instant::now() < deadline, "display frames did not resume");
                native::wait_for_input(50);
                assert!(native::pump_window_messages());
            }
            clock.stop()?;
            assert_eq!(clock.try_next(), None);
        }
        clock.stop_until(Instant::now() + Duration::from_secs(2))
    }

    #[test]
    fn pausing_and_resuming_reuses_a_worker_blocked_in_native_work() {
        let (entered, observed) = mpsc::channel();
        let (resume, blocked) = mpsc::channel();
        let mut clock = DisplayFrameClock::new(0);
        clock.worker = Some(
            WorkerJoin::spawn(
                "display clock test",
                std::thread::Builder::new(),
                move || {
                    entered.send(()).unwrap();
                    blocked.recv_timeout(Duration::from_secs(2)).unwrap();
                },
            )
            .unwrap(),
        );
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..2 {
            clock.start().unwrap();
            assert!(clock.is_running());
            clock.stop().unwrap();
            assert!(!clock.is_running());
            assert!(clock.worker.is_some());
        }
        resume.send(()).unwrap();
        clock
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert!(clock.worker.is_none());
    }

    #[test]
    fn retargeting_uses_snapshot_identities_and_refreshes_unchanged_geometry() {
        let mut clock = DisplayFrameClock::new(0);
        let displays = test_displays();
        clock.update_displays(&displays, &[11, 22]);
        let original = clock.shared.state.lock().unwrap().display;
        assert_eq!(original.0, 11);
        for x in [0.0, 500.0, 999.0] {
            clock.retarget(Point::new(x, 20.0));
            assert_eq!(clock.shared.state.lock().unwrap().display, original);
        }
        clock.retarget(Point::new(1500.0, 20.0));
        assert_eq!(clock.shared.state.lock().unwrap().display, (22, original.1));
        clock.update_displays(&displays, &[33, 44]);
        assert_eq!(
            clock.shared.state.lock().unwrap().display,
            (44, original.1 + 1)
        );
        clock.update_displays(&displays[..1], &[55]);
        assert_eq!(clock.shared.state.lock().unwrap().display.0, 55);
        clock.update_displays(&[], &[]);
        assert_eq!(clock.shared.state.lock().unwrap().display.0, 0);
        clock.retarget(Point::new(1500.0, 20.0));
        assert_eq!(clock.shared.state.lock().unwrap().display.0, 0);
        let began = Instant::now();
        let generation = {
            let mut state = clock.shared.state.lock().unwrap();
            state.frames.start(began);
            state.frames.generation().unwrap()
        };
        clock.update_displays(&displays, &[66, 77]);
        assert_eq!(clock.shared.state.lock().unwrap().display.0, 77);
        clock.retarget(Point::new(500.0, 20.0));
        let mut state = clock.shared.state.lock().unwrap();
        assert_eq!(state.display.0, 66);
        assert_eq!(state.frames.generation(), Some(generation));
        assert!(
            state
                .frames
                .publish(generation, began + Duration::from_millis(20))
        );
        assert_eq!(state.frames.take_elapsed(), Some(Duration::from_millis(20)));
    }

    #[test]
    fn retired_output_waits_are_rejected_without_disabling_the_compositor_path() {
        let began = Instant::now();
        let mut state = State::default();
        state.frames.start(began);
        let generation = state.frames.generation().unwrap();
        state.display = (11, 1);
        let previous = state.display;
        state.display = (22, 2);
        assert!(!state.publish_frame(
            generation,
            Some(previous),
            began + Duration::from_millis(16)
        ));
        assert_eq!(state.frames.take_elapsed(), None);
        assert!(state.publish_frame(
            generation,
            Some(state.display),
            began + Duration::from_millis(32)
        ));
        assert_eq!(state.frames.take_elapsed(), Some(Duration::from_millis(32)));
        state.display = (0, 3);
        assert!(state.publish_frame(generation, None, began + Duration::from_millis(48)));
        assert_eq!(state.frames.take_elapsed(), Some(Duration::from_millis(16)));
    }

    fn test_displays() -> [Screen; 2] {
        [0.0, 1000.0].map(|x| {
            let bounds = crate::api::Rect::new(x, 0.0, 1000.0, 1000.0);
            Screen {
                bounds,
                work_area: bounds,
                is_primary: x == 0.0,
                scale: 1.0,
                name: None,
            }
        })
    }
}
