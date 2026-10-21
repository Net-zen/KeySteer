//! Independent macOS sources; the existing UI scan queue owns scan ordering.
#![forbid(unsafe_code)]

use crate::api::command::UiScanStatus;
use crate::platform::common::image_tiles::MAX_OCR_IN_FLIGHT;
use crate::platform::common::spatial_index::TargetSource;
use crate::support::worker::WorkerJoin;
use smallvec::SmallVec;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak, mpsc};
use std::time::{Duration, Instant};

const STOP_TIMEOUT: Duration = Duration::from_millis(500);
const OCR_IDLE_TIMEOUT: Duration = Duration::from_secs(6);
const SOURCES: [TargetSource; 4] = [
    TargetSource::Accessibility,
    TargetSource::SystemOcr,
    TargetSource::NativeRectangles,
    TargetSource::Contour,
];
const NAMES: [&str; SOURCES.len()] = [
    "accessibility scan provider",
    "system OCR provider",
    "rectangle scan provider",
    "contour scan provider",
];

pub(super) fn interrupt_status(deadline: Instant, cancelled: bool) -> Option<UiScanStatus> {
    if cancelled {
        Some(UiScanStatus::ContextChanged)
    } else if Instant::now() >= deadline {
        Some(UiScanStatus::TimedOut)
    } else {
        None
    }
}
type Work = Arc<dyn Fn(TargetSource, &Signal) -> UiScanStatus + Send + Sync>;

enum Event {
    Completed(TargetSource, UiScanStatus),
    Cancelled,
}
pub(crate) struct Signal {
    cancelled: AtomicBool,
    events: mpsc::Sender<Event>,
    process: Mutex<ProcessSlots>,
    idle: Arc<Mutex<IdleProcesses>>,
}

#[derive(Default)]
struct IdleProcesses {
    children: Vec<IdleProcess>,
    limit: usize,
}
struct IdleProcess {
    child: Option<Child>,
    input: Option<ChildStdin>,
    output: Option<ChildStdout>,
    until: Instant,
}
impl Drop for IdleProcess {
    fn drop(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            if let Err(error) = child.wait() {
                crate::support::logging::report_error("macos-ocr-process", error.to_string());
            }
        }
    }
}
impl IdleProcesses {
    fn expired(&mut self, all: bool) -> Vec<IdleProcess> {
        let now = Instant::now();
        let mut expired = Vec::new();
        let mut index = 0;
        while index < self.children.len() {
            if all || self.children[index].until <= now || index >= self.limit {
                expired.push(self.children.swap_remove(index));
            } else {
                index += 1;
            }
        }
        expired
    }
}
// This guard belongs to the OCR source thread, including during unwinding.
// Neither idle expiry nor shutdown reaps children on the engine thread.
struct IdleCleanup<'a>(&'a Mutex<IdleProcesses>);
impl Drop for IdleCleanup<'_> {
    fn drop(&mut self) {
        let children = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .expired(true);
        drop(children);
    }
}
// Inline capacity avoids allocation on dual-display scans; it is not a limit.
type ProcessSlots = SmallVec<[ProcessSlot; 2]>;
#[derive(Default)]
struct ProcessSlot {
    child: Option<Child>,
    reserved: bool,
}
pub(super) struct Process<'a> {
    signal: &'a Signal,
    slot: usize,
    pub(super) input: Option<ChildStdin>,
    pub(super) output: Option<ChildStdout>,
    reusable: bool,
}
impl Process<'_> {
    /// Called only after every tile and the frame terminator were validated.
    pub(super) fn completed(&mut self) {
        self.reusable = true;
    }
}
impl Drop for Process<'_> {
    fn drop(&mut self) {
        let child = {
            let mut slots = self
                .signal
                .process
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            slots[self.slot].child.take()
        };
        let mut owner = Some(IdleProcess {
            child,
            input: self.input.take(),
            output: self.output.take(),
            until: Instant::now() + OCR_IDLE_TIMEOUT,
        });
        if self.reusable
            && owner
                .as_ref()
                .is_some_and(|owner| owner.input.is_some() && owner.output.is_some())
            && !self.signal.load(Ordering::Acquire)
        {
            let mut idle = self.signal.idle.lock().unwrap_or_else(|e| e.into_inner());
            if idle.children.len() < idle.limit
                && let Some(owner) = owner.take()
            {
                idle.children.push(owner);
            }
        }
        // Incomplete, cancelled and excess owners are killed and reaped here,
        // outside both locks, before returning their process reservation.
        drop(owner);
        // A sibling display may already be ready to spawn. Keep this slot
        // reserved until its owner is cached or reaped, so even that
        // handoff cannot briefly exceed the process/memory budget.
        self.signal
            .process
            .lock()
            .unwrap_or_else(|e| e.into_inner())[self.slot]
            .reserved = false;
    }
}
impl Signal {
    pub(super) fn reuse_processes(&self, limit: usize) {
        let expired = {
            let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
            idle.limit = limit.min(MAX_OCR_IN_FLIGHT);
            idle.expired(false)
        };
        drop(expired);
    }
    pub(super) fn load(&self, order: Ordering) -> bool {
        self.cancelled.load(order)
    }

