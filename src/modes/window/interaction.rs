//! Discrete layout commands and ordinary held move/size gestures.
use super::*;
use crate::api::command::WindowScreenTarget;

impl WindowSession {
    pub(super) fn action(
        &mut self,
        action: W,
        state: KeyState,
        key: &Key,
        ctx: &HostContext<'_>,
        out: &mut CommandBatch,
    ) {
        if matches!(action, W::MultiSelect | W::MultiConfirm | W::ClearMulti) {
            if self.kind == WindowKind::Move && state == KeyState::Down {
                match action {
                    W::MultiConfirm => self.finish_multi(),
                    W::MultiSelect => self.begin_multi(out),
                    W::ClearMulti => self.clear_multi(out),
                    _ => unreachable!(),
                }
                self.center_multi_pointer(ctx, out);
            }
            return;
        }
        if state == KeyState::Down {
            self.finish_multi();
        }
        if self.kind == WindowKind::Tab {
            if state == KeyState::Down {
                self.tab_input(tabs::Input::Action(action), ctx, out);
            }
            return;
        }
        if matches!(
            action,
            W::SystemVolumeDown
                | W::SystemVolumeUp
                | W::SystemVolumeMute
                | W::SystemAudioPrevious
                | W::SystemAudioNext
        ) {
            if state == KeyState::Down {
                use crate::api::audio::{AudioAction, AudioTarget};
                self.stop_movement(out);
                let change = match action {
                    W::SystemVolumeDown => AudioAction::Down,
                    W::SystemVolumeUp => AudioAction::Up,
                    W::SystemVolumeMute => AudioAction::ToggleMute,
                    W::SystemAudioPrevious => AudioAction::DevicePrevious,
                    _ => AudioAction::DeviceNext,
                };
                self.request_audio(AudioTarget::System, change, out);
            }
            return;
        }
        if matches!(
            action,
            W::VolumeDown | W::VolumeUp | W::VolumeMute | W::AudioPrevious | W::AudioNext
        ) {
            if state == KeyState::Down {
                self.stop_movement(out);
                use crate::api::audio::{AudioAction, AudioTarget};
                let change = match action {
                    W::VolumeDown => AudioAction::Down,
                    W::VolumeUp => AudioAction::Up,
                    W::AudioPrevious => AudioAction::DevicePrevious,
                    W::AudioNext => AudioAction::DeviceNext,
                    _ => AudioAction::ToggleMute,
                };
                let targets = self.operation_targets();
                if self.multi_anchor.is_some() {
                    if !targets.is_empty() {
                        self.request_audio(
                            AudioTarget::Applications(targets.into_vec()),
                            change,
                            out,
                        );
                    }
                } else if let Some(id) = targets.first() {
                    self.request_audio(AudioTarget::Application(*id), change, out);
                }
            }
            return;
        }
        if action.is_held() {
            if state == KeyState::Up {
                self.held.remove(key);
                if self.held.is_empty() {
                    out.push(Command::SetFrameClock(false));
                    if let Some(edit) = &mut self.edit {
                        edit.divider_gesture = false;
                    }
                }
            } else if (self.edit.is_none()
                || matches!((&self.edit, action), (Some(edit), W::Ratio(_)) if matches!(edit.model, EditModel::Tree(_))))
                && !self.temporary
                && (self.target.is_some()
                    || matches!((&self.edit, action), (Some(edit), W::Ratio(_)) if matches!(edit.model, EditModel::Tree(_))))
                && !self.held.contains_key(key)
            {
                if self.held.is_empty() {
                    self.group += 1;
                    out.push(Command::SetFrameClock(true));
                }
                self.held.insert(key.clone(), action);
                self.motion(None, ctx, out);
            }
            return;
        }
        if state != KeyState::Down {
            return;
        }
        self.stop_movement(out);
        if action == W::Confirm && self.number.pending() {
            let completed = self.number.finish(&self.window_index, &self.slot_index);
            out.push(Command::CancelTimer {
                id: NUMBER_TIMER.into(),
            });
            if let Some((slot, number)) = completed {
                self.choose_number(slot, number, ctx, out);
            }
            return;
        }
        if !matches!(action, W::Navigate(_) | W::Split(_) | W::Ratio(_)) {
            self.cancel_number(out);
            self.swap_source = None;
        }
        match action {
            W::SaveLayout => self.save_layout(out),
            W::RemoveRegion => self.remove_region(ctx, out),
            W::Navigate(direction) => self.edit_direction(direction, false, false, ctx, out),
            W::Split(direction) => self.edit_direction(direction, true, false, ctx, out),
            W::Ratio(direction) => self.edit_direction(direction, false, true, ctx, out),
            W::Select | W::SelectPrevious => {
                self.multi.clear();
                self.multi_anchor = None;
                let backwards = action == W::SelectPrevious;
                if let Some(edit) = &self.edit
                    && let EditModel::Tree(tree) = &edit.model
                {
                    let windows: Vec<_> = self.visible_windows().collect();
                    let current = self.target.as_ref().map(|w| w.id).or_else(|| {
                        tree.slots()
                            .iter()
                            .find(|s| s.id == tree.selected)
                            .and_then(|s| s.window)
                    });
                    let next = current
                        .and_then(|id| windows.iter().position(|w| w.id == id))
                        .map_or(0, |i| {
                            if backwards {
                                (i + windows.len() - 1) % windows.len().max(1)
                            } else {
                                (i + 1) % windows.len().max(1)
                            }
                        });
                    if let Some(window) = windows.get(next) {
                        let id = window.id;
                        if let Some(edit) = &mut self.edit
                            && let EditModel::Tree(tree) = &mut edit.model
                        {
                            tree.focus_window(self.tabs.state.representative(id));
                        }
                        self.request(WindowOperation::Select(id), out);
                    }
                } else if self.edit.is_some() {
                    self.finish_edit(Finish::Cycle { backwards }, out);
                } else {
                    self.request(
                        if backwards {
                            WindowOperation::CyclePrevious
                        } else {
                            WindowOperation::Cycle
                        },
                        out,
                    );
                }
            }
            W::Undo => {
                self.numbered_slots = 0;
                if let Some(edit) = &mut self.edit {
                    if edit.finishing.is_some() {
                        return;
                    }
                    if let Some(previous) = edit.history.pop() {
                        edit.redo.push(edit.model.clone());
                        if edit.history.is_empty()
                            && !edit.entry_layout
                            && matches!(previous, EditModel::Tree(_))
                        {
                            self.finish_edit(Finish::TreeReset, out);
                            return;
                        }
                        if previous == EditModel::Quick(QuickPlacement::default()) {
                            self.finish_edit(Finish::QuickReset, out);
                            return;
                        }
                        edit.model = previous;
                        edit.dirty = true;
                        self.flush_edit(out);
                        self.rebuild_numbers();
                    } else if edit.entry_layout && matches!(edit.model, EditModel::Tree(_)) {
                        edit.redo.push(edit.model.clone());
                        self.finish_edit(Finish::TreeReset, out);
                    } else if !edit.redo.is_empty() {
                        self.status = Some("Nothing to undo in this edit".into());
                    } else {
                        self.finish_edit(Finish::History { redo: false }, out);
                    }
                } else {
                    self.request_history(false, false, out);
                }
            }
            W::Redo => {
                self.numbered_slots = 0;
                if let Some(edit) = &mut self.edit {
                    if edit.finishing.is_some() {
                        return;
                    }
                    if let Some(next) = edit.redo.pop() {
                        if edit.history.len() == 32 {
                            edit.history.remove(0);
                        }
                        edit.history.push(edit.model.clone());
                        edit.model = next;
                        edit.dirty = true;
                        self.flush_edit(out);
                        self.rebuild_numbers();
                    } else if edit.history.is_empty() && !edit.entry_layout {
                        self.finish_edit(Finish::History { redo: true }, out);
                    } else {
                        self.status = Some("Nothing to redo in this edit".into());
                    }
                } else {
                    self.request_history(true, false, out);
                }
            }
            W::ResetInitial => {
                if self.edit.is_some() {
                    self.finish_edit(Finish::ResetInitial, out);
                } else {
                    self.request_initial(false, out);
                }
            }
            W::Tile => {
                if self.edit.is_some() {
                    self.finish_edit(Finish::Tile, out);
                } else {
                    self.tile(out);
                }
            }
            _ if self.edit.is_some() => {}
            W::Size => self.size = !self.size,
            W::Close if self.kind == WindowKind::Move => {
                for id in self.operation_targets() {
                    if self.closing.contains_key(&id) {
                        continue;
                    }
                    let operation = if self.multi_anchor.is_some()
                        && let Some(group) = self.tabs.state.containing(id)
                    {
                        for member in &group.members {
                            self.closing.insert(*member, self.request + 1);
                        }
                        WindowOperation::CloseGroup(id)
                    } else {
                        self.closing.insert(id, self.request + 1);
                        WindowOperation::Close(id)
                    };
                    // Keep geometry until native closure, including save/cancel dialogs.
                    self.request(operation, out);
                    self.status = Some("Close requested".into());
                }
            }
            W::NextScreen
            | W::PreviousScreen
            | W::CycleState
            | W::ToggleMaximize
            | W::ToggleMinimize
            | W::Center => {
                self.group += 1;
                let change = match action {
                    W::NextScreen => WindowChange::Screen(WindowScreenTarget::Next),
                    W::PreviousScreen => WindowChange::Screen(WindowScreenTarget::Previous),
                    W::CycleState => WindowChange::CycleState,
                    W::ToggleMaximize => WindowChange::ToggleMaximize,
                    W::ToggleMinimize => WindowChange::ToggleMinimize,
                    _ => WindowChange::Center,
                };
                self.adjust(change, out);
            }
            _ => {}
        }
    }
}
