#![forbid(unsafe_code)]

//! Native Windows visual UI-hint scanning without OpenCV.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak, mpsc};
use std::time::{Duration, Instant};

use windows::Media::Ocr::{OcrEngine, OcrResult};
use windows_future::{AsyncOperationCompletedHandler, AsyncStatus, IAsyncOperation};

use smallvec::SmallVec;

use crate::api::command::UiScanStatus;
#[cfg(test)]
use crate::api::geometry::SemanticRole;
use crate::api::geometry::{Rect, UiTarget};
use crate::platform::common::scan_reuse::{IdleWorker, JobJoin};
use crate::support::worker::WorkerJoin;

use super::accessibility::WindowsScanPlan;
use super::display::overlay_worker::CaptureLease;
use super::ui_scan::ScanSource;

mod capture;
mod discovery;
mod provider_mailbox;
mod providers;
mod system_ocr;

use capture::*;
use discovery::*;
use provider_mailbox::*;
use providers::*;
pub(super) use system_ocr::image_to_desktop;
use system_ocr::{OcrOperationGuard, stream_system_targets_from_result};

// Covers a native 3840x2160 desktop so common 4K scans use the BitBlt fast
// path. Apply this budget to each display in All scope; larger 5K/8K captures
// are still scaled individually.
const MAX_CAPTURE_PIXELS: f64 = 8_388_608.0;
const MAX_CAPTURE_EDGE: f64 = 4_096.0;
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
const PROVIDER_STOP_TIMEOUT: Duration = Duration::from_millis(500);
const MIN_SYSTEM_OCR_TILE_SIDE: u32 = 64;
const SYSTEM_OCR_TILE_OVERLAP: u32 = 64;
const MAX_SYSTEM_OCR_IN_FLIGHT: usize = crate::platform::common::image_tiles::MAX_OCR_IN_FLIGHT;

#[derive(Debug)]
enum VisionError {
    Cancelled,
    TimedOut,
    Unavailable(String),
    Operational(String),
    Cleanup(String),
}

impl VisionError {
    fn is_control_flow(&self) -> bool {
        matches!(self, Self::Cancelled | Self::TimedOut)
    }
}

impl std::fmt::Display for VisionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("provider cancelled"),
            Self::TimedOut => formatter.write_str("provider timed out"),
            Self::Unavailable(error) | Self::Operational(error) | Self::Cleanup(error) => {
                formatter.write_str(error)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CaptureGeometry {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) desktop_bounds: Rect,
    pub(super) scale: f64,
}

pub(super) fn diagnostic_lines() -> Vec<String> {
    let system = match probe_system_ocr(|| false) {
        Ok(descriptor) => format!(
            "system OCR: available (languages [{}], maximum image dimension {})",
            descriptor.languages.join(", "),
            descriptor.maximum_dimension
        ),
        Err(error) => format!("system OCR: unavailable ({error})"),
    };
    vec![system]
}

struct ScanJob {
    request: Arc<WindowsScanPlan>,
    generation: u64,
    source: ScanSource,
    capture: Option<CaptureLease>,
}

#[derive(Default)]
struct QueueState {
    pending: Option<ScanJob>,
    active_request_id: Option<u64>,
    active_cancellation: Option<Weak<ScanSignal>>,
    running: bool,
    stopping: bool,
}

#[derive(Default)]
struct SharedQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
    contour: IdleWorker<crate::platform::common::contour::Workspace>,
    system_ocr: IdleWorker<SystemOcrResources>,
    latest_generation: AtomicU64,
    stopping: AtomicBool,
    provider_quarantine_nonempty: AtomicBool,
    provider_quarantine: Mutex<Vec<JobJoin>>,
}

#[derive(Clone)]
struct ScanCancellation {
    shared: Arc<SharedQueue>,
    generation: u64,
    signal: Arc<ScanSignal>,
}

struct ScanSignal {
    local: AtomicBool,
    mailbox: Arc<ProviderMailbox>,
    system_wake: Mutex<Option<mpsc::SyncSender<SystemOcrInput>>>,
    credit_wakes: Mutex<SmallVec<[Weak<mpsc::Sender<()>>; 2]>>,
    discovery: Weak<DiscoveryShared>,
}

