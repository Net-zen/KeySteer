//! Mode-owned multi-selection input. The first run is digits; spaced runs are full labels.
use super::*;

pub(super) struct Input {
    pub text: String,
    pub selection: crate::api::text_edit::Selection,
    last_activation: Option<WindowId>,
    before: Vec<WindowId>,
    implicit_anchor: Option<WindowId>,
    // Numbering is frozen for this edit, so inventory changes cannot retarget typed labels.
    labels: BTreeMap<u32, WindowId>,
}

impl WindowSession {
    pub(super) fn retire_multi_windows(&mut self, closed: &[WindowId]) {
        if closed.is_empty() {
            return;
        }
        // A tab member disappearing does not discard the remaining logical window.
        let survivor = |id: WindowId| {
            if !closed.contains(&id) {
                return Some(id);
            }
            self.tabs.state.containing(id).and_then(|group| {
                group
                    .members
                    .iter()
                    .copied()
                    .find(|member| !closed.contains(member))
            })
        };
        let retain = |ids: &mut Vec<WindowId>| {
            *ids = ids.iter().copied().filter_map(survivor).collect();
        };
        retain(&mut self.multi);
        self.multi_anchor = self.multi_anchor.and_then(survivor);
        if let Some(input) = &mut self.multi_input {
            retain(&mut input.before);
            input.labels.retain(|_, id| {
                if let Some(replacement) = survivor(*id) {
                    *id = replacement;
                    true
                } else {
                    false
                }
            });
        }
        if self.multi_anchor.is_none() {
            self.multi_anchor = self.multi.first().copied();
            if self.multi_anchor.is_none() {
                self.multi_input = None;
            }
        }
    }

    pub(super) fn begin_multi(&mut self, out: &mut CommandBatch) {
        if self.multi_input.is_some() {
            self.finish_multi();
            return;
        }
        let Some(anchor) = self.target.as_ref().map(|w| w.id) else {
            return;
        };
        self.stop_movement(out);
        self.cancel_number(out);
        self.temporary = false;
        let implicit_anchor = self.multi_anchor.is_none().then_some(anchor);
        if self.multi_anchor.is_none() {
            self.multi_anchor = Some(anchor);
            self.multi = vec![anchor];
        }
        self.multi_input = Some(Input {
            text: String::new(),
            selection: crate::api::text_edit::Selection::default(),
            last_activation: None,
            before: self.multi.clone(),
            implicit_anchor,
            labels: self
                .visible
                .iter()
                .filter_map(|id| self.numbers.get(id).map(|n| (*n, *id)))
                .collect(),
        });
        self.status = None;
    }

    pub(super) fn multi_input_key(&mut self, key: &Key, repeat: bool) -> bool {
        let Some(input) = &mut self.multi_input else {
            return false;
        };
        match key.as_str() {
            "esc" => {
                self.multi = std::mem::take(&mut input.before);
                self.multi_input = None;
                return true;
            }
            "backspace" | "delete" | "arrow_left" | "arrow_right" | "home" | "end" => {
                use crate::api::text_edit::EditAction as E;
                let action = match key.as_str() {
                    "backspace" => E::Backspace,
                    "delete" => E::Delete,
                    "arrow_left" => E::Left,
                    "arrow_right" => E::Right,
                    "home" => E::Home,
                    _ => E::End,
                };
                input.selection.edit(&mut input.text, action);
            }
            "space" => {
                if !repeat {
                    input.selection.insert(&mut input.text, " ");
                }
            }
            _ if key.as_char().is_some_and(|c| c.is_ascii_digit()) => {
                if !repeat && let Some(c) = key.as_char() {
                    let mut byte = [0];
                    input
                        .selection
                        .insert(&mut input.text, c.encode_utf8(&mut byte));
                }
            }
            _ if key.as_char().is_some_and(|c| c.is_ascii_alphabetic()) => {
                self.finish_multi();
                return false;
            }
            _ => return key.is_modifier(),
        }
        self.recompute_multi_input(false);
        true
    }

    pub(super) fn edit_multi_input(&mut self, action: crate::api::text_edit::EditAction) {
        if let Some(input) = &mut self.multi_input {
            input.selection.edit(&mut input.text, action);
            self.recompute_multi_input(false);
        }
    }

    pub(super) fn activate_multi_front(&mut self, out: &mut CommandBatch) {
        let Some(input) = &mut self.multi_input else {
            return;
        };
        let prefix = input.text[..input.selection.cursor].trim_end();
        let number = if let Some((_, part)) = prefix.rsplit_once(' ') {
            part.parse::<u32>().ok()
        } else {
            prefix
                .bytes()
                .next_back()
                .map(|digit| u32::from(digit - b'0'))
        };
        // Deleting the complete edit restores its selection snapshot. Restore
        // foreground focus as well, rather than leaving only the anchor border.
        let target = if input.text.is_empty() {
            self.multi_anchor
        } else {
            number.and_then(|number| input.labels.get(&number).copied())
        }
        .filter(|id| self.inventory.contains_key(id));
        if target == input.last_activation {
            return;
        }
        input.last_activation = target;
        if let Some(id) = target {
            self.request(WindowOperation::Activate(id), out);
        }
    }

