#[test]
fn text_select_unambiguous_digits_need_no_separator_in_words_or_characters() {
    let mut config = Config::default();
    config.text_select.lifecycle.after_copy = crate::api::LifecycleAction::Keep;
    let mut engine = text_input_engine(config);
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine
        .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
        .unwrap();
    let id = log.lock().unwrap().scan_requests.last().unwrap().id;
    let text = (1..=120)
        .map(|i| format!("word{i:02}"))
        .collect::<Vec<_>>()
        .join(" ");
    engine
        .handle_backend_event(
            BackendEvent::UiScanned(crate::api::UiScanResult {
                id,
                status: UiScanStatus::Success,
                retired: vec![],
                targets: vec![crate::api::UiTarget::recognized_text(
                    Rect::new(10., 10., 700., 20.),
                    text,
                )],
            }),
            &mut backend,
        )
        .unwrap();
    point_keys(&mut engine, &mut backend, &["a"]);
    point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
    for expected in ["word08 word09", "or"] {
        point_keys(&mut engine, &mut backend, &["backspace"]);
        point_keys(&mut engine, &mut backend, &["8"]);
        point_keys(&mut engine, &mut backend, &["9"]);
        {
            let record = log.lock().unwrap();
            let scene = record.scenes.last().unwrap();
            assert!(
                scene
                    .labels
                    .iter()
                    .any(|label| label.text.as_str() == expected)
            );
            assert!(
                scene
                    .labels
                    .iter()
                    .any(|label| label.edit.is_some() && label.text.is_empty())
            );
        }
        point_keys(&mut engine, &mut backend, &["enter"]);
        assert_eq!(log.lock().unwrap().copied_text.last().unwrap(), expected);
        point_keys(&mut engine, &mut backend, &["left_ctrl", "a"]);
        point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
    }
    assert!(engine.input.pressed.is_empty());
    assert_eq!(log.lock().unwrap().scan_requests.len(), 1);
}

#[test]
fn text_select_numeric_ranges_and_search_use_fixed_ids_across_pages() {
    for copy_key in ["enter", "f9"] {
        let mut config = Config::default();
        config.text_select.lifecycle.after_copy = crate::api::LifecycleAction::Keep;
        config.text_select.bindings.insert(
            "f9".into(),
            Binding::TextSelect(crate::api::text_select::Action::Copy),
        );
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine
            .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
            .unwrap();
        let id = log.lock().unwrap().scan_requests.last().unwrap().id;
        let mut words: Vec<_> = (1..=35).map(|index| format!("word{index:02}")).collect();
        words[30] = "needle".into();
        engine
            .handle_backend_event(
                BackendEvent::UiScanned(crate::api::UiScanResult {
                    id,
                    retired: vec![],
                    status: UiScanStatus::Success,
                    targets: vec![crate::api::UiTarget::recognized_text(
                        Rect::new(10., 10., 700., 20.),
                        words.join(" "),
                    )],
                }),
                &mut backend,
            )
            .unwrap();
        point_keys(&mut engine, &mut backend, &["a"]);
        point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
        point_keys(&mut engine, &mut backend, &["backspace"]);
        point_keys(&mut engine, &mut backend, &["tab"]);
        for key in ["3", "0", "-", "3", "2", "space", "1", "-", "2"] {
            point_keys(&mut engine, &mut backend, &[key]);
        }
        point_keys(&mut engine, &mut backend, &[copy_key]);
        assert_eq!(
            log.lock().unwrap().copied_text,
            ["word30 needle word32 word01 word02"]
        );
        point_keys(&mut engine, &mut backend, &["backspace"]);
        for key in ["n", "e", "e", "d", "l", "e"] {
            point_keys(&mut engine, &mut backend, &[key]);
        }
        assert!(
            log.lock()
                .unwrap()
                .scenes
                .last()
                .unwrap()
                .labels
                .iter()
                .any(|label| label.text == "31")
        );
        for key in ["space", "3", "1"] {
            point_keys(&mut engine, &mut backend, &[key]);
        }
        point_keys(&mut engine, &mut backend, &[copy_key]);
        assert_eq!(log.lock().unwrap().copied_text.last().unwrap(), "needle");
        assert!(
            log.lock()
                .unwrap()
                .scenes
                .last()
                .unwrap()
                .labels
                .iter()
                .any(|label| label.text == "✓ 31")
        );
        point_keys(&mut engine, &mut backend, &["left_ctrl", "x"]);
        point_keys(&mut engine, &mut backend, &[copy_key]);
        let record = log.lock().unwrap();
        assert_eq!(
            record.copied_text,
            ["word30 needle word32 word01 word02", "needle", "needle"]
        );
        assert_eq!(record.scan_requests.len(), 1);
        assert!(record.text_capture);
        assert!(engine.input.pressed.is_empty());
    }
}