impl ScanCancellation {
    fn new(
        shared: &Arc<SharedQueue>,
        generation: u64,
        mailbox: &Arc<ProviderMailbox>,
        discovery: Weak<DiscoveryShared>,
    ) -> Self {
        Self {
            shared: Arc::clone(shared),
            generation,
            signal: Arc::new(ScanSignal {
                local: AtomicBool::new(false),
                mailbox: Arc::clone(mailbox),
                system_wake: Mutex::new(None),
                credit_wakes: Mutex::new(SmallVec::new()),
                discovery,
            }),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.signal.local.load(Ordering::Acquire)
            || self.shared.stopping.load(Ordering::Acquire)
            || self.shared.latest_generation.load(Ordering::Acquire) != self.generation
    }

    fn cancel(&self) {
        self.signal.cancel();
    }

    fn signal(&self) -> Weak<ScanSignal> {
        Arc::downgrade(&self.signal)
    }

    fn register_system_wake(&self, sender: mpsc::SyncSender<SystemOcrInput>) {
        self.signal.register_system_wake(sender);
    }

    fn register_credit_wake(&self, sender: &Arc<mpsc::Sender<()>>) {
        self.signal.register_credit_wake(sender);
    }
}

impl ScanSignal {
    fn cancel(&self) {
        if self.local.swap(true, Ordering::AcqRel) {
            return;
        }
        self.mailbox.close();
        let wake = self
            .system_wake
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(sender) = wake.as_ref() {
            let _ = sender.try_send(SystemOcrInput::CancelWake);
        }
        drop(wake);
        for sender in self
            .credit_wakes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter_map(Weak::upgrade)
        {
            let _ = sender.send(());
        }
        if let Some(discovery) = self.discovery.upgrade() {
            // Pair notification with the waiter's lock so cancellation cannot
            // land between its predicate check and entering the wait.
            let _state = discovery
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            discovery.ready.notify_all();
        }
    }

    fn register_system_wake(&self, sender: mpsc::SyncSender<SystemOcrInput>) {
        let mut wake = self
            .system_wake
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.local.load(Ordering::Acquire) {
            let _ = sender.try_send(SystemOcrInput::CancelWake);
        }
        *wake = Some(sender);
    }

    fn register_credit_wake(&self, sender: &Arc<mpsc::Sender<()>>) {
        let mut wake = self
            .credit_wakes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.local.load(Ordering::Acquire) {
            let _ = sender.send(());
        }
        wake.push(Arc::downgrade(sender));
    }
}

/// Own every provider job started for one scan. Dropping a scan first
/// invalidates its cancellation token and then joins all providers, so leaving
/// UI Hint cannot strand OCR or pixel-analysis work in normal mode.
struct ProviderThreads {
    cancellation: ScanCancellation,
    shared: Arc<SharedQueue>,
    threads: Vec<JobJoin>,
}

impl ProviderThreads {
    fn new(cancellation: ScanCancellation, shared: &Arc<SharedQueue>) -> Self {
        Self {
            cancellation,
            shared: Arc::clone(shared),
            threads: Vec::with_capacity(3),
        }
    }

    fn spawn_reused<T: 'static>(
        &mut self,
        worker: &IdleWorker<T>,
        name: &'static str,
        work: impl FnOnce(&mut crate::platform::common::scan_reuse::IdleCache<T>) + Send + 'static,
    ) -> bool {
        match worker.submit(name, move |idle| {
            if let Err(error) = super::native::prefer_background_work() {
                crate::report_warning!(
                    "windows-vision",
                    "cannot lower {name} provider priority: {error}"
                );
            }
            work(idle);
        }) {
            Ok(job) => {
                self.threads.push(job);
                true
            }
            Err(error) => {
                crate::report_warning!("windows-vision", "cannot start {name} provider: {error}");
                false
            }
        }
    }

    fn join_all(&mut self, deadline: Instant) -> Result<(), String> {
        self.cancellation.cancel();
        let mut failures = Vec::new();
        let mut quarantine = Vec::new();
        for mut worker in self.threads.drain(..) {
            if let Err(error) = worker.join_until(deadline) {
                failures.push(error);
                quarantine.push(worker);
            }
        }
        if !quarantine.is_empty() {
            // A quarantined provider retains its closure stack, including the
            // mailbox Arc. Drop already-published target strings here so the
            // native owner does not also pin unrelated UI results.
            self.cancellation.signal.mailbox.discard();
            let mut retained = self
                .shared
                .provider_quarantine
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            retained.extend(quarantine);
            self.shared
                .provider_quarantine_nonempty
                .store(true, Ordering::Release);
        }
        failures
            .is_empty()
            .then_some(())
            .ok_or_else(|| failures.join("; "))
    }
}

impl Drop for ProviderThreads {
    fn drop(&mut self) {
        if let Err(error) = self.join_all(Instant::now() + PROVIDER_STOP_TIMEOUT) {
            crate::support::logging::report_error("windows-vision", error);
        }
    }
}

pub(super) struct VisionWorker {
    shared: Arc<SharedQueue>,
    discovery: OcrDiscovery,
    workers: Vec<WorkerJoin>,
    shutdown_failure_returned: bool,
}

impl VisionWorker {
    pub(super) fn start() -> Self {
        Self {
            shared: Arc::new(SharedQueue::default()),
            discovery: OcrDiscovery::new(),
            workers: Vec::with_capacity(2),
            shutdown_failure_returned: false,
        }
    }

