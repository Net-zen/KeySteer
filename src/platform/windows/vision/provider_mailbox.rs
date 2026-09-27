//! Bounded provider result aggregation and publication.

use super::*;
use crate::platform::common::spatial_index::TargetSource;

pub(super) fn ocr_source(provider: &str) -> TargetSource {
    if provider == "wechat" {
        TargetSource::WechatOcr
    } else {
        TargetSource::SystemOcr
    }
}

pub(super) fn compare_ready(
    first_count: usize,
    first_elapsed: Duration,
    second_count: usize,
    second_elapsed: Duration,
) -> std::cmp::Ordering {
    second_count
        .cmp(&first_count)
        .then_with(|| first_elapsed.cmp(&second_elapsed))
}

pub(super) enum ProviderEvent {
    OcrBatch {
        provider: &'static str,
        elapsed: Duration,
        targets: Vec<UiTarget>,
    },
    OcrDone {
        provider: &'static str,
        elapsed: Duration,
        result: Result<usize, VisionError>,
    },
    ContourBatch(Vec<UiTarget>),
    ContourDone,
}

pub(super) type ProviderEvents = SmallVec<[ProviderEvent; 6]>;
pub(super) type ReadyOcrBatch = (&'static str, Duration, Vec<UiTarget>);
pub(super) type CompletedOcr = (&'static str, Duration, Result<usize, VisionError>);

const SYSTEM_READY: u8 = 1 << 0;
const WECHAT_READY: u8 = 1 << 1;
const CONTOUR_READY: u8 = 1 << 2;

/// Generation-owned provider mailbox with one fixed slot per provider.
///
/// A producer replaces an empty target vector or appends to its own bounded
/// slot. It never waits for coordinator capacity, and repeated batches merely
/// keep the same ready bit set. The coordinator takes ownership of each slot
/// when woken, so all valid targets are preserved without an event queue.
pub(super) struct ProviderMailbox {
    state: Mutex<ProviderMailboxState>,
    pub(super) ready_flags: AtomicU8,
    ready: Condvar,
}

#[derive(Default)]
struct ProviderMailboxState {
    system: OcrProviderSlot,
    wechat: OcrProviderSlot,
    contour: ContourProviderSlot,
    ready: u8,
    closed: bool,
}

#[derive(Default)]
struct OcrProviderSlot {
    targets: Vec<UiTarget>,
    target_elapsed: Option<Duration>,
    terminal: Option<(Duration, Result<usize, VisionError>)>,
    published_targets: usize,
    terminal_published: bool,
}

#[derive(Default)]
struct ContourProviderSlot {
    targets: Vec<UiTarget>,
    published_targets: usize,
    terminal_published: bool,
    terminal_ready: bool,
}

impl ProviderMailbox {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(ProviderMailboxState::default()),
            ready_flags: AtomicU8::new(0),
            ready: Condvar::new(),
        }
    }

    pub(super) fn publish(&self, event: ProviderEvent) -> Result<(), VisionError> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return Err(VisionError::Cancelled);
        }
        let was_empty = state.ready == 0;
        state.publish(event)?;
        self.ready_flags.store(state.ready, Ordering::Release);
        drop(state);
        if was_empty {
            self.ready.notify_one();
        }
        Ok(())
    }

    pub(super) fn wait_until_ready(&self, deadline: Instant) -> Result<(), VisionError> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            if state.ready != 0 {
                return Ok(());
            }
            if state.closed {
                return Err(VisionError::Cancelled);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(VisionError::TimedOut);
            }
            state = self
                .ready
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
    }

    pub(super) fn drain_into(&self, events: &mut ProviderEvents) {
        if self.ready_flags.load(Ordering::Acquire) == 0 {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.drain_into(events);
        self.ready_flags.store(state.ready, Ordering::Release);
    }

    pub(super) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        drop(state);
        self.ready.notify_all();
    }

    pub(super) fn discard(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.system = OcrProviderSlot::default();
        state.wechat = OcrProviderSlot::default();
        state.contour = ContourProviderSlot::default();
        state.ready = 0;
        self.ready_flags.store(0, Ordering::Release);
    }
}

