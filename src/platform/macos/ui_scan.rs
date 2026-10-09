#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::api::command::{UiScanRequest, UiScanStatus, UiScanStrategy};
use crate::platform::common::scan_fusion::{Handle, Options, Sink};
use crate::platform::common::scan_mailbox::ScanMailbox;
use crate::platform::common::spatial_index::TargetSource;
use crate::support::worker::WorkerJoin;
use objc2::rc::autoreleasepool;

use super::{EventSender, accessibility, vision};

static LATEST_SCAN: AtomicU64 = AtomicU64::new(0);
const STOP_TIMEOUT: Duration = Duration::from_millis(500);

struct ScanJob {
    request: UiScanRequest,
    generation: u64,
    mailbox: Arc<ScanMailbox>,
    wake: EventSender,
    fusion: Handle,
}

impl ScanJob {
    fn publish(&self, targets: Vec<crate::api::UiTarget>, status: UiScanStatus) {
        if self
            .mailbox
            .publish(self.generation, self.request.id, targets, status)
        {
            self.wake.wake();
        }
    }
}

struct ScanQueue {
    state: Mutex<ScanQueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct ScanQueueState {
    pending: Option<ScanJob>,
    stopping: bool,
}

/// Backend-owned scan worker. It is created lazily on the first UIHint scan,
/// remains warm between scans, and is explicitly stopped when the backend
/// shuts down instead of relying on a process-static detached thread.
pub(super) struct UiScanWorker {
    queue: Option<Arc<ScanQueue>>,
    worker: Option<WorkerJoin>,
    shutdown_failure_returned: bool,
}

impl UiScanWorker {
    pub(super) fn new() -> Self {
        Self {
            queue: None,
            worker: None,
            shutdown_failure_returned: false,
        }
    }

    pub(super) fn request_scan(
        &mut self,
        request: UiScanRequest,
        generation: u64,
        mailbox: Arc<ScanMailbox>,
        wake: EventSender,
        fusion: Handle,
    ) {
        LATEST_SCAN.store(generation, Ordering::Release);
        vision::mark_latest(generation);
        let queue = match self.ensure_started() {
            Ok(queue) => queue,
            Err(error) => {
                if mailbox.publish(
                    generation,
                    request.id,
                    Vec::new(),
                    UiScanStatus::Failed(error),
                ) {
                    wake.wake();
                }
                return;
            }
        };
        drop(queue.submit(ScanJob {
            request,
            generation,
            mailbox,
            wake,
            fusion,
        }));
    }

    pub(super) fn cancel_scan(&self, request_id: u64) {
        LATEST_SCAN.store(0, Ordering::Release);
        vision::mark_latest(0);
        if let Some(queue) = self.queue.as_ref() {
            queue.cancel(request_id);
        }
    }

    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let deadline = now.checked_add(STOP_TIMEOUT).unwrap_or(now);
        self.shutdown_until(deadline)
    }

    pub(super) fn request_stop(&self) {
        LATEST_SCAN.store(0, Ordering::Release);
        vision::mark_latest(0);
        if let Some(queue) = self.queue.as_ref() {
            queue.stop();
        }
    }

    pub(super) fn shutdown_until(&mut self, deadline: Instant) -> Result<(), String> {
        self.request_stop();
        let Some(worker) = self.worker.as_mut() else {
            self.shutdown_failure_returned = false;
            return Ok(());
        };
        let now = Instant::now();
        let local_deadline = deadline.min(now.checked_add(STOP_TIMEOUT).unwrap_or(deadline));
        if let Err(error) = worker.join_until(local_deadline) {
            self.shutdown_failure_returned = true;
            return Err(error);
        }
        self.worker.take();
        self.queue.take();
        self.shutdown_failure_returned = false;
        Ok(())
    }

    fn ensure_started(&mut self) -> Result<Arc<ScanQueue>, String> {
        if let Some(queue) = self.queue.as_ref() {
            return Ok(Arc::clone(queue));
        }
        let queue = Arc::new(ScanQueue {
            state: Mutex::new(ScanQueueState::default()),
            ready: Condvar::new(),
        });
        let worker_queue = Arc::clone(&queue);
        let worker = WorkerJoin::spawn(
            "macOS UI scan",
            std::thread::Builder::new().name("keysteer-ui-scan".into()),
            move || worker_queue.run(),
        )?;
        self.queue = Some(Arc::clone(&queue));
        self.worker = Some(worker);
        Ok(queue)
    }
}

