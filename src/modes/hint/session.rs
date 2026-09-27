use std::collections::HashMap;

use smallvec::SmallVec;

use crate::api::{Rect, SemanticRole, UiTarget};

use super::MAX_INLINE_TARGETS;
use super::labeling::CompactHint;
use crate::api::hint::LabelDirection;

/// Request-scoped scan data. Dropping or resetting this value retires every
/// target, label, retry marker, and asynchronous scan identity together.
#[derive(Default)]
pub(super) struct ScanSession {
    pub(super) scanned: Vec<UiTarget>,
    pub(super) scanned_names_lower: Vec<String>,
    pub(super) search_names_initialized: bool,
    pub(super) seen_targets: HashMap<(i64, i64, i64, i64), SmallVec<[usize; 2]>>,
    pub(super) hints: Vec<CompactHint<usize>>,
    pub(super) label_plan_count: usize,
    pub(super) next_label_index: usize,
    pub(super) scanning: bool,
    pub(super) status: Option<String>,
    pub(super) scan_id: u64,
    pub(super) retry_attempt: u32,
    pub(super) retry_pending: bool,
    pub(super) scan_bounds: Option<Rect>,
    pub(super) pending_relabel: bool,
    deferred_targets: Vec<UiTarget>,
    deferred_retired: Vec<Rect>,
    pub(super) selected: Option<usize>,
    pub(super) finished: bool,
    pub(super) active: bool,
}

impl ScanSession {
    pub(super) fn clear_results(&mut self) {
        self.scanned = Vec::new();
        self.scanned_names_lower = Vec::new();
        self.search_names_initialized = false;
        self.seen_targets = HashMap::new();
        self.hints = Vec::new();
        self.label_plan_count = 0;
        self.next_label_index = 0;
        self.pending_relabel = false;
        self.deferred_targets = Vec::new();
        self.deferred_retired = Vec::new();
    }

    pub(super) fn defer_update(&mut self, mut targets: Vec<UiTarget>, retired: Vec<Rect>) {
        crate::api::command::enrich_replacements(&mut targets, &self.deferred_targets, &retired);
        crate::api::command::remove_retired_targets(&mut self.deferred_targets, &retired);
        self.deferred_targets.extend(targets);
        self.deferred_retired.extend(retired);
        self.pending_relabel = true;
    }

    pub(super) fn apply_deferred(&mut self) {
        let targets = std::mem::take(&mut self.deferred_targets);
        let retired = std::mem::take(&mut self.deferred_retired);
        self.apply_update(targets, &retired);
    }

    pub(super) fn apply_update(&mut self, mut targets: Vec<UiTarget>, retired: &[Rect]) -> bool {
        crate::api::command::enrich_replacements(&mut targets, &self.scanned, retired);
        let before = self.scanned.len();
        crate::api::command::remove_retired_targets(&mut self.scanned, retired);
        let removed = self.scanned.len() != before;
        if removed {
            self.rebuild_target_lookup();
        }
        self.append_targets(targets) || removed
    }