#[test]
fn text_select_copies_words_and_characters_in_label_input_order() {
    let mut config = Config::default();
    config.text_select.lifecycle.after_copy = crate::api::LifecycleAction::Keep;
    let mut engine = text_input_engine(config);
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine
        .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
        .unwrap();
    let id = log.lock().unwrap().scan_requests.last().unwrap().id;
    engine
        .handle_backend_event(
            BackendEvent::UiScanned(crate::api::UiScanResult {
                id,
                retired: vec![],
                status: UiScanStatus::Success,
                targets: vec![crate::api::UiTarget::recognized_text(
                    Rect::new(10., 10., 200., 20.),
                    "one two three".into(),
                )],
            }),
            &mut backend,
        )
        .unwrap();
    point_keys(&mut engine, &mut backend, &["a"]);
    point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
    for expected in ["three one", "rt"] {
        point_keys(&mut engine, &mut backend, &["backspace"]);
        point_keys(&mut engine, &mut backend, &["3"]);
        point_keys(&mut engine, &mut backend, &["space"]);
        point_keys(&mut engine, &mut backend, &["1"]);
        point_keys(&mut engine, &mut backend, &["space"]);
        point_keys(&mut engine, &mut backend, &["left_ctrl", "x"]);
        assert!(
            log.lock()
                .unwrap()
                .scenes
                .last()
                .unwrap()
                .labels
                .iter()
                .any(|label| label.text.as_str() == expected)
        );
        point_keys(&mut engine, &mut backend, &["enter"]);
        assert_eq!(log.lock().unwrap().copied_text.last().unwrap(), expected);
        assert_eq!(engine.active_mode(), &ModeId::text_select());
        point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
    }
    point_keys(&mut engine, &mut backend, &["left_alt", "backspace"]);
    point_keys(&mut engine, &mut backend, &["enter"]);
    let record = log.lock().unwrap();
    assert_eq!(record.copied_text, ["three one", "rt", "three one"]);
    assert_eq!(record.scan_requests.len(), 1);
}

#[test]
fn text_select_prompt_resolves_configured_character_actions_before_typing() {
    for (symbol, physical, modifier) in [
        ('?', "/", Some("left_shift")),
        (':', ";", Some("right_shift")),
        ('€', "e", Some("right_alt")),
        ('@', "x", None),
    ] {
        let mut config = Config::default();
        config.text_select.bindings.remove("?");
        config
            .text_select
            .bindings
            .insert(symbol.to_string(), Binding::KeyHelp);
        config.text_select.bindings.insert(
            "!".into(),
            Binding::TextSelect(crate::api::text_select::Action::ClearInput),
        );
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine
            .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
            .unwrap();
        for query in ["", "abc"] {
            for key in query.chars() {
                point_keys(&mut engine, &mut backend, &[&key.to_string()]);
            }
            for visible in [true, false] {
                if let Some(modifier) = modifier {
                    engine
                        .handle_backend_event(key_down(modifier), &mut backend)
                        .unwrap();
                }
                engine
                    .handle_backend_event(character_down(physical, symbol), &mut backend)
                    .unwrap();
                assert_eq!(engine.overlay.key_help_visible, visible, "{symbol}");
                let BackendEvent::Input(mut repeat) = character_down(physical, symbol) else {
                    unreachable!()
                };
                repeat.repeat = true;
                engine
                    .handle_backend_event(BackendEvent::Input(repeat), &mut backend)
                    .unwrap();
                assert_eq!(engine.overlay.key_help_visible, visible);
                engine
                    .handle_backend_event(key_up(physical), &mut backend)
                    .unwrap();
                if let Some(modifier) = modifier {
                    engine
                        .handle_backend_event(key_up(modifier), &mut backend)
                        .unwrap();
                }
                assert!(
                    log.lock()
                        .unwrap()
                        .scenes
                        .last()
                        .unwrap()
                        .labels
                        .iter()
                        .any(|label| label.edit.is_some() && label.text.as_str() == query)
                );
                assert!(engine.input.pressed.is_empty());
            }
        }
        // Literal matching applies to other configured actions as well.
        for event in [
            key_down("left_shift"),
            character_down("1", '!'),
            key_up("1"),
            key_up("left_shift"),
        ] {
            engine.handle_backend_event(event, &mut backend).unwrap();
        }
        assert!(
            log.lock()
                .unwrap()
                .scenes
                .last()
                .unwrap()
                .labels
                .iter()
                .any(|label| label.edit.is_some() && label.text.is_empty())
        );
        assert!(log.lock().unwrap().copied_text.is_empty());
        assert_eq!(engine.active_mode(), &ModeId::text_select());
    }
}

#[test]
fn text_select_unbound_or_disabled_symbol_remains_input_after_help_is_remapped() {
    for disabled in [false, true] {
        let mut config = Config::default();
        config.text_select.bindings.remove("?");
        if disabled {
            config
                .text_select
                .bindings
                .insert("?".into(), Binding::Disabled);
        }
        config
            .text_select
            .bindings
            .insert("f1".into(), Binding::KeyHelp);
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine
            .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
            .unwrap();
        for event in [
            key_down("left_shift"),
            character_down("/", '?'),
            key_up("/"),
            key_up("left_shift"),
        ] {
            engine.handle_backend_event(event, &mut backend).unwrap();
        }
        assert!(!engine.overlay.key_help_visible);
        assert!(
            log.lock()
                .unwrap()
                .scenes
                .last()
                .unwrap()
                .labels
                .iter()
                .any(|label| label.edit.is_some() && label.text.as_str() == "?")
        );
        point_keys(&mut engine, &mut backend, &["f1"]);
        assert!(engine.overlay.key_help_visible);
        assert!(
            log.lock()
                .unwrap()
                .scenes
                .last()
                .unwrap()
                .labels
                .iter()
                .any(|label| label.edit.is_some() && label.text.as_str() == "?")
        );
    }
}