    pub(super) fn begin_discovery(&mut self) {
        self.discovery.start();
    }

    pub(super) fn submit(
        &mut self,
        request: Arc<WindowsScanPlan>,
        generation: u64,
        source: ScanSource,
        capture: CaptureLease,
    ) -> Result<(), String> {
        self.reap_finished();
        if self
            .shared
            .provider_quarantine_nonempty
            .load(Ordering::Acquire)
        {
            // Complete this source without cancelling the other providers in
            // a hybrid session. Do not start more native work while an old
            // provider still owns live OCR objects.
            source.finish(quarantined_scan_status(Some(capture)));
            return Ok(());
        }
        if crate::platform::common::contour::sources(request.strategy, &request.vision).0 {
            self.discovery.start();
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.stopping {
            return Err("visual UI scan worker is stopping".into());
        }
        self.shared
            .latest_generation
            .store(generation, Ordering::Release);
        let active_cancellation = state.active_cancellation.as_ref().and_then(Weak::upgrade);
        let superseded = state.pending.replace(ScanJob {
            request,
            generation,
            source,
            capture: Some(capture),
        });
        self.shared.ready.notify_one();
        if !state.running {
            state.running = true;
            let shared = Arc::clone(&self.shared);
            let discovery = self.discovery.handle();
            drop(state);
            let worker = match WorkerJoin::spawn(
                "Windows visual UI scanner",
                std::thread::Builder::new().name("keysteer-vision".into()),
                move || worker_main(shared, discovery),
            ) {
                Ok(worker) => worker,
                Err(error) => {
                    let mut state = self
                        .shared
                        .state
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    state.running = false;
                    let failed = state.pending.take();
                    self.shared.latest_generation.store(0, Ordering::Release);
                    drop(state);
                    if let Some(job) = superseded {
                        finish_cancelled_job(job);
                    }
                    if let Some(job) = failed {
                        finish_cancelled_job(job);
                    }
                    return Err(error);
                }
            };
            self.workers.push(worker);
        } else {
            drop(state);
        }
        if let Some(job) = superseded {
            finish_cancelled_job(job);
        }
        if let Some(cancellation) = active_cancellation {
            cancellation.cancel();
        }
        Ok(())
    }

    pub(super) fn cancel(&mut self, request_id: u64) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let pending_request_id = state.pending.as_ref().map(|job| job.request.id);
        let clear_generation =
            cancellation_clears_generation(state.active_request_id, pending_request_id, request_id);
        let pending = state.pending.take_if(|job| job.request.id == request_id);
        if clear_generation {
            self.shared.latest_generation.store(0, Ordering::Release);
        }
        let active_cancellation = clear_generation
            .then(|| state.active_cancellation.as_ref().and_then(Weak::upgrade))
            .flatten();
        drop(state);
        if let Some(cancellation) = active_cancellation {
            cancellation.cancel();
        }
        if let Some(job) = pending {
            finish_cancelled_job(job);
        }
        self.reap_finished();
    }

    pub(super) fn reap_finished(&mut self) {
        if let Err(error) = self.discovery.reap_finished() {
            crate::support::logging::report_error("windows-vision", error);
        }
        let mut index = 0;
        while index < self.workers.len() {
            match self.workers[index].reap_finished() {
                Ok(true) => {
                    drop(self.workers.swap_remove(index));
                }
                Ok(false) => index += 1,
                Err(error) => {
                    crate::support::logging::report_error("windows-vision", error);
                    drop(self.workers.swap_remove(index));
                }
            }
        }
        if self
            .shared
            .provider_quarantine_nonempty
            .load(Ordering::Acquire)
        {
            let mut quarantine = self
                .shared
                .provider_quarantine
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut index = 0;
            while index < quarantine.len() {
                match quarantine[index].reap_finished() {
                    Ok(true) => {
                        drop(quarantine.swap_remove(index));
                    }
                    Ok(false) => index += 1,
                    Err(error) => {
                        crate::support::logging::report_error("windows-vision", error);
                        drop(quarantine.swap_remove(index));
                    }
                }
            }
            if quarantine.is_empty() {
                self.shared
                    .provider_quarantine_nonempty
                    .store(false, Ordering::Release);
            }
        }
    }

    pub(super) fn stop(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let deadline = now.checked_add(STOP_TIMEOUT).unwrap_or(now);
        self.stop_until(deadline)
    }

    pub(super) fn request_stop(&self) {
        let (pending, active_cancellation) = {
            let mut state = self
                .shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.stopping = true;
            let pending = state.pending.take();
            let active_cancellation = state.active_cancellation.as_ref().and_then(Weak::upgrade);
            self.shared.stopping.store(true, Ordering::Release);
            self.shared.ready.notify_all();
            (pending, active_cancellation)
        };
        self.shared.latest_generation.store(0, Ordering::Release);
        self.shared.system_ocr.request_stop();
        self.shared.contour.request_stop();
        self.discovery.request_stop();
        if let Some(cancellation) = active_cancellation {
            cancellation.cancel();
        }
        if let Some(job) = pending {
            finish_cancelled_job(job);
        }
    }

