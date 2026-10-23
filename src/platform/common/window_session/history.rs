//! Compact restoration checkpoints and bounded undo/redo history.
use super::*;
/// Undo state contains no titles or application strings. Metadata stays in the
/// native inventory and is refreshed when a placement is restored.
#[derive(Clone, Copy, Debug)]
pub(super) struct PlacementSnapshot {
    pub(super) info: PlacementInfo,
    pub(super) restored: Rect,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct PlacementInfo {
    pub(super) id: WindowId,
    pub(super) bounds: Rect,
    pub(super) screen: usize,
    pub(super) resizable: bool,
    pub(super) maximized: bool,
    pub(super) minimized: bool,
    pub(super) fullscreen: bool,
}
impl From<&Snapshot> for PlacementSnapshot {
    fn from(snapshot: &Snapshot) -> Self {
        let info = &snapshot.info;
        Self {
            info: PlacementInfo {
                id: info.id,
                bounds: info.bounds,
                screen: info.screen,
                resizable: info.resizable,
                maximized: info.maximized,
                minimized: info.minimized,
                fullscreen: info.fullscreen,
            },
            restored: snapshot.restored,
        }
    }
}
impl From<Snapshot> for PlacementSnapshot {
    fn from(snapshot: Snapshot) -> Self {
        Self::from(&snapshot)
    }
}
impl From<&PlacementSnapshot> for PlacementSnapshot {
    fn from(snapshot: &PlacementSnapshot) -> Self {
        *snapshot
    }
}
impl PlacementSnapshot {
    pub(super) fn apply_to(&self, snapshot: &mut Snapshot) {
        snapshot.info.bounds = self.info.bounds;
        snapshot.info.screen = self.info.screen;
        snapshot.info.resizable = self.info.resizable;
        snapshot.info.maximized = self.info.maximized;
        snapshot.info.minimized = self.info.minimized;
        snapshot.info.fullscreen = self.info.fullscreen;
        snapshot.restored = self.restored;
    }
    pub(super) fn restore(
        &self,
        access: &mut impl WindowAccess,
        screens: &[Screen],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<WindowInfo, String> {
        let mut snapshot = access.snapshot(self.info.id, screens)?;
        self.apply_to(&mut snapshot);
        access.restore(&snapshot, screens, cancelled)
    }
}

impl Session {
    pub(super) fn recover_edit(&mut self, access: &mut impl WindowAccess) {
        if let Some(edit) = self.edit.take() {
            // Only native partial failure uses bounded entry-layout recovery.
            let deadline = Instant::now() + Duration::from_millis(1500);
            for before in edit.before.iter().rev() {
                if Instant::now() >= deadline {
                    crate::report_error!("window-worker", "entry-layout recovery timed out");
                    break;
                }
                if let Err(error) =
                    before.restore(access, &self.screens, &|| Instant::now() >= deadline)
                {
                    crate::report_error!(
                        "window-worker",
                        "recover window {:?}: {error}",
                        before.info.id
                    );
                }
            }
        }
    }

    pub(super) fn remember(&mut self, group: u64, before: impl Into<PlacementSnapshot>) {
        let before = before.into();
        self.initial.entry(before.info.id).or_insert(before);
        self.changed_windows.insert(before.info.id);
        self.redo.clear();
        if self.history.back().is_none_or(|(g, _)| *g != group) {
            self.history.retain_back(31);
            self.history.push_back((group, Vec::new()));
        }
        if let Some((_, snapshots)) = self.history.back_mut()
            && !snapshots.iter().any(|s| s.info.id == before.info.id)
        {
            snapshots.push(before);
        }
    }

    pub(super) fn capture_initial(
        &mut self,
        access: &impl WindowAccess,
        id: WindowId,
        screens: &[Screen],
    ) {
        if let std::collections::btree_map::Entry::Vacant(entry) = self.initial.entry(id)
            && let Ok(snapshot) = access.snapshot(id, screens)
        {
            entry.insert((&snapshot).into());
        }
    }

    pub(super) fn push_history(
        stack: &mut VecDeque<(u64, Vec<PlacementSnapshot>)>,
        group: u64,
        snapshots: Vec<PlacementSnapshot>,
    ) {
        if snapshots.is_empty() {
            return;
        }
        stack.retain_back(31);
        stack.push_back((group, snapshots));
    }