#[test]
fn text_select_empty_prompt_controls_switch_to_editing_and_can_be_remapped() {
    use crate::api::text_select::Action;
    for remapped in [false, true] {
        let mut config = Config::default();
        config.text_select.lifecycle.after_copy = crate::api::LifecycleAction::Keep;
        if remapped {
            config.text_select.bindings.remove("backspace");
            config.text_select.bindings.remove("ctrl+a");
            config
                .text_select
                .bindings
                .insert("f7".into(), Binding::TextSelect(Action::ClearSelection));
            config
                .text_select
                .bindings
                .insert("f8".into(), Binding::TextSelect(Action::SelectAll));
        }
        let clear: &[&str] = if remapped { &["f7"] } else { &["backspace"] };
        let all: &[&str] = if remapped {
            &["f8"]
        } else {
            &["left_ctrl", "a"]
        };
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine
            .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
            .unwrap();
        // A clear on an empty, unfinished scan must not freeze out its results.
        point_keys(&mut engine, &mut backend, clear);
        let id = log.lock().unwrap().scan_requests.last().unwrap().id;
        let selected_text = "字".repeat(94);
        let text = selected_text.as_str();
        engine
            .handle_backend_event(
                BackendEvent::UiScanned(crate::api::UiScanResult {
                    id,
                    retired: vec![],
                    status: UiScanStatus::Success,
                    targets: vec![crate::api::UiTarget::recognized_text(
                        Rect::new(10., 10., 200., 20.),
                        text.into(),
                    )],
                }),
                &mut backend,
            )
            .unwrap();
        point_keys(&mut engine, &mut backend, all);
        point_keys(&mut engine, &mut backend, &["enter"]);
        assert_eq!(log.lock().unwrap().copied_text, [text]);
        point_keys(&mut engine, &mut backend, clear);
        point_keys(&mut engine, &mut backend, &["enter"]);
        assert_eq!(log.lock().unwrap().copied_text.len(), 1);
        // Nonempty query: all selects input, clear deletes it, neither changes
        // the cleared base selection nor navigates to another level.
        point_keys(&mut engine, &mut backend, &["h"]);
        point_keys(&mut engine, &mut backend, &["e"]);
        point_keys(&mut engine, &mut backend, all);
        {
            let record = log.lock().unwrap();
            let input = record
                .scenes
                .last()
                .unwrap()
                .labels
                .iter()
                .find(|l| l.edit.is_some())
                .unwrap();
            let selection: crate::api::text_edit::Selection = input.edit.unwrap().into();
            assert_eq!(input.text, "he");
            assert_eq!(selection.range(), 0..2);
        }
        point_keys(&mut engine, &mut backend, clear);
        point_keys(&mut engine, &mut backend, &["enter"]);
        assert_eq!(log.lock().unwrap().copied_text.len(), 1);
        // Empty again: restore all; check both refinement levels and repeat.
        point_keys(&mut engine, &mut backend, all);
        for expected in ["Words", "Characters"] {
            point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
            let before = log.lock().unwrap().copied_text.len();
            point_keys(&mut engine, &mut backend, &["tab"]);
            point_keys(&mut engine, &mut backend, clear);
            point_keys(&mut engine, &mut backend, &["enter"]);
            assert_eq!(log.lock().unwrap().copied_text.len(), before);
            // Decimal numbers remain pending until a separator or copy/refine.
            // Take an actual two-digit number's prefix from this page.
            let prefix = log
                .lock()
                .unwrap()
                .scenes
                .last()
                .unwrap()
                .labels
                .iter()
                .find_map(|label| {
                    let code = label.text.as_str();
                    (code.len() == 2 && code.chars().all(|c| c.is_ascii_digit()))
                        .then(|| code[..1].to_ascii_lowercase())
                })
                .expect("the second page contains two-digit numbers");
            point_keys(&mut engine, &mut backend, &[prefix.as_str()]);
            point_keys(&mut engine, &mut backend, all);
            point_keys(&mut engine, &mut backend, clear);
            point_keys(&mut engine, &mut backend, &["enter"]);
            assert_eq!(log.lock().unwrap().copied_text.len(), before);
            point_keys(&mut engine, &mut backend, all);
            point_keys(&mut engine, &mut backend, &[prefix.as_str()]);
            point_keys(&mut engine, &mut backend, all);
            point_keys(&mut engine, &mut backend, clear);
            point_keys(&mut engine, &mut backend, &["enter"]);
            assert_eq!(log.lock().unwrap().copied_text.len(), before + 1);
            assert_eq!(log.lock().unwrap().copied_text.last().unwrap(), text);
            assert_eq!(
                engine
                    .registry
                    .get(&ModeId::text_select())
                    .unwrap()
                    .indicator_detail()
                    .as_deref(),
                Some(expected)
            );
        }
    }
}