    pub(super) fn process(&self, command: &mut Command) -> Result<Process<'_>, String> {
        if self.load(Ordering::Acquire) {
            return Err("OCR was cancelled before process creation".into());
        }
        let slot = {
            let mut slots = self.process.lock().unwrap_or_else(|e| e.into_inner());
            let free = slots.iter().position(|slot| !slot.reserved);
            let slot = match free {
                Some(slot) => slot,
                None if slots.len() < MAX_OCR_IN_FLIGHT => {
                    // The CPU/display plan bounds actual callers. Reserve
                    // storage on demand, with the shared request ceiling as
                    // the final guard rather than a separate two-process cap.
                    slots.push(ProcessSlot::default());
                    slots.len() - 1
                }
                None => return Err("OCR process budget is occupied".into()),
            };
            slots[slot].reserved = true;
            slot
        };
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // Host regression: no console window.
        }
        let mut cached = self
            .idle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .children
            .pop();
        if cached.as_mut().is_some_and(|owner| {
            owner.until <= Instant::now()
                || !matches!(owner.child.as_mut().map(Child::try_wait), Some(Ok(None)))
        }) {
            drop(cached.take());
        }
        // Spawn outside the cancellation lock: exiting never waits for exec.
        let spawned = if let Some(child) = cached.as_mut().and_then(|owner| owner.child.take()) {
            Ok(child)
        } else {
            drop(cached.take());
            command.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()
        };
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                self.process.lock().unwrap_or_else(|e| e.into_inner())[slot].reserved = false;
                return Err(format!("cannot start OCR process: {error}"));
            }
        };
        let input = cached
            .as_mut()
            .and_then(|owner| owner.input.take())
            .or_else(|| child.stdin.take());
        let output = cached
            .as_mut()
            .and_then(|owner| owner.output.take())
            .or_else(|| child.stdout.take());
        let mut slots = self.process.lock().unwrap_or_else(|e| e.into_inner());
        if input.is_none() || output.is_none() {
            drop(slots);
            let _ = child.kill();
            let _ = child.wait();
            self.process.lock().unwrap_or_else(|e| e.into_inner())[slot].reserved = false;
            return Err("OCR process has no exclusive pipe owner".into());
        }
        slots[slot].child = Some(child);
        // A cancellation racing with spawn must kill the just-created child.
        if self.load(Ordering::Acquire)
            && let Some(child) = slots[slot].child.as_mut()
        {
            let _ = child.kill();
        }
        drop(slots);
        Ok(Process {
            signal: self,
            slot,
            input,
            output,
            reusable: false,
        })
    }

    fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            let _ = self.events.send(Event::Cancelled);
            for child in self
                .process
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter_mut()
                .filter_map(|slot| slot.child.as_mut())
            {
                let _ = child.kill();
            }
        }
    }
}
struct Task {
    signal: Arc<Signal>,
    work: Work,
}
#[derive(Default)]
struct State {
    pending: [Option<Task>; SOURCES.len()],
    active: [Option<Weak<Signal>>; SOURCES.len()],
    stopping: bool,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: [Condvar; SOURCES.len()],
    idle: Arc<Mutex<IdleProcesses>>,
}
impl Shared {
    fn run(&self, index: usize) {
        let ocr = SOURCES[index] == TargetSource::SystemOcr;
        let _cleanup = ocr.then(|| IdleCleanup(self.idle.as_ref()));
        loop {
            if ocr {
                let expired = self
                    .idle
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .expired(false);
                drop(expired);
            }
            let Task { signal, work } = {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if state.pending[index].is_none() && !state.stopping {
                    let until = ocr
                        .then(|| {
                            self.idle
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .children
                                .iter()
                                .map(|owner| owner.until)
                                .min()
                        })
                        .flatten();
                    if let Some(until) = until {
                        drop(
                            self.ready[index]
                                .wait_timeout(
                                    state,
                                    until.saturating_duration_since(Instant::now()),
                                )
                                .unwrap_or_else(|e| e.into_inner()),
                        );
                    } else {
                        drop(
                            self.ready[index]
                                .wait(state)
                                .unwrap_or_else(|e| e.into_inner()),
                        );
                    }
                    continue;
                }
                if state.stopping {
                    return;
                }
                let Some(task) = state.pending[index].take() else {
                    continue;
                };
                state.active[index] = Some(Arc::downgrade(&task.signal));
                task
            };
            let run = || {
                let status = if signal.cancelled.load(Ordering::Acquire) {
                    UiScanStatus::ContextChanged
                } else {
                    work(SOURCES[index], &signal)
                };
                // Pixel conversion may create autoreleased framework objects
                // too. Drain them per job on these persistent worker threads,
                // after releasing every captured native owner.
                drop(work);
                status
            };
            #[cfg(target_os = "macos")]
            let status = objc2::rc::autoreleasepool(|_| run());
            #[cfg(not(target_os = "macos"))]
            let status = run();
            let _ = signal.events.send(Event::Completed(SOURCES[index], status));
            self.state.lock().unwrap_or_else(|e| e.into_inner()).active[index] = None;
        }
    }

