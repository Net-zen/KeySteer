use super::*;
use crate::api::{
    Point,
    point_sample::{ColorFormat, PromptKeys, Request},
};

pub(super) const TIMER: &str = "ui_hint.point_sample";

#[derive(Debug, Clone, PartialEq)]
pub struct PointSettings {
    pub field_modes: [crate::api::point_sample::FieldMode; 4],
    pub keys: PromptKeys,
    pub formats: Vec<ColorFormat>,
    pub marker_color: Option<crate::api::theme::CompiledColor>,
    pub marker_radius: u16,
    pub marker_width: u16,
    pub movement: crate::modes::normal::Settings,
}

pub(super) struct Inspection {
    movement: crate::modes::NormalMode,
    pub point: Point,
    /// Selection identity and original geometry, retained only for this query.
    pub members: SmallVec<[(usize, Rect); 8]>,
    pub sampling: bool,
    pending_request: Option<Request>,
    pub position: usize,
    pub target: Option<usize>,
    pub color: Option<Color>,
    pub format: usize,
    pub adjusting: bool,
    pub copy_pending: bool,
}

impl Inspection {
    fn new(movement: &crate::modes::normal::Settings) -> Self {
        Self {
            movement: crate::modes::NormalMode::new(movement.clone()),
            point: Point::new(0.0, 0.0),
            members: SmallVec::new(),
            sampling: false,
            pending_request: None,
            position: 0,
            target: None,
            color: None,
            format: 0,
            adjusting: false,
            copy_pending: false,
        }
    }
}

impl HintMode {
    pub(super) fn point_candidates(&self) -> &[CompactHint<usize>] {
        self.search_result_hints().map_or(&[], |(_, hints)| hints)
    }

    pub(super) fn point_members_match(&self, point: &Inspection) -> bool {
        let candidates = self.point_candidates();
        point.members.len() == candidates.len()
            && point
                .members
                .iter()
                .zip(candidates)
                .all(|((index, bounds), hint)| {
                    *index == hint.value
                        && self
                            .session
                            .scanned
                            .get(*index)
                            .is_some_and(|target| target.rect == *bounds)
                })
    }

    pub(super) fn point_center(&self, candidates: &[CompactHint<usize>]) -> Point {
        let count = candidates.len() as f64;
        candidates
            .iter()
            .fold(Point::new(0.0, 0.0), |mut center, hint| {
                let point = self.target_point(hint.value);
                center.x += point.x / count;
                center.y += point.y / count;
                center
            })
    }

    fn sample_point(&mut self) -> Option<Command> {
        let point = self.inspection.as_mut().filter(|point| point.sampling)?;
        self.sample_serial = self.sample_serial.wrapping_add(1);
        let request = Request {
            id: self.sample_serial,
            point: point.point,
        };
        point.pending_request = Some(request);
        Some(Command::SamplePoint(request))
    }

    pub(super) fn refresh_point_hit(&mut self) -> bool {
        let Some(point) = self.inspection.as_mut() else {
            return false;
        };
        let before = point.target;
        // Search filtering never removes the original scan hit-test targets.
        point.target = self
            .session
            .scanned
            .iter()
            .enumerate()
            .filter(|(_, target)| target.rect.contains(&point.point))
            .min_by(|(_, a), (_, b)| {
                (a.rect.width * a.rect.height).total_cmp(&(b.rect.width * b.rect.height))
            })
            .map(|(index, _)| index);
        point.target != before
    }

    #[inline(never)]
    pub(super) fn sync_point(&mut self, ctx: &HostContext<'_>, out: &mut CommandBatch) {
        let available = !self.point_candidates().is_empty();
        // A resolved selection may coexist with other preview candidates.
        // Capture pixels only when the visible result is unique.
        let sampling = available && self.session.hints.len() == 1;
        let same_members = self
            .inspection
            .as_ref()
            .map_or(!available, |p| self.point_members_match(p));
        if same_members && self.inspection.as_ref().is_some_and(|p| p.sampling) == sampling {
            return;
        }
        if !same_members {
            let mut inspection = self.inspection.take();
            if let Some(point) = inspection.as_mut() {
                point.movement.handle(&ModeEvent::Deactivated, ctx);
            }
            if available {
                let mut point = inspection.unwrap_or_else(|| {
                    Box::new(Inspection::new(&self.config.search_point.movement))
                });
                let candidates = self.point_candidates();
                point.point = self.point_center(candidates);
                point.members.clear();
                point.members.extend(
                    candidates
                        .iter()
                        .map(|hint| (hint.value, self.session.scanned[hint.value].rect)),
                );
                point.position = 0;
                point.target = (candidates.len() == 1).then(|| candidates[0].value);
                point.color = None;
                point.format = 0;
                point.adjusting = false;
                point.copy_pending = false;
                self.inspection = Some(point);
            }
            if self
                .inspection
                .as_ref()
                .is_some_and(|p| p.members.len() > 1)
            {
                self.refresh_point_hit();
            }
            out.push(Command::SetFrameClock(false));
            out.push(Command::SetPointAdjustment {
                available,
                adjusting: false,
            });
        }
        if let Some(point) = self.inspection.as_mut() {
            point.sampling = sampling;
            point.pending_request = None;
            point.color = None;
            point.copy_pending = false;
        }
        self.redraw();
        if sampling {
            out.extend(self.sample_point());
            out.push(Command::SetTimer {
                id: TIMER.into(),
                delay: Duration::from_millis(100),
                repeating: true,
            });
        } else {
            out.push(Command::CancelPointSample);
            out.push(Command::CancelTimer { id: TIMER.into() });
        }
    }