#[test]
fn text_select_default_keys_override_live_prompt_edits_and_copy_only_after_success() {
    let mut engine = text_input_engine(Config::default());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine
        .activate(ModeId::normal(), Some(ModeId::idle()), &mut backend)
        .unwrap();
    point_keys(&mut engine, &mut backend, &["c"]);
    assert_eq!(engine.active_mode(), &ModeId::text_select());
    assert!(log.lock().unwrap().text_capture);
    let request = log.lock().unwrap().scan_requests.last().unwrap().clone();
    assert_eq!(request.strategy, crate::api::UiScanStrategy::Vision);
    engine
        .handle_backend_event(
            BackendEvent::UiScanned(crate::api::UiScanResult {
                id: request.id,
                status: UiScanStatus::Success,
                retired: vec![],
                targets: vec![crate::api::UiTarget::recognized_text(
                    Rect::new(10., 10., 200., 20.),
                    "hello world".into(),
                )],
            }),
            &mut backend,
        )
        .unwrap();
    point_keys(&mut engine, &mut backend, &["a"]);
    point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
    let detail = |engine: &Engine| {
        engine
            .registry
            .get(&ModeId::text_select())
            .unwrap()
            .indicator_detail()
            .unwrap()
    };
    assert_eq!(detail(&engine), "Words");
    point_keys(&mut engine, &mut backend, &["right_alt", "c"]);
    assert_eq!(detail(&engine), "Characters");
    point_keys(&mut engine, &mut backend, &["left_alt", "backspace"]);
    assert_eq!(detail(&engine), "Words");
    point_keys(&mut engine, &mut backend, &["left_alt", "backspace"]);
    assert_eq!(detail(&engine), "Regions");
    point_keys(&mut engine, &mut backend, &["left_ctrl", "backspace"]);
    point_keys(&mut engine, &mut backend, &["enter"]);
    assert!(log.lock().unwrap().copied_text.is_empty());
    point_keys(&mut engine, &mut backend, &["a"]);
    point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
    log.lock().unwrap().fail_copy = true;
    point_keys(&mut engine, &mut backend, &["enter"]);
    assert_eq!(engine.active_mode(), &ModeId::text_select());
    log.lock().unwrap().fail_copy = false;
    point_keys(&mut engine, &mut backend, &["enter"]);
    assert_eq!(engine.active_mode(), &ModeId::normal());
    let record = log.lock().unwrap();
    assert_eq!(record.copied_text.last().unwrap(), "hello world");
    assert_eq!(record.scan_requests.len(), 1);
    assert_eq!(record.clicks, 0);
    assert!(!record.text_capture);
}

#[test]
fn text_select_copy_shortcut_and_refinement_are_independent_of_primary() {
    for primary in ["left_win", "left_alt", "left_ctrl"] {
        let config = Config::parse(&format!(
            "[key_aliases.windows]\nPrimary = '{primary}'\n[key_aliases.macos]\nPrimary = '{primary}'\n[key_aliases.linux]\nPrimary = '{primary}'\n[ui_hint.search_edit_keys]\n'ctrl+shift+c' = 'copy'"
        )).unwrap();
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine
            .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
            .unwrap();
        let request = log.lock().unwrap().scan_requests.last().unwrap().clone();
        engine
            .handle_backend_event(
                BackendEvent::UiScanned(crate::api::UiScanResult {
                    id: request.id,
                    status: UiScanStatus::Success,
                    retired: vec![],
                    targets: vec![crate::api::UiTarget::recognized_text(
                        Rect::new(10., 10., 200., 20.),
                        "hello world".into(),
                    )],
                }),
                &mut backend,
            )
            .unwrap();
        point_keys(&mut engine, &mut backend, &["a"]);
        point_keys(&mut engine, &mut backend, &[primary, "a"]);
        // Shared query-copy shortcuts are not active in Text Select.
        point_keys(&mut engine, &mut backend, &["left_ctrl", "left_shift", "c"]);
        let mut copied = Vec::new();
        assert!(log.lock().unwrap().copied_text.is_empty());
        let copy_modifier = if cfg!(target_os = "macos") {
            "right_win"
        } else {
            "right_ctrl"
        };
        point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
        assert_eq!(
            engine
                .registry
                .get(&ModeId::text_select())
                .unwrap()
                .indicator_detail()
                .as_deref(),
            Some("Words")
        );
        assert_eq!(
            log.lock().unwrap().copied_text,
            copied,
            "refinement must not copy"
        );
        log.lock().unwrap().fail_copy = true;
        point_keys(&mut engine, &mut backend, &[copy_modifier, "c"]);
        assert_eq!(engine.active_mode(), &ModeId::text_select());
        assert_eq!(log.lock().unwrap().copied_text, copied);
        log.lock().unwrap().fail_copy = false;
        point_keys(&mut engine, &mut backend, &[copy_modifier, "c"]);
        copied.push("hello world");
        assert_eq!(log.lock().unwrap().copied_text, copied);
        assert_eq!(engine.active_mode(), &ModeId::normal());
    }
}