    pub(super) fn finish_multi(&mut self) {
        self.recompute_multi_input(true);
        self.multi_input = None;
    }

    pub(super) fn discard_multi(&mut self) {
        // Mode handoffs keep their existing target. Explicit clear still restores the anchor.
        self.multi_input = None;
        self.multi_anchor = None;
        self.multi.clear();
    }

    fn recompute_multi_input(&mut self, commit_last: bool) {
        let Some(input) = &self.multi_input else {
            return;
        };
        self.multi.clone_from(&input.before);
        let mut parts = input.text.split(' ');
        let singles = parts
            .next()
            .unwrap_or_default()
            .bytes()
            .map(|c| u32::from(c - b'0'));
        let mut parts = parts.peekable();
        let complete = std::iter::from_fn(|| {
            let part = parts.next()?;
            if !commit_last && parts.peek().is_none() {
                return None;
            }
            Some(part.parse::<u32>().ok())
        })
        .flatten();
        let numbers = singles.chain(complete);
        let mut implicit_anchor = input
            .implicit_anchor
            .map(|id| self.tabs.state.representative(id));
        for number in numbers {
            let Some(id) = input
                .labels
                .get(&number)
                .copied()
                .filter(|id| self.inventory.contains_key(id))
            else {
                continue;
            };
            let representative = self.tabs.state.representative(id);
            // The first explicit mention of the original single selection is idempotent.
            // Subsequent mentions, and later editing sessions, toggle normally.
            if implicit_anchor == Some(representative) {
                implicit_anchor = None;
                continue;
            }
            if let Some(index) = self
                .multi
                .iter()
                .position(|id| self.tabs.state.representative(*id) == representative)
            {
                self.multi.remove(index);
            } else {
                self.multi.push(id);
            }
        }
    }

    pub(super) fn operation_targets(&self) -> smallvec::SmallVec<[WindowId; 4]> {
        let mut targets = smallvec::SmallVec::new();
        if matches!(
            self.kind,
            WindowKind::Move | WindowKind::Quick | WindowKind::Editor
        ) && self.selection.is_none()
            && self.multi_anchor.is_some()
        {
            targets.extend(self.logical_multi_targets());
        } else if let Some(target) = &self.target {
            targets.push(target.id);
        }
        targets
    }

    fn logical_multi_targets(&self) -> impl Iterator<Item = WindowId> + '_ {
        let active = |id| {
            self.tabs
                .state
                .containing(id)
                .map_or(id, |group| group.active)
        };
        self.multi
            .iter()
            .enumerate()
            .filter_map(move |(index, id)| {
                let target = active(*id);
                (!self.multi[..index].iter().any(|id| active(*id) == target)).then_some(target)
            })
    }

    /// Follow the confirmed geometry of the selected logical windows, not
    /// each worker result's independently captured relative pointer offset.
    /// A tab group contributes one center; minimized windows contribute none.
    pub(super) fn uses_multi_pointer(&self) -> bool {
        !self.temporary
            && self.multi_anchor.is_some()
            && self.selection.is_none()
            && matches!(
                self.kind,
                WindowKind::Move | WindowKind::Quick | WindowKind::Editor
            )
            && self.logical_multi_targets().nth(1).is_some()
    }

    pub(super) fn multi_pointer(&self) -> Option<Point> {
        if !self.uses_multi_pointer() {
            return None;
        }
        let mut center = Point::default();
        let mut count = 0;
        for id in self.logical_multi_targets() {
            let Some(window) = self.inventory.get(&id).filter(|w| !w.minimized) else {
                continue;
            };
            let point = window.bounds.center();
            if !point.x.is_finite() || !point.y.is_finite() {
                continue;
            }
            count += 1;
            center.x += (point.x - center.x) / f64::from(count);
            center.y += (point.y - center.y) / f64::from(count);
        }
        (count > 0).then_some(center)
    }

    pub(super) fn center_multi_pointer(&self, ctx: &HostContext<'_>, out: &mut CommandBatch) {
        if let Some(point) = self.multi_pointer()
            && point != ctx.cursor
        {
            out.push(Command::warp_to(point));
        }
    }

    pub(super) fn clear_multi(&mut self, out: &mut CommandBatch) {
        self.stop_movement(out);
        self.multi_input = None;
        self.cancel_number(out);
        if let Some(id) = self.multi_anchor.take() {
            // Restore the captured identity; never re-resolve active/mouse preferences.
            self.request(WindowOperation::Select(id), out);
        }
        self.multi.clear();
    }
}