    pub(super) fn stop_until(&mut self, deadline: Instant) -> Result<(), String> {
        self.request_stop();
        let mut errors = crate::support::errors::ErrorBundle::default();
        errors.record(
            "OCR source shutdown",
            self.shared.system_ocr.stop_until(deadline),
        );
        errors.record(
            "contour source shutdown",
            self.shared.contour.stop_until(deadline),
        );
        errors.record(
            "OCR discovery shutdown",
            self.discovery.stop_until(deadline),
        );
        let mut index = 0;
        while index < self.workers.len() {
            match self.workers[index].join_until(deadline) {
                Ok(()) => {
                    drop(self.workers.swap_remove(index));
                }
                Err(error) => {
                    errors.push("vision coordinator shutdown", error);
                    index += 1;
                }
            }
        }
        if self
            .shared
            .provider_quarantine_nonempty
            .load(Ordering::Acquire)
        {
            let mut quarantine = self
                .shared
                .provider_quarantine
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut index = 0;
            while index < quarantine.len() {
                match quarantine[index].join_until(deadline) {
                    Ok(()) => {
                        drop(quarantine.swap_remove(index));
                    }
                    Err(error) => {
                        errors.push("quarantined provider shutdown", error);
                        index += 1;
                    }
                }
            }
            if quarantine.is_empty() {
                self.shared
                    .provider_quarantine_nonempty
                    .store(false, Ordering::Release);
            }
        }
        let result = errors.into_result();
        self.shutdown_failure_returned = result.is_err();
        result
    }
}

impl Drop for VisionWorker {
    fn drop(&mut self) {
        if self.shutdown_failure_returned {
            return;
        }
        if let Err(error) = self.stop() {
            crate::support::logging::report_error("windows-vision", error);
        }
    }
}

fn worker_main(shared: Arc<SharedQueue>, discovery: DiscoveryHandle) {
    use crate::platform::common::scan_reuse::{IDLE_TIMEOUT, IdleCache, wait_until};
    let mut idle = IdleCache::<CaptureResources>::default();
    idle.set_limit(1, Instant::now());
    let mut idle_until = Instant::now() + IDLE_TIMEOUT;
    loop {
        drop(idle.take_expired(Instant::now()));
        let job = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.pending.is_none() && !state.stopping && Instant::now() < idle_until {
                let until = idle
                    .next_expiration()
                    .map_or(idle_until, |until| until.min(idle_until));
                drop(wait_until(&shared.ready, state, Some(until)));
                continue;
            }
            if state.stopping || state.pending.is_none() {
                state.running = false;
                return;
            }
            let Some(job) = state.pending.take() else {
                continue;
            };
            state.active_request_id = Some(job.request.id);
            job
        };
        let request_id = job.request.id;
        let (cached, expired) = idle.take(Instant::now());
        drop(expired);
        let mut resources = cached.unwrap_or_default();
        if run_scan(job, &shared, &discovery, &mut resources) {
            drop(idle.retain(resources, Instant::now()));
        }
        idle_until = Instant::now() + IDLE_TIMEOUT;
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.active_request_id == Some(request_id) {
            state.active_request_id = None;
            state.active_cancellation = None;
        }
    }
}

fn generation_is_current(shared: &SharedQueue, generation: u64) -> bool {
    !shared.stopping.load(Ordering::Acquire)
        && shared.latest_generation.load(Ordering::Acquire) == generation
}

fn context_is_current(shared: &SharedQueue, generation: u64, plan: &WindowsScanPlan) -> bool {
    generation_is_current(shared, generation) && plan.target_is_current()
}

fn finish_cancelled_job(job: ScanJob) {
    let ScanJob {
        source, capture, ..
    } = job;
    drop(capture);
    source.finish(UiScanStatus::ContextChanged);
}

fn quarantined_scan_status(capture: Option<CaptureLease>) -> UiScanStatus {
    if let Some(capture) = capture
        && let Err(error) = capture.release()
    {
        return UiScanStatus::Failed(format!("capture gate release: {error}"));
    }
    UiScanStatus::Unsupported("visual scan is waiting for a previous provider to stop".into())
}

fn cancellation_clears_generation(
    active_request_id: Option<u64>,
    pending_request_id: Option<u64>,
    cancelled_request_id: u64,
) -> bool {
    pending_request_id == Some(cancelled_request_id)
        || (pending_request_id.is_none() && active_request_id == Some(cancelled_request_id))
}

#[cfg(test)]
mod tests;