    /// Apply a delta without renumbering unaffected labels when the current
    /// prefix-free code space has enough room. No timer or provider barrier.
    /// Returns (targets changed, labels reconciled); false in the second slot
    /// asks the caller to rebuild the code space after genuine capacity growth.
    pub(super) fn apply_stable_update(
        &mut self,
        mut targets: Vec<UiTarget>,
        retired: &[Rect],
        alphabet: &[char],
        direction: LabelDirection,
        preserve_anchors: bool,
    ) -> (bool, bool) {
        if targets.is_empty() && retired.is_empty() {
            return (false, true);
        }
        crate::api::command::enrich_replacements(&mut targets, &self.scanned, retired);
        let mut removed_hints = Vec::new();
        let before = self.scanned.len();
        if !retired.is_empty() {
            // Existing semantic-key buckets identify exact removals without
            // quadratic rectangle comparisons or a second hash table.
            let mut remap = SmallVec::<[usize; MAX_INLINE_TARGETS]>::new();
            remap.resize(before, 0);
            for rect in retired {
                if let Some(indices) = self.seen_targets.get(&rect_key(*rect)) {
                    for &index in indices {
                        if self.scanned[index].rect == *rect {
                            remap[index] = usize::MAX;
                        }
                    }
                }
            }
            let mut old = 0;
            let mut kept = 0;
            self.scanned.retain(|_| {
                let retain = remap[old] != usize::MAX;
                if retain {
                    remap[old] = kept;
                    kept += 1;
                }
                old += 1;
                retain
            });
            removed_hints.extend(self.hints.extract_if(.., |hint| {
                let index = remap[hint.value];
                if index == usize::MAX {
                    true
                } else {
                    hint.value = index;
                    false
                }
            }));
            self.rebuild_target_lookup();
        }
        let retained = self.scanned.len();
        let changed = self.append_targets(targets) || retained != before;
        let added = self.scanned.len() - retained;
        let Some(plan) = super::labeling::LabelPlan::for_stream(
            self.label_plan_count,
            alphabet.len(),
            direction,
        ) else {
            return (changed, false);
        };
        let capacity = plan.capacity(alphabet.len());
        if added > removed_hints.len() + capacity.saturating_sub(self.next_label_index) {
            return (changed, false);
        }
        self.hints.reserve(added);
        if removed_hints.is_empty() {
            for (offset, target) in self.scanned[retained..].iter().enumerate() {
                let label = plan.code(self.next_label_index, alphabet);
                self.next_label_index += 1;
                self.hints.push(CompactHint {
                    label,
                    bounds: target.rect,
                    value: retained + offset,
                });
            }
            return (changed, true);
        }
        // Associate a replacement with the closest old visible anchor inside
        // its verified control bounds. Row sorting bounds each normal query
        // to one control-height band, instead of testing every pair of boxes.
        removed_hints.sort_unstable_by(|a, b| {
            a.bounds
                .center()
                .y
                .total_cmp(&b.bounds.center().y)
                .then(a.value.cmp(&b.value))
        });
        let mut replacements = SmallVec::<[usize; MAX_INLINE_TARGETS]>::new();
        replacements.resize(added, usize::MAX);
        let mut used = SmallVec::<[bool; MAX_INLINE_TARGETS]>::new();
        used.resize(removed_hints.len(), false);
        // Give a nested checkbox/button its own anchor before its enclosing
        // row chooses one. Output order and code-space order stay unchanged.
        let mut association_order: SmallVec<[usize; MAX_INLINE_TARGETS]> = (0..added).collect();
        association_order.sort_unstable_by(|&a, &b| {
            let area = |i: usize| {
                let r = self.scanned[retained + i].rect;
                r.width * r.height
            };
            area(a).total_cmp(&area(b)).then(a.cmp(&b))
        });
        for offset in association_order {
            let target = &self.scanned[retained + offset];
            let rect = target.rect;
            let start = removed_hints.partition_point(|hint| hint.bounds.center().y < rect.y);
            let end = removed_hints.partition_point(|hint| hint.bounds.center().y <= rect.bottom());
            let center = rect.center();
            let best = (start..end)
                .filter(|&i| !used[i] && rect.contains(&removed_hints[i].bounds.center()))
                .min_by(|&a, &b| {
                    let distance = |i: usize| {
                        let p = removed_hints[i].bounds.center();
                        (p.x - center.x).powi(2) + (p.y - center.y).powi(2)
                    };
                    distance(a).total_cmp(&distance(b)).then(a.cmp(&b))
                });
            if let Some(index) = best {
                replacements[offset] = index;
                used[index] = true;
            }
        }
        let mut free = 0;
        for (offset, target) in self.scanned[retained..].iter().enumerate() {
            let inherited = replacements[offset];
            let (label, bounds) = if inherited != usize::MAX {
                let old = &mut removed_hints[inherited];
                // The visual anchor is retained only inside the new control.
                // Clicking still uses scanned[target].rect, never this anchor.
                (
                    std::mem::take(&mut old.label),
                    if preserve_anchors {
                        old.bounds
                    } else {
                        target.rect
                    },
                )
            } else {
                while free < used.len() && used[free] {
                    free += 1;
                }
                if free < used.len() {
                    let label = std::mem::take(&mut removed_hints[free].label);
                    used[free] = true;
                    free += 1;
                    (label, target.rect)
                } else {
                    let label = plan.code(self.next_label_index, alphabet);
                    self.next_label_index += 1;
                    (label, target.rect)
                }
            };
            self.hints.push(CompactHint {
                label,
                bounds,
                value: retained + offset,
            });
        }
        (changed, true)
    }

