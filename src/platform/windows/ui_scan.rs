#![forbid(unsafe_code)]
//! Provider completion and data submission; fusion runs on its own reusable worker.
use super::EventSender;
use super::accessibility::WindowsScanPlan;
use crate::api::command::UiScanStatus;
use crate::api::geometry::UiTarget;
#[cfg(test)]
use crate::api::geometry::{Rect, SemanticRole};
use crate::platform::common::scan_fusion::{Handle, Options, Sink};
use crate::platform::common::scan_mailbox::ScanMailbox;
use crate::platform::common::spatial_index::TargetSource;
#[cfg(test)]
use crate::platform::common::spatial_index::{SpatialIndex, rectangles_match};
use smallvec::SmallVec;
use std::sync::{Arc, Mutex};
#[cfg(test)]
const MAX_TARGETS: usize = crate::api::command::MAX_UI_SCAN_TARGETS;

pub(super) struct ScanSession {
    sink: Sink,
    state: Mutex<SessionState>,
}
struct SessionState {
    remaining: usize,
    statuses: SmallVec<[UiScanStatus; 2]>,
    finished: bool,
}
impl ScanSession {
    pub(super) fn new(
        plan: Arc<WindowsScanPlan>,
        generation: u64,
        sources: usize,
        mailbox: Arc<ScanMailbox>,
        wake: EventSender,
        fusion: &Handle,
    ) -> Arc<Self> {
        let current_plan = Arc::clone(&plan);
        Self::with_current(
            plan,
            generation,
            sources,
            mailbox,
            wake,
            fusion,
            Arc::new(move || current_plan.target_is_current()),
        )
    }
    #[cfg(test)]
    pub(super) fn test(
        plan: Arc<WindowsScanPlan>,
        generation: u64,
        sources: usize,
        mailbox: Arc<ScanMailbox>,
        wake: EventSender,
        fusion: &Handle,
    ) -> Arc<Self> {
        Self::with_current(
            plan,
            generation,
            sources,
            mailbox,
            wake,
            fusion,
            Arc::new(|| true),
        )
    }
    fn with_current(
        plan: Arc<WindowsScanPlan>,
        generation: u64,
        sources: usize,
        mailbox: Arc<ScanMailbox>,
        wake: EventSender,
        fusion: &Handle,
        current: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Arc<Self> {
        let sink = fusion.begin(Options {
            id: plan.id,
            generation,
            threshold: plan.vision.merge_iou_threshold,
            output: mailbox,
            wake: Arc::new(move || wake.wake()),
            accepts: Arc::new(move |target| plan.target_center_is_visible(target)),
            current,
        });
        Arc::new(Self {
            sink,
            state: Mutex::new(SessionState {
                remaining: sources,
                statuses: SmallVec::new(),
                finished: false,
            }),
        })
    }
    pub(super) fn source(self: &Arc<Self>, name: &'static str) -> ScanSource {
        ScanSource {
            name,
            session: Arc::clone(self),
            complete: false,
        }
    }
    fn finish_source(&self, status: UiScanStatus) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finished {
            return;
        }
        let context_changed = status == UiScanStatus::ContextChanged;
        state.statuses.push(status);
        state.remaining = state.remaining.saturating_sub(1);
        if state.remaining != 0 && !context_changed {
            return;
        }
        state.finished = true;
        let terminal = combined_status(&state.statuses);
        if terminal == UiScanStatus::Success {
            for status in &state.statuses {
                if let UiScanStatus::Failed(error) = status {
                    crate::report_error!(
                        "windows-ui-scan",
                        "a provider failed while another provider completed the hybrid scan: {error}"
                    );
                }
            }
        }
        self.sink.finish(terminal);
    }
    #[cfg(test)]
    pub(super) fn flush(&self) {
        self.sink.flush();
    }
}
/// A token for one independently scheduled provider. Counts mean queued data,
/// not accepted fusion results: producers never execute or wait for fusion.
pub(super) struct ScanSource {
    name: &'static str,
    session: Arc<ScanSession>,
    complete: bool,
}
impl ScanSource {
    pub(super) fn sink(&self) -> Sink {
        self.session.sink.clone()
    }
    pub(super) fn push(&self, targets: Vec<UiTarget>) -> usize {
        self.push_from(TargetSource::Accessibility, targets)
    }
    pub(super) fn push_from(&self, source: TargetSource, targets: Vec<UiTarget>) -> usize {
        self.session.sink.submit(source, targets)
    }
    pub(super) fn finish(mut self, status: UiScanStatus) {
        self.complete = true;
        self.session.finish_source(status);
    }
}
impl Drop for ScanSource {
    fn drop(&mut self) {
        if !self.complete {
            crate::report_error!(
                "windows-ui-scan",
                "{} provider exited without a terminal status",
                self.name
            );
            self.session.finish_source(UiScanStatus::Failed(format!(
                "{} provider exited without a terminal status",
                self.name
            )));
        }
    }
}

