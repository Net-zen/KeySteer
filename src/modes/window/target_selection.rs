//! Optional per-gesture selection. Native lookup stays on the window worker.
use super::*;
use crate::api::window::WindowTarget;
use std::collections::VecDeque;

pub(super) struct Action {
    action: W,
    source: Option<WindowTarget>,
    key: Key,
    released: bool,
}

#[derive(Default)]
pub(super) struct Selection {
    request: Option<u64>,
    queue: VecDeque<Action>,
}

impl Selection {
    pub(super) fn ready_for_result(&self, id: u64) -> bool {
        self.request.is_none_or(|request| request == id)
    }
}

impl WindowSession {
    pub(super) fn cancel_target_selection(&mut self, out: &mut CommandBatch) {
        if let Some(selection) = self.selection.take()
            && let Some(id) = selection.request
        {
            self.ignored_selections
                .get_or_insert_with(Default::default)
                .insert(id);
            // The worker may already have resolved the lookup. Restore its anchor
            // before later inventory/gesture requests, without focusing or warping.
            self.request(
                WindowOperation::RestoreTarget(self.target.as_ref().map(|w| w.id)),
                out,
            );
        }
        self.targeted_audio = None;
    }

    pub(super) fn ignores_target_result(&mut self, id: u64) -> bool {
        let Some(ignored) = &mut self.ignored_selections else {
            return false;
        };
        let found = ignored.remove(&id);
        ignored.retain(|request| *request > id);
        if ignored.is_empty() {
            self.ignored_selections = None;
        }
        found
    }

    pub(super) fn targeted_action(
        &mut self,
        action: W,
        source: Option<WindowTarget>,
        state: KeyState,
        key: &Key,
        ctx: &HostContext<'_>,
        out: &mut CommandBatch,
    ) {
        if self.kind == WindowKind::Move && self.multi_anchor.is_some() {
            self.action(action, state, key, ctx, out);
            return;
        }
        if state == KeyState::Up {
            if let Some(held) = &mut self.targeted_audio {
                held.remove(key);
                if held.is_empty() {
                    self.targeted_audio = None;
                }
            }
            if let Some(selection) = &mut self.selection
                && let Some(pending) = selection
                    .queue
                    .iter_mut()
                    .rev()
                    .find(|a| a.key == *key && !a.released)
            {
                pending.released = true;
            }
            self.action(action, state, key, ctx, out);
            return;
        }
        if self.held.contains_key(key)
            || self
                .selection
                .as_ref()
                .is_some_and(|s| s.queue.iter().any(|a| a.key == *key && !a.released))
        {
            return;
        }
        if self.temporary || self.pending_transition.is_some() {
            return;
        }
        if let Some(id) = self
            .targeted_audio
            .as_ref()
            .and_then(|held| held.get(key))
            .copied()
        {
            use crate::api::audio::{AudioAction, AudioTarget};
            let audio = match action {
                W::VolumeDown => AudioAction::Down,
                W::VolumeUp => AudioAction::Up,
                _ => return,
            };
            self.request_audio(AudioTarget::Application(id), audio, out);
            return;
        }
        self.stop_movement(out);
        let selection = self.selection.get_or_insert_with(Default::default);
        if selection.queue.len() >= 64 {
            self.status = Some("Too many pending window actions".into());
            return;
        }
        let first = selection.queue.is_empty();
        selection.queue.push_back(Action {
            action,
            source,
            key: key.clone(),
            released: !action.is_held(),
        });
        if first {
            if self
                .edit
                .as_ref()
                .is_some_and(|edit| matches!(edit.model, EditModel::Quick(_)))
            {
                self.finish_edit(Finish::TargetSelection, out);
            } else {
                self.start_target_selection(ctx, out);
            }
        }
    }

    pub(super) fn start_target_selection(&mut self, ctx: &HostContext<'_>, out: &mut CommandBatch) {
        loop {
            let Some(selection) = self.selection.as_mut() else {
                return;
            };
            if selection.request.is_some() {
                return;
            }
            let Some(action) = selection.queue.front() else {
                self.selection = None;
                return;
            };
            if let Some(source) = action.source {
                self.request(WindowOperation::ResolveTarget(source), out);
                if let Some(selection) = &mut self.selection {
                    selection.request = Some(self.request);
                }
                return;
            }
            let Some(action) = selection.queue.pop_front() else {
                return;
            };
            self.replay_target_action(action, ctx, out);
        }
    }

    pub(super) fn take_target_action(&mut self, id: u64) -> Option<Action> {
        let selection = self.selection.as_mut()?;
        if selection.request != Some(id) {
            return None;
        }
        selection.request = None;
        selection.queue.pop_front()
    }

    pub(super) fn replay_target_action(
        &mut self,
        action: Action,
        ctx: &HostContext<'_>,
        out: &mut CommandBatch,
    ) {
        if self.kind == WindowKind::Tab && action.source.is_some() {
            use crate::api::window_tabs::TabOperation as T;
            let operation = match action.action {
                W::TabRemove => Some(T::RemoveActive),
                W::TabDissolve => Some(T::Dissolve),
                W::TabNext => Some(T::Cycle { backwards: false }),
                W::TabPrevious => Some(T::Cycle { backwards: true }),
                W::TabMoveLeft => Some(T::Reorder { backwards: true }),
                W::TabMoveRight => Some(T::Reorder { backwards: false }),
                _ => None,
            };
            if let (Some(operation), Some(target)) = (operation, self.target.as_ref()) {
                self.tab_request(
                    T::At {
                        target: target.id,
                        operation: Box::new(operation),
                    },
                    out,
                );
                return;
            }
        }
        if let Some(edit) = &mut self.edit
            && let EditModel::Tree(tree) = &mut edit.model
            && matches!(
                action.action,
                W::Navigate(_) | W::Split(_) | W::Ratio(_) | W::RemoveRegion
            )
        {
            let Some(target) = &self.target else { return };
            if !tree.focus_window(self.tabs.state.representative(target.id)) {
                self.status = Some("Window is outside this layout".into());
                return;
            }
            self.swap_source = None;
        }
        if action.source.is_some()
            && !action.released
            && matches!(action.action, W::VolumeDown | W::VolumeUp)
            && let Some(target) = &self.target
        {
            self.targeted_audio
                .get_or_insert_with(Default::default)
                .insert(action.key.clone(), target.id);
        }
        self.action(action.action, KeyState::Down, &action.key, ctx, out);
        if action.released && action.action.is_held() {
            self.action(action.action, KeyState::Up, &action.key, ctx, out);
        }
    }

    pub(super) fn retarget_entry(&mut self, source: WindowTarget, out: &mut CommandBatch) {
        self.restore_multi_focus(out);
        self.discard_multi();
        self.target = None;
        self.enter_pending = true;
        self.request(WindowOperation::Retarget(source), out);
    }
}