    fn cancel(&self, only: Option<&Arc<Signal>>, stopping: bool) {
        let mut discarded = SmallVec::<[Task; SOURCES.len()]>::new();
        let mut active = SmallVec::<[Arc<Signal>; SOURCES.len()]>::new();
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.stopping |= stopping;
            for index in 0..SOURCES.len() {
                if state.pending[index]
                    .as_ref()
                    .is_some_and(|task| only.is_none_or(|signal| Arc::ptr_eq(&task.signal, signal)))
                {
                    discarded.extend(state.pending[index].take());
                }
                if let Some(signal) = state.active[index].as_ref().and_then(Weak::upgrade)
                    && only.is_none_or(|only| Arc::ptr_eq(&signal, only))
                {
                    active.push(signal);
                }
            }
            if stopping {
                for wake in &self.ready {
                    wake.notify_one();
                }
            }
        }
        for task in &discarded {
            task.signal.cancel();
        }
        for signal in active {
            signal.cancel();
        }
        // A queued capture can be freed immediately, outside the state lock.
        drop(discarded);
    }
}

/// One executing task and one replaceable latest task for each fixed source.
/// Idle threads retain no screenshot or native request; completed OCR helpers
/// can retain model state for a short, bounded reuse window.
#[derive(Default)]
pub(crate) struct Workers {
    shared: Arc<Shared>,
    joins: Mutex<[Option<WorkerJoin>; SOURCES.len()]>,
}
pub(crate) struct Batch {
    shared: Arc<Shared>,
    signal: Arc<Signal>,
    events: mpsc::Receiver<Event>,
    statuses: SmallVec<[(TargetSource, Option<UiScanStatus>); SOURCES.len()]>,
    pending: usize,
}
fn name(source: TargetSource) -> &'static str {
    SOURCES
        .iter()
        .position(|old| *old == source)
        .map_or("scan provider", |index| NAMES[index])
}
impl Workers {
    pub(crate) fn start(
        &self,
        sources: &[TargetSource],
        work: impl Fn(TargetSource, &Signal) -> UiScanStatus + Send + Sync + 'static,
    ) -> Batch {
        let (sender, events) = mpsc::channel();
        let signal = Arc::new(Signal {
            cancelled: AtomicBool::new(false),
            events: sender,
            process: Mutex::new(ProcessSlots::default()),
            idle: Arc::clone(&self.shared.idle),
        });
        let work: Work = Arc::new(work);
        let mut batch = Batch {
            shared: Arc::clone(&self.shared),
            signal,
            events,
            statuses: SmallVec::new(),
            pending: 0,
        };
        let mut discarded = SmallVec::<[Task; SOURCES.len()]>::new();
        let mut replaced = SmallVec::<[Arc<Signal>; SOURCES.len()]>::new();
        let mut joins = self.joins.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        for &source in sources {
            if batch.statuses.iter().any(|(old, _)| *old == source) {
                continue;
            }
            let Some(index) = SOURCES.iter().position(|old| *old == source) else {
                batch.statuses.push((
                    source,
                    Some(UiScanStatus::Unsupported("unknown native source".into())),
                ));
                continue;
            };
            let status = if state.stopping {
                Some(UiScanStatus::ContextChanged)
            } else {
                if let Some(worker) = joins[index].as_mut() {
                    match worker.reap_finished() {
                        Ok(false) => {}
                        result => {
                            if let Err(error) = result {
                                crate::support::logging::report_error("ui-scan-workers", error);
                            }
                            joins[index] = None;
                        }
                    }
                }
                let started = if joins[index].is_some() {
                    Ok(())
                } else {
                    let shared = Arc::clone(&self.shared);
                    WorkerJoin::spawn(
                        NAMES[index],
                        std::thread::Builder::new().name(NAMES[index].into()),
                        move || shared.run(index),
                    )
                    .map(|worker| joins[index] = Some(worker))
                };
                match started {
                    Ok(()) => {
                        discarded.extend(state.pending[index].replace(Task {
                            signal: Arc::clone(&batch.signal),
                            work: Arc::clone(&work),
                        }));
                        replaced.extend(state.active[index].as_ref().and_then(Weak::upgrade));
                        self.shared.ready[index].notify_one();
                        batch.pending += 1;
                        None
                    }
                    Err(error) => Some(UiScanStatus::Failed(error)),
                }
            };
            batch.statuses.push((source, status));
        }
        drop(state);
        drop(joins);
        for task in &discarded {
            task.signal.cancel();
        }
        for signal in replaced {
            signal.cancel();
        }
        drop(discarded);
        batch
    }
    pub(crate) fn cancel_all(&self) {
        self.shared.cancel(None, false);
    }
    pub(crate) fn request_stop(&self) {
        self.shared.cancel(None, true);
    }
    pub(crate) fn stop_until(&self, deadline: Instant) -> Result<(), String> {
        self.request_stop();
        let mut joins = self.joins.lock().unwrap_or_else(|e| e.into_inner());
        let mut errors = crate::support::errors::ErrorBundle::default();
        for worker in joins.iter_mut().flatten() {
            errors.record("native source", worker.join_until(deadline));
        }
        errors.into_result()
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        self.request_stop();
        let deadline = Instant::now() + STOP_TIMEOUT;
        for worker in self
            .joins
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
            .flatten()
        {
            if !worker.shutdown_failure_was_returned()
                && let Err(error) = worker.join_until(deadline)
            {
                crate::support::logging::report_error("ui-scan-workers", error);
            }
        }
        // WorkerJoin retains an unresponsive native owner until actual return.
    }
}
impl Batch {
    pub(crate) fn wait(
        mut self,
        deadline: Instant,
        cancelled: impl Fn() -> bool,
    ) -> SmallVec<[UiScanStatus; SOURCES.len()]> {
        while self.pending != 0 {
            if cancelled() || self.signal.cancelled.load(Ordering::Acquire) {
                self.finish_pending(UiScanStatus::ContextChanged);
                break;
            }
            match self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(Event::Completed(source, status)) => {
                    if let Some((_, slot)) = self
                        .statuses
                        .iter_mut()
                        .find(|(old, slot)| *old == source && slot.is_none())
                    {
                        *slot = Some(status);
                        self.pending -= 1;
                    }
                }
                Ok(Event::Cancelled) => {
                    self.finish_pending(UiScanStatus::ContextChanged);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    for (source, status) in &self.statuses {
                        if status.is_none() {
                            crate::report_warning!(
                                "ui-scan-workers",
                                "{} exceeded the scan deadline; cancelling its request and retaining the native owner until it returns",
                                name(*source)
                            );
                        }
                    }
                    self.finish_pending(UiScanStatus::TimedOut);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.finish_pending(UiScanStatus::Failed(
                        "native scan worker stopped without reporting completion".into(),
                    ));
                    break;
                }
            }
        }
        self.statuses
            .iter_mut()
            .filter_map(|(_, status)| status.take())
            .collect()
    }
    fn finish_pending(&mut self, status: UiScanStatus) {
        self.signal.cancel();
        for (_, slot) in &mut self.statuses {
            if slot.is_none() {
                *slot = Some(status.clone());
            }
        }
        self.pending = 0;
    }
}
impl Drop for Batch {
    fn drop(&mut self) {
        self.signal.cancel();
        self.shared.cancel(Some(&self.signal), false);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Rect, SemanticRole, UiTarget};
    use crate::platform::common::scan_fusion::{Options, Worker, combined_status};
    use crate::platform::common::scan_mailbox::ScanMailbox;
    use std::sync::atomic::AtomicUsize;