    fn rebuild_target_lookup(&mut self) {
        self.seen_targets.clear();
        for (index, target) in self.scanned.iter().enumerate() {
            self.seen_targets
                .entry(target_key(target))
                .or_default()
                .push(index);
        }
        self.scanned_names_lower.clear();
        self.search_names_initialized = false;
    }

    #[inline]
    pub(super) fn append_targets(&mut self, targets: Vec<UiTarget>) -> bool {
        let before = self.scanned.len();

        // Take the first platform batch by ownership; every scan buffer is
        // released on exit or before the next scan.
        if self.scanned.is_empty()
            && self.scanned.capacity() == 0
            && self.seen_targets.is_empty()
            && !self.search_names_initialized
        {
            self.scanned = targets;
            self.scanned.retain(|target| {
                self.scan_bounds
                    .is_none_or(|bounds| bounds.contains(&target.rect.center()))
            });
            self.seen_targets.reserve(self.scanned.len());

            let mut retained = 0;
            for index in 0..self.scanned.len() {
                let key = target_key(&self.scanned[index]);
                let indices = self.seen_targets.entry(key).or_default();
                let duplicate = {
                    indices.iter().any(|&existing_index| {
                        let existing = &self.scanned[existing_index];
                        let candidate = &self.scanned[index];
                        existing.name == candidate.name && existing.role == candidate.role
                    })
                };
                if !duplicate {
                    // Compact in source order without shifting the remaining
                    // batch for each duplicate. Indices always refer to the
                    // retained prefix, so later duplicates see canonical data.
                    if retained != index {
                        self.scanned.swap(retained, index);
                    }
                    indices.push(retained);
                    retained += 1;
                }
            }
            self.scanned.truncate(retained);
            return !self.scanned.is_empty();
        }

        let incoming = targets.len();
        self.scanned.reserve(incoming);
        self.seen_targets.reserve(incoming);
        if self.search_names_initialized {
            self.scanned_names_lower.reserve(incoming);
        }
        for target in targets {
            self.append_target(target);
        }
        self.scanned.len() != before
    }

    pub(super) fn ensure_search_names(&mut self) {
        if self.search_names_initialized {
            return;
        }
        self.scanned_names_lower.clear();
        self.scanned_names_lower
            .extend(self.scanned.iter().map(|target| target.name.to_lowercase()));
        self.search_names_initialized = true;
    }

    fn append_target(&mut self, target: UiTarget) -> bool {
        if !self
            .scan_bounds
            .is_none_or(|bounds| bounds.contains(&target.rect.center()))
        {
            return false;
        }
        let key = target_key(&target);
        let indices = self.seen_targets.entry(key).or_default();
        let duplicate = {
            indices.iter().any(|&index| {
                let existing = &self.scanned[index];
                existing.name == target.name && existing.role == target.role
            })
        };
        if duplicate {
            return false;
        }
        let index = self.scanned.len();
        if self.search_names_initialized {
            self.scanned_names_lower.push(target.name.to_lowercase());
        }
        self.scanned.push(target);
        indices.push(index);
        true
    }
}

fn target_key(target: &UiTarget) -> (i64, i64, i64, i64) {
    rect_key(target.rect)
}

