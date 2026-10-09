//! Generation-scoped submission, fair single-writer fusion and ready deltas.
#![forbid(unsafe_code)]
use super::{
    scan_accumulator::ScanAccumulator, scan_mailbox::ScanMailbox, spatial_index::TargetSource,
};
use crate::api::{
    UiTarget,
    command::{MAX_UI_SCAN_TARGETS, UiScanStatus},
};
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
type Wake = Arc<dyn Fn() + Send + Sync>;
type Accept = Arc<dyn Fn(&UiTarget) -> bool + Send + Sync>;
type Current = Arc<dyn Fn() -> bool + Send + Sync>;

pub(crate) struct Options {
    pub(crate) id: u64,
    pub(crate) generation: u64,
    pub(crate) threshold: f64,
    pub(crate) output: Arc<ScanMailbox>,
    pub(crate) wake: Wake,
    pub(crate) accepts: Accept,
    pub(crate) current: Current,
}
struct Lane {
    source: TargetSource,
    batches: VecDeque<std::vec::IntoIter<UiTarget>>,
    submitted: usize,
}
#[derive(Default)]
struct InboxState {
    lanes: SmallVec<[Lane; 4]>,
    ready: VecDeque<usize>,
    terminal: Option<UiScanStatus>,
    finished: bool,
    version: u64,
    #[cfg(test)]
    settled: u64,
}
struct Inbox {
    id: u64,
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
    /// Reserve bounded storage, move the ready buffer, and wake once. No fusion,
    /// native query, capacity wait, or dependency on another source occurs here.
    pub(crate) fn submit(&self, source: TargetSource, mut targets: Vec<UiTarget>) -> usize {
        if targets.is_empty() || self.inbox.cancelled.load(Ordering::Acquire) {
            return 0;
        }
        let (index, accepted) = {
            let mut state = self.inbox.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.finished || self.inbox.cancelled.load(Ordering::Acquire) {
                return 0;
            }
            let index = state
                .lanes
                .iter()
                .position(|l| l.source == source)
                .unwrap_or_else(|| {
                    state.lanes.push(Lane {
                        source,
                        batches: VecDeque::new(),
                        submitted: 0,
                    });
                    state.lanes.len() - 1
                });
            let lane = &mut state.lanes[index];
            let accepted = targets
                .len()
                .min(MAX_UI_SCAN_TARGETS.saturating_sub(lane.submitted));
            lane.submitted += accepted;
            (index, accepted)
        };
        // Drop an oversized provider's excess outside the shared submission lock.
        targets.truncate(accepted);
        if accepted == 0 {
            return 0;
        }
        {
            let mut state = self.inbox.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.finished || self.inbox.cancelled.load(Ordering::Acquire) {
                return 0;
            }
            if state.lanes[index].batches.is_empty() {
                state.ready.push_back(index);
            }
            state.lanes[index].batches.push_back(targets.into_iter());
            state.version += 1;
        }
        let _ = self.wake.try_send(());
        accepted
    }
    pub(crate) fn finish(&self, status: UiScanStatus) {
        let mut state = self.inbox.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finished || self.inbox.cancelled.load(Ordering::Acquire) {
            return;
        }
        state.finished = true;
        state.terminal = Some(status);
        state.version += 1;
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
            let mut targets = Vec::with_capacity(limit);
            while targets.len() < limit {
                let Some(front) = lane.batches.front_mut() else {
                    break;
                };
                targets.extend(front.by_ref().take(limit - targets.len()));
                if front.len() == 0 {
                    lane.batches.pop_front();
                }
            }
            let source = lane.source;
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
    #[cfg(feature = "benchmark-hooks")]
    pub(crate) fn benchmark(maximum: usize) -> Self {
        let mut worker = Self::default();
        Arc::get_mut(&mut worker.shared)
            .unwrap_or_else(|| panic!("fresh benchmark worker must be exclusively owned"))
            .maximum = maximum.clamp(8, 512);
        worker
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
            current = Some((job, ScanAccumulator::new(), Budget::new(shared.maximum)));
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
                Step::Targets(source, mut targets) => {
                    let started = Instant::now();
                    let count = targets.len();
                    targets.retain(|t| (job.options.accepts)(t));
                    let mut update = accumulator.push(source, targets, job.options.threshold);
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
                    for batch in update.batches {
                        if job.options.output.publish_update(
                            job.options.generation,
                            job.options.id,
                            batch,
                            std::mem::take(&mut update.retired),
                            UiScanStatus::Partial,
                        ) {
                            (job.options.wake)();
                        }
                    }
                    continue;
                }
                Step::Terminal(status) => {
                    let status = if (job.options.current)() {
                        status
                    } else {
                        UiScanStatus::ContextChanged
                    };
                    drop(accumulator.finish());
                    if job.options.output.publish(
                        job.options.generation,
                        job.options.id,
                        Vec::new(),
                        status,
                    ) {
                        (job.options.wake)();
                    }
                    job.inbox.discard();
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
    fn empty_and_oversized_submissions_do_not_starve_other_sources() {
        let (sink, receiver) = detached();
        assert_eq!(sink.submit(TargetSource::Contour, Vec::new()), 0);
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            sink.submit(
                TargetSource::Accessibility,
                targets(0, MAX_UI_SCAN_TARGETS + 5)
            ),
            MAX_UI_SCAN_TARGETS
        );
        assert_eq!(sink.submit(TargetSource::Accessibility, targets(0, 1)), 0);
        assert_eq!(
            sink.submit(TargetSource::SystemOcr, targets(MAX_UI_SCAN_TARGETS, 1)),
            1
        );
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