    #[test]
    #[ignore = "child-process fixture; invoked only by the cancellation regression"]
    fn ocr_process_child_fixture() {
        use std::io::{BufRead, Write};
        let Ok(mode) = std::env::var("KEYSTEER_OCR_CHILD_FIXTURE") else {
            return;
        };
        if mode == "reuse" {
            for line in std::io::stdin().lock().lines() {
                let line = line.unwrap();
                println!("frame:{}:{line}", std::process::id());
                std::io::stdout().flush().unwrap();
            }
            return;
        }
        if mode == "hang" {
            std::io::stdout().write_all(b"ocr-ready\n").unwrap();
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        assert_eq!(mode, "fresh");
        std::io::stdout().write_all(b"fresh-result\n").unwrap();
        std::io::stdout().flush().unwrap();
    }

    #[test]
    fn completed_helpers_are_reused_across_generations_and_expire_on_the_worker() {
        use std::io::{BufRead, BufReader, Write};
        let workers = Workers::default();
        let (sent, received) = mpsc::channel();
        let frame = |sent: mpsc::Sender<u32>| {
            move |_: TargetSource, signal: &Signal| {
                signal.reuse_processes(1);
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args(["--ignored", "--nocapture", "ocr_process_child_fixture"])
                    .env("KEYSTEER_OCR_CHILD_FIXTURE", "reuse");
                let mut process = signal.process(&mut command).unwrap();
                writeln!(process.input.as_mut().unwrap(), "next").unwrap();
                let mut output = BufReader::new(process.output.take().unwrap());
                let mut line = String::new();
                loop {
                    line.clear();
                    assert_ne!(output.read_line(&mut line).unwrap(), 0);
                    if let Some(id) = line
                        .strip_prefix("frame:")
                        .and_then(|line| line.strip_suffix(":next\n"))
                    {
                        sent.send(id.parse().unwrap()).unwrap();
                        break;
                    }
                }
                process.output = Some(output.into_inner());
                process.completed();
                UiScanStatus::Success
            }
        };
        let first = workers.start(&[TargetSource::SystemOcr], frame(sent.clone()));
        let id = received.recv_timeout(Duration::from_secs(5)).unwrap();
        // Keep the old Batch alive until the next generation owns its helper.
        // Wait for completed ownership transfer without consuming that Batch.
        let transfer_deadline = Instant::now() + Duration::from_secs(5);
        while workers.shared.idle.lock().unwrap().children.is_empty() {
            assert!(
                Instant::now() < transfer_deadline,
                "helper was not returned"
            );
            std::thread::yield_now();
        }
        let second = workers.start(&[TargetSource::SystemOcr], frame(sent.clone()));
        drop(first);
        assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), id);
        assert_eq!(
            second.wait(Instant::now() + Duration::from_secs(5), || false)[0],
            UiScanStatus::Success
        );
        {
            let mut idle = workers.shared.idle.lock().unwrap();
            assert_eq!(idle.children.len(), 1);
            idle.children[0].until = Instant::now();
        }
        workers.shared.ready[1].notify_one();
        let third = workers.start(&[TargetSource::SystemOcr], frame(sent));
        assert_ne!(received.recv_timeout(Duration::from_secs(5)).unwrap(), id);
        assert_eq!(
            third.wait(Instant::now() + Duration::from_secs(5), || false)[0],
            UiScanStatus::Success
        );
        workers
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert!(workers.shared.idle.lock().unwrap().children.is_empty());
    }

    #[test]
    fn permanently_stuck_ocr_process_is_killed_and_the_next_scan_reuses_its_slot() {
        use std::io::{BufRead, BufReader};
        let workers = Workers::default();
        for iteration in 0..4 {
            let displays = if iteration % 2 == 0 { 2 } else { 4 };
            let image = Arc::new(vec![0u8; 64 * 1024]);
            let observer = Arc::downgrade(&image);
            let (entered, entry) = mpsc::channel();
            let mut old = Some(workers.start(&[TargetSource::SystemOcr], move |_, signal| {
                std::hint::black_box(image.len());
                let hang = || {
                    let mut command = Command::new(std::env::current_exe().unwrap());
                    command
                        .args(["--ignored", "--nocapture", "ocr_process_child_fixture"])
                        .env("KEYSTEER_OCR_CHILD_FIXTURE", "hang");
                    let mut process = signal.process(&mut command).unwrap();
                    let mut output = BufReader::new(process.output.take().unwrap());
                    let mut line = String::new();
                    loop {
                        line.clear();
                        assert_ne!(output.read_line(&mut line).unwrap(), 0);
                        if line.trim() == "ocr-ready" {
                            break;
                        }
                    }
                    entered.send(()).unwrap();
                    // No release channel: the child genuinely cannot finish its
                    // OCR. Only process termination can unblock this pipe read.
                    while output.read_line(&mut line).unwrap() != 0 {
                        line.clear();
                    }
                    assert!(signal.load(Ordering::Acquire));
                };
                std::thread::scope(|scope| {
                    let others: Vec<_> = (1..displays).map(|_| scope.spawn(hang)).collect();
                    hang();
                    for other in others {
                        other.join().unwrap();
                    }
                });
                UiScanStatus::ContextChanged
            }));
            for _ in 0..displays {
                entry.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            match iteration % 4 {
                0 => workers.cancel_all(),
                1 => {} // Replacement below must cancel the active process.
                2 => assert_eq!(
                    old.take().unwrap().wait(Instant::now(), || false)[0],
                    UiScanStatus::TimedOut
                ),
                _ => drop(old.take()),
            }
            let next = workers.start(&[TargetSource::SystemOcr], |_, signal| {
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args(["--ignored", "--nocapture", "ocr_process_child_fixture"])
                    .env("KEYSTEER_OCR_CHILD_FIXTURE", "fresh");
                let mut process = signal.process(&mut command).unwrap();
                let output = BufReader::new(process.output.take().unwrap());
                assert!(
                    output
                        .lines()
                        .map(Result::unwrap)
                        .any(|line| line == "fresh-result")
                );
                assert!(!signal.load(Ordering::Acquire));
                UiScanStatus::Success
            });
            // Late cleanup of the old batch must leave the new child alone.
            drop(old);
            assert_eq!(
                next.wait(Instant::now() + Duration::from_secs(5), || false)[0],
                UiScanStatus::Success
            );
            assert!(observer.upgrade().is_none());
        }
        assert_eq!(workers.joins.lock().unwrap().iter().flatten().count(), 1);
        workers
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn process_budget_reserves_before_spawn_and_releases_only_its_owner() {
        let (events, _) = mpsc::channel();
        let signal = Signal {
            cancelled: AtomicBool::new(false),
            events,
            process: Mutex::new(ProcessSlots::default()),
            idle: Arc::default(),
        };
        let command = || {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--ignored", "ocr_process_child_fixture"])
                .env("KEYSTEER_OCR_CHILD_FIXTURE", "fresh")
                // This budget test deliberately closes unopened output pipes
                // and kills owners; libtest may otherwise log BrokenPipe.
                .stderr(Stdio::null());
            command
        };
        let mut processes: Vec<_> = (0..MAX_OCR_IN_FLIGHT)
            .map(|_| signal.process(&mut command()).unwrap())
            .collect();
        let first = processes.remove(0);
        let second = &processes[0];
        let second_id = signal.process.lock().unwrap()[second.slot]
            .child
            .as_ref()
            .unwrap()
            .id();
        let mut invalid = Command::new("this-ocr-executable-must-never-be-resolved");
        assert!(
            signal
                .process(&mut invalid)
                .err()
                .unwrap()
                .contains("budget")
        );
        drop(first);
        // Failed spawn returns its reservation without touching the other owner.
        assert!(
            signal
                .process(&mut invalid)
                .err()
                .unwrap()
                .contains("cannot start")
        );
        let slots = signal.process.lock().unwrap();
        assert_eq!(
            slots.iter().filter(|slot| slot.reserved).count(),
            MAX_OCR_IN_FLIGHT - 1
        );
        assert_eq!(slots[second.slot].child.as_ref().unwrap().id(), second_id);
        drop(slots);
        signal.cancel();
        drop(processes);
        assert!(
            signal
                .process
                .lock()
                .unwrap()
                .iter()
                .all(|slot| slot.child.is_none() && !slot.reserved)
        );
    }

    #[test]
    fn a_process_started_after_cancellation_is_never_spawned() {
        let (events, _) = mpsc::channel();
        let signal = Signal {
            cancelled: AtomicBool::new(false),
            events,
            process: Mutex::new(ProcessSlots::default()),
            idle: Arc::default(),
        };
        signal.cancel();
        let mut command = Command::new("this-ocr-executable-must-never-be-resolved");
        assert!(
            signal
                .process(&mut command)
                .err()
                .unwrap()
                .contains("cancelled before")
        );
        assert!(
            signal
                .process
                .lock()
                .unwrap()
                .iter()
                .all(|slot| slot.child.is_none() && !slot.reserved)
        );
    }

    #[test]
    fn expired_provider_does_not_cancel_fresh_visual_results() {
        for source in [TargetSource::Accessibility, TargetSource::SystemOcr] {
            let workers = Workers::default();
            let (entered, entry) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            let wait = Mutex::new(wait);
            let old = workers.start(&[source], move |_, _| {
                entered.send(()).unwrap();
                wait.lock().unwrap().recv().unwrap();
                UiScanStatus::Success
            });
            entry.recv_timeout(Duration::from_secs(2)).unwrap();

            let output = Arc::new(ScanMailbox::default());
            let generation = output.begin(2);
            let mut fusion = Worker::default();
            let current = fusion.prepare(generation).unwrap().begin(Options {
                id: 2,
                generation,
                threshold: 0.5,
                defer_visual: false,
                output: Arc::clone(&output),
                wake: Arc::new(|| {}),
                accepts: Arc::new(|_| true),
                current: Arc::new(|| true),
            });
            current.submit(
                TargetSource::Contour,
                vec![UiTarget {
                    details: None,
                    rect: Rect::new(200.0, 0.0, 50.0, 20.0),
                    name: "page button".into(),
                    role: SemanticRole::Button,
                }],
            );
            // The source's deadline expires behind an old native call while
            // another source still has usable results in the fusion inbox.
            let deadline = Instant::now();
            let next = workers.start(&[source], move |_, stopped| {
                interrupt_status(deadline, stopped.load(Ordering::Acquire))
                    .expect("an expired source must not start native recognition")
            });
            drop(old);
            release.send(()).unwrap();
            let statuses = next.wait(Instant::now() + Duration::from_secs(2), || false);
            assert_eq!(statuses.as_slice(), &[UiScanStatus::TimedOut]);
            current.finish(combined_status(&[
                UiScanStatus::Success,
                statuses[0].clone(),
            ]));
            current.flush();
            let result = output.take().unwrap();
            assert_eq!(result.status, UiScanStatus::Success);
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].name, "page button");
            assert_eq!(
                interrupt_status(deadline, true),
                Some(UiScanStatus::ContextChanged),
                "an actual exit must still cancel even after the deadline"
            );
            workers
                .stop_until(Instant::now() + Duration::from_secs(2))
                .unwrap();
        }
    }

    #[test]
    fn repeated_cancellation_wakes_the_coordinator_before_native_ocr_returns() {
        let workers = Workers::default();
        for _ in 0..64 {
            let image = Arc::new(vec![0; 64 * 1024]);
            let observer = Arc::downgrade(&image);
            let (entered, entry) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            let wait = Mutex::new(wait);
            let batch = workers.start(&[TargetSource::SystemOcr], move |_, cancelled| {
                std::hint::black_box(image.len());
                entered.send(()).unwrap();
                wait.lock().unwrap().recv().unwrap();
                assert!(cancelled.load(Ordering::Acquire));
                UiScanStatus::Success
            });
            entry.recv_timeout(Duration::from_secs(2)).unwrap();
            let (waiting, wait_started) = mpsc::sync_channel(1);
            let (finished, result) = mpsc::sync_channel(1);
            let coordinator = std::thread::spawn(move || {
                let statuses = batch.wait(Instant::now() + Duration::from_secs(30), || {
                    let _ = waiting.try_send(());
                    false
                });
                finished.send(statuses).unwrap();
            });
            wait_started.recv_timeout(Duration::from_secs(2)).unwrap();
            workers.cancel_all();
            let statuses = result
                .recv_timeout(Duration::from_secs(2))
                .expect("exit must not wait for the blocked native OCR or its deadline");
            assert_eq!(statuses.as_slice(), &[UiScanStatus::ContextChanged]);
            assert!(
                observer.upgrade().is_some(),
                "in-flight images must stay valid until native return"
            );
            coordinator.join().unwrap();
            release.send(()).unwrap();
        }
        workers
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn rapid_reentry_releases_superseded_captures_and_runs_only_the_latest_ocr() {
        struct Capture {
            live: Arc<AtomicUsize>,
            pixels: Box<[u8]>,
        }
        impl Drop for Capture {
            fn drop(&mut self) {
                self.live.fetch_sub(1, Ordering::AcqRel);
            }
        }
        let live = Arc::new(AtomicUsize::new(0));
        let peak = AtomicUsize::new(0);
        let capture = || {
            let count = live.fetch_add(1, Ordering::AcqRel) + 1;
            peak.fetch_max(count, Ordering::AcqRel);
            Arc::new(Capture {
                live: Arc::clone(&live),
                pixels: vec![0; 64 * 1024].into_boxed_slice(),
            })
        };
        let workers = Workers::default();
        let (entered, entry) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let image = capture();
        let first = workers.start(&[TargetSource::SystemOcr], move |_, cancelled| {
            std::hint::black_box(image.pixels.len());
            entered.send(()).unwrap();
            wait.lock().unwrap().recv().unwrap();
            assert!(cancelled.load(Ordering::Acquire));
            UiScanStatus::Success
        });
        entry.recv_timeout(Duration::from_secs(2)).unwrap();

        // Keep an old native call blocked across 512 rapid enter/exit cycles.
        // Drop old coordinators after submitting replacements to exercise late
        // cleanup racing with the new generation's pending capture.
        let mut previous = None;
        for generation in 1..=512 {
            let image = capture();
            let observer = Arc::downgrade(&image);
            let next = workers.start(&[TargetSource::SystemOcr], move |_, _| {
                std::hint::black_box(image.pixels.len());
                panic!("cancelled generation {generation} must not run")
            });
            drop(previous.replace(next));
            assert!(
                observer.upgrade().is_some(),
                "old cleanup cancelled the new capture"
            );
            if generation % 2 == 0 {
                workers.cancel_all();
                drop(previous.take());
                assert!(observer.upgrade().is_none());
                assert_eq!(live.load(Ordering::Acquire), 1);
            } else {
                assert_eq!(live.load(Ordering::Acquire), 2);
            }
        }
        assert!(peak.load(Ordering::Acquire) <= 3);
        assert_eq!(workers.joins.lock().unwrap().iter().flatten().count(), 1);

        let image = capture();
        let (ready, received) = mpsc::channel();
        let last = workers.start(&SOURCES, move |source, cancelled| {
            std::hint::black_box(image.pixels.len());
            assert!(!cancelled.load(Ordering::Acquire));
            ready.send(source).unwrap();
            UiScanStatus::Success
        });
        drop(first);
        // Other sources still complete while the old OCR owns its image.
        for _ in 0..SOURCES.len() - 1 {
            assert_ne!(
                received.recv_timeout(Duration::from_secs(2)).unwrap(),
                TargetSource::SystemOcr
            );
        }
        release.send(()).unwrap();
        let statuses = last.wait(Instant::now() + Duration::from_secs(2), || false);
        assert_eq!(statuses.len(), SOURCES.len());
        assert!(
            statuses
                .iter()
                .all(|status| *status == UiScanStatus::Success)
        );
        assert_eq!(
            received.recv_timeout(Duration::from_secs(2)).unwrap(),
            TargetSource::SystemOcr
        );
        assert_eq!(live.load(Ordering::Acquire), 0);
        workers
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn reentry_publishes_fresh_partials_and_rejects_late_cancelled_ocr() {
        let workers = Workers::default();
        let output = Arc::new(ScanMailbox::default());
        let mut fusion = Worker::default();
        let options = |id, generation| Options {
            id,
            generation,
            threshold: 0.5,
            defer_visual: false,
            output: Arc::clone(&output),
            wake: Arc::new(|| {}),
            accepts: Arc::new(|_| true),
            current: Arc::new(|| true),
        };
        let generation = output.begin(1);
        let old = fusion
            .prepare(generation)
            .unwrap()
            .begin(options(1, generation));
        let (entered, entry) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let first = workers.start(&[TargetSource::SystemOcr], move |_, _| {
            entered.send(()).unwrap();
            wait.lock().unwrap().recv().unwrap();
            // Emulate a native provider ignoring cancellation until it returns.
            assert_eq!(
                old.submit(
                    TargetSource::SystemOcr,
                    vec![UiTarget::recognized_text(
                        Rect::new(0.0, 0.0, 50.0, 20.0),
                        "stale OCR".into(),
                    )]
                ),
                0
            );
            UiScanStatus::Success
        });
        entry.recv_timeout(Duration::from_secs(2)).unwrap();
        workers.cancel_all();
        assert_eq!(
            first.wait(Instant::now() + Duration::from_secs(2), || false)[0],
            UiScanStatus::ContextChanged
        );

        let generation = output.begin(2);
        let current = fusion
            .prepare(generation)
            .unwrap()
            .begin(options(2, generation));
        let publisher = current.clone();
        let (ready, received) = mpsc::channel();
        let second = workers.start(
            &[
                TargetSource::SystemOcr,
                TargetSource::NativeRectangles,
                TargetSource::Accessibility,
                TargetSource::Contour,
            ],
            move |source, _| {
                let target = if source == TargetSource::SystemOcr {
                    UiTarget::recognized_text(Rect::new(100.0, 0.0, 50.0, 20.0), "fresh OCR".into())
                } else if source == TargetSource::Accessibility {
                    UiTarget {
                        details: None,
                        rect: Rect::new(300.0, 200.0, 80.0, 20.0),
                        name: "网页链接".into(),
                        role: SemanticRole::Link,
                    }
                } else if source == TargetSource::Contour {
                    UiTarget {
                        details: None,
                        rect: Rect::new(400.0, 200.0, 50.0, 20.0),
                        name: String::new(),
                        role: SemanticRole::Control,
                    }
                } else {
                    UiTarget {
                        details: None,
                        rect: Rect::new(200.0, 0.0, 50.0, 20.0),
                        name: "fresh button".into(),
                        role: SemanticRole::Button,
                    }
                };
                publisher.submit(source, vec![target]);
                ready.send(source).unwrap();
                UiScanStatus::Success
            },
        );
        let ready: Vec<_> = (0..3)
            .map(|_| received.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect();
        for source in [
            TargetSource::NativeRectangles,
            TargetSource::Accessibility,
            TargetSource::Contour,
        ] {
            assert!(ready.contains(&source));
        }
        current.flush();
        let partial = output
            .take()
            .expect("second entry must produce labels before old OCR returns");
        assert_eq!(partial.id, 2);
        assert_eq!(partial.status, UiScanStatus::Partial);
        assert_eq!(partial.targets.len(), 3);
        assert!(partial.targets.iter().any(|t| t.name == "网页链接"));
        assert!(
            partial
                .targets
                .iter()
                .any(|t| t.role == SemanticRole::Control)
        );
        assert!(partial.targets.iter().any(|t| t.name == "fresh button"));

        release.send(()).unwrap();
        let statuses = second.wait(Instant::now() + Duration::from_secs(2), || false);
        assert!(
            statuses
                .iter()
                .all(|status| *status == UiScanStatus::Success)
        );
        current.finish(combined_status(&statuses));
        current.flush();
        let terminal = output.take().unwrap();
        assert_eq!(terminal.id, 2);
        assert_eq!(terminal.status, UiScanStatus::Success);
        assert_eq!(terminal.targets.len(), 1);
        assert_eq!(terminal.targets[0].ocr_text(), "fresh OCR");
        assert!(output.take().is_none());
        workers
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn blocked_ocr_allows_reentry_and_keeps_its_image_until_actual_return() {
        let workers = Workers::default();
        let image = Arc::new(());
        let observer = Arc::downgrade(&image);
        let (entered, entry) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let first = workers.start(&[TargetSource::SystemOcr], move |_, cancelled| {
            let _image = &image;
            entered.send(()).unwrap();
            wait.lock().unwrap().recv().unwrap();
            assert!(cancelled.load(Ordering::Acquire));
            UiScanStatus::Success
        });
        entry.recv_timeout(Duration::from_secs(2)).unwrap();
        workers.cancel_all();
        assert_eq!(
            first.wait(Instant::now() + Duration::from_secs(2), || false)[0],
            UiScanStatus::ContextChanged
        );
        assert!(observer.upgrade().is_some());
        let (ready, received) = mpsc::channel();
        let second = workers.start(
            &[
                TargetSource::SystemOcr,
                TargetSource::NativeRectangles,
                TargetSource::Accessibility,
            ],
            move |source, _| {
                ready.send(source).unwrap();
                UiScanStatus::Success
            },
        );
        for _ in 0..2 {
            assert_ne!(
                received.recv_timeout(Duration::from_secs(2)).unwrap(),
                TargetSource::SystemOcr
            );
        }
        release.send(()).unwrap();
        let statuses = second.wait(Instant::now() + Duration::from_secs(2), || false);
        assert_eq!(combined_status(&statuses), UiScanStatus::Success);
        assert_eq!(
            received.recv_timeout(Duration::from_secs(2)).unwrap(),
            TargetSource::SystemOcr
        );
        assert!(observer.upgrade().is_none());
        assert_eq!(workers.joins.lock().unwrap().iter().flatten().count(), 3);
        workers
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn timeout_drops_queued_images_and_the_next_scan_recovers() {
        let workers = Workers::default();
        let (entered, entry) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let first = workers.start(&[TargetSource::SystemOcr], move |_, _| {
            entered.send(()).unwrap();
            wait.lock().unwrap().recv().unwrap();
            UiScanStatus::Success
        });
        entry.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(first);
        let image = Arc::new(());
        let observer = Arc::downgrade(&image);
        let second = workers.start(&[TargetSource::SystemOcr], move |_, _| {
            let _image = &image;
            panic!("expired queued scan must not execute")
        });
        assert_eq!(
            second.wait(Instant::now(), || false)[0],
            UiScanStatus::TimedOut
        );
        assert!(observer.upgrade().is_none());
        let third = workers.start(&[TargetSource::SystemOcr], |_, _| UiScanStatus::Success);
        release.send(()).unwrap();
        assert_eq!(
            third.wait(Instant::now() + Duration::from_secs(2), || false)[0],
            UiScanStatus::Success
        );
        assert_eq!(workers.joins.lock().unwrap().iter().flatten().count(), 1);
        workers
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn shutdown_wakes_waiters_and_retains_an_unresponsive_owner_for_retry() {
        let workers = Workers::default();
        let (entered, entry) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let batch = workers.start(&[TargetSource::Accessibility], move |_, cancelled| {
            entered.send(()).unwrap();
            wait.lock().unwrap().recv().unwrap();
            assert!(cancelled.load(Ordering::Acquire));
            UiScanStatus::Success
        });
        entry.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(workers.stop_until(Instant::now()).is_err());
        assert_eq!(
            batch.wait(Instant::now() + Duration::from_secs(2), || false)[0],
            UiScanStatus::ContextChanged
        );
        let rejected = workers.start(&[TargetSource::SystemOcr], |_, _| panic!("stopping"));
        assert_eq!(
            rejected.wait(Instant::now(), || false)[0],
            UiScanStatus::ContextChanged
        );
        release.send(()).unwrap();
        workers
            .stop_until(Instant::now() + Duration::from_secs(2))
            .unwrap();
    }
}
