//! Generation-scoped submission, fair single-writer fusion and ready deltas.
#![forbid(unsafe_code)]
use super::{
    scan_accumulator::ScanAccumulator, scan_mailbox::ScanMailbox, spatial_index::TargetSource,
};
use crate::api::{UiTarget, command::UiScanStatus};
use crate::support::worker::WorkerJoin;
use smallvec::SmallVec;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::{Duration, Instant};

pub(crate) const MAX_FUSION_BATCH: usize = 128;
const FIRST_FUSION_BATCH: usize = 16;
// Feedback for the next ready-work quantity, never a wait or publication timer.
const FUSION_CPU_TARGET: Duration = Duration::from_micros(500);
const STOP_TIMEOUT: Duration = Duration::from_millis(500);

/// Backend-independent terminal precedence for independently published sources.
pub(crate) fn combined_status(statuses: &[UiScanStatus]) -> UiScanStatus {
    if statuses
        .iter()
        .any(|status| status == &UiScanStatus::ContextChanged)
    {
        return UiScanStatus::ContextChanged;
    }
    if statuses
        .iter()
        .any(|status| status == &UiScanStatus::Success)
    {
        return UiScanStatus::Success;
    }
    if statuses
        .iter()
        .any(|status| status == &UiScanStatus::TimedOut)
    {
        return UiScanStatus::TimedOut;
    }
    statuses
        .iter()
        .find_map(|status| match status {
            UiScanStatus::PermissionDenied(message) => {
                Some(UiScanStatus::PermissionDenied(message.clone()))
            }
            UiScanStatus::Unsupported(message) => Some(UiScanStatus::Unsupported(message.clone())),
            UiScanStatus::Failed(message) => Some(UiScanStatus::Failed(message.clone())),
            _ => None,
        })
        .unwrap_or(UiScanStatus::Success)
}
type Wake = Arc<dyn Fn() + Send + Sync>;
type Accept = Arc<dyn Fn(&UiTarget) -> bool + Send + Sync>;
type Current = Arc<dyn Fn() -> bool + Send + Sync>;

pub(crate) struct Options {
    pub(crate) id: u64,

    pub(crate) generation: u64,
    pub(crate) threshold: f64,
    pub(crate) defer_visual: bool,
    // Publication is independent of scan scope. Backends select native roots;
    // confirmed semantic targets stream while regional visual work continues.
    pub(crate) progress_sources: Vec<TargetSource>,
    pub(crate) output: Arc<ScanMailbox>,
    pub(crate) wake: Wake,
    pub(crate) accepts: Accept,
    pub(crate) current: Current,
}
struct Lane {
    source: TargetSource,
    batches: VecDeque<Input>,
}
enum Input {
    Targets(std::vec::IntoIter<UiTarget>),
    Progress(super::scan_progress::Progress),
}
#[derive(Default)]
struct InboxState {
    lanes: SmallVec<[Lane; 4]>,
    ready: VecDeque<usize>,
    terminal: Option<UiScanStatus>,
    finished: bool,
    #[cfg(test)]
    version: u64,
    #[cfg(test)]
    settled: u64,
}
struct Inbox {
    id: u64,
    progress_enabled: bool,

