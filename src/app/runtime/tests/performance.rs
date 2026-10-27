#[test]
#[ignore = "run alone with --test-threads=1 so other tests cannot pollute allocator counts"]
fn empty_binding_profiles_do_not_allocate() {
    let plan = crate::app::configuration::compile(&Config::default()).unwrap();
    let engine = Engine::from_plan(plan, Appearance::Dark).unwrap();
    let app = FocusedApp {
        bundle_id: "com.example.editor".into(),
        window_title: "Document".into(),
        process_id: 42,
    };
    for focused in [Some(&app), None] {
        let region = Region::new(TEST_ALLOCATOR);
        let profile = engine.binding_profile_key_for(focused);
        let stats = region.change();
        assert!(profile.is_empty());
        println!("empty binding profile focused={} {stats:?}", focused.is_some());
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.reallocations, 0);
    }
}

#[test]
fn simulator_menu_uses_the_active_config_source() {
    let source = "# current profile\n[normal]\nlong_press_toggle_ms = 777\n";
    let mut engine = Engine::new(Config::parse(source).unwrap(), Appearance::Dark);
    attach_test_repository(
        &mut engine,
        Config::parse(source).unwrap(),
        Some(ConfigStore::from_validated_text(
            "keysteer.profile.toml",
            source.to_string(),
            crate::platform::atomic_replace,
        )),
        None,
        source.to_string(),
    );
    let (mut backend, log) = FakeBackend::new(vec![BackendEvent::OpenConfigSimulator]);

    engine.run(&mut backend).unwrap();

    assert_eq!(
        log.lock().unwrap().opened_urls,
        [config_handoff::url_for_config(source)]
    );
}

#[test]
fn simulator_menu_includes_current_saved_presets_without_manual_import() {
    let config = Config::default(); let source = config.to_toml().unwrap();
    let mut engine = Engine::new(config, Appearance::Dark);
    engine.attach_preset_store(Box::new(crate::app::preset_store::PresetStore::default()));
    engine.window_presets.store.save(crate::api::window_presets::RegionTemplate::Slot { id: 1 }.into(), 1, "Coding".into()).unwrap();
    let expected = config_handoff::url_for_workspace(&source, engine.window_presets.store.export_file());
    let (mut backend, log) = FakeBackend::new(vec![BackendEvent::OpenConfigSimulator]);
    engine.run(&mut backend).unwrap();
    assert_eq!(log.lock().unwrap().opened_urls, [expected]);
}

#[test]
fn simulator_menu_serializes_config_without_a_store() {
    let config = Config::default();
    let expected = config_handoff::url_for_config(&config.to_toml().unwrap());
    let mut engine = Engine::new(config, Appearance::Dark);
    let (mut backend, log) = FakeBackend::new(vec![BackendEvent::OpenConfigSimulator]);

    engine.run(&mut backend).unwrap();

    assert_eq!(log.lock().unwrap().opened_urls, [expected]);
}

#[test]
fn simulator_menu_debounces_rapid_repeated_clicks() {
    let mut engine = Engine::new(Config::default(), Appearance::Dark);
    let (mut backend, log) = FakeBackend::new(vec![
        BackendEvent::OpenConfigSimulator,
        BackendEvent::OpenConfigSimulator,
        BackendEvent::OpenConfigSimulator,
    ]);

    engine.run(&mut backend).unwrap();

    assert_eq!(log.lock().unwrap().opened_urls.len(), 1);
}

#[test]
fn rejected_mouse_injection_does_not_stop_the_engine() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut engine = Engine::new(Config::default(), Appearance::Dark);
    let mut idle = ProbeMode::new("idle", seen.clone());
    idle.captures = false;
    let mut normal = ProbeMode::new("normal", seen);
    normal.on_key = vec![Command::click(MouseButton::Right)];
    engine.register(Box::new(idle));
    engine.register(Box::new(normal));

    let events = enter_normal().into_iter().chain([key_down("x")]).collect();
    let (mut backend, log) = FakeBackend::new(events);
    backend.fail_mouse = true;

    assert!(engine.run(&mut backend).is_ok());
    assert!(engine.input_failure_active);
    assert_eq!(engine.active_mode(), &ModeId::idle());
    assert_eq!(log.lock().unwrap().clicks, 0);
    assert_eq!(log.lock().unwrap().shutdowns, 1);
}