    // Return inverse snapshots for every actual native change, and retain
    // uncompleted work so cancellation or a refused write can be retried.
    pub(super) fn restore_snapshots(
        access: &mut impl WindowAccess,
        mut snapshots: Vec<PlacementSnapshot>,
        screens: &[Screen],
        cancelled: &dyn Fn() -> bool,
        result: &mut WindowResult,
    ) -> (Vec<PlacementSnapshot>, Vec<PlacementSnapshot>) {
        let mut inverse = Vec::new();
        let mut remaining = Vec::new();
        // One geometry operation per live group, preferring its stable anchor.
        snapshots.sort_by_key(|s| access.layout_representative(s.info.id) != s.info.id);
        let mut represented = std::collections::BTreeSet::new();
        snapshots.retain(|s| represented.insert(access.layout_representative(s.info.id)));
        while let Some(desired) = snapshots.pop() {
            if cancelled() {
                snapshots.push(desired);
                remaining.extend(snapshots);
                break;
            }
            let Ok(before) = access.snapshot(desired.info.id, screens) else {
                remaining.push(desired);
                result.skipped += 1;
                continue;
            };
            if same_placement(&before, desired) {
                continue;
            }
            let applied = desired.restore(access, screens, cancelled);
            let after = access.snapshot(desired.info.id, screens);
            if let Ok(after) = &after {
                if !same_placement(&before, after) {
                    inverse.push((&before).into());
                    result.changed += 1;
                }
                if same_placement(after, desired) {
                    continue;
                }
            } else {
                // The write may have succeeded before the read timed out.
                // Keep its inverse without claiming an observed state change.
                inverse.push((&before).into());
            }
            remaining.push(desired);
            result.skipped += 1;
            if let Err(error) = applied
                && !cancelled()
            {
                crate::report_error!("window-worker", "restore history: {error}");
            }
        }
        (inverse, remaining)
    }
}

/// Owns the source undo/redo batch until confirmed changes and retries are committed.
pub(super) struct PendingHistory {
    pub request: WindowRequest,
    screens: Arc<[Screen]>,
    group: u64,
    pending: VecDeque<PlacementSnapshot>,
    active: Vec<(PlacementSnapshot, PendingAdjustment)>,
    watched: Vec<WindowId>,
    inverse: Vec<PlacementSnapshot>,
    remaining: Vec<PlacementSnapshot>,
    result: WindowResult,
}
impl PendingHistory {
    pub fn begin(
        session: &mut Session,
        access: &mut impl WindowAccess,
        request: &WindowRequest,
        screens: &Arc<[Screen]>,
    ) -> Result<Option<Self>, String> {
        if !matches!(
            request.operation,
            WindowOperation::Undo | WindowOperation::Redo | WindowOperation::ResetInitial { .. }
        ) {
            return Ok(None);
        }
        if session.edit.is_some() {
            return Err("Finish the current window edit first".into());
        }
        session.prepare_context(access, request, screens);
        let (group, mut goals) = match request.operation {
            WindowOperation::Undo => match session.history.pop_back() {
                Some(batch) => batch,
                None => return Ok(None),
            },
            WindowOperation::Redo => match session.redo.pop_back() {
                Some(batch) => batch,
                None => return Ok(None),
            },
            WindowOperation::ResetInitial { group } => (
                group,
                session
                    .changed_windows
                    .iter()
                    .filter_map(|id| session.initial.get(id).copied())
                    .collect(),
            ),
            _ => unreachable!(),
        };
        goals.sort_by_key(|goal| access.layout_representative(goal.info.id) != goal.info.id);
        let mut represented = std::collections::BTreeSet::new();
        goals.retain(|goal| represented.insert(access.layout_representative(goal.info.id)));
        Ok(Some(Self {
            request: request.clone(),
            screens: screens.clone(),
            group,
            watched: goals.iter().map(|goal| goal.info.id).collect(),
            pending: goals.into_iter().rev().collect(),
            active: Vec::with_capacity(16),
            inverse: Vec::new(),
            remaining: Vec::new(),
            result: Session::result_for(request),
        }))
    }
    pub fn targets(&self) -> impl Iterator<Item = WindowId> + '_ {
        self.watched.iter().copied()
    }
    pub fn preparing(&self) -> bool {
        !self.pending.is_empty() && self.active.len() < 16
    }
    fn record(
        &mut self,
        goal: PlacementSnapshot,
        before: PlacementSnapshot,
        observed: Option<&Snapshot>,
    ) {
        if let Some(after) = observed {
            if !same_placement(before, after) {
                self.inverse.push(before);
                self.result.changed += 1;
            }
            if same_placement(goal, after) {
                return;
            }
        } else {
            self.inverse.push(before);
        }
        self.remaining.push(goal);
        self.result.skipped += 1;
    }
    pub fn advance(
        &mut self,
        session: &mut Session,
        access: &mut impl WindowAccess,
        cancelled: &dyn Fn() -> bool,
        now: Instant,
    ) -> Option<WindowResult> {
        let mut index = 0;
        while index < self.active.len() {
            self.active[index].1.advance_at(access, false, now);
            if !self.active[index].1.ready() {
                index += 1;
                continue;
            }
            let (goal, frame) = self.active.swap_remove(index);
            if let Some(error) = &frame.failure {
                crate::support::logging::report_error_context(
                    "window-history",
                    error,
                    format_args!(
                        "session={} request={}",
                        self.request.session, self.request.id
                    ),
                );
            }
            if !frame.observation_failed
                && let Err(error) = access.confirmed_placement(&frame.observed, &self.screens)
            {
                crate::report_error!("window-history", "{error}");
            }
            self.record(
                goal,
                frame.before,
                (!frame.observation_failed).then_some(&frame.observed),
            );
        }
        let budget = Instant::now();
        for _ in 0..8 {
            if cancelled() {
                self.remaining.extend(self.pending.drain(..));
                break;
            }
            if self.active.len() >= 16 {
                break;
            }
            let Some(goal) = self.pending.pop_front() else {
                break;
            };
            let Ok(mut observed) = access.snapshot(goal.info.id, &self.screens) else {
                self.remaining.push(goal);
                self.result.skipped += 1;
                continue;
            };
            if same_placement(goal, &observed) {
                continue;
            }
            if cancelled() {
                self.remaining.push(goal);
                self.remaining.extend(self.pending.drain(..));
                break;
            }
            if access.can_submit_frame(goal.info.id)
                && !goal.info.minimized
                && !goal.info.fullscreen
                && !observed.info.fullscreen
            {
                let mut frame = PendingAdjustment::geometry(
                    &self.request,
                    &self.screens,
                    observed,
                    if goal.info.maximized {
                        goal.restored
                    } else {
                        goal.info.bounds
                    },
                    goal.info.maximized.then_some(goal.info.bounds),
                );
                frame.force_submission();
                frame.advance_at(access, false, now + budget.elapsed());
                self.active.push((goal, frame));
            } else {
                // Unsupported state transitions still yield between windows and
                // reuse the snapshot already obtained by this operation.
                let before = PlacementSnapshot::from(&observed);
                goal.apply_to(&mut observed);
                let applied = access.restore(&observed, &self.screens, cancelled);
                let readable = access
                    .refresh_geometry(&mut observed, &self.screens)
                    .is_ok();
                self.record(goal, before, readable.then_some(&observed));
                if let Err(error) = applied {
                    crate::report_error!("window-history", "{error}");
                }
                break;
            }
            if budget.elapsed() >= Duration::from_millis(4) {
                break;
            }
        }
        if !self.active.is_empty() || !self.pending.is_empty() {
            return None;
        }
        let inverse = std::mem::take(&mut self.inverse);
        let remaining = std::mem::take(&mut self.remaining);
        match self.request.operation {
            WindowOperation::Redo => {
                Session::push_history(&mut session.redo, self.group, remaining);
                Session::push_history(&mut session.history, self.group, inverse);
            }
            WindowOperation::Undo => {
                Session::push_history(&mut session.history, self.group, remaining);
                Session::push_history(&mut session.redo, self.group, inverse);
            }
            WindowOperation::ResetInitial { .. } => {
                if !inverse.is_empty() {
                    session.redo.clear();
                    Session::push_history(&mut session.history, self.group, inverse);
                }
            }
            _ => unreachable!(),
        }
        if !cancelled() {
            match session.enumerate(access, &self.screens, cancelled) {
                Ok(windows) => self.result.windows = Some(windows),
                Err(error) => {
                    crate::report_error!("window-history", "{error}");
                    self.result.message = Some(error);
                }
            }
        }
        if self.result.message.is_none() {
            self.result.message = Some(format!(
                "{}Restored {} · skipped {}",
                if matches!(self.request.operation, WindowOperation::ResetInitial { .. }) {
                    "Initial state · "
                } else {
                    ""
                },
                self.result.changed,
                self.result.skipped
            ));
        }
        let result = std::mem::replace(&mut self.result, Session::result_for(&self.request));
        Some(session.complete_result(access, result, &self.screens))
    }
}
