//! Mode-owned multi-selection input. The first run is digits; spaced runs are full labels.
use super::*;

pub(super) struct Input {
    pub text: String,
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
            "backspace" => {
                input.text.pop();
            }
            "delete" => input.text.clear(),
            "space" if !repeat => input.text.push(' '),
            _ if !repeat && key.as_char().is_some_and(|c| c.is_ascii_digit()) => {
                if let Some(c) = key.as_char() {
                    input.text.push(c);
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

    pub(super) fn finish_multi(&mut self) {
        self.recompute_multi_input(true);
        self.multi_input = None;
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
            for id in &self.multi {
                let id = self
                    .tabs
                    .state
                    .containing(*id)
                    .map_or(*id, |group| group.active);
                if !targets.contains(&id) {
                    targets.push(id);
                }
            }
        } else if let Some(target) = &self.target {
            targets.push(target.id);
        }
        targets
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
