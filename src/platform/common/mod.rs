//! Infrastructure shared by native backends.
#![forbid(unsafe_code)]

#[cfg(any(target_os = "macos", test))]
pub(crate) mod accessibility_window;

pub(crate) mod window_visibility;

pub(crate) mod app_info;
pub(crate) mod audio_worker;
pub(crate) mod character_candidates;
pub(crate) mod contour;
pub(crate) mod display_target;
pub(crate) mod disposition_mailbox;
pub(crate) mod event_queue;
pub(crate) mod frame_clock;
pub(crate) mod image_tiles;
#[cfg(any(target_os = "windows", test))]
pub(crate) mod overlay_damage;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod overlay_reconcile;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod point_sample;
pub(crate) mod scan_accumulator;
pub(crate) mod scan_fusion;
pub(crate) mod scan_mailbox;
pub(crate) mod scan_progress;
// Run the safe macOS source-lifecycle regressions on every host.
#[cfg(all(test, not(target_os = "macos")))]
#[path = "../macos/scan_workers.rs"]
mod macos_scan_workers;
pub(crate) mod scroll_worker;
pub(crate) mod spatial_index;
pub(crate) mod text_segmentation;
pub(crate) mod update;
pub(crate) mod window_geometry;
pub(crate) mod window_placement;
pub(crate) mod window_session;
mod window_tab_model;
mod window_tabs;
#[cfg(all(test, any(target_os = "windows", target_os = "macos")))]
pub(crate) use window_tabs::Grouped as WindowGroupsProbe;

/// Build a native reverse-key table once, preserving the first alias.
pub(crate) fn build_key_map<const N: usize>(
    bindings: &[(u16, &str)],
) -> [Option<crate::api::Key>; N] {
    use crate::api::Key;

    let mut keys = std::array::from_fn(|_| None);
    // The first binding wins for aliases, including an invalid first name.
    // Walk the table once instead of searching it for every possible code.
    for &(code, name) in bindings.iter().rev() {
        if let Some(slot) = keys.get_mut(usize::from(code)) {
            *slot = Key::new(name).ok();
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    #[test]
    fn reverse_mapping_preserves_first_alias_invalid_names_and_empty_codes() {
        let bindings = [(1, ""), (1, "a"), (2, "q"), (2, "w"), (9, "z")];
        let keys = super::build_key_map::<4>(&bindings);
        assert_eq!(keys[0], None);
        assert_eq!(keys[1], None);
        assert_eq!(keys[2].as_ref().map(crate::api::Key::as_str), Some("q"));
        assert_eq!(keys[3], None);
        assert!(super::build_key_map::<0>(&bindings).is_empty());
    }
}