#[test]
fn text_select_grouped_copy_keys_always_copy_checked_text() {
    use crate::api::text_select::Action;
    for key in ["ctrl+c", "cmd+c", "f9"] {
        let config = Config::parse(&format!(
            "[text_select.bindings]\n'{key} enter' = 'text_copy'\n'alt+c' = 'text_refine'\n'ctrl+a' = 'text_select_all'\n[ui_hint.search_edit_keys]\n'ctrl+shift+c' = 'copy'\n[text_select.lifecycle]\nafter_copy = 'keep'"
        )).unwrap();
        let chord =
            crate::api::KeyChord::parse_with_aliases(key, config.resolved_key_aliases()).unwrap();
        let physical: Vec<_> = chord.injection_keys().iter().map(Key::as_str).collect();
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine
            .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
            .unwrap();
        point_keys(&mut engine, &mut backend, &physical);
        assert!(log.lock().unwrap().copied_text.is_empty());
        let id = log.lock().unwrap().scan_requests.last().unwrap().id;
        engine
            .handle_backend_event(
                BackendEvent::UiScanned(crate::api::UiScanResult {
                    id,
                    status: UiScanStatus::Success,
                    retired: vec![],
                    targets: vec![crate::api::UiTarget::recognized_text(
                        Rect::new(10., 10., 200., 20.),
                        "hello world".into(),
                    )],
                }),
                &mut backend,
            )
            .unwrap();
        point_keys(&mut engine, &mut backend, &["a"]);
        assert!(
            engine
                .key_help_entries()
                .iter()
                .any(|entry| entry.ends_with(Action::Copy.as_str()))
        );
        // A query and its highlighted input characters never change the copy target.
        for keys in [physical.as_slice(), &["enter"]] {
            point_keys(&mut engine, &mut backend, keys);
        }
        assert_eq!(log.lock().unwrap().copied_text, ["hello world"; 2]);
        point_keys(&mut engine, &mut backend, &["left_ctrl", "a"]);
        for keys in [physical.as_slice(), &["enter"]] {
            point_keys(&mut engine, &mut backend, keys);
        }
        assert_eq!(log.lock().unwrap().copied_text, ["hello world"; 4]);
        // Even a highlighted space still copies the checked OCR text.
        point_keys(&mut engine, &mut backend, &["right"]);
        point_keys(&mut engine, &mut backend, &["space"]);
        point_keys(&mut engine, &mut backend, &["left_shift", "left"]);
        for keys in [physical.as_slice(), &["enter"]] {
            point_keys(&mut engine, &mut backend, keys);
        }
        assert_eq!(log.lock().unwrap().copied_text, ["hello world"; 6]);
        assert_eq!(engine.active_mode(), &ModeId::text_select());
        point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
        assert!(
            engine
                .key_help_entries()
                .iter()
                .any(|entry| entry.ends_with(Action::Copy.as_str()))
        );
        for keys in [physical.as_slice(), &["enter"]] {
            point_keys(&mut engine, &mut backend, keys);
        }
        point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
        for keys in [physical.as_slice(), &["enter"]] {
            point_keys(&mut engine, &mut backend, keys);
        }
        assert_eq!(log.lock().unwrap().copied_text, ["hello world"; 10]);
        assert_eq!(engine.active_mode(), &ModeId::text_select());
        assert_eq!(log.lock().unwrap().scan_requests.len(), 1);
    }
}

#[test]
fn text_select_clear_input_clears_the_whole_query_without_copying_and_can_be_remapped() {
    for primary in ["left_alt", "left_win", "left_ctrl"] {
        for remapped in [false, true] {
            let mut config = Config::parse(&format!(
                "[key_aliases]\nPrimary = '{primary}'\n[key_aliases.windows]\nPrimary = '{primary}'\n[ui_hint.search_edit_keys]\n'ctrl+shift+x' = 'cut'\n[text_select.lifecycle]\nafter_copy = 'keep'"
            )).unwrap();
            if remapped {
                config.text_select.bindings.remove("ctrl+x");
                config.text_select.bindings.insert(
                    "f8".into(),
                    Binding::TextSelect(crate::api::text_select::Action::ClearInput),
                );
            }
            let mut engine = text_input_engine(config);
            let (mut backend, log) = FakeBackend::new(Vec::new());
            engine.screens = backend.screens().unwrap();
            engine
                .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
                .unwrap();
            let id = log.lock().unwrap().scan_requests.last().unwrap().id;
            engine
                .handle_backend_event(
                    BackendEvent::UiScanned(crate::api::UiScanResult {
                        id,
                        retired: vec![],
                        status: UiScanStatus::Success,
                        targets: vec![crate::api::UiTarget::recognized_text(
                            Rect::new(10., 10., 200., 20.),
                            "hello world".into(),
                        )],
                    }),
                    &mut backend,
                )
                .unwrap();
            for key in ["h", "e", "l", "l", "o"] {
                point_keys(&mut engine, &mut backend, &[key]);
            }
            point_keys(&mut engine, &mut backend, &["left_shift", "left"]);
            point_keys(&mut engine, &mut backend, &["left_ctrl", "left_shift", "x"]);
            if remapped {
                point_keys(&mut engine, &mut backend, &["left_ctrl", "x"]);
            }
            assert!(
                log.lock()
                    .unwrap()
                    .scenes
                    .last()
                    .unwrap()
                    .labels
                    .iter()
                    .any(|label| label.edit.is_some() && label.text.as_str() == "hello")
            );
            let clear = if remapped {
                &["f8"][..]
            } else {
                &["right_ctrl", "x"][..]
            };
            point_keys(&mut engine, &mut backend, clear);
            assert!(
                log.lock()
                    .unwrap()
                    .scenes
                    .last()
                    .unwrap()
                    .labels
                    .iter()
                    .any(|label| label.edit.is_some()
                        && label.text.is_empty()
                        && crate::api::text_edit::Selection::from(label.edit.unwrap())
                            == crate::api::text_edit::Selection::default())
            );
            assert!(log.lock().unwrap().copied_text.is_empty());
            assert_eq!(engine.active_mode(), &ModeId::text_select());
            point_keys(&mut engine, &mut backend, &["left_ctrl", "a"]);
            // Clearing an already-empty prompt keeps the checked panel text.
            point_keys(&mut engine, &mut backend, clear);
            point_keys(&mut engine, &mut backend, &["enter"]);
            assert_eq!(log.lock().unwrap().copied_text, ["hello world"]);
            assert_eq!(log.lock().unwrap().scan_requests.len(), 1);
        }
    }
}

