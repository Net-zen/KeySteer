//! Stable native-slot reconciliation, independent of drawing APIs.
use crate::api::overlay::OverlayLabel;
use smallvec::SmallVec;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LabelIdentity<'a> {
    text: &'a str,
    editing: bool,
    x: u64,
    y: u64,
    width: u64,
    height: u64,
}

pub(crate) fn label_identity(label: &OverlayLabel) -> LabelIdentity<'_> {
    LabelIdentity {
        // Query changes must retain the editor slot, not borrow the bitmap
        // of a hint removed by the same filtering update.
        text: if label.edit.is_some() || label.scroll_to_cursor {
            ""
        } else {
            &label.text
        },
        editing: label.edit.is_some() || label.scroll_to_cursor,
        x: label.rect.x.to_bits(),
        y: label.rect.y.to_bits(),
        width: label.rect.width.to_bits(),
        height: label.rect.height.to_bits(),
    }
}

pub(crate) fn matched_previous_indices(
    previous: &[OverlayLabel],
    current: &[OverlayLabel],
    old_count: usize,
) -> SmallVec<[Option<usize>; 128]> {
    let mut matched = SmallVec::new();
    matched.resize(current.len(), None);
    let previous_limit = previous.len().min(old_count);
    let mut previous_cursor = 0;
    let remains_in_order = current.iter().enumerate().all(|(current_index, label)| {
        while previous_cursor < previous_limit
            && label_identity(&previous[previous_cursor]) != label_identity(label)
        {
            previous_cursor += 1;
        }
        if previous_cursor == previous_limit {
            return false;
        }
        matched[current_index] = Some(previous_cursor);
        previous_cursor += 1;
        true
    });
    if remains_in_order {
        return matched;
    }

    matched.fill(None);
    let mut previous_by_identity: HashMap<LabelIdentity<'_>, SmallVec<[usize; 2]>> =
        HashMap::with_capacity(previous_limit);
    for (index, label) in previous.iter().take(previous_limit).enumerate() {
        previous_by_identity
            .entry(label_identity(label))
            .or_default()
            .push(index);
    }
    for (index, label) in current.iter().enumerate() {
        let Some(candidates) = previous_by_identity.get_mut(&label_identity(label)) else {
            continue;
        };
        matched[index] = candidates.pop();
    }
    matched
}

/// Place each matched native slot at its current label's index. Unmatched
/// positions may reuse any remaining slot; callers must rebuild their content.
/// Matches must be unique, in bounds, and no longer than `slots`.
pub(crate) fn reorder_matched_slots<T>(slots: &mut [T], matches: &[Option<usize>]) {
    let Some(first_moved) = matches
        .iter()
        .enumerate()
        .position(|(index, previous)| previous.is_some_and(|previous| previous != index))
    else {
        return;
    };

    let mut old_at_position: SmallVec<[usize; 128]> = (0..slots.len()).collect();
    let mut position_of_old = old_at_position.clone();
    for (target, previous) in matches.iter().enumerate().skip(first_moved) {
        let Some(previous) = *previous else {
            continue;
        };
        let source = position_of_old[previous];
        if source != target {
            let displaced = old_at_position[target];
            slots.swap(target, source);
            old_at_position.swap(target, source);
            position_of_old[previous] = target;
            position_of_old[displaced] = source;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Rect, overlay::LabelStyle};
    #[test]
    fn label_identity_survives_prefix_and_overlap_z_changes() {
        let first = OverlayLabel::new(
            "adj",
            Rect::new(10.0, 20.0, 30.0, 24.0),
            LabelStyle::default(),
        );
        let mut raised = first.clone();
        raised.matched_prefix_len = 2;
        raised.z_index = 3;
        assert_eq!(label_identity(&first), label_identity(&raised));

        raised.rect.y += 1.0;
        assert_ne!(label_identity(&first), label_identity(&raised));
    }

    #[test]
    fn editor_slot_survives_typing_and_filtering() {
        let hint = OverlayLabel::new(
            "sdf",
            Rect::new(10.0, 20.0, 30.0, 24.0),
            LabelStyle::default(),
        );
        let mut editor = hint.clone();
        editor.text = "".into();
        editor.edit = Some(
            crate::api::overlay::LabelEdit::try_from(crate::api::text_edit::Selection::default())
                .unwrap(),
        );
        let mut typed = editor.clone();
        typed.text = "s".into();
        let previous = [hint, editor];
        assert_eq!(
            matched_previous_indices(&previous, std::slice::from_ref(&typed), 2).as_slice(),
            [Some(1)]
        );
        assert_ne!(previous[1].text, typed.text);
        assert_ne!(label_identity(&previous[0]), label_identity(&typed));
        let mut point = typed.clone();
        point.edit = None;
        point.scroll_to_cursor = true;
        assert_eq!(label_identity(&typed), label_identity(&point));
    }

    #[test]
    fn native_slots_follow_filtered_and_overlap_reordered_labels() {
        let label = |text: &str, x: f64| {
            OverlayLabel::new(text, Rect::new(x, 20.0, 30.0, 24.0), LabelStyle::default())
        };
        let previous = vec![label("aa", 10.0), label("ab", 20.0), label("ac", 30.0)];
        let mut filtered = vec![previous[0].clone(), previous[2].clone()];
        filtered[0].matched_prefix_len = 1;
        filtered[1].matched_prefix_len = 1;
        assert_eq!(
            matched_previous_indices(&previous, &filtered, previous.len()).as_slice(),
            [Some(0), Some(2)]
        );

        filtered.swap(0, 1);
        filtered[0].z_index = 3;
        assert_eq!(
            matched_previous_indices(&previous, &filtered, previous.len()).as_slice(),
            [Some(2), Some(0)]
        );
    }

    #[test]
    fn reordering_preserves_matched_slots_and_reuses_only_unmatched_slots() {
        for (old_count, matches) in [
            (0, vec![]),
            (0, vec![None, None, None]),
            (3, vec![]),
            (3, vec![Some(0), Some(1), Some(2)]),
            (3, vec![Some(0), Some(1), Some(2), None]),
            (3, vec![Some(2), Some(0)]),
            (3, vec![Some(1), Some(2), Some(0)]),
            (4, vec![None, Some(3)]),
            (3, vec![None, Some(0), Some(2), None, Some(1)]),
        ] {
            let count = old_count.max(matches.len());
            let mut slots: Vec<_> = (0..count).collect();
            reorder_matched_slots(&mut slots, &matches);
            for (index, previous) in matches.iter().enumerate() {
                if let Some(previous) = previous {
                    assert_eq!(slots[index], *previous, "{matches:?}");
                }
            }
            // Every native owner survives exactly once, including unused slots
            // that the renderer will detach after the permutation.
            slots.sort_unstable();
            assert!(slots.into_iter().eq(0..count), "{matches:?}");
        }
    }
}