impl Drop for UiScanWorker {
    fn drop(&mut self) {
        if self.shutdown_failure_returned
            || self
                .worker
                .as_ref()
                .is_some_and(WorkerJoin::shutdown_failure_was_returned)
        {
            return;
        }
        if let Err(error) = self.shutdown() {
            crate::support::logging::report_error("macos-ui-scan", &error);
        }
    }
}

/// AX and Vision only submit buffers; one common worker owns all fusion state.
struct PartialPublisher {
    sink: Sink,
}
impl PartialPublisher {
    fn new(job: &ScanJob, pid: libc::pid_t, activation_pid: Option<libc::pid_t>) -> Self {
        let generation = job.generation;
        let bounds = job.request.bounds;
        let wake = job.wake.clone();
        let sink = job.fusion.begin(Options {
            id: job.request.id,
            generation,
            threshold: job.request.vision.merge_iou_threshold,
            output: Arc::clone(&job.mailbox),
            wake: Arc::new(move || wake.wake()),
            accepts: Arc::new(move |target| {
                bounds.is_none_or(|bounds| bounds.contains(&target.rect.center()))
            }),
            current: Arc::new(move || {
                autoreleasepool(|_| scan_is_current(generation, pid, activation_pid))
            }),
        });
        Self { sink }
    }
    fn push(&self, source: TargetSource, targets: Vec<crate::api::UiTarget>) {
        self.sink.submit(source, targets);
    }
    fn finish(&self, status: UiScanStatus) {
        self.sink.finish(status);
    }
}

impl ScanQueue {
    fn submit(&self, job: ScanJob) -> Option<ScanJob> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.stopping {
            return Some(job);
        }
        let replaced = state.pending.replace(job);
        self.ready.notify_one();
        replaced
    }

    fn cancel(&self, request_id: u64) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state
            .pending
            .as_ref()
            .is_some_and(|job| job.request.id == request_id)
        {
            state.pending.take();
        }
    }

    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.stopping = true;
        state.pending.take();
        self.ready.notify_all();
    }

    fn run(&self) {
        loop {
            let job = {
                let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                while state.pending.is_none() && !state.stopping {
                    state = self
                        .ready
                        .wait(state)
                        .unwrap_or_else(|error| error.into_inner());
                }
                if state.stopping {
                    return;
                }
                let Some(job) = state.pending.take() else {
                    continue;
                };
                job
            };
            autoreleasepool(|_| run_scan(job));
        }
    }
}

fn run_scan(mut job: ScanJob) {
    let original_pid = accessibility::frontmost_pid();
    let request_context_changed = job
        .request
        .app
        .as_ref()
        .is_some_and(|app| Some(app.process_id as libc::pid_t) != original_pid);
    if request_context_changed {
        job.publish(Vec::new(), UiScanStatus::ContextChanged);
        return;
    }

    let pid = original_pid.unwrap_or(0);
    // Resolve metadata once, before either provider starts. Quartz also works
    // when an application exposes no AXFocusedWindow (menus, desktop, etc.).
    let mut windows = None;
    let target = job.request.scope.resolve_window(|source| {
        let windows = windows.get_or_insert_with(|| {
            accessibility::window_manager::visible_windows().unwrap_or_default()
        });
        let pointer = (source == crate::api::UiScanScope::Window)
            .then(super::input::cursor_position)
            .transpose()
            .ok()
            .flatten();
        windows
            .iter()
            .find(|window| match source {
                crate::api::UiScanScope::Window => {
                    pointer.is_some_and(|point| window.bounds.contains(&point))
                }
                crate::api::UiScanScope::Active => Some(window.pid) == original_pid,
                crate::api::UiScanScope::Screen => false,
            })
            .map(|window| (window.pid, window.bounds))
    });
    let activation_pid = target
        .as_ref()
        .filter(|target| target.activate)
        .map(|target| target.window.0);
    if !scan_id_is_current(job.generation) {
        return;
    }
    if let Some(target) = target.as_ref().filter(|target| target.activate) {
        job.mailbox
            .expect_activation(job.generation, job.request.id, target.window.0 as u32);
        job.wake.wake();
        // Best effort; refusal never selects another scope or starts another scan.
        let _ =
            accessibility::window_manager::activate_scan_window(target.window.0, target.window.1);
    }
    let target = target.map(|target| target.window);
    if let Some((_, bounds)) = target {
        job.request.bounds = Some(requested_window_bounds(bounds, &job.request).unwrap_or(bounds));
    } else {
        job.request.scope = crate::api::UiScanScope::Screen;
    }

    let publisher = PartialPublisher::new(&job, pid, activation_pid);
    let status = match scan_sources(job.request.strategy) {
        (true, false) => stream_ax(&job, target, &publisher),
        (false, true) => stream_vision(&job, &publisher),
        (true, true) => std::thread::scope(|scope| {
            let ax = scope.spawn(|| autoreleasepool(|_| stream_ax(&job, target, &publisher)));
            let vision_status = stream_vision(&job, &publisher);
            let ax_status = ax
                .join()
                .unwrap_or_else(|_| UiScanStatus::Failed("AX scan worker panicked".into()));
            combined_status(ax_status, vision_status)
        }),
        (false, false) => UiScanStatus::Failed("UI scan strategy has no enabled source".into()),
    };

    let status = if scan_is_current(job.generation, pid, activation_pid) {
        status
    } else {
        UiScanStatus::ContextChanged
    };
    publisher.finish(status);
}