#[test]
fn text_select_copy_lifecycle_runs_after_success_and_keep_preserves_selection() {
    for action in ["normal", "idle", "keep", "restart", "return"] {
        let mut config = Config::default();
        config.text_select.lifecycle.after_copy =
            crate::api::LifecycleAction::parse(action).unwrap();
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine
            .activate(ModeId::text_select(), Some(ModeId::grid()), &mut backend)
            .unwrap();
        let request = log.lock().unwrap().scan_requests.last().unwrap().clone();
        engine
            .handle_backend_event(
                BackendEvent::UiScanned(crate::api::UiScanResult {
                    id: request.id,
                    status: UiScanStatus::Success,
                    retired: vec![],
                    targets: vec![crate::api::UiTarget::recognized_text(
                        Rect::new(10., 10., 200., 20.),
                        "hello world".into(),
                    )],
                }),
                &mut backend,
            )
            .unwrap();
        point_keys(&mut engine, &mut backend, &["a"]);
        point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
        log.lock().unwrap().fail_copy = true;
        point_keys(&mut engine, &mut backend, &["enter"]);
        assert_eq!(
            engine.active_mode(),
            &ModeId::text_select(),
            "{action}: failed copy"
        );
        assert_eq!(log.lock().unwrap().scan_requests.len(), 1);
        log.lock().unwrap().fail_copy = false;
        point_keys(&mut engine, &mut backend, &["enter"]);
        let expected = match action {
            "keep" | "restart" => ModeId::text_select(),
            "return" => ModeId::grid(),
            mode => ModeId::new(mode).unwrap(),
        };
        assert_eq!(engine.active_mode(), &expected, "{action}");
        {
            let record = log.lock().unwrap();
            assert_eq!(record.copied_text, ["hello world"]);
            assert_eq!(
                record.scan_requests.len(),
                if action == "restart" { 2 } else { 1 }
            );
            assert_eq!(record.clicks, 0);
            assert_eq!(record.text_capture, matches!(action, "keep" | "restart"));
        }
        if matches!(action, "keep" | "restart") {
            point_keys(&mut engine, &mut backend, &["enter"]);
            assert_eq!(
                log.lock().unwrap().copied_text.len(),
                if action == "keep" { 2 } else { 1 }
            );
        }
    }
}

#[test]
fn text_select_primary_q_exits_without_copying_empty_or_selected_text() {
    for config in [
        Config::default(),
        Config::parse(include_str!("../../../../keysteer.default.toml")).unwrap(),
        Config::parse(
            r"
            [key_aliases.windows]
            Primary = 'left_win'
            [key_aliases.macos]
            Primary = 'left_alt'
            [key_aliases.linux]
            Primary = 'left_alt'
        ",
        )
        .unwrap(),
    ] {
        let primary = config
            .resolved_key_aliases()
            .get("primary")
            .cloned()
            .unwrap_or_else(|| {
                if cfg!(target_os = "macos") {
                    "left_win"
                } else {
                    "left_ctrl"
                }
                .into()
            });
        for selected in [false, true] {
            let mut engine = text_input_engine(config.clone());
            let (mut backend, log) = FakeBackend::new(Vec::new());
            engine.screens = backend.screens().unwrap();
            engine
                .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
                .unwrap();
            let (badge, _) = engine.build_indicator(&ModeId::text_select()).unwrap();
            assert_eq!(badge.text, "Text Select");
            assert_eq!(badge.style.background, engine.palette.accent);
            let request = log.lock().unwrap().scan_requests.last().unwrap().clone();
            engine
                .handle_backend_event(
                    BackendEvent::UiScanned(crate::api::UiScanResult {
                        id: request.id,
                        status: UiScanStatus::Success,
                        retired: vec![],
                        targets: vec![crate::api::UiTarget::recognized_text(
                            Rect::new(10., 10., 200., 20.),
                            "hello world".into(),
                        )],
                    }),
                    &mut backend,
                )
                .unwrap();
            if selected {
                point_keys(&mut engine, &mut backend, &["a"]);
            }
            assert!(
                engine
                    .key_help_entries()
                    .iter()
                    .any(|entry| entry.contains("+q") && entry.ends_with("normal"))
            );
            point_keys(&mut engine, &mut backend, &[&primary, "q"]);
            assert_eq!(engine.active_mode(), &ModeId::normal());
            assert!(engine.input.pressed.is_empty());
            assert!(engine.quick_switch.pending.is_none());
            let record = log.lock().unwrap();
            assert!(record.copied_text.is_empty());
            assert!(!record.text_capture);
        }
    }
}