#[test]
fn leaving_a_scan_owner_cancels_native_work_without_stopping_the_backend() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut engine = Engine::new(Config::default(), Appearance::Dark);
    engine.register(Box::new(ProbeMode::new("idle", Arc::clone(&seen))));
    engine.register(Box::new(ProbeMode::new("ui_hint", seen)));
    engine.registry.active = ModeId::ui_hint();
    engine.scan_owners.insert(41, ModeId::ui_hint());
    let (mut backend, log) = FakeBackend::new(Vec::new());

    engine
        .activate(ModeId::idle(), Some(ModeId::ui_hint()), &mut backend)
        .unwrap();

    assert!(engine.scan_owners.is_empty());
    assert_eq!(log.lock().unwrap().cancelled_scans, [41]);
    assert_eq!(log.lock().unwrap().shutdowns, 0);
}

#[test]
fn capture_loss_discards_physical_keys_that_can_no_longer_receive_key_up() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut engine = engine_with_probes(&seen, &[]);
    engine.registry.active = ModeId::normal();
    let key = Key::new("h").unwrap();
    engine.input.pressed.insert(key.clone());
    engine
        .input
        .key_dispositions
        .insert(key, KeyDisposition::Consume);
    engine.scheduler.frame_clock_owner = Some(ModeId::normal());
    let (mut backend, _) = FakeBackend::new(Vec::new());

    engine
        .handle_backend_event(
            BackendEvent::InputCaptureLost("Accessibility permission removed".into()),
            &mut backend,
        )
        .unwrap();

    assert!(engine.input.pressed.is_empty());
    assert!(engine.input.key_dispositions.is_empty());
    assert!(engine.scheduler.frame_clock_owner.is_none());
    assert_eq!(engine.active_mode(), &ModeId::idle());
}

#[test]
fn finish_cancels_scan_even_when_the_mode_lifecycle_keeps_it_active() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut engine = Engine::new(Config::default(), Appearance::Dark);
    engine.register(Box::new(ProbeMode::new("ui_hint", seen)));
    engine.registry.active = ModeId::ui_hint();
    engine.scan_owners.insert(73, ModeId::ui_hint());
    let (mut backend, log) = FakeBackend::new(Vec::new());

    engine
        .execute(
            [Command::FinishMode {
                cause: FinishCause::Explicit,
            }],
            &mut backend,
        )
        .unwrap();

    assert_eq!(engine.active_mode(), &ModeId::ui_hint());
    assert!(engine.scan_owners.is_empty());
    assert_eq!(log.lock().unwrap().cancelled_scans, [73]);
}

#[test]
fn pointer_interest_skips_only_the_mode_event() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mode = ProbeMode::new("normal", Arc::clone(&seen));
    let mut engine = Engine::new(Config::default(), Appearance::Dark);
    engine.register(Box::new(mode));
    engine.registry.active = ModeId::normal();
    engine.screens = vec![Screen {
        bounds: Rect::new(0.0, 0.0, 1_000.0, 800.0),
        work_area: Rect::new(0.0, 0.0, 1_000.0, 800.0),
        is_primary: true,
        scale: 1.0,
        name: None,
    }];
    let (mut backend, _) = FakeBackend::new(Vec::new());

    engine
        .handle_backend_event(
            BackendEvent::PointerMoved(Point::new(400.0, 300.0)),
            &mut backend,
        )
        .unwrap();

    assert_eq!(engine.cursor, Point::new(400.0, 300.0));
    assert!(!seen.lock().unwrap().iter().any(|event| event == "pointer"));
}

#[test]
#[ignore = "microbenchmark probe; run in release with --test-threads=1"]
fn semantic_role_stays_inline_probe() {
    const SAMPLES: usize = 20_000;

    fn make_target(role: crate::api::geometry::SemanticRole) -> crate::api::geometry::UiTarget {
        crate::api::geometry::UiTarget {
            details: None,
            rect: Rect::new(0.0, 0.0, 40.0, 20.0),
            name: String::new(),
            role,
        }
    }

    // The role used to be a `String` built from a literal for every scanned
    // element, which cost one heap allocation per target. It is a fieldless
    // enum now, so building a target with an empty name must not allocate.
    let region = Region::new(TEST_ALLOCATOR);
    for _ in 0..SAMPLES {
        drop(std::hint::black_box(make_target(
            crate::api::geometry::SemanticRole::Button,
        )));
    }
    let allocations = region.change().allocations;
    assert_eq!(
        allocations, 0,
        "UiTarget construction allocated {allocations} times; the semantic role must stay inline"
    );
}