fn scan_sources(strategy: UiScanStrategy) -> (bool, bool) {
    match strategy {
        UiScanStrategy::AxTree => (true, false),
        UiScanStrategy::Vision | UiScanStrategy::Contour => (false, true),
        UiScanStrategy::Hybrid => (true, true),
    }
}

fn stream_ax(
    job: &ScanJob,
    target: Option<(libc::pid_t, crate::api::Rect)>,
    publisher: &PartialPublisher,
) -> UiScanStatus {
    let Some((pid, bounds)) = target else {
        return accessibility::scan_screen_stream(
            &job.request,
            || scan_id_is_current(job.generation),
            |batch| publisher.push(TargetSource::Accessibility, batch),
        )
        .map_or_else(UiScanStatus::Failed, |_| UiScanStatus::Success);
    };
    accessibility::scan_process_stream(
        pid,
        bounds,
        &job.request,
        || scan_id_is_current(job.generation),
        |batch| publisher.push(TargetSource::Accessibility, batch),
    )
    .map_or_else(UiScanStatus::Failed, |_| UiScanStatus::Success)
}

fn stream_vision(job: &ScanJob, publisher: &PartialPublisher) -> UiScanStatus {
    scan_vision(job.generation, &job.request, publisher)
}

fn combined_status(ax: UiScanStatus, vision: UiScanStatus) -> UiScanStatus {
    match (&ax, &vision) {
        (UiScanStatus::Success, other) => {
            report_masked_provider_status("Vision", other);
            UiScanStatus::Success
        }
        (other, UiScanStatus::Success) => {
            report_masked_provider_status("AX", other);
            UiScanStatus::Success
        }
        _ => vision,
    }
}

fn report_masked_provider_status(provider: &str, status: &UiScanStatus) {
    match status {
        UiScanStatus::Failed(error) => crate::report_error!(
            "macos-ui-scan",
            "{provider} provider failed while the other Hybrid provider succeeded: {error}"
        ),
        UiScanStatus::PermissionDenied(message) | UiScanStatus::Unsupported(message) => {
            crate::report_warning!(
                "macos-ui-scan",
                "{provider} provider is unavailable while the other Hybrid provider succeeded: {message}"
            );
        }
        UiScanStatus::Partial
        | UiScanStatus::Success
        | UiScanStatus::TimedOut
        | UiScanStatus::ContextChanged => {}
    }
}

fn scan_is_current(generation: u64, pid: libc::pid_t, activation_pid: Option<libc::pid_t>) -> bool {
    let current = accessibility::frontmost_pid().unwrap_or(0);
    scan_id_is_current(generation) && (current == pid || Some(current) == activation_pid)
}

fn scan_id_is_current(generation: u64) -> bool {
    LATEST_SCAN.load(Ordering::Acquire) == generation
}

fn requested_window_bounds(
    window: crate::api::geometry::Rect,
    request: &UiScanRequest,
) -> Option<crate::api::geometry::Rect> {
    match request.bounds {
        Some(screen) => window.intersect(&screen),
        None => Some(window),
    }
}