    #[inline(never)]
    pub(super) fn point_event(
        &mut self,
        event: &ModeEvent,
        ctx: &HostContext<'_>,
    ) -> Option<CommandBatch> {
        let next_point = if matches!(event, ModeEvent::CyclePointTarget) {
            self.inspection.as_ref().and_then(|point| {
                point
                    .members
                    .get(point.position % point.members.len())
                    .map(|(index, _)| self.target_point(*index))
            })
        } else {
            None
        };
        let point = self.inspection.as_mut()?;
        match event {
            ModeEvent::TogglePointAdjustment => {
                point.adjusting = !point.adjusting;
                let adjusting = point.adjusting;
                point.movement.handle(&ModeEvent::Deactivated, ctx);
                let mut out = self.redraw();
                out.push(Command::SetFrameClock(false));
                out.push(Command::SetPointAdjustment {
                    available: true,
                    adjusting,
                });
                Some(out)
            }
            ModeEvent::CyclePointColor => {
                if !point.sampling {
                    return Some(CommandBatch::new());
                }
                point.adjusting = true;
                point.format = (point.format + 1) % self.config.search_point.formats.len();
                let mut out = self.redraw();
                out.push(Command::SetPointAdjustment {
                    available: true,
                    adjusting: true,
                });
                Some(out)
            }
            ModeEvent::CyclePointTarget if point.adjusting => {
                if point.members.len() < 2 || point.copy_pending {
                    return Some(CommandBatch::new());
                }
                point.position = point.position % point.members.len() + 1;
                let index = point.members[point.position - 1].0;
                point.point = next_point?;
                point.target = Some(index);
                point.color = None;
                point.movement.handle(&ModeEvent::Deactivated, ctx);
                self.refresh_point_hit();
                let mut out = self.redraw();
                out.push(Command::SetFrameClock(false));
                out.extend(self.sample_point());
                Some(out)
            }
            ModeEvent::CopyTextField(3) => {
                if !point.sampling {
                    return Some(CommandBatch::new());
                }
                point.movement.handle(&ModeEvent::Deactivated, ctx);
                let mut out = CommandBatch::one(Command::SetFrameClock(false));
                if point.color.is_some() {
                    // The visible color already belongs to this exact point.
                    // Copy the same value, never a placeholder or an extra
                    // capture that could fail after the panel was updated.
                    if let Some(info) = self.search_info() {
                        let text = info.copy_value(3);
                        if !text.is_empty() {
                            out.push(Command::CopyText(text));
                        }
                    }
                } else {
                    point.copy_pending = true;
                    if point.pending_request.is_none() {
                        out.extend(self.sample_point());
                    }
                }
                Some(out)
            }
            ModeEvent::Timer { id, .. } if id == TIMER => {
                // A clipboard sample cannot be superseded by the refresh timer.
                Some(if point.pending_request.is_some() {
                    CommandBatch::new()
                } else {
                    self.sample_point().into_iter().collect()
                })
            }
            ModeEvent::PointSampled(sample) => {
                if !point.sampling || point.pending_request != Some(sample.request) {
                    return Some(CommandBatch::new());
                }
                point.pending_request = None;
                let changed = point.color != sample.color;
                point.color = sample.color;
                let copy = std::mem::take(&mut point.copy_pending);
                let mut out = if changed {
                    self.redraw()
                } else {
                    CommandBatch::new()
                };
                if copy && let Some(info) = self.search_info() {
                    let text = info.copy_value(3);
                    if !text.is_empty() {
                        out.push(Command::CopyText(text));
                    }
                }
                Some(out)
            }
            ModeEvent::Binding { .. } | ModeEvent::Frame { .. }
                if point.adjusting && !point.copy_pending =>
            {
                let mut out = CommandBatch::new();
                let before = point.point;
                for command in point.movement.handle(event, ctx) {
                    match command {
                        Command::MovePointer { dx, dy } => {
                            let bounds = self
                                .session
                                .scan_bounds
                                .unwrap_or_else(|| ctx.active_bounds());
                            point.point.x = (point.point.x + dx)
                                .clamp(bounds.x, (bounds.right() - 1.0).max(bounds.x));
                            point.point.y = (point.point.y + dy)
                                .clamp(bounds.y, (bounds.bottom() - 1.0).max(bounds.y));
                        }
                        Command::SetFrameClock(_) => out.push(command),
                        _ => {}
                    }
                }
                if point.point != before {
                    point.color = None;
                    point.copy_pending = false;
                    self.refresh_point_hit();
                    out.extend(self.redraw());
                    out.extend(self.sample_point());
                }
                Some(out)
            }
            _ => None,
        }
    }
}