fn rect_key(rect: Rect) -> (i64, i64, i64, i64) {
    (
        (rect.x * 4.0).round() as i64,
        (rect.y * 4.0).round() as i64,
        (rect.width * 4.0).round() as i64,
        (rect.height * 4.0).round() as i64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labeled_session(count: usize, alphabet: &[char], direction: LabelDirection) -> ScanSession {
        let mut session = ScanSession::default();
        session.append_targets(
            (0..count)
                .map(|i| UiTarget {
                    rect: Rect::new(i as f64 * 30.0, 10.0, 20.0, 20.0),
                    name: i.to_string(),
                    role: SemanticRole::Control,
                })
                .collect(),
        );
        super::super::labeling::assign_compact_into(
            &mut session.hints,
            session.scanned.iter().enumerate().map(|(i, t)| (t.rect, i)),
            alphabet,
            direction,
        )
        .unwrap();
        session.label_plan_count = count;
        session.next_label_index = count;
        session
    }

    #[test]
    fn repeated_scans_release_all_result_capacity() {
        let mut session = ScanSession::default();
        for count in [24, 512, 2000, 24, 10000, 1].into_iter().cycle().take(30) {
            session.append_targets(
                (0..count)
                    .map(|i| UiTarget {
                        rect: Rect::new(i as f64 * 30., 0., 20., 20.),
                        name: format!("target {i}"),
                        role: SemanticRole::Button,
                    })
                    .collect(),
            );
            session.ensure_search_names();
            session.deferred_targets = session.scanned.clone();
            session.deferred_retired = session.scanned.iter().map(|t| t.rect).collect();
            session.clear_results();
            assert_eq!(session.scanned.capacity(), 0);
            assert_eq!(session.scanned_names_lower.capacity(), 0);
            assert_eq!(session.seen_targets.capacity(), 0);
            assert_eq!(session.hints.capacity(), 0);
            assert_eq!(session.deferred_targets.capacity(), 0);
            assert_eq!(session.deferred_retired.capacity(), 0);
        }
    }

    #[test]
    fn stable_updates_preserve_codes_and_expand_only_when_code_space_is_full() {
        for direction in [LabelDirection::Normal, LabelDirection::Reverse] {
            let alphabet: Vec<_> = "asdfghjkl".chars().collect();
            let mut session = labeled_session(20, &alphabet, direction);
            let old_codes: Vec<_> = session.hints.iter().map(|h| h.label.clone()).collect();
            let new_target = |i: usize| UiTarget {
                rect: Rect::new(i as f64 * 30.0, 50.0, 20.0, 20.0),
                name: i.to_string(),
                role: SemanticRole::Control,
            };
            assert_eq!(
                session.apply_stable_update(
                    (20..23).map(new_target).collect(),
                    &[],
                    &alphabet,
                    direction,
                    true
                ),
                (true, true)
            );
            for (hint, code) in session.hints.iter().zip(old_codes) {
                assert_eq!(hint.label, code);
            }
            let capacity =
                super::super::labeling::LabelPlan::for_stream(20, alphabet.len(), direction)
                    .unwrap()
                    .capacity(alphabet.len());
            assert_eq!(
                session.apply_stable_update(
                    (23..capacity + 1).map(new_target).collect(),
                    &[],
                    &alphabet,
                    direction,
                    true
                ),
                (true, false)
            );
        }
    }

    #[test]
    fn replacement_inherits_one_label_and_anchor_without_renumbering_other_controls() {
        let alphabet: Vec<_> = "asdfghjkl".chars().collect();
        let mut session = labeled_session(4, &alphabet, LabelDirection::Normal);
        let unaffected = session.hints[3].label.clone();
        let inherited = session.hints[1].label.clone();
        let anchor = session.hints[1].bounds;
        let retired: Vec<_> = session.scanned[..3].iter().map(|t| t.rect).collect();
        let row = UiTarget {
            rect: Rect::new(0.0, 0.0, 85.0, 40.0),
            name: "row".into(),
            role: SemanticRole::ListItem,
        };
        assert_eq!(
            session.apply_stable_update(
                vec![row.clone()],
                &retired,
                &alphabet,
                LabelDirection::Normal,
                true
            ),
            (true, true)
        );
        assert_eq!(session.hints.len(), 2);
        assert_eq!(session.hints[0].label, unaffected);
        assert_eq!(session.hints[1].label, inherited);
        assert_eq!(session.hints[1].bounds, anchor);
        assert_eq!(session.scanned[session.hints[1].value], row);
        assert_ne!(
            anchor.center(),
            row.rect.center(),
            "visual anchor is not the authoritative click point"
        );
    }

    #[test]
    fn replacement_never_reuses_an_anchor_outside_its_new_control() {
        let alphabet: Vec<_> = "asdfghjkl".chars().collect();
        let mut session = labeled_session(4, &alphabet, LabelDirection::Normal);
        let rect = Rect::new(500.0, 300.0, 30.0, 20.0);
        let retired = [session.scanned[0].rect];
        let incoming = UiTarget {
            rect,
            name: "new".into(),
            role: SemanticRole::Button,
        };
        assert_eq!(
            session.apply_stable_update(
                vec![incoming],
                &retired,
                &alphabet,
                LabelDirection::Normal,
                true
            ),
            (true, true)
        );
        assert_eq!(session.hints.last().unwrap().bounds, rect);
    }

    #[test]
    fn nested_control_inherits_its_anchor_before_a_row_can_claim_it() {
        let alphabet: Vec<_> = "asdfghjkl".chars().collect();
        let mut session = labeled_session(2, &alphabet, LabelDirection::Normal);
        let first = session.hints[0].label.clone();
        let retired: Vec<_> = session.scanned.iter().map(|t| t.rect).collect();
        let checkbox = UiTarget {
            rect: session.scanned[0].rect,
            name: "check".into(),
            role: SemanticRole::Checkbox,
        };
        let row = UiTarget {
            rect: Rect::new(-50.0, 0.0, 105.0, 40.0),
            name: "row".into(),
            role: SemanticRole::Row,
        };
        assert_eq!(
            session.apply_stable_update(
                vec![row, checkbox.clone()],
                &retired,
                &alphabet,
                LabelDirection::Normal,
                true
            ),
            (true, true)
        );
        let hint = session
            .hints
            .iter()
            .find(|h| session.scanned[h.value] == checkbox)
            .unwrap();
        assert_eq!(hint.label, first);
    }

    #[test]
    fn deferred_refinements_preserve_existing_selection_until_applied() {
        let mut session = ScanSession::default();
        let old = UiTarget {
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            name: "old".into(),
            role: SemanticRole::Control,
        };
        let middle = UiTarget {
            rect: Rect::new(2.0, 0.0, 24.0, 20.0),
            name: "text".into(),
            role: SemanticRole::Control,
        };
        let final_target = UiTarget {
            name: "button".into(),
            role: SemanticRole::Button,
            ..old.clone()
        };
        session.append_targets(vec![old.clone()]);
        session.ensure_search_names();
        session.defer_update(vec![middle.clone()], vec![old.rect]);
        session.defer_update(vec![final_target.clone()], vec![middle.rect]);
        assert_eq!(session.scanned, vec![old]);
        session.apply_deferred();
        assert_eq!(session.scanned, vec![final_target]);
        session.ensure_search_names();
        assert_eq!(session.scanned_names_lower, vec!["button"]);
        assert!(!session.append_targets(session.scanned.clone()));
    }

    #[test]
    fn owned_batches_preserve_first_occurrence_order_and_collision_indices() {
        for count in [24, 128, 129, 500, 2_000] {
            let mut targets = Vec::new();
            let mut expected = Vec::new();
            for index in 0..count {
                let target = UiTarget {
                    rect: Rect::new((index % 10) as f64, 0.0, 1.0, 1.0),
                    name: format!("Target {index}"),
                    role: SemanticRole::Button,
                };
                expected.push(target.clone());
                targets.push(target.clone());
                // An identical second copy must collapse into the first one.
                targets.push(target);
            }
            let original_storage = targets.as_ptr();
            let mut session = ScanSession::default();
            assert!(session.append_targets(targets));
            assert_eq!(session.scanned.as_ptr(), original_storage);
            assert_eq!(session.scanned, expected);
            for indices in session.seen_targets.values() {
                for &index in indices {
                    assert_eq!(session.scanned[index], expected[index]);
                }
            }
            session.ensure_search_names();
            // Later partials use the append path and the same canonical index.
            assert!(!session.append_targets(expected.clone()));
            assert_eq!(session.scanned, expected);
            assert_eq!(session.scanned_names_lower.len(), count);
            session.clear_results();
            assert!(session.scanned.is_empty());
            assert_eq!(session.scanned.capacity(), 0);
        }
    }
}