fn scan_vision(
    generation: u64,
    request: &UiScanRequest,
    publisher: &PartialPublisher,
) -> UiScanStatus {
    match request.bounds {
        Some(bounds) => vision::detect(
            generation,
            bounds,
            &request.vision,
            request.strategy,
            || !scan_id_is_current(generation),
            |source, targets| publisher.push(source, targets),
        ),
        None => UiScanStatus::Failed("screen scan requires display bounds".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::command::VisionOptions;

    fn request(id: u64) -> UiScanRequest {
        UiScanRequest {
            scope: crate::api::UiScanScope::Window,
            id,
            timeout_ms: 2_500,
            bounds: None,
            roles: Vec::new(),
            max_depth: 1,
            visible_only: true,
            clickable_only: true,
            strategy: UiScanStrategy::AxTree,
            vision: VisionOptions::default(),
            app: None,
        }
    }

    #[test]
    fn scan_strategies_use_only_their_configured_sources() {
        assert_eq!(scan_sources(UiScanStrategy::AxTree), (true, false));
        assert_eq!(scan_sources(UiScanStrategy::Vision), (false, true));
        assert_eq!(scan_sources(UiScanStrategy::Contour), (false, true));
        assert_eq!(scan_sources(UiScanStrategy::Hybrid), (true, true));
    }

    #[test]
    fn scan_worker_shutdown_is_idempotent_without_a_started_worker() {
        let mut worker = UiScanWorker::new();

        worker.shutdown().unwrap();
        worker.shutdown().unwrap();

        assert!(!worker.shutdown_failure_returned);
        assert!(worker.queue.is_none());
        assert!(worker.worker.is_none());
    }

    #[test]
    fn hybrid_succeeds_when_either_source_succeeds() {
        assert_eq!(
            combined_status(
                UiScanStatus::Failed("AX failed".into()),
                UiScanStatus::Success,
            ),
            UiScanStatus::Success
        );
        assert_eq!(
            combined_status(
                UiScanStatus::Success,
                UiScanStatus::Failed("Vision failed".into()),
            ),
            UiScanStatus::Success
        );
    }

    #[test]
    fn vision_capture_is_clipped_to_the_requested_cursor_screen() {
        let mut request = request(1);
        request.bounds = Some(crate::api::geometry::Rect::new(1000.0, 0.0, 800.0, 600.0));
        assert_eq!(
            requested_window_bounds(
                crate::api::geometry::Rect::new(700.0, 100.0, 900.0, 400.0),
                &request,
            ),
            Some(crate::api::geometry::Rect::new(1000.0, 100.0, 600.0, 400.0))
        );
        assert_eq!(
            requested_window_bounds(
                crate::api::geometry::Rect::new(0.0, 100.0, 700.0, 400.0),
                &request,
            ),
            None
        );
    }

    #[test]
    fn pending_scan_slot_keeps_only_the_latest_request() {
        let queue = ScanQueue {
            state: Mutex::new(ScanQueueState::default()),
            ready: Condvar::new(),
        };
        let (sender, _receiver) = crate::platform::common::event_queue::channel();
        let wake = EventSender::new(sender);
        let mailbox = Arc::new(ScanMailbox::default());
        let first_generation = mailbox.begin(1);
        let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
        assert!(
            queue
                .submit(ScanJob {
                    request: request(1),
                    generation: first_generation,
                    mailbox: Arc::clone(&mailbox),
                    wake: wake.clone(),
                    fusion: fusion_worker.prepare(first_generation).unwrap(),
                })
                .is_none()
        );
        let second_generation = mailbox.begin(2);
        let replaced = queue
            .submit(ScanJob {
                request: request(2),
                generation: second_generation,
                mailbox,
                wake,
                fusion: fusion_worker.prepare(second_generation).unwrap(),
            })
            .expect("the single slot should replace its old request");
        assert_eq!(replaced.request.id, 1);
        assert_eq!(
            queue
                .state
                .lock()
                .unwrap()
                .pending
                .as_ref()
                .unwrap()
                .request
                .id,
            2
        );
    }

    #[test]
    fn stopping_the_scan_queue_drops_pending_work() {
        let queue = ScanQueue {
            state: Mutex::new(ScanQueueState::default()),
            ready: Condvar::new(),
        };
        let (sender, _receiver) = crate::platform::common::event_queue::channel();
        let mailbox = Arc::new(ScanMailbox::default());
        let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
        let generation = mailbox.begin(7);
        queue.submit(ScanJob {
            request: request(7),
            generation,
            mailbox,
            wake: EventSender::new(sender),
            fusion: fusion_worker.prepare(generation).unwrap(),
        });

        queue.stop();

        let state = queue.state.lock().unwrap();
        assert!(state.stopping);
        assert!(state.pending.is_none());
    }
}
