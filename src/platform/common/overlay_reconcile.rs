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
        text: if label.edit.is_some() {
            ""
        } else {
            &label.text
        },
        editing: label.edit.is_some(),
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
}