    cancelled: AtomicBool,
    state: Mutex<InboxState>,
    #[cfg(test)]
    settled: std::sync::Condvar,
}
#[derive(Clone)]
pub(crate) struct Sink {
    inbox: Arc<Inbox>,
    wake: mpsc::SyncSender<()>,
}
impl Sink {
    /// Move the complete ready buffer and wake once. No fusion,
    /// native query, capacity wait, or dependency on another source occurs here.
    pub(crate) fn submit(&self, source: TargetSource, targets: Vec<UiTarget>) -> usize {
        if targets.is_empty() || self.inbox.cancelled.load(Ordering::Acquire) {
            return 0;
        }
        let accepted = targets.len();
        if self.enqueue(source, Input::Targets(targets.into_iter())) {
            accepted
        } else {
            0
        }
    }
    pub(crate) fn progress(&self, source: TargetSource, progress: super::scan_progress::Progress) {
        if self.inbox.progress_enabled {
            self.enqueue(source, Input::Progress(progress));
        }
    }
    fn enqueue(&self, source: TargetSource, input: Input) -> bool {
        let mut state = self.inbox.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finished || self.inbox.cancelled.load(Ordering::Acquire) {
            return false;
        }
        let index = state
            .lanes
            .iter()
            .position(|lane| lane.source == source)
            .unwrap_or_else(|| {
                state.lanes.push(Lane {
                    source,
                    batches: VecDeque::new(),
                });
                state.lanes.len() - 1
            });
        if state.lanes[index].batches.is_empty() {
            state.ready.push_back(index);
        }
        state.lanes[index].batches.push_back(input);
        #[cfg(test)]
        {
            state.version += 1;
        }
        drop(state);
        let _ = self.wake.try_send(());
        true
    }
    pub(crate) fn finish(&self, status: UiScanStatus) {
        let mut state = self.inbox.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finished || self.inbox.cancelled.load(Ordering::Acquire) {
            return;
        }
        state.finished = true;
        state.terminal = Some(status);
        #[cfg(test)]
        {
            state.version += 1;
        }
        drop(state);
        let _ = self.wake.try_send(());
    }
    #[cfg(test)]
    pub(crate) fn flush(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut state = self.inbox.state.lock().unwrap_or_else(|e| e.into_inner());
        let version = state.version;
        while state.settled < version && !self.inbox.cancelled.load(Ordering::Acquire) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "fusion failed to settle submitted data"
            );
            state = self
                .inbox
                .settled
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}
enum Step {
    Targets(TargetSource, Vec<UiTarget>),
    Progress(TargetSource, super::scan_progress::Progress),
    Terminal(UiScanStatus),
    Idle,
}
impl Inbox {
    fn take(&self, limit: usize) -> Step {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(state.terminal, Some(UiScanStatus::ContextChanged)) {
            state.terminal = None;
            return Step::Terminal(UiScanStatus::ContextChanged);
        }
        if let Some(index) = state.ready.pop_front() {
            let lane = &mut state.lanes[index];
            let source = lane.source;
            let Some(input) = lane.batches.pop_front() else {
                unreachable!("ready lane has input")
            };
            let mut targets = match input {
                Input::Targets(mut front) => {
                    if front.len() <= limit {
                        // Consuming a complete provider buffer can transfer its
                        // allocation directly to fusion instead of moving every
                        // target through another temporary Vec.
                        front.collect()
                    } else {
                        let targets: Vec<_> = front.by_ref().take(limit).collect();
                        lane.batches.push_front(Input::Targets(front));
                        targets
                    }
                }
                Input::Progress(progress) => {
                    if !lane.batches.is_empty() {
                        state.ready.push_back(index);
                    }
                    return Step::Progress(source, progress);
                }
            };
            if targets.len() < limit && matches!(lane.batches.front(), Some(Input::Targets(_))) {
                targets.reserve(limit - targets.len());
            }
            while targets.len() < limit {
                let Some(Input::Targets(front)) = lane.batches.front_mut() else {
                    break;
                };
                targets.extend(front.by_ref().take(limit - targets.len()));
                if front.len() == 0 {
                    lane.batches.pop_front();
                }
            }
            if !lane.batches.is_empty() {
                state.ready.push_back(index);
            }
            Step::Targets(source, targets)
        } else if let Some(status) = state.terminal.take() {
            Step::Terminal(status)
        } else {
            Step::Idle
        }
    }
    fn settle(&self) {
        #[cfg(test)]
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            // A submission may race the consumer's earlier Idle decision.
            // Never acknowledge newly queued work before it has been processed.
            if self.cancelled.load(Ordering::Acquire)
                || state.ready.is_empty() && state.terminal.is_none()
            {
                state.settled = state.version;
                self.settled.notify_all();
            }
        }
    }
    fn discard(&self) {
        let old = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.finished = true;
            state.ready.clear();
            std::mem::take(&mut state.lanes)
        };
        drop(old);
        self.cancelled.store(true, Ordering::Release);
        self.settle();
    }
}
struct Job {
    inbox: Arc<Inbox>,
    options: Options,
}
#[derive(Default)]
struct QueueState {
    generation: u64,
    stopping: bool,
    pending: Option<Job>,
    active: Option<Weak<Inbox>>,
}
struct Shared {
    maximum: usize,
    state: Mutex<QueueState>,
    wake: mpsc::SyncSender<()>,
}
#[derive(Clone)]
pub(crate) struct Handle {
    shared: Arc<Shared>,
    generation: u64,
}
impl Handle {
    pub(crate) fn begin(&self, options: Options) -> Sink {
        let inbox = Arc::new(Inbox {
            id: options.id,
            progress_enabled: options.defer_visual && !options.progress_sources.is_empty(),

            cancelled: AtomicBool::new(false),
            state: Mutex::new(InboxState::default()),
            #[cfg(test)]
            settled: std::sync::Condvar::new(),
        });
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        let replaced = if state.stopping
            || state.generation != self.generation
            || options.generation != self.generation
        {
            inbox.cancelled.store(true, Ordering::Release);
            None
        } else {
            if let Some(active) = state.active.as_ref().and_then(Weak::upgrade) {
                active.cancelled.store(true, Ordering::Release);
            }
            state.active = Some(Arc::downgrade(&inbox));
            state.pending.replace(Job {
                inbox: Arc::clone(&inbox),
                options,
            })
        };
        drop(state);
        if let Some(old) = replaced {
            old.inbox.discard();
        }
        let _ = self.shared.wake.try_send(());
        Sink {
            inbox,
            wake: self.shared.wake.clone(),
        }
    }
}
/// One lazy, reusable consumer owned and stopped by the native backend.
pub(crate) struct Worker {
    shared: Arc<Shared>,
    receiver: Option<mpsc::Receiver<()>>,
    thread: Option<WorkerJoin>,
    setup: Wake,
}
impl Worker {
    pub(crate) fn with_setup(setup: impl Fn() + Send + Sync + 'static) -> Self {
        let (wake, receiver) = mpsc::sync_channel(1);
        Self {
            shared: Arc::new(Shared {
                maximum: MAX_FUSION_BATCH,
                state: Mutex::new(QueueState::default()),
                wake,
            }),
            receiver: Some(receiver),
            thread: None,
            setup: Arc::new(setup),
        }
    }
    pub(crate) fn prepare(&mut self, generation: u64) -> Result<Handle, String> {
        if self.thread.is_none() {
            let receiver = self
                .receiver
                .take()
                .ok_or("fusion worker was already stopped")?;
            let shared = Arc::clone(&self.shared);
            let setup = Arc::clone(&self.setup);
            self.thread = Some(WorkerJoin::spawn(
                "UI scan fusion",
                std::thread::Builder::new().name("keysteer-scan-fusion".into()),
                move || {
                    setup();
                    run(shared, receiver);
                },
            )?);
        }
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopping {
            return Err("UI scan fusion worker is stopping".into());
        }
        state.generation = generation;
        if let Some(active) = state.active.as_ref().and_then(Weak::upgrade) {
            active.cancelled.store(true, Ordering::Release);
        }
        let pending = state.pending.take();
        drop(state);
        if let Some(old) = pending {
            old.inbox.discard();
        }
        let _ = self.shared.wake.try_send(());
        Ok(Handle {
            shared: Arc::clone(&self.shared),
            generation,
        })
    }
    pub(crate) fn cancel(&self, id: u64) {
        let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(active) = state
            .active
            .as_ref()
            .and_then(Weak::upgrade)
            .filter(|i| i.id == id)
        {
            active.cancelled.store(true, Ordering::Release);
        }
        drop(state);
        let _ = self.shared.wake.try_send(());
    }
    pub(crate) fn request_stop(&self) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stopping = true;
        if let Some(active) = state.active.as_ref().and_then(Weak::upgrade) {
            active.cancelled.store(true, Ordering::Release);
        }
        drop(state);
        let _ = self.shared.wake.try_send(());
    }
    pub(crate) fn stop_until(&mut self, deadline: Instant) -> Result<(), String> {
        self.request_stop();
        if let Some(worker) = &mut self.thread {
            worker.join_until(deadline)?;
        }
        self.thread.take();
        Ok(())
    }
}
impl Default for Worker {
    fn default() -> Self {
        Self::with_setup(|| {})
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if self
            .thread
            .as_ref()
            .is_some_and(WorkerJoin::shutdown_failure_was_returned)
        {
            return;
        }
        if let Err(error) = self.stop_until(Instant::now() + STOP_TIMEOUT) {
            crate::report_error!("ui-scan-fusion", "{error}");
        }
    }
}
struct Budget {
    next: usize,
    maximum: usize,
}
impl Budget {
    fn new(maximum: usize) -> Self {
        Self {
            maximum,
            next: FIRST_FUSION_BATCH.min(maximum),
        }
    }
    fn observe(&mut self, count: usize, elapsed: Duration) {
        if count == 0 {
            return;
        }
        // This is a soft CPU budget, not a timer: never wait to fill a quantum.
        let target = count as u128 * FUSION_CPU_TARGET.as_nanos() / elapsed.as_nanos().max(1);
        self.next = (target as usize).clamp(8, self.maximum);
    }
}
fn run(shared: Arc<Shared>, receiver: mpsc::Receiver<()>) {
    let mut current: Option<(Job, ScanAccumulator, Budget)> = None;
    loop {
        let (pending, stopping) = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            (state.pending.take(), state.stopping)
        };
        if let Some(job) = pending {
            if let Some((old, _, _)) = current.take() {
                old.inbox.discard();
            }
            let accumulator = ScanAccumulator::new()
                .defer_visual(job.options.defer_visual)
                .progress_sources(&job.options.progress_sources);
            current = Some((job, accumulator, Budget::new(shared.maximum)));
        }
        if stopping {
            if let Some((job, _, _)) = current.take() {
                job.inbox.discard();
            }
            return;
        }
        if let Some((job, accumulator, budget)) = &mut current {
            if job.inbox.cancelled.load(Ordering::Acquire) {
                job.inbox.discard();
                current = None;
                continue;
            }
            match job.inbox.take(budget.next) {
                Step::Progress(source, progress) => {
                    let targets = accumulator.progress(source, progress);
                    if job.inbox.cancelled.load(Ordering::Acquire) {
                        continue;
                    }
                    if targets.is_empty() {
                        continue;
                    }
                    if !(job.options.current)() {
                        if job.options.output.publish(
                            job.options.generation,
                            job.options.id,
                            Vec::new(),
                            UiScanStatus::ContextChanged,
                        ) {
                            (job.options.wake)();
                        }
                        job.inbox.discard();
                        current = None;
                        continue;
                    }
                    if !targets.is_empty()
                        && job.options.output.publish(
                            job.options.generation,
                            job.options.id,
                            targets,
                            UiScanStatus::Partial,
                        )
                    {
                        (job.options.wake)();
                    }
                    continue;
                }
                Step::Targets(source, mut targets) => {
                    let started = Instant::now();
                    let count = targets.len();
                    targets.retain(|t| (job.options.accepts)(t));
                    let update = accumulator.push(source, targets, job.options.threshold);
                    if source != TargetSource::Accessibility && update.accepted != 0 {
                        crate::support::perf_probe::mark("vision_targets_accepted");
                    }
                    // Adapt to fusion CPU cost, not a slow native context query
                    // or a busy output mailbox. Otherwise slower publication
                    // would shrink quanta and multiply those same native calls.
                    budget.observe(count, started.elapsed());
                    if job.inbox.cancelled.load(Ordering::Acquire) {
                        continue;
                    }
                    if !(job.options.current)() {
                        if job.options.output.publish(
                            job.options.generation,
                            job.options.id,
                            Vec::new(),
                            UiScanStatus::ContextChanged,
                        ) {
                            (job.options.wake)();
                        }
                        job.inbox.discard();
                        current = None;
                        continue;
                    }
                    if (!update.targets.is_empty() || !update.retired.is_empty())
                        && job.options.output.publish_update(
                            job.options.generation,
                            job.options.id,
                            update.targets,
                            update.retired,
                            UiScanStatus::Partial,
                        )
                    {
                        (job.options.wake)();
                    }
                    continue;
                }
                Step::Terminal(status) => {
                    let status = if (job.options.current)() {
                        status
                    } else {
                        UiScanStatus::ContextChanged
                    };
                    let targets = if status == UiScanStatus::ContextChanged {
                        Vec::new()
                    } else {
                        accumulator.take_deferred()
                    };
                    if job.inbox.cancelled.load(Ordering::Acquire) {
                        continue;
                    }
                    if job.options.output.publish(
                        job.options.generation,
                        job.options.id,
                        targets,
                        status,
                    ) {
                        (job.options.wake)();
                    }
                    job.inbox.discard();
                    // Release scan evidence with its task, immediately after
                    // publication; do not rebuild an empty accumulator first.
                    current = None;
                    continue;
                }
                Step::Idle => job.inbox.settle(),
            }
        }
        if receiver.recv().is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Rect, SemanticRole};
    use std::sync::atomic::AtomicUsize;

    fn targets(start: usize, count: usize) -> Vec<UiTarget> {
        (start..start + count)
            .map(|i| UiTarget {
                rect: Rect::new((i % 100 * 80) as f64, (i / 100 * 40) as f64, 20.0, 20.0),
                name: i.to_string(),
                role: SemanticRole::Button,
                details: None,
            })
            .collect()
    }
    fn options(output: &Arc<ScanMailbox>, id: u64, generation: u64) -> Options {
        Options {
            id,
            generation,
            threshold: 0.5,
            defer_visual: false,
            progress_sources: Vec::new(),
            output: Arc::clone(output),
            wake: Arc::new(|| {}),
            accepts: Arc::new(|_| true),
            current: Arc::new(|| true),
        }
    }
    fn detached() -> (Sink, mpsc::Receiver<()>) {
        let (wake, receiver) = mpsc::sync_channel(1);
        (
            Sink {
                inbox: Arc::new(Inbox {
                    id: 1,
                    progress_enabled: true,
                    cancelled: AtomicBool::new(false),
                    state: Mutex::new(InboxState::default()),
                    settled: std::sync::Condvar::new(),
                }),
                wake,
            },
            receiver,
        )
    }
    #[test]
    fn source_completion_preserves_success_and_prioritizes_cancellation_on_both_backends() {
        for (first, second, expected) in [
            (
                UiScanStatus::Success,
                UiScanStatus::Failed("OCR failed".into()),
                UiScanStatus::Success,
            ),
            (
                UiScanStatus::Success,
                UiScanStatus::TimedOut,
                UiScanStatus::Success,
            ),
            (
                UiScanStatus::Success,
                UiScanStatus::ContextChanged,
                UiScanStatus::ContextChanged,
            ),
            (
                UiScanStatus::TimedOut,
                UiScanStatus::Unsupported("unavailable".into()),
                UiScanStatus::TimedOut,
            ),
        ] {
            assert_eq!(combined_status(&[first.clone(), second.clone()]), expected);
            assert_eq!(combined_status(&[second, first]), expected);
        }
    }

    #[test]
    fn idle_acknowledgment_does_not_overtake_a_racing_submission() {
        let (sink, _receiver) = detached();
        assert!(matches!(sink.inbox.take(16), Step::Idle));
        sink.submit(TargetSource::Accessibility, targets(0, 1));
        sink.inbox.settle();
        let state = sink.inbox.state.lock().unwrap();
        assert_eq!(state.settled, 0);
        assert_eq!(state.version, 1);
    }

    #[test]
    fn region_completion_cannot_overtake_queued_targets() {
        use super::super::scan_progress::Progress;
        let (sink, _) = detached();
        let region = Rect::new(0.0, 0.0, 1000.0, 1000.0);
        sink.progress(TargetSource::SystemOcr, Progress::Plan(vec![region]));
        sink.submit(TargetSource::SystemOcr, targets(0, 259));
        sink.progress(TargetSource::SystemOcr, Progress::Complete(region));
        sink.submit(TargetSource::SystemOcr, targets(300, 1));
        assert!(matches!(
            sink.inbox.take(128),
            Step::Progress(_, Progress::Plan(_))
        ));
        for count in [128, 128, 3] {
            let Step::Targets(_, batch) = sink.inbox.take(128) else {
                panic!("completion overtook results")
            };
            assert_eq!(batch.len(), count);
        }
        assert!(matches!(
            sink.inbox.take(128),
            Step::Progress(_, Progress::Complete(_))
        ));
        let Step::Targets(_, batch) = sink.inbox.take(128) else {
            panic!("missing tail")
        };
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn complete_provider_buffers_move_into_fusion_and_keep_batch_boundaries() {
        let (sink, _) = detached();
        for count in [1, 16, 128] {
            let input = targets(0, count);
            let storage = input.as_ptr();
            sink.submit(TargetSource::Accessibility, input);
            let Step::Targets(TargetSource::Accessibility, batch) = sink.inbox.take(128) else {
                panic!("missing provider buffer")
            };
            assert_eq!(batch.as_ptr(), storage);
            assert_eq!(batch, targets(0, count));
        }
        sink.submit(TargetSource::Accessibility, targets(0, 4));
        sink.submit(TargetSource::Accessibility, targets(4, 20));
        sink.submit(TargetSource::SystemOcr, targets(99, 1));
        for (source, start, count) in [
            (TargetSource::Accessibility, 0, 16),
            (TargetSource::SystemOcr, 99, 1),
            (TargetSource::Accessibility, 16, 8),
        ] {
            let Step::Targets(actual_source, batch) = sink.inbox.take(16) else {
                panic!("missing bounded batch")
            };
            assert_eq!(actual_source, source);
            assert_eq!(batch, targets(start, count));
        }
        assert!(matches!(sink.inbox.take(16), Step::Idle));
    }

    #[test]
    fn completed_region_streams_before_terminal_and_stale_progress_is_discarded() {
        use super::super::scan_progress::Progress;
        let output = Arc::new(ScanMailbox::default());
        let generation = output.begin(1);
        let mut worker = Worker::default();
        let mut config = options(&output, 1, generation);
        config.defer_visual = true;
        config.progress_sources = vec![TargetSource::Contour, TargetSource::SystemOcr];
        let sink = worker.prepare(generation).unwrap().begin(config);
        let left = Rect::new(-100.0, -100.0, 200.0, 200.0);
        let right = Rect::new(500.0, -100.0, 200.0, 200.0);
        sink.progress(TargetSource::Contour, Progress::Plan(vec![left]));
        sink.progress(TargetSource::SystemOcr, Progress::Plan(vec![left, right]));
        sink.submit(TargetSource::Contour, targets(0, 1));
        sink.progress(TargetSource::Contour, Progress::Complete(left));
        sink.flush();
        assert!(output.take().is_none());
        sink.progress(TargetSource::SystemOcr, Progress::Complete(left));
        sink.flush();
        let result = output.take().unwrap();
        assert_eq!(result.status, UiScanStatus::Partial);
        assert_eq!(result.targets.len(), 1);
        worker.cancel(1);
        let generation = output.begin(2);
        let latest = worker
            .prepare(generation)
            .unwrap()
            .begin(options(&output, 2, generation));
        sink.progress(TargetSource::SystemOcr, Progress::Complete(right));
        sink.finish(UiScanStatus::Success);
        latest.submit(TargetSource::Accessibility, targets(2, 1));
        latest.finish(UiScanStatus::Success);
        latest.flush();
        let result = output.take().unwrap();
        assert_eq!(result.id, 2);
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].name, "2");
    }

    #[test]
    fn ready_tail_does_not_wait_for_another_source_or_completion() {
        let output = Arc::new(ScanMailbox::default());
        let generation = output.begin(1);
        let mut worker = Worker::default();
        let sink = worker
            .prepare(generation)
            .unwrap()
            .begin(options(&output, 1, generation));
        sink.submit(TargetSource::Accessibility, targets(0, 1));
        sink.flush();
        assert_eq!(output.take().unwrap().targets.len(), 1);
        sink.submit(TargetSource::SystemOcr, Vec::new());
        sink.submit(TargetSource::Accessibility, targets(1, 3));
        sink.flush();
        let ready = output.take().unwrap();
        assert_eq!(ready.targets.len(), 3);
        assert_eq!(ready.status, UiScanStatus::Partial);
        sink.finish(UiScanStatus::Success);
        sink.flush();
        let terminal = output.take().unwrap();
        assert_eq!(terminal.status, UiScanStatus::Success);
        assert!(terminal.targets.is_empty());
        assert!(output.take().is_none());
    }

    #[test]
    fn semantic_preview_streams_while_regions_are_pending_and_full_results_stay_complete() {
        use super::super::scan_progress::Progress;

        let output = Arc::new(ScanMailbox::default());
        let generation = output.begin(1);
        let mut worker = Worker::default();
        let mut config = options(&output, 1, generation);
        config.defer_visual = true;
        config.progress_sources = vec![
            TargetSource::Accessibility,
            TargetSource::Contour,
            TargetSource::SystemOcr,
        ];
        let sink = worker.prepare(generation).unwrap().begin(config);
        let region = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        for source in [
            TargetSource::Accessibility,
            TargetSource::Contour,
            TargetSource::SystemOcr,
        ] {
            sink.progress(source, Progress::Plan(vec![region]));
        }
        // This shared path has no window/screen or platform-specific policy.
        // The overlapping fragment stays hidden even before native work starts.
        let fragment = |start| {
            targets(start, 1)
                .into_iter()
                .map(|mut target| {
                    target.role = SemanticRole::Control;
                    target.name.clear();
                    target
                })
                .collect()
        };
        sink.submit(TargetSource::Contour, fragment(0));
        sink.submit(TargetSource::Contour, fragment(2));
        sink.progress(TargetSource::Contour, Progress::Complete(region));
        sink.flush();
        assert!(output.take().is_none());

        sink.submit(TargetSource::Accessibility, targets(0, 1));
        sink.flush();
        let preview = output.take().unwrap();
        assert_eq!(preview.status, UiScanStatus::Partial);
        assert_eq!(preview.targets, targets(0, 1));
        assert!(preview.retired.is_empty());

        sink.submit(TargetSource::Accessibility, targets(0, 2));
        sink.progress(TargetSource::Accessibility, Progress::Complete(region));
        sink.flush();
        let bulk = output.take().unwrap();
        assert_eq!(bulk.status, UiScanStatus::Partial);
        assert_eq!(bulk.targets, targets(1, 1));
        assert!(bulk.retired.is_empty());

        sink.progress(TargetSource::SystemOcr, Progress::Complete(region));
        sink.flush();
        let visual = output.take().unwrap();
        assert_eq!(visual.status, UiScanStatus::Partial);
        let mut expected_visual = fragment(2);
        expected_visual[0].details = Some(Default::default());
        assert_eq!(visual.targets, expected_visual);
        assert!(visual.retired.is_empty());

        sink.finish(UiScanStatus::Success);
        sink.flush();
        let terminal = output.take().unwrap();
        assert_eq!(terminal.status, UiScanStatus::Success);
        assert!(terminal.targets.is_empty());
        assert!(terminal.retired.is_empty());
    }
    #[test]
    fn deferred_candidates_flush_on_completion_but_never_after_cancellation() {
        for status in [
            UiScanStatus::Success,
            UiScanStatus::TimedOut,
            UiScanStatus::Failed("OCR failed".into()),
            UiScanStatus::ContextChanged,
        ] {
            let output = Arc::new(ScanMailbox::default());
            let generation = output.begin(1);
            let mut worker = Worker::default();
            let mut config = options(&output, 1, generation);
            config.defer_visual = true;
            let sink = worker.prepare(generation).unwrap().begin(config);
            sink.submit(TargetSource::Contour, targets(0, 4));
            sink.flush();
            assert!(output.take().is_none());
            sink.finish(status.clone());
            sink.flush();
            let result = output.take().unwrap();
            assert_eq!(result.status, status);
            assert_eq!(
                result.targets.len(),
                if status == UiScanStatus::ContextChanged {
                    0
                } else {
                    4
                }
            );
            assert!(result.retired.is_empty());
        }

        let output = Arc::new(ScanMailbox::default());
        let generation = output.begin(1);
        let mut worker = Worker::default();
        let mut config = options(&output, 1, generation);
        config.defer_visual = true;
        let old = worker.prepare(generation).unwrap().begin(config);
        old.submit(TargetSource::Contour, targets(0, 4));
        old.flush();
        worker.cancel(1);
        let generation = output.begin(2);
        let latest = worker
            .prepare(generation)
            .unwrap()
            .begin(options(&output, 2, generation));
        latest.submit(TargetSource::Contour, targets(4, 1));
        latest.flush();
        assert_eq!(
            output.take().unwrap().targets.len(),
            1,
            "contour-only remains immediate"
        );
        old.finish(UiScanStatus::Success);
        latest.finish(UiScanStatus::Success);
        latest.flush();
        let result = output.take().unwrap();
        assert_eq!(result.id, 2);
        assert!(result.targets.is_empty());
    }
    #[test]
    fn empty_and_large_submissions_do_not_starve_other_sources() {
        let (sink, receiver) = detached();
        assert_eq!(sink.submit(TargetSource::Contour, Vec::new()), 0);
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            sink.submit(TargetSource::Accessibility, targets(0, 10_025)),
            10_025
        );
        assert_eq!(sink.submit(TargetSource::Accessibility, targets(0, 1)), 1);
        assert_eq!(sink.submit(TargetSource::SystemOcr, targets(10_025, 1)), 1);
        let Step::Targets(source, first) = sink.inbox.take(MAX_FUSION_BATCH) else {
            panic!("missing first quantum")
        };
        assert_eq!(source, TargetSource::Accessibility);
        assert_eq!(first.len(), MAX_FUSION_BATCH);
        let Step::Targets(source, second) = sink.inbox.take(MAX_FUSION_BATCH) else {
            panic!("small source was starved")
        };
        assert_eq!(source, TargetSource::SystemOcr);
        assert_eq!(second.len(), 1);
        assert_eq!(sink.inbox.state.lock().unwrap().lanes.len(), 2);
    }
    #[test]
    fn producer_returns_while_fusion_is_busy() {
        let output = Arc::new(ScanMailbox::default());
        let generation = output.begin(1);
        let mut worker = Worker::default();
        let (entered, waiting) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let blocked = Mutex::new(blocked);
        let first = AtomicBool::new(true);
        let mut config = options(&output, 1, generation);
        config.accepts = Arc::new(move |_| {
            if first.swap(false, Ordering::AcqRel) {
                entered.send(()).unwrap();
                blocked
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            true
        });
        let sink = worker.prepare(generation).unwrap().begin(config);
        sink.submit(TargetSource::Accessibility, targets(0, 1));
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let producer = sink.clone();
        let (submitted, done) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            submitted
                .send(producer.submit(TargetSource::SystemOcr, targets(1, 512)))
                .unwrap()
        });
        let queued = done.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        assert_eq!(
            queued.unwrap(),
            512,
            "producer must not acquire the fusion lock"
        );
        thread.join().unwrap();
        sink.finish(UiScanStatus::Success);
        sink.flush();
        assert_eq!(output.take().unwrap().targets.len(), 513);
    }
    #[test]
    fn generations_cancel_stale_sources_and_reuse_one_worker() {
        let setups = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&setups);
        let mut worker = Worker::with_setup(move || {
            seen.fetch_add(1, Ordering::AcqRel);
        });
        let output = Arc::new(ScanMailbox::default());
        let first_generation = output.begin(1);
        let old_handle = worker.prepare(first_generation).unwrap();
        let first = old_handle.begin(options(&output, 1, first_generation));
        first.submit(TargetSource::Accessibility, targets(0, 1));
        first.flush();
        output.take().unwrap();
        let generation = output.begin(2);
        let handle = worker.prepare(generation).unwrap();
        assert_eq!(
            first.submit(TargetSource::Accessibility, targets(0, 512)),
            0
        );
        let stale = old_handle.begin(options(&output, 1, first_generation));
        assert_eq!(stale.submit(TargetSource::Contour, targets(0, 1)), 0);
        let second = handle.begin(options(&output, 2, generation));
        second.submit(TargetSource::Accessibility, targets(1, 1));
        second.finish(UiScanStatus::Success);
        second.flush();
        let result = output.take().unwrap();
        assert_eq!(result.id, 2);
        assert_eq!(result.targets.len(), 1);
        assert_eq!(setups.load(Ordering::Acquire), 1);
        worker
            .stop_until(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(worker.prepare(generation + 1).is_err());
    }
    #[test]
    fn replacing_generation_while_fusing_drops_old_results_and_runs_latest() {
        let output = Arc::new(ScanMailbox::default());
        let mut worker = Worker::default();
        let generation = output.begin(1);
        let (entered, waiting) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let blocked = Mutex::new(blocked);
        let mut config = options(&output, 1, generation);
        config.accepts = Arc::new(move |_| {
            entered.send(()).unwrap();
            blocked
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            true
        });
        let old = worker.prepare(generation).unwrap().begin(config);
        old.submit(TargetSource::Accessibility, targets(0, 1));
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let generation = output.begin(2);
        let latest = worker
            .prepare(generation)
            .unwrap()
            .begin(options(&output, 2, generation));
        latest.submit(TargetSource::Accessibility, targets(1, 1));
        latest.finish(UiScanStatus::Success);
        assert_eq!(old.submit(TargetSource::Accessibility, targets(2, 1)), 0);
        release.send(()).unwrap();
        latest.flush();
        let result = output.take().unwrap();
        assert_eq!(result.id, 2);
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].name, "1");
        old.finish(UiScanStatus::Failed("stale".into()));
        assert!(output.take().is_none());
    }

    #[test]
    fn context_change_preempts_queued_data_and_success_terminal() {
        let (sink, _receiver) = detached();
        sink.submit(TargetSource::Accessibility, targets(0, 512));
        sink.finish(UiScanStatus::ContextChanged);
        assert!(matches!(
            sink.inbox.take(16),
            Step::Terminal(UiScanStatus::ContextChanged)
        ));
        let output = Arc::new(ScanMailbox::default());
        let generation = output.begin(1);
        let mut worker = Worker::default();
        let mut config = options(&output, 1, generation);
        config.current = Arc::new(|| false);
        let sink = worker.prepare(generation).unwrap().begin(config);
        sink.submit(TargetSource::Accessibility, targets(0, 512));
        sink.finish(UiScanStatus::Success);
        sink.flush();
        // Stop joins the consumer even when flush observes cancellation early.
        worker
            .stop_until(Instant::now() + Duration::from_secs(5))
            .unwrap();
        let result = output.take().unwrap();
        assert_eq!(result.status, UiScanStatus::ContextChanged);
        assert!(result.targets.is_empty());
    }
    #[test]
    fn quantum_adapts_to_cpu_cost_and_never_to_inline_label_capacity() {
        let mut budget = Budget::new(MAX_FUSION_BATCH);
        assert_eq!(budget.next, 16);
        budget.observe(16, Duration::from_micros(10));
        assert_eq!(budget.next, MAX_FUSION_BATCH);
        budget.observe(MAX_FUSION_BATCH, Duration::from_millis(2));
        assert_eq!(budget.next, 32);
        budget.observe(32, Duration::from_millis(8));
        assert_eq!(budget.next, 8);
    }
}
