#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::api::command::{UiScanRequest, UiScanStatus, UiScanStrategy};
use crate::platform::common::scan_fusion::{Handle, Options, Sink};
use crate::platform::common::scan_mailbox::ScanMailbox;
use crate::platform::common::spatial_index::TargetSource;
use crate::support::errors::ErrorBundle;
use crate::support::worker::WorkerJoin;
use objc2::rc::autoreleasepool;

use super::{EventSender, accessibility, scan_workers::Workers, vision};

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

#[derive(Default)]
struct ScanQueue {
    state: Mutex<ScanQueueState>,
    ready: Condvar,
    providers: Workers,
}

#[derive(Default)]
struct ScanQueueState {
    pending: Option<ScanJob>,
    // Keep only the identity after completion: exiting must still cancel any
    // native owner that outlived the terminal result in the output mailbox.
    current: Option<(u64, u64)>,
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
        queue.providers.cancel_all();
        drop(queue.submit(ScanJob {
            request,
            generation,
            mailbox,
            wake,
            fusion,
        }));
    }

    pub(super) fn cancel_scan(&self, request_id: u64) {
        if let Some(queue) = self.queue.as_ref()
            && let Some(generation) = queue.cancel(request_id)
            && LATEST_SCAN
                .compare_exchange(generation, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            queue.providers.cancel_all();
            vision::mark_latest(0);
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
        let now = Instant::now();
        let local_deadline = deadline.min(now.checked_add(STOP_TIMEOUT).unwrap_or(deadline));
        let mut errors = ErrorBundle::default();
        if let Some(worker) = self.worker.as_mut() {
            errors.record("scan coordinator", worker.join_until(local_deadline));
        }
        if let Some(queue) = self.queue.as_ref() {
            errors.record(
                "native scan providers",
                queue.providers.stop_until(local_deadline),
            );
        }
        self.shutdown_failure_returned = !errors.is_empty();
        if !self.shutdown_failure_returned {
            self.worker.take();
            self.queue.take();
        }
        errors.into_result()
    }

    fn ensure_started(&mut self) -> Result<Arc<ScanQueue>, String> {
        if let Some(queue) = self.queue.as_ref() {
            return Ok(Arc::clone(queue));
        }
        let queue = Arc::new(ScanQueue::default());
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
#[derive(Clone)]
struct PartialPublisher {
    sink: Sink,
}
impl PartialPublisher {
    fn new(job: &ScanJob, pid: libc::pid_t, activation_pid: Option<libc::pid_t>) -> Self {
        let generation = job.generation;
        let bounds = job.request.bounds;
        let screens = job.request.screen_bounds.clone();
        let wake = job.wake.clone();
        let sink = job.fusion.begin(Options {
            id: job.request.id,

            generation,
            threshold: job.request.vision.merge_iou_threshold,
            defer_visual: crate::platform::common::contour::sources(
                job.request.strategy,
                &job.request.vision,
            )
            .0,
            progress_sources: {
                let (text, contours) = crate::platform::common::contour::sources(
                    job.request.strategy,
                    &job.request.vision,
                );
                let mut sources = Vec::new();
                if scan_sources(job.request.strategy).0 {
                    sources.push(TargetSource::Accessibility);
                }
                if text {
                    sources.push(TargetSource::SystemOcr);
                }
                if contours {
                    sources.push(TargetSource::Contour);
                    if job.request.strategy != UiScanStrategy::Contour {
                        sources.push(TargetSource::NativeRectangles);
                    }
                }
                sources
            },
            output: Arc::clone(&job.mailbox),
            wake: Arc::new(move || wake.wake()),
            accepts: Arc::new(move |target| {
                let center = target.rect.center();
                bounds.is_none_or(|bounds| bounds.contains(&center))
                    && (screens.is_empty() || screens.iter().any(|screen| screen.contains(&center)))
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
        state.current = Some((job.request.id, job.generation));
        let replaced = state.pending.replace(job);
        self.ready.notify_one();
        replaced
    }

    fn cancel(&self, request_id: u64) -> Option<u64> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let (_, generation) = state.current.filter(|(id, _)| *id == request_id)?;
        state.current = None;
        let pending = state.pending.take();
        drop(state);
        drop(pending);
        Some(generation)
    }

    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.stopping = true;
        state.current = None;
        let pending = state.pending.take();
        self.ready.notify_all();
        drop(state);
        drop(pending);
        self.providers.request_stop();
    }

    fn run(&self) {
        let mut captures = vision::CaptureCache::default();
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
            autoreleasepool(|_| run_scan(job, &self.providers, &mut captures));
        }
    }
}

fn run_scan(mut job: ScanJob, providers: &Workers, captures: &mut vision::CaptureCache) {
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
                crate::api::UiScanScope::Screen | crate::api::UiScanScope::All => false,
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
    } else if job.request.scope != crate::api::UiScanScope::All {
        job.request.scope = crate::api::UiScanScope::Screen;
    }

    let publisher = PartialPublisher::new(&job, pid, activation_pid);
    let (ax_enabled, vision_enabled) = scan_sources(job.request.strategy);
    let ax_deadline =
        Instant::now() + Duration::from_millis(job.request.timeout_ms.clamp(1, 30_000));
    let ax = ax_enabled.then(|| {
        let request = job.request.clone();
        let generation = job.generation;
        let publisher = publisher.clone();
        providers.start(&[TargetSource::Accessibility], move |_, cancelled| {
            autoreleasepool(|_| {
                stream_ax(
                    generation,
                    &request,
                    target,
                    &publisher,
                    ax_deadline,
                    || !cancelled.load(Ordering::Acquire) && scan_id_is_current(generation),
                )
            })
        })
    });
    let vision = vision_enabled.then(|| {
        scan_vision(
            job.generation,
            &job.request,
            &publisher,
            providers,
            captures,
        )
    });
    let ax = ax.map(|batch| {
        crate::platform::common::scan_fusion::combined_status(
            &batch.wait(ax_deadline, || !scan_id_is_current(job.generation)),
        )
    });
    let status = match (ax, vision) {
        (Some(ax), Some(vision)) => combined_status(("AX", ax), ("Vision", vision)),
        (Some(status), None) | (None, Some(status)) => status,
        (None, None) => UiScanStatus::Failed("UI scan strategy has no enabled source".into()),
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
    generation: u64,
    request: &UiScanRequest,
    target: Option<(libc::pid_t, crate::api::Rect)>,
    publisher: &PartialPublisher,
    deadline: Instant,
    is_current: impl Fn() -> bool,
) -> UiScanStatus {
    if let Some(status) = super::scan_workers::interrupt_status(deadline, !is_current()) {
        return status;
    }
    let current = || is_current() && Instant::now() < deadline;
    let started = Instant::now();
    let mut count = 0usize;
    let publish = |batch: Vec<crate::api::UiTarget>| {
        let first = count == 0 && !batch.is_empty();
        count += batch.len();
        publisher.push(TargetSource::Accessibility, batch);
        if first {
            crate::log_info!(
                "macos-ui-scan",
                "scan {generation}: AX first targets after {} ms",
                started.elapsed().as_millis()
            );
        }
    };
    let progress = |progress| {
        publisher
            .sink
            .progress(TargetSource::Accessibility, progress)
    };
    let result = if let Some((pid, bounds)) = target {
        progress(crate::platform::common::scan_progress::Progress::Plan(
            vec![bounds],
        ));
        let result =
            accessibility::scan_process_stream(pid, bounds, request, deadline, current, publish);
        progress(crate::platform::common::scan_progress::Progress::Complete(
            bounds,
        ));
        result
    } else {
        accessibility::scan_screen_stream(request, deadline, current, publish, progress)
    };
    crate::log_info!(
        "macos-ui-scan",
        "scan {generation}: AX returned {} targets after {} ms",
        count,
        started.elapsed().as_millis()
    );
    super::scan_workers::interrupt_status(deadline, !is_current())
        .unwrap_or_else(|| result.map_or_else(UiScanStatus::Failed, |_| UiScanStatus::Success))
}

fn combined_status(
    (first, ax): (&str, UiScanStatus),
    (second, vision): (&str, UiScanStatus),
) -> UiScanStatus {
    let statuses = [ax, vision];
    let status = crate::platform::common::scan_fusion::combined_status(&statuses);
    if status == UiScanStatus::Success {
        report_masked_provider_status(first, &statuses[0]);
        report_masked_provider_status(second, &statuses[1]);
    }
    status
}

fn report_masked_provider_status(provider: &str, status: &UiScanStatus) {
    match status {
        UiScanStatus::Failed(error) => crate::report_error!(
            "macos-ui-scan",
            "{provider} scan failed while another source or display succeeded: {error}"
        ),
        UiScanStatus::PermissionDenied(message) | UiScanStatus::Unsupported(message) => {
            crate::report_warning!(
                "macos-ui-scan",
                "{provider} scan is unavailable while another source or display succeeded: {message}"
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
    providers: &Workers,
    captures: &mut vision::CaptureCache,
) -> UiScanStatus {
    vision::detect(
        (generation, captures),
        request.capture_regions(),
        &request.vision,
        request.strategy,
        providers,
        move || !scan_id_is_current(generation),
        publisher.sink.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::command::VisionOptions;

    fn request(id: u64) -> UiScanRequest {
        UiScanRequest {
            screen_bounds: Vec::new(),

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
    fn superseded_cancellation_preserves_the_new_pending_scan() {
        let queue = ScanQueue::default();
        let (sender, _receiver) = crate::platform::common::event_queue::channel();
        let mailbox = Arc::new(ScanMailbox::default());
        let old_generation = mailbox.begin(1);
        let generation = mailbox.begin(2);
        let mut fusion = crate::platform::common::scan_fusion::Worker::default();
        queue.state.lock().unwrap().current = Some((1, old_generation));
        assert!(
            queue
                .submit(ScanJob {
                    request: request(2),
                    generation,
                    mailbox,
                    wake: EventSender::new(sender),
                    fusion: fusion.prepare(generation).unwrap(),
                })
                .is_none()
        );
        assert_eq!(queue.cancel(1), None);
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
        assert_eq!(queue.cancel(2), Some(generation));
        assert!(queue.state.lock().unwrap().pending.is_none());
        assert_eq!(queue.cancel(1), None);
        assert_eq!(queue.cancel(3), None);
    }

    #[test]
    fn completed_request_can_be_cancelled_after_its_terminal_was_consumed() {
        let queue = ScanQueue::default();
        let mailbox = ScanMailbox::default();
        let generation = mailbox.begin(9);
        queue.state.lock().unwrap().current = Some((9, generation));
        assert!(mailbox.publish(generation, 9, Vec::new(), UiScanStatus::Success));
        assert!(mailbox.take().is_some());
        assert!(!mailbox.cancel(9));
        assert_eq!(queue.cancel(9), Some(generation));
        assert_eq!(queue.cancel(9), None);
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
                ("AX", UiScanStatus::Failed("AX failed".into())),
                ("Vision", UiScanStatus::Success),
            ),
            UiScanStatus::Success
        );
        assert_eq!(
            combined_status(
                ("AX", UiScanStatus::Success),
                ("Vision", UiScanStatus::Failed("Vision failed".into())),
            ),
            UiScanStatus::Success
        );
    }

    #[test]
    fn failed_displays_do_not_become_a_success_without_any_successful_scan() {
        assert_eq!(
            combined_status(
                ("first display", UiScanStatus::TimedOut),
                ("display", UiScanStatus::Unsupported("unavailable".into()))
            ),
            UiScanStatus::TimedOut
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
        let queue = ScanQueue::default();
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
        let queue = ScanQueue::default();
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