impl ProviderMailboxState {
    pub(super) fn publish(&mut self, event: ProviderEvent) -> Result<(), VisionError> {
        match event {
            ProviderEvent::OcrBatch {
                provider,
                elapsed,
                targets,
            } => {
                let (slot, ready) = self.ocr_slot(provider)?;
                slot.append(provider, elapsed, targets)?;
                self.ready |= ready;
            }
            ProviderEvent::OcrDone {
                provider,
                elapsed,
                result,
            } => {
                let (slot, ready) = self.ocr_slot(provider)?;
                slot.finish(provider, elapsed, result)?;
                self.ready |= ready;
            }
            ProviderEvent::ContourBatch(targets) => {
                if targets.is_empty() || targets.len() > MAX_OCR_TARGETS {
                    return Err(VisionError::Operational(format!(
                        "contour published an invalid batch of {} targets",
                        targets.len()
                    )));
                }
                let next = self
                    .contour
                    .published_targets
                    .checked_add(targets.len())
                    .ok_or_else(|| {
                        VisionError::Operational("contour target count overflow".into())
                    })?;
                if next > MAX_OCR_TARGETS {
                    return Err(VisionError::Operational(format!(
                        "contour exceeded the {MAX_OCR_TARGETS}-target limit"
                    )));
                }
                self.contour.published_targets = next;
                if self.contour.targets.is_empty() {
                    self.contour.targets = targets;
                } else {
                    self.contour.targets.extend(targets);
                }
                self.ready |= CONTOUR_READY;
            }
            ProviderEvent::ContourDone => {
                if std::mem::replace(&mut self.contour.terminal_published, true) {
                    return Err(VisionError::Operational(
                        "contour published duplicate terminal results".into(),
                    ));
                }
                self.contour.terminal_ready = true;
                self.ready |= CONTOUR_READY;
            }
        }
        Ok(())
    }

    pub(super) fn ocr_slot(
        &mut self,
        provider: &'static str,
    ) -> Result<(&mut OcrProviderSlot, u8), VisionError> {
        let slot = match provider {
            "system" => (&mut self.system, SYSTEM_READY),
            "wechat" => (&mut self.wechat, WECHAT_READY),
            _ => {
                return Err(VisionError::Operational(format!(
                    "unknown OCR provider {provider}"
                )));
            }
        };
        Ok(slot)
    }

    pub(super) fn drain_into(&mut self, events: &mut ProviderEvents) {
        Self::drain_ocr_slot(
            &mut self.system,
            "system",
            SYSTEM_READY,
            &mut self.ready,
            events,
        );
        Self::drain_ocr_slot(
            &mut self.wechat,
            "wechat",
            WECHAT_READY,
            &mut self.ready,
            events,
        );
        if self.ready & CONTOUR_READY != 0 {
            if !self.contour.targets.is_empty() {
                events.push(ProviderEvent::ContourBatch(std::mem::take(
                    &mut self.contour.targets,
                )));
            }
            if self.contour.terminal_ready {
                self.contour.terminal_ready = false;
                events.push(ProviderEvent::ContourDone);
            }
            if self.contour.targets.is_empty() && !self.contour.terminal_ready {
                self.ready &= !CONTOUR_READY;
            }
        }
    }

    pub(super) fn drain_ocr_slot(
        slot: &mut OcrProviderSlot,
        provider: &'static str,
        ready_bit: u8,
        ready: &mut u8,
        events: &mut ProviderEvents,
    ) {
        if *ready & ready_bit == 0 {
            return;
        }
        if !slot.targets.is_empty() {
            events.push(ProviderEvent::OcrBatch {
                provider,
                elapsed: slot.target_elapsed.take().unwrap_or_default(),
                targets: std::mem::take(&mut slot.targets),
            });
        }
        if let Some((elapsed, result)) = slot.terminal.take() {
            events.push(ProviderEvent::OcrDone {
                provider,
                elapsed,
                result,
            });
        }
        if slot.targets.is_empty() && slot.terminal.is_none() {
            *ready &= !ready_bit;
        }
    }
}