#[test]
fn text_select_space_is_input_and_remapped_controls_drive_help_and_navigation() {
    let mut config = Config::default();
    config.text_select.bindings.remove("ctrl+backspace");
    config
        .text_select
        .bindings
        .insert("f8".into(), Binding::SelectionReset);
    let mut engine = text_input_engine(config);
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine
        .activate(ModeId::normal(), Some(ModeId::idle()), &mut backend)
        .unwrap();
    point_keys(&mut engine, &mut backend, &["c"]);
    let request = log.lock().unwrap().scan_requests.last().unwrap().clone();
    engine
        .handle_backend_event(
            BackendEvent::UiScanned(crate::api::UiScanResult {
                id: request.id,
                status: UiScanStatus::Success,
                retired: vec![],
                targets: vec![crate::api::UiTarget::recognized_text(
                    Rect::new(10., 10., 200., 20.),
                    "hello world".into(),
                )],
            }),
            &mut backend,
        )
        .unwrap();
    point_keys(&mut engine, &mut backend, &["a"]);
    point_keys(&mut engine, &mut backend, &["space"]);
    assert!(
        log.lock()
            .unwrap()
            .scenes
            .last()
            .unwrap()
            .labels
            .iter()
            .any(|l| l.edit.is_some() && l.text.as_str() == "a ")
    );
    let help = engine.key_help_entries();
    assert!(
        help.iter()
            .any(|entry| entry.contains("f8") && entry.contains("first_level"))
    );
    assert!(!help.iter().any(|entry| entry.contains("ctrl+backspace")));
    assert!(
        !help
            .iter()
            .any(|entry| entry.starts_with("backspace") && entry.contains("text_clear_selection"))
    );
    point_keys(&mut engine, &mut backend, &["backspace"]);
    assert!(
        log.lock()
            .unwrap()
            .scenes
            .last()
            .unwrap()
            .labels
            .iter()
            .any(|l| l.edit.is_some() && l.text.as_str() == "a")
    );
    point_keys(&mut engine, &mut backend, &["f8"]);
    point_keys(&mut engine, &mut backend, &["enter"]);
    assert!(log.lock().unwrap().copied_text.is_empty());
    assert!(
        engine
            .key_help_entries()
            .iter()
            .any(|entry| entry.starts_with("backspace") && entry.contains("text_clear_selection"))
    );
    point_keys(&mut engine, &mut backend, &["a"]);
    point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
    point_keys(&mut engine, &mut backend, &["enter"]);
    assert_eq!(
        log.lock().unwrap().copied_text.last().map(String::as_str),
        Some("hello world")
    );
}

#[test]
fn grid_remapping_disables_old_controls_and_help_uses_the_effective_bindings() {
    for mode in [ModeId::grid(), ModeId::recursive_grid()] {
        let mut config = Config::default();
        config.recursive_grid.lifecycle.after_finish =
            crate::api::LifecycleAction::Mode(ModeId::normal());
        let bindings = if mode == ModeId::grid() {
            &mut config.grid.bindings
        } else {
            &mut config.recursive_grid.bindings
        };
        bindings.insert("space".into(), Binding::Disabled);
        bindings.insert("f8".into(), Binding::SelectionReset);
        bindings.insert("enter".into(), Binding::Disabled);
        bindings.insert("f9".into(), Binding::SelectionAccept);
        let mut engine = text_input_engine(config);
        let (mut backend, _) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine
            .activate(mode.clone(), Some(ModeId::normal()), &mut backend)
            .unwrap();
        let help = engine.key_help_entries();
        assert!(help.iter().any(|e| e == "f8  ·  first_level"));
        assert!(
            !help
                .iter()
                .any(|e| e.starts_with("space ") || e.starts_with("enter "))
        );
        point_keys(&mut engine, &mut backend, &["enter"]);
        assert_eq!(engine.active_mode(), &mode);
        point_keys(&mut engine, &mut backend, &["f9"]);
        assert_eq!(engine.active_mode(), &ModeId::normal());
    }
}

