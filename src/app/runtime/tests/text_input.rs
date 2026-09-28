fn text_input_engine(config: Config) -> Engine {
    let mut engine = Engine::from_plan(
        crate::app::configuration::compile(&config).unwrap(),
        Appearance::Dark,
    )
    .unwrap();
    engine.rebuild_tables();
    engine
}

#[test]
fn hint_overlay_search_routes_editing_copy_and_stale_results_by_owner() {
    let mut engine = text_input_engine(Config::default());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine.set_active(ModeId::normal());
    engine.activate(ModeId::ui_hint(), Some(ModeId::normal()), &mut backend).unwrap();
    let scan_id = log.lock().unwrap().scan_requests.last().unwrap().id;
    engine.handle_backend_event(BackendEvent::UiScanned(crate::api::UiScanResult {
        id: scan_id, status: UiScanStatus::Success, retired: Vec::new(),
        targets: vec![crate::api::UiTarget {
            rect: Rect::new(10.0, 10.0, 50.0, 30.0), name: "复制".into(), role: crate::api::SemanticRole::Button,
            details: Some(Box::new(crate::api::geometry::UiTargetDetails { ocr: "复制文本".into(), accessibility: "Copy".into(), color: None })),
        }],
    }), &mut backend).unwrap();
    for event in [key_down("/"), key_up("/")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    assert!(log.lock().unwrap().text_prompts.is_empty(), "search must not create a native editor");
    let old_id = engine.scheduler.text_prompt.as_ref().unwrap().1.id;
    assert!(log.lock().unwrap().text_capture);
    log.lock().unwrap().clipboard_input = "复制".into();
    // Native activation can briefly report no foreground application.
    engine.handle_backend_event(BackendEvent::FocusChanged(None), &mut backend).unwrap();
    assert_eq!(engine.scheduler.text_prompt.as_ref().unwrap().1.id, old_id);
    log.lock().unwrap().dispositions.clear();
    for event in [key_down("left_ctrl"), key_down("v"), key_up("v"), key_up("left_ctrl")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    assert_eq!(log.lock().unwrap().dispositions, [KeyDisposition::Consume; 4]);
    engine.handle_backend_event(BackendEvent::TextPromptChanged { id: old_id, text: "fzwb".into() }, &mut backend).unwrap();
    log.lock().unwrap().dispositions.clear();
    let copy_modifier = if cfg!(target_os = "macos") { "left_win" } else { "left_ctrl" };
    log.lock().unwrap().fail_copy = true;
    for event in [key_down(copy_modifier), key_down("1"), key_up("1"), key_up(copy_modifier)] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    assert_eq!(engine.scheduler.text_prompt.as_ref().unwrap().1.id, old_id);
    assert!(log.lock().unwrap().copied_text.is_empty());
    log.lock().unwrap().fail_copy = false;
    log.lock().unwrap().dispositions.clear();
    for event in [key_down(copy_modifier), key_down("1"), key_up("1"), key_up(copy_modifier)] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    assert_eq!(log.lock().unwrap().dispositions, [KeyDisposition::Consume; 4]);
    assert_eq!(log.lock().unwrap().copied_text, ["复制文本"]);
    assert!(engine.scheduler.text_prompt.is_none());
    assert!(!log.lock().unwrap().text_capture);
    assert_eq!(engine.active_mode(), &ModeId::ui_hint());
    assert!(log.lock().unwrap().warps.is_empty());
    engine.handle_backend_event(BackendEvent::TextPromptResult { id: old_id, value: Ok(None) }, &mut backend).unwrap();
    for event in [key_down("/"), key_up("/")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    let id = engine.scheduler.text_prompt.as_ref().unwrap().1.id;
    assert_ne!(id, old_id);
    engine.handle_backend_event(BackendEvent::TextPromptResult { id: old_id, value: Ok(Some("stale".into())) }, &mut backend).unwrap();
    assert_eq!(engine.scheduler.text_prompt.as_ref().unwrap().1.id, id);
    engine.handle_backend_event(BackendEvent::TextPromptResult { id, value: Ok(Some("fzwb".into())) }, &mut backend).unwrap();
    assert_eq!(engine.active_mode(), &ModeId::ui_hint());
    assert_eq!(log.lock().unwrap().warps.last(), Some(&Point::new(35.0, 25.0)));
    assert!(engine.scheduler.text_prompt.is_none());
    assert!(!log.lock().unwrap().text_capture);
    assert!(log.lock().unwrap().released_text_prompts >= 1);
}

#[test]
fn text_input_forwards_typing_and_consumes_enter_to_restore_normal() {
    let mut engine = text_input_engine(Config::default());
    engine.set_active(ModeId::normal());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    for event in [key_down("\\"), key_up("\\")] {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert_eq!(engine.active_mode(), &ModeId::text_input());
    assert_eq!(
        log.lock().unwrap().dispositions,
        [KeyDisposition::Consume; 2]
    );
    log.lock().unwrap().dispositions.clear();
    for key in ["h", "j", "q", "i", "left_shift", "left_ctrl"] {
        engine
            .handle_backend_event(key_down(key), &mut backend)
            .unwrap();
        assert!(engine.quick_switch.pending.is_none());
        engine
            .handle_backend_event(key_up(key), &mut backend)
            .unwrap();
    }
    engine
        .handle_backend_event(key_down("enter"), &mut backend)
        .unwrap();
    assert_eq!(engine.active_mode(), &ModeId::normal());
    engine
        .handle_backend_event(key_up("enter"), &mut backend)
        .unwrap();
    assert_eq!(engine.active_mode(), &ModeId::normal());
    assert_eq!(
        &log.lock().unwrap().dispositions[..12],
        [KeyDisposition::Forward; 12]
    );
    assert_eq!(
        &log.lock().unwrap().dispositions[12..14],
        [KeyDisposition::Consume; 2]
    );
    assert!(log.lock().unwrap().sent.is_empty());
    assert!(log.lock().unwrap().moves.is_empty());
    for event in [key_down("h"), key_up("h")] {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert_eq!(
        &log.lock().unwrap().dispositions[14..],
        [KeyDisposition::Consume; 2]
    );
    assert!(engine.input.key_dispositions.is_empty());
}

#[test]
fn text_input_custom_return_bindings_roundtrip() {
    let config = Config::parse(
        r#"
[key_aliases]
typing_key = "f8"
[normal.bindings]
typing_key = "text_input"
[text_input.bindings]
"ctrl+typing_key" = "normal"
"#,
    )
    .unwrap();
    let config = Config::parse(&config.to_toml().unwrap()).unwrap();
    let mut engine = text_input_engine(config);
    engine.set_active(ModeId::normal());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    for event in [key_down("f8"), key_up("f8")] {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    log.lock().unwrap().dispositions.clear();
    for key in ["enter", "esc", "\\", "f2", "f8"] {
        for event in [key_down(key), key_up(key)] {
            engine.handle_backend_event(event, &mut backend).unwrap();
        }
        assert_eq!(engine.active_mode(), &ModeId::text_input());
    }
    assert_eq!(
        log.lock().unwrap().dispositions,
        [KeyDisposition::Forward; 10]
    );
    for event in tap_chord("left_ctrl+f8") {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert_eq!(engine.active_mode(), &ModeId::normal());
    assert!(engine.input.key_dispositions.is_empty());
}

#[test]
fn text_input_return_bindings_consume_both_edges_and_ignore_extra_modifiers() {
    for exit_key in ["enter", "\\", "esc"] {
        let mut engine = text_input_engine(Config::default());
        engine.set_active(ModeId::text_input());
        let (mut backend, log) = FakeBackend::new(Vec::new());
        for event in tap_chord(&format!("left_ctrl+{exit_key}")) {
            engine.handle_backend_event(event, &mut backend).unwrap();
        }
        assert_eq!(engine.active_mode(), &ModeId::text_input());
        assert_eq!(
            log.lock().unwrap().dispositions,
            [KeyDisposition::Forward; 4]
        );
        log.lock().unwrap().dispositions.clear();
        for event in [key_down(exit_key), key_up(exit_key)] {
            engine.handle_backend_event(event, &mut backend).unwrap();
        }
        assert_eq!(engine.active_mode(), &ModeId::normal());
        assert_eq!(
            log.lock().unwrap().dispositions,
            [KeyDisposition::Consume; 2]
        );
    }
}

#[test]
fn text_input_entry_stops_movement_and_releases_latched_inputs() {
    let mut config = Config::default();
    config
        .normal
        .bindings
        .insert("f3".into(), Binding::parse("press mouse_left").unwrap());
    let mut engine = text_input_engine(config);
    engine.set_active(ModeId::normal());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    for event in [
        key_down("f3"),
        key_up("f3"),
        key_down("h"),
        key_down("\\"),
        key_up("\\"),
        key_up("h"),
    ] {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert_eq!(engine.active_mode(), &ModeId::text_input());
    assert!(engine.input.latched.is_empty());
    assert!(engine.input.active_gestures.is_empty());
    assert_eq!(
        log.lock().unwrap().buttons.last(),
        Some(&(MouseButton::Left, ButtonAction::Release))
    );
    assert!(engine.input.key_dispositions.is_empty());
}

#[test]
fn text_input_home_row_mappings_support_modified_navigation_and_repeats() {
    // Exercise the shipped examples as explicit user bindings, not defaults.
    let shipped = include_str!("../../../../keysteer.default.toml");
    let examples: Vec<_> = shipped
        .split("[text_input.bindings]")
        .nth(1)
        .unwrap()
        .split("[window]")
        .next()
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("# \""))
        .filter(|line| line.contains("primary+"))
        .map(|line| format!("\"{line}"))
        .collect();
    assert_eq!(examples.len(), 36);
    let config = Config::parse(&format!(
        "[text_input.bindings]\n'\\' = 'normal'\nesc = 'normal'\n{}",
        examples.join("\n")
    ))
    .unwrap();
    config.validate().unwrap();
    let primary = config.text_input.temporary_mode_keys[0].clone();
    let mut engine = text_input_engine(config);
    engine.set_active(ModeId::text_input());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    for (source, target) in [
        ("h", "arrow_left"),
        ("j", "arrow_down"),
        ("k", "arrow_up"),
        ("l", "arrow_right"),
        ("u", "backspace"),
        ("i", "delete"),
        ("o", "insert"),
        ("t", "home"),
        ("y", "end"),
    ] {
        for modifiers in ["", "shift+", "ctrl+", "ctrl+shift+"] {
            let source_chord = format!("{modifiers}{primary}+{source}");
            let keys = KeyChord::parse(&source_chord).unwrap();
            let resolved = engine
                .lookup_for_pressed(&Key::new(source).unwrap(), keys.keys())
                .unwrap();
            assert_eq!(
                *resolved.binding,
                Binding::parse(&format!("send {modifiers}{target}")).unwrap()
            );
            let before = log.lock().unwrap().sent.len();
            for event in tap_chord(&source_chord) {
                engine.handle_backend_event(event, &mut backend).unwrap();
            }
            assert_eq!(engine.active_mode(), &ModeId::text_input());
            let recorded = log.lock().unwrap();
            assert!(recorded.sent[before..].contains(&(target.into(), KeyState::Down)));
            assert!(recorded.sent[before..].contains(&(target.into(), KeyState::Up)));
        }
    }
    engine
        .handle_backend_event(key_down(&primary), &mut backend)
        .unwrap();
    engine
        .handle_backend_event(key_down("h"), &mut backend)
        .unwrap();
    let mut repeat = key_down("h");
    if let BackendEvent::Input(ref mut input) = repeat {
        input.repeat = true;
    }
    let before = log.lock().unwrap().sent.len();
    engine
        .handle_backend_event(repeat.clone(), &mut backend)
        .unwrap();
    assert!(log.lock().unwrap().sent.len() > before);
    engine
        .handle_backend_event(key_up(&primary), &mut backend)
        .unwrap();
    let before = log.lock().unwrap().sent.len();
    engine.handle_backend_event(repeat, &mut backend).unwrap();
    assert_eq!(log.lock().unwrap().sent.len(), before);
    engine
        .handle_backend_event(key_up("h"), &mut backend)
        .unwrap();
    assert!(engine.input.key_dispositions.is_empty());
}

#[test]
fn text_input_borrows_normal_and_resumes_typing_when_modifier_is_released() {
    let config = Config::default();
    let primary = config.text_input.temporary_mode_keys[0].clone();
    let mut engine = text_input_engine(config);
    engine.set_active(ModeId::text_input());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine
        .handle_backend_event(key_down(&primary), &mut backend)
        .unwrap();
    assert_eq!(engine.display_mode(), ModeId::normal());
    engine
        .handle_backend_event(key_down("h"), &mut backend)
        .unwrap();
    assert_eq!(
        engine
            .input
            .active_gestures
            .get(&Key::new("h").unwrap())
            .unwrap()
            .owner,
        ModeId::normal()
    );
    engine
        .handle_backend_event(key_up(&primary), &mut backend)
        .unwrap();
    assert!(engine.input.active_gestures.is_empty());
    engine
        .handle_backend_event(key_up("h"), &mut backend)
        .unwrap();
    assert_eq!(engine.active_mode(), &ModeId::text_input());
    assert_eq!(engine.display_mode(), ModeId::text_input());
    assert_eq!(
        log.lock().unwrap().dispositions,
        [KeyDisposition::Consume; 4]
    );
    log.lock().unwrap().dispositions.clear();
    for event in [key_down("h"), key_up("h")] {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert_eq!(
        log.lock().unwrap().dispositions,
        [KeyDisposition::Forward; 2]
    );
}

#[test]
fn text_input_inherits_normal_with_local_overrides_and_validates_layers() {
    let config = Config::parse(
        r#"
[text_input]
inherits = ["normal"]
temporary_mode = "normal"
temporary_mode_keys = ["right_ctrl"]
temporary_mode_passthrough_keys = ["h"]
[text_input.bindings]
h = "backspace"
'\' = "normal"
"#,
    )
    .unwrap();
    let config = Config::parse(&config.to_toml().unwrap()).unwrap();
    let mut engine = text_input_engine(config);
    engine.set_active(ModeId::text_input());
    let lookup = |key: &str, pressed: &[&str]| {
        engine
            .lookup_for_pressed(
                &Key::new(key).unwrap(),
                &pressed
                    .iter()
                    .map(|k| Key::new(k).unwrap())
                    .collect::<Vec<_>>(),
            )
            .map(|r| (*r.binding).clone())
    };
    assert_eq!(
        lookup("h", &["h"]),
        Some(Binding::parse("backspace").unwrap())
    );
    assert_eq!(lookup("j", &["j"]), Some(Binding::Move(Direction::Down)));
    assert_eq!(
        lookup("h", &["right_ctrl", "h"]),
        Some(Binding::parse("backspace").unwrap())
    );
    assert!(
        Config::parse("[text_input]\ninherits=['text_input']")
            .unwrap()
            .validate()
            .is_err()
    );
    assert!(
        Config::parse("[text_input]\ntemporary_mode='missing'")
            .unwrap()
            .validate()
            .is_err()
    );
    assert!(
        Config::parse("[text_input]\ntemporary_mode_keys=['h']")
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn text_input_default_editing_chords_are_opt_in_and_primary_is_aliased() {
    let defaults = Config::default();
    assert_eq!(defaults.text_input.bindings.len(), 3);
    for physical in ["left_alt", "right_ctrl", "left_win"] {
        let config = Config::parse(&format!("[key_aliases]\nprimary = '{physical}'\n[key_aliases.windows]\nprimary = '{physical}'\n[key_aliases.macos]\nprimary = '{physical}'")).unwrap();
        config.validate().unwrap();
        assert_eq!(config.text_input.temporary_mode_keys, [physical]);
        let mut engine = text_input_engine(config);
        engine.set_active(ModeId::text_input());
        let resolved = engine
            .lookup_for_pressed(
                &Key::new("h").unwrap(),
                &[Key::new(physical).unwrap(), Key::new("h").unwrap()],
            )
            .unwrap();
        assert_eq!(*resolved.binding, Binding::Move(Direction::Left));
    }
}

#[test]
fn text_input_can_submit_enter_with_an_ordinary_action_sequence() {
    let shipped = include_str!("../../../../keysteer.default.toml");
    let bindings: Vec<_> = shipped
        .lines()
        .filter_map(|line| line.strip_prefix("# "))
        .filter(|line| line.starts_with("enter = [") || line.starts_with("'\\ esc' ="))
        .collect();
    assert_eq!(bindings.len(), 2);
    let config =
        Config::parse(&format!("[text_input.bindings]\n{}\n", bindings.join("\n"))).unwrap();
    let mut engine = text_input_engine(config);
    engine.set_active(ModeId::text_input());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    for event in [key_down("enter"), key_up("enter")] {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert_eq!(engine.active_mode(), &ModeId::normal());
    assert_eq!(
        log.lock().unwrap().sent,
        [
            ("enter".into(), KeyState::Down),
            ("enter".into(), KeyState::Up)
        ]
    );
    assert_eq!(
        log.lock().unwrap().dispositions,
        [KeyDisposition::Consume; 2]
    );
}

#[test]
fn text_input_temporary_normal_routes_plugin_screen_commands_to_normal() {
    for literal in [false, true] {
        let mut config = Config::default();
        config.normal.bindings.clear();
        config
            .normal
            .bindings
            .insert("s".into(), Binding::parse("screen next").unwrap());
        let primary = config.resolved_key_aliases()["primary"].clone();
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine.screens.push(Screen {
            bounds: Rect::new(1000.0, 0.0, 1000.0, 800.0),
            work_area: Rect::new(1000.0, 0.0, 1000.0, 800.0),
            is_primary: false,
            scale: 1.0,
            name: None,
        });
        engine.cursor = Point::new(500.0, 400.0);
        engine.set_active(ModeId::text_input());
        engine
            .handle_backend_event(key_down(&primary), &mut backend)
            .unwrap();
        let physical = if literal { "x" } else { "s" };
        for target_screen in [1, 0] {
            let down = if literal {
                character_down(physical, 's')
            } else {
                key_down(physical)
            };
            engine.handle_backend_event(down, &mut backend).unwrap();
            engine
                .handle_backend_event(key_up(physical), &mut backend)
                .unwrap();
            assert!(
                engine.screens[target_screen]
                    .bounds
                    .contains(&engine.cursor),
                "literal={literal}, expected screen={target_screen}, cursor={:?}",
                engine.cursor
            );
            assert_eq!(engine.active_mode(), &ModeId::text_input());
        }
        assert_eq!(log.lock().unwrap().warps.len(), 2);
        engine
            .handle_backend_event(key_up(&primary), &mut backend)
            .unwrap();
        log.lock().unwrap().dispositions.clear();
        for event in [key_down("s"), key_up("s")] {
            engine.handle_backend_event(event, &mut backend).unwrap();
        }
        assert_eq!(
            log.lock().unwrap().dispositions,
            [KeyDisposition::Forward; 2]
        );
        assert_eq!(log.lock().unwrap().warps.len(), 2);
    }
}

#[test]
fn text_input_default_indicator_is_disabled_in_code_and_shipped_config() {
    for config in [
        Config::default(),
        Config::parse(include_str!("../../../../keysteer.default.toml")).unwrap(),
    ] {
        assert_eq!(
            config.mode_indicator.modes["text_input"].enabled,
            Some(false)
        );
        let engine = text_input_engine(config);
        assert!(engine.build_indicator(&ModeId::text_input()).is_none());
        assert!(engine.build_indicator(&ModeId::normal()).is_some());
    }
}

#[test]
fn text_input_default_backslash_leaves_f2_available_to_applications() {
    let config = Config::default();
    assert!(!config.normal.bindings.contains_key("f2"));
    assert!(!config.text_input.bindings.contains_key("f2"));
    let mut engine = text_input_engine(config);
    let (mut backend, log) = FakeBackend::new(Vec::new());
    for mode in [ModeId::normal(), ModeId::text_input()] {
        engine.set_active(mode.clone());
        log.lock().unwrap().dispositions.clear();
        for event in [key_down("f2"), key_up("f2")] {
            engine.handle_backend_event(event, &mut backend).unwrap();
        }
        assert_eq!(engine.active_mode(), &mode);
        assert_eq!(
            log.lock().unwrap().dispositions,
            [KeyDisposition::Forward; 2]
        );
    }
}

#[test]
fn overlay_editor_uses_layout_text_custom_paste_and_paired_capture() {
    let mut config = Config::default();
    config.ui_hint.search_edit_keys.insert(crate::api::text_edit::EditAction::Paste, "f8".into());
    let mut engine = text_input_engine(config);
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine.set_active(ModeId::normal());
    engine.activate(ModeId::ui_hint(), Some(ModeId::normal()), &mut backend).unwrap();
    for event in [key_down("/"), key_up("/")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    let scan_id = log.lock().unwrap().scan_requests.last().unwrap().id;
    engine.handle_backend_event(BackendEvent::UiScanActivationExpected { id: scan_id, process_id: 700 }, &mut backend).unwrap();
    engine.handle_backend_event(BackendEvent::FocusChanged(None), &mut backend).unwrap();
    engine.handle_backend_event(BackendEvent::FocusChanged(Some(crate::api::FocusedApp { process_id: 700, bundle_id: "test".into(), window_title: "target".into() })), &mut backend).unwrap();
    assert!(engine.scheduler.text_prompt.is_some(), "expected activation must not close search");
    assert_eq!(log.lock().unwrap().scan_requests.len(), 1);
    let mut event = key_down("z");
    if let BackendEvent::Input(input) = &mut event { input.character = Some('Z'); }
    engine.handle_backend_event(event, &mut backend).unwrap();
    engine.handle_backend_event(key_up("z"), &mut backend).unwrap();
    log.lock().unwrap().clipboard_input = "复制🦀".into();
    for event in [key_down("f8"), key_up("f8")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    let query = |log: &Recorder| log.scenes.last().unwrap().labels.iter().find(|label| label.z_index == 10_002).unwrap().text.to_string();
    assert_eq!(query(&log.lock().unwrap()), "Z复制🦀");
    for event in [key_down("left"), key_up("left"), key_down("backspace"), key_up("backspace")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    assert_eq!(query(&log.lock().unwrap()), "Z复🦀");
    assert!(log.lock().unwrap().text_prompts.is_empty());
    assert!(log.lock().unwrap().dispositions.iter().all(|d| *d == KeyDisposition::Consume));
    for event in [key_down("enter"), key_up("enter")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    assert_eq!(engine.active_mode(), &ModeId::ui_hint());
    assert!(engine.scheduler.text_prompt.is_none());
    assert!(!log.lock().unwrap().text_capture);
}

#[test]
fn overlay_search_reentry_and_mode_exit_retire_capture_without_native_windows() {
    let mut engine = text_input_engine(Config::default());
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine.set_active(ModeId::normal());
    engine.activate(ModeId::ui_hint(), Some(ModeId::normal()), &mut backend).unwrap();
    for _ in 0..32 {
        for event in [key_down("/"), key_up("/"), key_down("a"), key_up("a"), key_down("esc"), key_up("esc")] {
            engine.handle_backend_event(event, &mut backend).unwrap();
        }
        assert!(engine.scheduler.text_prompt.is_none());
        assert!(!log.lock().unwrap().text_capture);
        assert_eq!(engine.active_mode(), &ModeId::ui_hint());
    }
    for event in [key_down("/"), key_up("/")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    engine.activate(ModeId::normal(), Some(ModeId::ui_hint()), &mut backend).unwrap();
    assert!(!log.lock().unwrap().text_capture);
    assert!(engine.scheduler.text_prompt.is_none());
    assert!(log.lock().unwrap().text_prompts.is_empty());
}

#[test]
fn search_completion_error_and_capture_loss_retire_input_before_owner_is_dropped() {
    for outcome in [Ok(None), Err("editor failed".to_string())] {
        let mut engine = text_input_engine(Config::default());
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine.activate(ModeId::ui_hint(), Some(ModeId::normal()), &mut backend).unwrap();
        for event in [key_down("/"), key_up("/")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
        let id = engine.scheduler.text_prompt.as_ref().unwrap().1.id;
        engine.handle_backend_event(BackendEvent::TextPromptResult { id, value: outcome }, &mut backend).unwrap();
        assert!(!log.lock().unwrap().text_capture);
        assert!(engine.scheduler.text_prompt.is_none());
        assert!(!engine.scheduler.text_prompt_returning_focus);
        for event in [key_down("/"), key_up("/")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
        let current = engine.scheduler.text_prompt.as_ref().unwrap().1.id;
        engine.handle_backend_event(BackendEvent::TextPromptResult { id, value: Ok(None) }, &mut backend).unwrap();
        assert!(log.lock().unwrap().text_capture, "stale completion must not close the new editor");
        assert_eq!(engine.scheduler.text_prompt.as_ref().unwrap().1.id, current);
        engine.handle_backend_event(BackendEvent::InputCaptureLost("test capture lost".into()), &mut backend).unwrap();
        assert!(!log.lock().unwrap().text_capture);
        assert!(engine.scheduler.text_prompt.is_none());
    }
}

#[test]
fn search_accept_alternatives_close_capture_and_keep_uihint() {
    for binding in ["enter", "/", "primary+q", "f9"] {
        let mut config = Config::default();
        if binding == "f9" {
            config.ui_hint.search_edit_keys.insert(crate::api::text_edit::EditAction::Accept, "f9".into());
        }
        let primary = if cfg!(target_os = "macos") { "left_win" } else { "left_ctrl" };
        let mut engine = text_input_engine(config);
        let (mut backend, log) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        engine.activate(ModeId::ui_hint(), Some(ModeId::normal()), &mut backend).unwrap();
        for event in [key_down("/"), key_up("/")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
        assert!(engine.scheduler.text_prompt.is_some());
        let modified = binding == "primary+q";
        if modified { engine.handle_backend_event(key_down(primary), &mut backend).unwrap(); }
        let key = if modified { "q" } else { binding };
        for event in [key_down(key), key_up(key)] { engine.handle_backend_event(event, &mut backend).unwrap(); }
        if modified { engine.handle_backend_event(key_up(primary), &mut backend).unwrap(); }
        assert!(engine.scheduler.text_prompt.is_none(), "{binding}");
        assert!(!log.lock().unwrap().text_capture, "{binding}");
        assert_eq!(engine.active_mode(), &ModeId::ui_hint());
    }
}
