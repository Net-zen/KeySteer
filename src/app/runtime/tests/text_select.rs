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
    point_keys(&mut engine, &mut backend, &[if cfg!(target_os = "macos") { "left_win" } else { "left_ctrl" }, "c"]);
    let detail = |engine: &Engine| engine.registry.get(&ModeId::text_select()).unwrap().indicator_detail().unwrap();
    assert_eq!(detail(&engine), "Words");
    point_keys(&mut engine, &mut backend, &[if cfg!(target_os = "macos") { "left_win" } else { "left_ctrl" }, "c"]);
    assert_eq!(detail(&engine), "Characters");
    point_keys(&mut engine, &mut backend, &["backspace"]);
    assert_eq!(detail(&engine), "Words");
    point_keys(&mut engine, &mut backend, &["tab"]);
    assert_eq!(detail(&engine), "Regions");
    point_keys(&mut engine, &mut backend, &["left_ctrl", "backspace"]);
    point_keys(&mut engine, &mut backend, &["enter"]);
    assert!(log.lock().unwrap().copied_text.is_empty());
    point_keys(&mut engine, &mut backend, &["a"]);
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
            .any(|entry| entry.starts_with("backspace") && entry.contains("previous_level"))
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
            .any(|entry| entry.starts_with("backspace") && entry.contains("previous_level"))
    );
    point_keys(&mut engine, &mut backend, &["a"]);
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