#[test]
fn text_select_overlap_modifiers_reach_live_prompt_and_preserve_selection() {
    for (configured, left, right) in [
        ("shift", "left_shift", "right_shift"),
        ("alt", "left_alt", "right_alt"),
    ] {
        let mut config = Config::default();
        config.ui_hint.overlap_cycle_key = configured.into();
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine
            .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
            .unwrap();
        let id = log.lock().unwrap().scan_requests.last().unwrap().id;
        engine
            .handle_backend_event(
                BackendEvent::UiScanned(crate::api::UiScanResult {
                    id,
                    status: UiScanStatus::Success,
                    retired: vec![],
                    targets: ["keep one", "keep two", "keep three", "other"]
                        .into_iter()
                        .enumerate()
                        .map(|(i, text)| {
                            crate::api::UiTarget::recognized_text(
                                Rect::new(100. + i as f64, 100., 80., 30.),
                                text.into(),
                            )
                        })
                        .collect(),
                }),
                &mut backend,
            )
            .unwrap();
        let labels = |base: bool| {
            let record = log.lock().unwrap();
            let scene = record.scenes.last().unwrap();
            let scene = if base {
                scene
                    .retained
                    .as_ref()
                    .map_or_else(|| scene.clone(), |r| r.scene.as_ref().clone())
            } else {
                scene.materialized()
            };
            let mut labels = scene
                .labels
                .iter()
                .filter(|label| {
                    label.edit.is_none() && label.text.len() <= 2 && label.z_index < 10_000
                })
                .map(|label| (label.text.clone(), label.z_index))
                .collect::<Vec<_>>();
            labels.sort_by_key(|(_, z)| *z);
            labels.into_iter().map(|(text, _)| text).collect::<Vec<_>>()
        };
        assert!(
            engine.key_help_entries().iter().any(
                |entry| entry.contains(configured) && entry.contains("cycle overlapping hints")
            )
        );
        for query in ["", "keep"] {
            for character in query.chars() {
                point_keys(&mut engine, &mut backend, &[&character.to_string()]);
            }
            engine.fire_due_timers(&mut backend).unwrap();
            let initial = labels(true);
            assert_eq!(
                initial.len(),
                if query.is_empty() { 4 } else { 3 },
                "{configured}: query {query}, scene: {:?}",
                log.lock()
                    .unwrap()
                    .scenes
                    .last()
                    .unwrap()
                    .materialized()
                    .labels
                    .iter()
                    .map(|label| (&label.text, label.edit.is_some(), label.z_index))
                    .collect::<Vec<_>>()
            );
            let preview = labels(false).last().unwrap().clone();
            engine
                .handle_backend_event(key_down(left), &mut backend)
                .unwrap();
            let switched = labels(true);
            assert_ne!(initial, switched, "{configured}: {query}");
            if !query.is_empty() {
                assert_eq!(labels(false).last().unwrap(), &preview);
            }
            let mut repeat = key_down(left);
            if let BackendEvent::Input(input) = &mut repeat {
                input.repeat = true;
            }
            engine.handle_backend_event(repeat, &mut backend).unwrap();
            engine
                .handle_backend_event(key_down(right), &mut backend)
                .unwrap();
            engine
                .handle_backend_event(key_up(left), &mut backend)
                .unwrap();
            assert_eq!(labels(true), switched);
            engine
                .handle_backend_event(key_up(right), &mut backend)
                .unwrap();
            assert_eq!(labels(true), initial);
        }
        point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
        assert!(
            !engine
                .key_help_entries()
                .iter()
                .any(|entry| entry.contains("cycle overlapping hints"))
        );
        let before = log.lock().unwrap().scenes.len();
        point_keys(&mut engine, &mut backend, &["left_shift"]);
        assert_eq!(
            log.lock().unwrap().scenes.len(),
            before,
            "word checkboxes do not have overlapping hint layers"
        );
        point_keys(&mut engine, &mut backend, &["enter"]);
        assert_eq!(log.lock().unwrap().copied_text.last().unwrap(), "keep one");
        assert_eq!(engine.active_mode(), &ModeId::normal());
    }
}

#[test]
fn text_select_tab_cycles_all_pages_without_changing_the_checked_text() {
    let mut engine = text_input_engine(Config::default());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine
        .activate(ModeId::text_select(), Some(ModeId::normal()), &mut backend)
        .unwrap();
    let id = log.lock().unwrap().scan_requests.last().unwrap().id;
    let text = "字".repeat(94);
    engine
        .handle_backend_event(
            BackendEvent::UiScanned(crate::api::UiScanResult {
                id,
                retired: vec![],
                status: UiScanStatus::Success,
                targets: vec![crate::api::UiTarget::recognized_text(
                    Rect::new(10., 10., 700., 20.),
                    text.clone(),
                )],
            }),
            &mut backend,
        )
        .unwrap();
    point_keys(&mut engine, &mut backend, &["a"]);
    point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
    let page = || {
        log.lock()
            .unwrap()
            .scenes
            .last()
            .unwrap()
            .labels
            .iter()
            .find(|label| label.text.starts_with("Page "))
            .unwrap()
            .text
            .to_string()
    };
    assert_eq!(page(), "Page 1 / 4  ·  TAB: next page");
    for expected in [2, 3, 4, 1] {
        point_keys(&mut engine, &mut backend, &["tab"]);
        assert_eq!(page(), format!("Page {expected} / 4  ·  TAB: next page"));
        assert_eq!(
            engine
                .registry
                .get(&ModeId::text_select())
                .unwrap()
                .indicator_detail()
                .as_deref(),
            Some("Words")
        );
    }
    point_keys(&mut engine, &mut backend, &["page_down"]);
    point_keys(&mut engine, &mut backend, &["page_up"]);
    assert_eq!(page(), "Page 1 / 4  ·  TAB: next page");
    point_keys(&mut engine, &mut backend, &["left_alt", "c"]);
    point_keys(&mut engine, &mut backend, &["tab"]);
    assert_eq!(page(), "Page 2 / 4  ·  TAB: next page");
    point_keys(&mut engine, &mut backend, &["left_alt", "backspace"]);
    assert_eq!(
        engine
            .registry
            .get(&ModeId::text_select())
            .unwrap()
            .indicator_detail()
            .as_deref(),
        Some("Words")
    );
    point_keys(&mut engine, &mut backend, &["enter"]);
    assert_eq!(log.lock().unwrap().copied_text, [text]);
    assert_eq!(engine.active_mode(), &ModeId::normal());
}