fn combined_status(statuses: &[UiScanStatus]) -> UiScanStatus {
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::api::command::{UiScanRequest, UiScanStrategy, VisionOptions};

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect::new(x, y, width, height)
    }

    fn request(id: u64) -> UiScanRequest {
        UiScanRequest {
            scope: crate::api::UiScanScope::Window,
            id,
            timeout_ms: 1_000,
            bounds: Some(rect(0.0, 0.0, 1_920.0, 1_080.0)),
            roles: Vec::new(),
            max_depth: 8,
            visible_only: true,
            clickable_only: true,
            strategy: UiScanStrategy::Hybrid,
            vision: VisionOptions::default(),
            app: None,
        }
    }

    #[test]
    fn late_accessible_row_retires_both_immediately_published_visual_fragments() {
        let mailbox = Arc::new(ScanMailbox::default());
        let generation = mailbox.begin(41);
        let (events, _) = crate::platform::common::event_queue::channel();
        let mut input = request(41);
        input.scope = crate::api::UiScanScope::Screen;
        let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
        let fusion = fusion_worker.prepare(generation).unwrap();
        let session = ScanSession::test(
            super::super::accessibility::test_scan_plan(input),
            generation,
            2,
            Arc::clone(&mailbox),
            super::super::EventSender::without_wake(events),
            &fusion,
        );
        let visual = session.source("visual");
        let ax = session.source("accessibility");
        let fragment = |x| UiTarget {
            details: None,
            rect: rect(x, 100.0, 30.0, 20.0),
            name: String::new(),
            role: SemanticRole::Control,
        };
        visual.push_from(TargetSource::Contour, vec![fragment(20.0)]);
        session.flush();
        assert_eq!(mailbox.take().unwrap().targets.len(), 1);
        visual.push_from(TargetSource::Contour, vec![fragment(200.0)]);
        session.flush();
        assert_eq!(mailbox.take().unwrap().targets.len(), 1);
        let row = UiTarget {
            details: None,
            rect: rect(10.0, 95.0, 400.0, 32.0),
            name: "file".into(),
            role: SemanticRole::ListItem,
        };
        ax.push(vec![row.clone()]);
        session.flush();
        let update = mailbox.take().unwrap();
        assert_eq!(update.targets, vec![row]);
        assert_eq!(update.retired.len(), 2);
        visual.finish(UiScanStatus::Success);
        ax.finish(UiScanStatus::Success);
        session.flush();
        assert!(mailbox.take().unwrap().targets.is_empty());
    }

    #[test]
    fn merged_sources_accept_ten_thousand_and_bound_the_tail() {
        let mailbox = Arc::new(ScanMailbox::default());
        let generation = mailbox.begin(42);
        let (events, _) = crate::platform::common::event_queue::channel();
        let mut input = request(42);
        input.scope = crate::api::UiScanScope::Screen;
        input.bounds = Some(rect(0.0, 0.0, 4096.0, 4096.0));
        let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
        let fusion = fusion_worker.prepare(generation).unwrap();
        let session = ScanSession::test(
            super::super::accessibility::test_scan_plan(input),
            generation,
            2,
            Arc::clone(&mailbox),
            super::super::EventSender::without_wake(events),
            &fusion,
        );
        let ax = session.source("accessibility");
        let visual = session.source("OCR + contour");
        let targets = |range: std::ops::Range<usize>| {
            range
                .map(|i| UiTarget {
                    details: None,
                    rect: rect((i % 101 * 32) as f64, (i / 101 * 32) as f64, 12.0, 12.0),
                    name: String::new(),
                    role: SemanticRole::Control,
                })
                .collect()
        };
        assert_eq!(ax.push(targets(0..5000)), 5000);
        assert_eq!(
            visual.push_from(TargetSource::Contour, targets(4000..10001)),
            6001
        );
        ax.finish(UiScanStatus::Success);
        visual.finish(UiScanStatus::Success);
        session.flush();
        let result = mailbox.take().unwrap();
        assert_eq!(result.targets.len(), MAX_TARGETS);
        assert_eq!(result.status, UiScanStatus::Success);
    }

    #[test]
    fn screen_scope_accepts_desktop_vision_targets_but_clips_other_displays() {
        let mut input = request(99);
        input.scope = crate::api::UiScanScope::Screen;
        let plan = super::super::accessibility::test_scan_plan(input);
        let target = UiTarget {
            details: None,
            rect: rect(100.0, 100.0, 20.0, 20.0),
            name: String::new(),
            role: SemanticRole::Control,
        };
        assert!(plan.target_center_is_visible(&target));
        let outside = UiTarget {
            rect: rect(-200.0, 100.0, 20.0, 20.0),
            ..target
        };
        assert!(!plan.target_center_is_visible(&outside));
    }

    #[test]
    fn matches_iou_containment_and_same_baseline_centres() {
        assert!(rectangles_match(
            rect(0.0, 0.0, 20.0, 20.0),
            rect(2.0, 2.0, 20.0, 20.0),
            0.5,
            8.0
        ));
        assert!(rectangles_match(
            rect(0.0, 0.0, 30.0, 30.0),
            rect(4.0, 4.0, 5.0, 5.0),
            0.9,
            8.0
        ));
        assert!(rectangles_match(
            rect(0.0, 0.0, 8.0, 10.0),
            rect(6.0, 0.0, 8.0, 10.0),
            0.9,
            8.0
        ));
    }

    #[test]
    fn adjacent_buttons_and_cross_line_text_remain_distinct() {
        assert!(!rectangles_match(
            rect(0.0, 0.0, 20.0, 20.0),
            rect(24.0, 0.0, 20.0, 20.0),
            0.5,
            8.0
        ));
        assert!(!rectangles_match(
            rect(0.0, 0.0, 20.0, 8.0),
            rect(0.0, 10.0, 20.0, 8.0),
            0.5,
            8.0
        ));
    }

    #[test]
    fn spatial_index_finds_containment_outside_neighbouring_center_cells() {
        let mut index = SpatialIndex::new(64.0, 8.0, 2.0);
        let unique = |index: &mut SpatialIndex, candidate| {
            index.insert_if_unique(candidate, |a, b| rectangles_match(a, b, 0.5, 8.0))
        };
        assert!(unique(&mut index, rect(0.0, 0.0, 200.0, 80.0)));
        assert!(!unique(&mut index, rect(170.0, 10.0, 20.0, 20.0)));
        assert!(unique(&mut index, rect(220.0, 10.0, 20.0, 20.0)));
    }

    #[test]
    fn providers_share_first_writer_dedup_and_one_terminal() {
        let mailbox = Arc::new(ScanMailbox::default());
        let generation = mailbox.begin(41);
        let (events, _ignored) = crate::platform::common::event_queue::channel();
        let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
        let fusion = fusion_worker.prepare(generation).unwrap();
        let session = ScanSession::test(
            super::super::accessibility::test_scan_plan(request(41)),
            generation,
            2,
            Arc::clone(&mailbox),
            super::super::EventSender::without_wake(events),
            &fusion,
        );
        let first = session.source("first");
        let second = session.source("second");
        let target = |rect, name: &str| UiTarget {
            details: None,
            rect,
            name: name.into(),
            role: SemanticRole::Control,
        };
        assert_eq!(
            first.push(vec![target(rect(0.0, 0.0, 40.0, 20.0), "first text")]),
            1
        );
        session.flush();
        let first_result = mailbox.take().unwrap();
        assert_eq!(first_result.status, UiScanStatus::Partial);
        assert_eq!(first_result.targets[0].name, "first text");
        assert_eq!(
            second.push(vec![
                target(rect(1.0, 1.0, 40.0, 20.0), "later replacement"),
                target(rect(80.0, 0.0, 40.0, 20.0), "unique"),
            ]),
            2
        );
        first.finish(UiScanStatus::TimedOut);
        session.flush();
        let ready = mailbox.take().unwrap();
        assert_eq!(ready.status, UiScanStatus::Partial);
        assert_eq!(ready.targets.len(), 1);
        assert_eq!(ready.targets[0].name, "unique");
        second.finish(UiScanStatus::Success);
        session.flush();
        let result = mailbox.take().unwrap();
        assert_eq!(result.status, UiScanStatus::Success);
        assert!(result.targets.is_empty());
        assert!(mailbox.take().is_none());
    }

    #[test]
    fn completed_provider_group_releases_the_shared_scan_plan() {
        let mailbox = Arc::new(ScanMailbox::default());
        let generation = mailbox.begin(42);
        let (events, _ignored) = crate::platform::common::event_queue::channel();
        let plan = super::super::accessibility::test_scan_plan(request(42));
        let observer = Arc::clone(&plan);
        let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
        let fusion = fusion_worker.prepare(generation).unwrap();
        let session = ScanSession::test(
            plan,
            generation,
            2,
            mailbox,
            super::super::EventSender::without_wake(events),
            &fusion,
        );
        let first = session.source("first");
        let second = session.source("second");
        drop(session);

        first.finish(UiScanStatus::ContextChanged);
        second.finish(UiScanStatus::ContextChanged);
        fusion_worker
            .stop_until(std::time::Instant::now() + std::time::Duration::from_secs(5))
            .unwrap();

        assert_eq!(
            Arc::strong_count(&observer),
            1,
            "provider completion must not retain request vectors or occluders",
        );
    }

    #[test]
    fn one_context_change_terminates_hybrid_without_waiting_for_the_other_provider() {
        let mailbox = Arc::new(ScanMailbox::default());
        let generation = mailbox.begin(43);
        let (events, _ignored) = crate::platform::common::event_queue::channel();
        let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
        let fusion = fusion_worker.prepare(generation).unwrap();
        let session = ScanSession::test(
            super::super::accessibility::test_scan_plan(request(43)),
            generation,
            2,
            Arc::clone(&mailbox),
            super::super::EventSender::without_wake(events),
            &fusion,
        );
        let first = session.source("first");
        let second = session.source("second");

        first.finish(UiScanStatus::ContextChanged);
        session.flush();
        let result = mailbox
            .take()
            .expect("context change must publish immediately");
        assert_eq!(result.status, UiScanStatus::ContextChanged);
        assert!(result.targets.is_empty());

        second.finish(UiScanStatus::Success);
        session.flush();
        assert!(mailbox.take().is_none());
    }
}