impl OcrProviderSlot {
    pub(super) fn append(
        &mut self,
        provider: &'static str,
        elapsed: Duration,
        targets: Vec<UiTarget>,
    ) -> Result<(), VisionError> {
        if targets.is_empty() || targets.len() > PROVIDER_BATCH_SIZE {
            return Err(VisionError::Operational(format!(
                "{provider} OCR published an invalid batch of {} targets",
                targets.len()
            )));
        }
        let next = self
            .published_targets
            .checked_add(targets.len())
            .ok_or_else(|| {
                VisionError::Operational(format!("{provider} OCR target count overflow"))
            })?;
        if next > MAX_OCR_TARGETS {
            return Err(VisionError::Operational(format!(
                "{provider} OCR exceeded the {MAX_OCR_TARGETS}-target limit"
            )));
        }
        self.published_targets = next;
        if self.targets.is_empty() {
            self.targets = targets;
            self.target_elapsed = Some(elapsed);
        } else {
            self.targets.extend(targets);
        }
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        provider: &'static str,
        elapsed: Duration,
        result: Result<usize, VisionError>,
    ) -> Result<(), VisionError> {
        if std::mem::replace(&mut self.terminal_published, true) {
            return Err(VisionError::Operational(format!(
                "{provider} OCR published duplicate terminal results"
            )));
        }
        self.terminal = Some((elapsed, result));
        Ok(())
    }
}

pub(super) fn drain_early_ocr_events(
    mailbox: &ProviderMailbox,
    source: &ScanSource,
    deferred: &mut ProviderEvents,
    mut context_is_current: impl FnMut() -> bool,
) -> bool {
    let mut ready: SmallVec<[ReadyOcrBatch; 2]> = SmallVec::new();
    let mut events = ProviderEvents::new();
    mailbox.drain_into(&mut events);
    for event in events {
        match event {
            ProviderEvent::OcrBatch {
                provider,
                elapsed,
                targets,
            } => ready.push((provider, elapsed, targets)),
            ProviderEvent::ContourBatch(targets) => {
                if !context_is_current() {
                    return false;
                }
                source.push_from(TargetSource::Contour, targets);
            }
            event => deferred.push(event),
        }
    }
    ready.sort_by(|a, b| compare_ready(a.2.len(), a.1, b.2.len(), b.1));
    for (provider, _elapsed, targets) in ready {
        if !context_is_current() {
            return false;
        }
        let count = targets.len();
        let accepted = source.push_from(ocr_source(provider), targets);
        if accepted != 0 {
            crate::support::perf_probe::mark("vision_targets_accepted");
        }
        crate::log_info!(
            "windows-vision",
            "{provider} OCR streamed {count} valid targets ({accepted} new)"
        );
    }
    true
}

pub(super) fn send_ocr_batches(
    mailbox: &ProviderMailbox,
    provider: &'static str,
    started: Instant,
    targets: Vec<UiTarget>,
) -> Result<usize, VisionError> {
    let count = targets.len();
    if count > MAX_OCR_TARGETS {
        return Err(VisionError::Operational(format!(
            "{provider} OCR returned {count} targets; limit is {MAX_OCR_TARGETS}"
        )));
    }
    if count <= PROVIDER_BATCH_SIZE {
        if count != 0 {
            mailbox.publish(ProviderEvent::OcrBatch {
                provider,
                elapsed: started.elapsed(),
                targets,
            })?;
        }
        return Ok(count);
    }
    let mut batch = Vec::with_capacity(PROVIDER_BATCH_SIZE);
    for target in targets {
        batch.push(target);
        if batch.len() == PROVIDER_BATCH_SIZE {
            mailbox.publish(ProviderEvent::OcrBatch {
                provider,
                elapsed: started.elapsed(),
                targets: std::mem::replace(&mut batch, Vec::with_capacity(PROVIDER_BATCH_SIZE)),
            })?;
        }
    }
    if !batch.is_empty() {
        mailbox.publish(ProviderEvent::OcrBatch {
            provider,
            elapsed: started.elapsed(),
            targets: batch,
        })?;
    }
    Ok(count)
}

pub(super) fn send_contour_batches(
    mailbox: &ProviderMailbox,
    targets: Vec<UiTarget>,
) -> Result<(), VisionError> {
    if targets.len() > MAX_OCR_TARGETS {
        return Err(VisionError::Operational(format!(
            "contour returned {} targets; limit is {MAX_OCR_TARGETS}",
            targets.len()
        )));
    }
    // Contour completes before it has candidates to publish. Moving its Vec
    // once avoids splitting and rejoining hundreds of allocations at 10000
    // candidates. ScanSession still emits the normal count-driven Partials.
    if !targets.is_empty() {
        mailbox.publish(ProviderEvent::ContourBatch(targets))?;
    }
    mailbox.publish(ProviderEvent::ContourDone)
}
