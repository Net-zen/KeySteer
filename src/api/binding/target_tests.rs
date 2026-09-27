use super::*;

#[test]
fn window_target_bindings_roundtrip_and_preserve_held_semantics() {
    for base in [
        "window",
        "window_quick",
        "window_editor",
        "window_restore",
        "window_tab",
        "window_overlap_next",
        "window_overlap_previous",
        "window_activate_next",
        "window_activate_previous",
        "window_left",
        "window_size",
        "window_close",
        "window_maximize",
        "window_minimize",
        "size_cycle",
        "window_center",
        "window_screen_next",
        "window_volume_up",
        "window_audio_next",
        "window_select",
        "window_split_left",
        "window_ratio_right",
        "window_tab_remove",
        "move_window next",
        "move_window 2",
    ] {
        for source in ["active", "mouse"] {
            let text = format!("{base} {source}");
            let parsed = Binding::parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(parsed.canonical(), text);
            assert_eq!(parsed.is_held(), Binding::parse(base).unwrap().is_held());
            assert_eq!(parsed.mode(), Binding::parse(base).unwrap().mode());
        }
    }
    for invalid in [
        "window_close active mouse",
        "window_left target=active",
        "window active active",
        "window_undo mouse",
        "window_redo active",
        "window_system_volume_up mouse",
        "window_save_layout active",
        "move_window 0 active",
        "move_window invalid mouse",
        "window_close keyboard",
    ] {
        assert!(Binding::parse(invalid).is_err(), "{invalid}");
    }
    assert_eq!(
        Binding::parse("exec echo active").unwrap().canonical(),
        "exec echo active"
    );
}
