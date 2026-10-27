#[test]
fn identical_scenes_are_presented_only_once() {
    struct Redrawer(ModeId);
    impl Mode for Redrawer {
        fn id(&self) -> ModeId {
            self.0.clone()
        }
        fn handle(&mut self, event: &ModeEvent, _c: &HostContext<'_>) -> CommandBatch {
            CommandBatch::from(match event {
                ModeEvent::Activated { .. } | ModeEvent::Key { .. } => {
                    vec![Command::show_overlay(OverlayScene::new())]
                }
                _ => Vec::new(),
            })
        }
    }

    let mut engine = Engine::new(Config::default(), Appearance::Dark);
    engine.register(Box::new(Redrawer(ModeId::idle())));
    let (mut backend, log) = FakeBackend::new(vec![key_down("a"), key_down("b")]);
    engine.run(&mut backend).unwrap();

    // Activation draws once; the two identical redraws are suppressed.
    assert_eq!(log.lock().unwrap().presents, 1);
}

fn visible_normal_overlay() -> (Engine, FakeBackend, Arc<Mutex<Recorder>>) {
    let mut config = Config::default();
    config.normal.bindings.insert("?".into(), Binding::KeyHelp);
    let mut engine = Engine::new(config.clone(), Appearance::Dark);
    for mode in crate::app::mode_catalog::built_in(&config) {
        engine.register(mode);
    }
    engine.registry.active = ModeId::normal();
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine.cursor = backend.pointer().unwrap();
    engine
        .show_overlay(Arc::new(OverlayScene::new()), &mut backend)
        .unwrap();
    (engine, backend, log)
}

fn visible_dense_overlay() -> (Engine, FakeBackend, Arc<Mutex<Recorder>>) {
    let (mut engine, mut backend, log) = visible_normal_overlay();
    let mut scene = OverlayScene::with_capacity(0, 1_000);
    for index in 0..1_000 {
        scene.labels.push(crate::api::overlay::OverlayLabel::new(
            format!("{index:03}"),
            Rect::new(
                (index % 40) as f64 * 24.0,
                (index / 40) as f64 * 24.0,
                22.0,
                22.0,
            ),
            crate::api::overlay::LabelStyle::default(),
        ));
    }
    engine.show_overlay(Arc::new(scene), &mut backend).unwrap();
    backend.accept_position_updates = true;
    (engine, backend, log)
}

#[test]
fn pointer_motion_uses_position_only_backend_update() {
    let (mut engine, mut backend, log) = visible_normal_overlay();
    backend.accept_position_updates = true;
    engine.cursor = Point::new(200.0, 300.0);

    engine.refresh_overlay_positions(&mut backend).unwrap();

    let log = log.lock().unwrap();
    assert_eq!(log.presents, 1, "movement rebuilt the complete scene");
    assert_eq!(log.positions.len(), 1);
    assert_eq!(log.positions[0].0, Some(Point::new(200.0, 300.0)));
    assert!(log.positions[0].1.is_some());
}

#[test]
fn complete_scene_restores_coordinates_after_a_position_only_move() {
    let (mut engine, mut backend, log) = visible_normal_overlay();
    backend.accept_position_updates = true;
    let original = engine.cursor;
    engine.cursor = Point::new(200.0, 300.0);
    engine.refresh_overlay_positions(&mut backend).unwrap();

    // A command can return to the last complete scene's point without going
    // through the position path (for example a target selection/refresh).
    engine.cursor = original;
    engine.refresh_overlay(&mut backend).unwrap();
    let log = log.lock().unwrap();
    assert_eq!(log.positions.len(), 1);
    assert_eq!(log.presents, 2, "stale complete-scene cache suppressed the return");
    assert_eq!(log.scenes.last().unwrap().cursor_marker.as_ref().unwrap().center, original);
}

#[test]
#[ignore = "allocation probe; run alone with --test-threads=1"]
fn overlay_position_path_is_allocation_free() {
    let (mut engine, mut backend, log) = visible_dense_overlay();
    engine.cursor = Point::new(100.0, 100.0);
    engine.refresh_overlay_positions(&mut backend).unwrap();
    log.lock().unwrap().positions.clear();

    let region = Region::new(TEST_ALLOCATOR);
    for index in 0..10_000 {
        engine.cursor = Point::new(100.0 + (index % 500) as f64, 100.0);
        engine.refresh_overlay_positions(&mut backend).unwrap();
        log.lock().unwrap().positions.clear();
    }
    let change = region.change();
    assert_eq!(change.allocations, 0, "position path allocated: {change:?}");
    assert_eq!(change.deallocations, 0, "position path freed: {change:?}");
    assert_eq!(
        change.bytes_allocated, 0,
        "position path allocated: {change:?}"
    );
}

#[test]
#[ignore = "allocation probe; run alone with --test-threads=1"]
fn pending_long_press_wait_is_allocation_free() {
    let mut engine = engine_with_normal_binding("n", "toggle");
    engine.registry.active = ModeId::normal();
    engine
        .input
        .pending_long_press_toggles
        .push(PendingLongPressToggle {
            fires_at: Instant::now() + Duration::from_secs(60),
            key: Key::new("n").unwrap(),
            target: InputTarget::Key(Key::new("n").unwrap()),
            short_action: None,
        });
    let (mut backend, _) = FakeBackend::new(Vec::new());

    let region = Region::new(TEST_ALLOCATOR);
    for _ in 0..10_000 {
        engine.fire_due_long_press_toggles(&mut backend).unwrap();
    }
    let change = region.change();
    assert_eq!(change.allocations, 0, "waiting allocated: {change:?}");
    assert_eq!(change.deallocations, 0, "waiting freed: {change:?}");
    assert_eq!(change.bytes_allocated, 0, "waiting allocated: {change:?}");
}

#[test]
#[ignore = "allocation probe; run alone with --test-threads=1"]
fn pending_short_press_queue_is_allocation_free() {
    let mut engine = engine_with_normal_binding("x", "left_click");
    let pending = PendingLongPressToggle {
        fires_at: Instant::now() + Duration::from_secs(60),
        key: Key::new("x").unwrap(),
        target: InputTarget::Mouse(Button::Left),
        short_action: Some(ButtonAction::Click),
    };

    let region = Region::new(TEST_ALLOCATOR);
    for _ in 0..10_000 {
        engine
            .input
            .pending_long_press_toggles
            .push(pending.clone());
        std::hint::black_box(engine.take_pending_long_press_toggle(&pending.key));
    }
    let change = region.change();
    assert_eq!(change.allocations, 0, "queue allocated: {change:?}");
    assert_eq!(change.deallocations, 0, "queue freed: {change:?}");
    assert_eq!(change.bytes_allocated, 0, "queue allocated: {change:?}");
}

#[test]
#[ignore = "allocation probe; run alone with --test-threads=1"]
fn common_toggle_transaction_is_allocation_free() {
    let mut engine = engine_with_normal_binding("n", "toggle");
    let (mut backend, log) = FakeBackend::new(Vec::new());
    let targets = [
        InputTarget::Mouse(Button::Left),
        InputTarget::Mouse(Button::Right),
    ];
    engine.toggle_targets(&targets, &mut backend).unwrap();
    engine.toggle_targets(&targets, &mut backend).unwrap();
    {
        let mut log = log.lock().unwrap();
        log.buttons.clear();
        log.timeline.clear();
    }

    let region = Region::new(TEST_ALLOCATOR);
    for _ in 0..10_000 {
        engine.toggle_targets(&targets, &mut backend).unwrap();
        engine.toggle_targets(&targets, &mut backend).unwrap();
        let mut log = log.lock().unwrap();
        log.buttons.clear();
        log.timeline.clear();
    }
    let change = region.change();
    assert_eq!(change.allocations, 0, "toggle allocated: {change:?}");
    assert_eq!(change.deallocations, 0, "toggle freed: {change:?}");
    assert_eq!(change.bytes_allocated, 0, "toggle allocated: {change:?}");
}

#[test]
fn unsupported_or_failed_position_update_falls_back_to_full_scene() {
    for fail in [false, true] {
        let (mut engine, mut backend, log) = visible_normal_overlay();
        backend.fail_position_updates = fail;
        engine.cursor = Point::new(200.0, 300.0);

        engine.refresh_overlay_positions(&mut backend).unwrap();
        engine.cursor = Point::new(300.0, 300.0);
        engine.refresh_overlay_positions(&mut backend).unwrap();

        let log = log.lock().unwrap();
        assert_eq!(log.presents, 3, "fail={fail}: no complete fallback");
        assert_eq!(
            log.position_update_attempts, 1,
            "fail={fail}: rejected fast path was retried"
        );
        let scene = log.scenes.last().unwrap();
        assert_eq!(
            scene.cursor_marker.as_ref().map(|marker| marker.center),
            Some(Point::new(300.0, 300.0))
        );
    }
}

#[test]
fn crossing_a_screen_edge_rebuilds_the_cursor_only_overlay() {
    let (mut engine, mut backend, log) = visible_normal_overlay();
    backend.accept_position_updates = true;
    engine.screens.push(Screen {
        bounds: Rect::new(1000.0, 0.0, 1000.0, 800.0),
        work_area: Rect::new(1000.0, 0.0, 1000.0, 800.0),
        is_primary: false,
        scale: 2.0,
        name: None,
    });
    engine.cursor = Point::new(1200.0, 300.0);

    engine.refresh_overlay_positions(&mut backend).unwrap();

    let log = log.lock().unwrap();
    assert_eq!(log.presents, 2);
    assert!(log.positions.is_empty());
    assert_eq!(
        log.scenes.last().and_then(|scene| scene.clip),
        Some(Rect::new(1000.0, 0.0, 1000.0, 800.0))
    );
}

#[test]
fn one_command_batch_submits_only_its_final_overlay_state() {
    let mut engine = Engine::new(Config::default(), Appearance::Dark);
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine.cursor = backend.pointer().unwrap();

    let mut first = OverlayScene::new();
    first.backdrop = Some(crate::api::Color::rgb(1, 2, 3));
    let mut final_scene = OverlayScene::new();
    final_scene.backdrop = Some(crate::api::Color::rgb(4, 5, 6));
    engine
        .execute(
            vec![
                Command::show_overlay(first),
                Command::warp_to(Point::new(30.0, 40.0)),
                Command::show_overlay(final_scene),
            ],
            &mut backend,
        )
        .unwrap();

    let log = log.lock().unwrap();
    assert_eq!(log.warps, vec![Point::new(30.0, 40.0)]);
    assert_eq!(log.presents, 1);
    assert_eq!(
        log.scenes[0].backdrop,
        Some(crate::api::Color::rgb(4, 5, 6))
    );
}


#[test]
fn mouse_help_defaults_are_toggleable_and_restored_on_reentry() {
    for visible in [false, true] {
        let mut config = Config::default();
        config.key_help.mouse_key_help = visible;
        let config = Config::parse(&config.to_toml().unwrap()).unwrap();
        assert_eq!(config.key_help.mouse_key_help, visible);
        let (mut engine, mut backend, _) = window_test_engine(&config);
        engine.activate(ModeId::normal(), Some(ModeId::idle()), &mut backend).unwrap();
        assert_eq!(engine.overlay.key_help_visible, visible);
        for event in [character_down("/", '?'), key_up("/")] {
            engine.handle_backend_event(event, &mut backend).unwrap();
        }
        assert_eq!(engine.overlay.key_help_visible, !visible);
        for mode in [ModeId::grid(), ModeId::recursive_grid(), ModeId::ui_hint()] {
            engine.set_active(mode);
            assert_eq!(engine.overlay.key_help_visible, !visible);
        }
        engine.set_active(ModeId::idle());
        assert!(!engine.overlay.key_help_visible);
        engine.set_active(ModeId::normal());
        assert_eq!(engine.overlay.key_help_visible, visible);
    }
}

#[test]
fn key_help_toggle_pairs_release_and_ignores_repeat() {
    let (mut engine, mut backend, log) = visible_normal_overlay();
    engine.handle_backend_event(key_down("left_shift"), &mut backend).unwrap();
    engine.handle_backend_event(character_down("/", '?'), &mut backend).unwrap();
    assert!(engine.overlay.key_help_visible);
    assert!(engine.overlay.last_scene.as_ref().unwrap().labels.iter()
        .any(|label| label.text.contains("Available keys")));
    let BackendEvent::Input(mut repeated) = key_down("/") else { unreachable!() };
    repeated.repeat = true;
    engine.handle_key(repeated, &mut backend).unwrap();
    assert!(engine.overlay.key_help_visible);
    engine.handle_backend_event(key_up("left_shift"), &mut backend).unwrap();
    engine.handle_backend_event(key_up("/"), &mut backend).unwrap();
    assert_eq!(log.lock().unwrap().dispositions.last(), Some(&KeyDisposition::Consume));
    for event in tap_chord("right_shift+/") {
        let event = match event {
            BackendEvent::Input(mut input) if input.key.as_str() == "/" && input.state == KeyState::Down => {
                input.character = Some('?'); BackendEvent::Input(input)
            }
            other => other,
        };
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert!(!engine.overlay.key_help_visible);
    assert!(engine.overlay.last_scene.as_ref().unwrap().labels.is_empty());
}

#[test]
fn key_help_does_not_capture_question_mark_in_idle() {
    let mut engine = engine_with_normal_binding("h", "left");
    let (mut backend, log) = FakeBackend::new(Vec::new());
    for event in tap_chord("shift+/") {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert!(!engine.overlay.key_help_visible);
    assert!(log.lock().unwrap().dispositions.iter().all(|d| *d == KeyDisposition::Forward));
}

#[test]
fn key_help_uses_configured_binding_and_style() {
    let config = Config::parse(r##"
[normal.bindings]
f1 = "key_help"
[key_help]
font_family = "Arial"
font_size = 14
background_color = { light = "#FFFFFFFF", dark = "#112233FF" }
text_color = "#FFEEDDFF"
border_color = "#AABBCCFF"
border_width = 2
border_radius = 7
padding_x = 30
padding_y = 10
"##).unwrap();
    let mut engine = Engine::new(config.clone(), Appearance::Dark);
    for mode in crate::app::mode_catalog::built_in(&config) {
        engine.register(mode);
    }
    engine.registry.active = ModeId::normal();
    let (mut backend, _) = FakeBackend::new(Vec::new());
    engine.screens = backend.screens().unwrap();
    engine.cursor = backend.pointer().unwrap();
    for event in tap_chord("shift+/") {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert!(!engine.overlay.key_help_visible);
    for event in tap_chord("f1") {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert!(engine.overlay.key_help_visible);
    let scene = engine.overlay.last_scene.as_ref().unwrap();
    let panel = scene.labels.iter().find(|label| label.text.is_empty()).unwrap();
    assert_eq!(panel.style.background, crate::api::Color::rgb(0x11, 0x22, 0x33));
    assert_eq!(panel.style.border_color, crate::api::Color::rgb(0xaa, 0xbb, 0xcc));
    assert_eq!(panel.style.border_width, 2.0);
    assert_eq!(panel.style.border_radius, 7.0);
    assert!(scene.labels.iter().all(|label| label.style.font_family == "Arial"));
    assert!(scene.labels.iter().any(|label| label.text == "F1  Close"));
    for event in tap_chord("f1") {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert!(!engine.overlay.key_help_visible);
    assert!(engine.overlay.key_help_cache.is_none());
    engine.settings.key_help.mouse_key_help = false;
    for event in tap_chord("f1") {
        engine.handle_backend_event(event, &mut backend).unwrap();
    }
    assert!(engine.overlay.key_help_visible, "default-hidden must still allow manual toggling");
}

#[test]
fn key_help_rejects_invalid_styles_and_round_trips_verb() {
    assert_eq!(Binding::parse("key_help").unwrap().canonical(), "key_help");
    for value in ["font_size = 0", "padding_x = -1", "border_width = nan", "background_color = 'red'", "max_columns = 2"] {
        assert!(Config::parse(&format!("[key_help]\n{value}")).and_then(|config| config.validate()).is_err(), "{value}");
    }
}

#[test]
fn key_help_binding_defaults_can_be_replaced_by_an_explicit_table() {
    for source in ["", "[normal]\n"] {
        let config = Config::parse(source).unwrap();
        assert_eq!(config.normal.bindings.get("?"), Some(&Binding::KeyHelp));
    }
    let config = Config::parse("[normal.bindings]\n# explicit empty table").unwrap();
    assert!(!config.normal.bindings.values().any(|binding| *binding == Binding::KeyHelp));
    let config = Config::parse("[normal.bindings]\n\"?\" = \"key_help\"").unwrap();
    assert_eq!(config.normal.bindings.get("?"), Some(&Binding::KeyHelp));
    assert_eq!(Config::default().normal.bindings.get("?"), Some(&Binding::KeyHelp));
}

#[test]
fn key_help_resolves_current_binding_table_and_keeps_position_fast_path() {
    let (mut engine, mut backend, log) = visible_normal_overlay();
    let expected = engine.bindings_in(&ModeId::normal());
    assert!(!expected.is_empty());
    let help = engine.key_help_entries();
    assert!(help.iter().any(|entry| entry.contains(&expected[0].1.canonical())));
    engine.overlay.key_help_visible = true;
    engine.refresh_overlay(&mut backend).unwrap();
    backend.accept_position_updates = true;
    let presents = log.lock().unwrap().presents;
    engine.cursor = Point::new(200.0, 300.0);
    engine.refresh_overlay_positions(&mut backend).unwrap();
    assert_eq!(log.lock().unwrap().presents, presents);
    engine.set_active(ModeId::idle());
    assert!(!engine.overlay.key_help_visible);
}

#[test]
fn key_help_narrows_to_held_chord_prefix() {
    let mut config = Config::default();
    config.normal.bindings.clear();
    for (key, action) in [("alt+x", "left"), ("alt+x+y", "right"), ("h", "up")] {
        config.normal.bindings.insert(key.into(), Binding::parse(action).unwrap());
    }
    let mut engine = Engine::new(config.clone(), Appearance::Dark);
    for mode in crate::app::mode_catalog::built_in(&config) { engine.register(mode); }
    engine.set_active(ModeId::normal());
    let (mut backend, _) = FakeBackend::new(Vec::new());
    assert!(engine.key_help_entries().iter().any(|entry| entry.starts_with("h  ")));
    engine.handle_backend_event(key_down("left_alt"), &mut backend).unwrap();
    engine.handle_backend_event(key_down("x"), &mut backend).unwrap();
    assert!(!engine.input.pending_chords.is_empty());
    let entries = engine.key_help_entries();
    assert!(entries.iter().any(|entry| entry.starts_with("alt+x+y  ")));
    assert!(!entries.iter().any(|entry| entry.starts_with("h  ")));
}

#[test]
#[cfg(target_os = "windows")]
fn key_help_high_dpi_panel_text_fits_without_overlap() {
    let (mut engine, mut backend, _) = visible_normal_overlay();
    engine.screens[0].scale = 1.5;
    engine.overlay.key_help_visible = true;
    engine.refresh_overlay(&mut backend).unwrap();
    let screen = &engine.screens[0];
    let scene = engine.overlay.last_scene.as_ref().unwrap();
    let panels: Vec<_> = scene.labels.iter().filter(|label| label.text.is_empty() && !label.style.background.is_transparent()).collect();
    assert_eq!(panels.len(), 1, "help must use one shared panel, not per-key badges");
    let panel = panels[0].rect;
    assert_eq!(panels[0].style.background, scene.indicator.as_ref().unwrap().style.background);
    let keycaps: Vec<_> = scene.labels.iter().filter(|label| !label.text.is_empty() && !label.style.background.is_transparent()).collect();
    assert!(!keycaps.is_empty());
    let body_top = keycaps.iter().map(|label| label.rect.center().y).fold(f64::INFINITY, f64::min);
    let body_font = keycaps[0].style.font_size;
    assert!(scene.labels.iter().filter(|label| !label.text.is_empty() && label.rect.center().y >= body_top - 0.5)
        .all(|label| (label.style.font_size - body_font).abs() < 0.001),
        "long descriptions and keycaps must use the same font size as short entries");
    let mut right_edges = [None::<f64>; 2];
    for label in keycaps {
        let rect = crate::api::overlay::scaled_label_geometry(&label.text, label.rect, &label.style, screen.scale).0;
        let column = usize::from(rect.center().x > panel.center().x);
        if let Some(edge) = right_edges[column] { assert!((edge - rect.right()).abs() <= 1.0); }
        right_edges[column] = Some(rect.right());
    }
    assert!(scene.labels.iter().filter(|label| !label.text.is_empty() && label.style.background.is_transparent())
        .all(|label| label.style.text_alignment == crate::api::overlay::TextAlignment::Left));
    let rects: Vec<_> = scene.labels.iter().filter(|label| !label.text.is_empty())
        .map(|label| crate::api::overlay::scaled_label_geometry(&label.text, label.rect, &label.style, screen.scale).0).collect();
    for (index, rect) in rects.iter().enumerate() {
        assert!(rect.x >= panel.x && rect.right() <= panel.right());
        assert!(rect.y >= panel.y && rect.bottom() <= panel.bottom());
        assert!(rect.x >= screen.work_area.x && rect.right() <= screen.work_area.right());
        assert!(rect.y >= screen.work_area.y && rect.bottom() <= screen.work_area.bottom());
        for other in &rects[index + 1..] {
            assert!(rect.right() <= other.x || other.right() <= rect.x || rect.bottom() <= other.y || other.bottom() <= rect.y);
        }
    }
}

#[test]
#[ignore = "allocation/timing probe; run alone in release"]
fn key_help_decoration_probe() {
    let (mut engine, _backend, _log) = visible_normal_overlay();
    engine.overlay.key_help_visible = true;
    let source = OverlayScene::new();
    let mut warm = source.clone();
    engine.decorate_key_help(&mut warm);
    drop(warm);
    let mut samples = Vec::with_capacity(20_000);
    let region = Region::new(TEST_ALLOCATOR);
    for _ in 0..20_000 {
        let mut scene = source.clone();
        let start = Instant::now();
        engine.decorate_key_help(&mut scene);
        samples.push(start.elapsed().as_nanos());
    }
    let change = region.change();
    samples.sort_unstable();
    println!("key_help 20000 samples p50={}ns p95={}ns p99={}ns allocations={} deallocations={} bytes={}",
        samples[10_000], samples[19_000], samples[19_800], change.allocations, change.deallocations, change.bytes_allocated);
    assert_eq!(change.allocations, 0, "cached help allocated: {change:?}");
    assert_eq!(change.deallocations, 0, "cached help freed: {change:?}");
}

#[test]
fn key_help_cache_reuses_labels_and_releases_on_close() {
    let (mut engine, mut backend, _) = visible_normal_overlay();
    engine.overlay.key_help_visible = true;
    engine.refresh_overlay(&mut backend).unwrap();
    let first = engine.overlay.last_scene.as_ref().unwrap().labels.clone();
    engine.refresh_overlay(&mut backend).unwrap();
    assert!(first.shares_storage_with(&engine.overlay.last_scene.as_ref().unwrap().labels));
    assert!(engine.overlay.key_help_cache.is_some());
    for event in [character_down("/", '?'), key_up("/")] { engine.handle_backend_event(event, &mut backend).unwrap(); }
    assert!(engine.overlay.key_help_cache.is_none());
    assert!(engine.overlay.last_scene.as_ref().unwrap().labels.is_empty());
    engine.overlay.key_help_visible = true;
    engine.refresh_overlay(&mut backend).unwrap();
    engine.set_active(ModeId::idle());
    assert!(engine.overlay.key_help_cache.is_none());
    engine.set_active(ModeId::normal());
    engine.overlay.key_help_visible = true;
    engine.refresh_overlay(&mut backend).unwrap();
    engine.finish_runtime(&mut backend, Ok(())).unwrap();
    assert!(engine.overlay.key_help_cache.is_none());
    assert!(engine.overlay.last_scene.is_none());
}

#[test]
fn key_help_cache_rebuilds_on_routes_theme_and_mode_events() {
    struct ChangingHelp { key: &'static str }
    impl Mode for ChangingHelp {
        fn id(&self) -> ModeId { ModeId::normal() }
        fn handle(&mut self, event: &ModeEvent, _: &HostContext<'_>) -> CommandBatch {
            match event {
                ModeEvent::Key { .. } => self.key = "f23",
                ModeEvent::UiScanned(_) => self.key = "f24",
                _ => return CommandBatch::new(),
            }
            Command::show_overlay(OverlayScene::new()).into()
        }
        fn available_keys(&self) -> Vec<(String, String)> { vec![(self.key.into(), "dynamic test".into())] }
    }
    let (mut engine, mut backend, _) = visible_normal_overlay();
    engine.register(Box::new(ChangingHelp { key: "f22" }));
    engine.overlay.key_help_visible = true;
    engine.refresh_overlay(&mut backend).unwrap();
    let contains = |engine: &Engine, text: &str| engine.overlay.last_scene.as_ref().unwrap().labels.iter().any(|label| label.text.as_str() == text);
    assert!(contains(&engine, "F22"));
    engine.dispatch(ModeEvent::Key { key: Key::new("f21").unwrap(), state: KeyState::Down, repeat: false }, &mut backend).unwrap();
    assert!(contains(&engine, "F23"));
    assert!(!contains(&engine, "F22"));
    engine.dispatch_owned_to(&ModeId::normal(), ModeEvent::UiScanned(crate::api::command::UiScanResult {
        retired: Vec::new(),
id: 1, targets: Vec::new(), status: UiScanStatus::Partial,
    }), &mut backend).unwrap();
    assert!(contains(&engine, "F24"));
    assert!(!contains(&engine, "F23"));
    engine.registry.routes.get_mut(&ModeId::normal()).unwrap().bindings.insert("f24".into(), Binding::parse("right_click").unwrap());
    engine.rebuild_tables();
    assert!(engine.overlay.key_help_cache.is_none());
    engine.refresh_overlay(&mut backend).unwrap();
    assert!(!contains(&engine, "dynamic test"));
    let old = engine.overlay.last_scene.as_ref().unwrap().labels.clone();
    engine.handle_backend_event(BackendEvent::AppearanceChanged(Appearance::Light), &mut backend).unwrap();
    assert!(!old.shares_storage_with(&engine.overlay.last_scene.as_ref().unwrap().labels));
}

#[test]
fn key_help_recenters_across_screens_with_static_mode_content() {
    let (mut engine, mut backend, _) = visible_dense_overlay();
    engine.screens.push(Screen {
        bounds: Rect::new(1000.0, -200.0, 1600.0, 1000.0),
        work_area: Rect::new(1000.0, -180.0, 1600.0, 940.0),
        is_primary: false, scale: 1.5, name: None,
    });
    engine.overlay.key_help_visible = true;
    engine.refresh_overlay(&mut backend).unwrap();
    let source = engine.overlay.content.as_ref().unwrap().labels.clone();
    assert!(!engine.overlay.dynamic.follows_cursor_screen);
    engine.cursor = Point::new(1500.0, 300.0);
    engine.refresh_overlay_positions(&mut backend).unwrap();
    let panel = engine.overlay.last_scene.as_ref().unwrap().labels.iter()
        .find(|label| label.text.is_empty() && label.z_index == i32::MAX - 1).unwrap();
    assert!((panel.rect.center().x - engine.screens[1].work_area.center().x).abs() < 0.01);
    assert!(source.shares_storage_with(&engine.overlay.content.as_ref().unwrap().labels));
    assert_eq!(source.len(), 1000);
}

#[test]
#[ignore = "allocation probe; run alone in release"]
fn key_help_cache_close_cycles_release_allocations() {
    let (mut engine, _backend, _log) = visible_normal_overlay();
    let source = OverlayScene::new();
    let region = Region::new(TEST_ALLOCATOR);
    for _ in 0..20_000 {
        engine.overlay.key_help_visible = true;
        {
            let mut scene = source.clone();
            engine.decorate_key_help(&mut scene);
        }
        engine.overlay.key_help_visible = false;
        let mut scene = source.clone();
        engine.decorate_key_help(&mut scene);
        assert!(engine.overlay.key_help_cache.is_none());
    }
    let change = region.change();
    println!("key_help close cycles=20000 allocations={} deallocations={} bytes_allocated={} bytes_deallocated={}",
        change.allocations, change.deallocations, change.bytes_allocated, change.bytes_deallocated);
    assert_eq!(change.allocations, change.deallocations);
    assert_eq!(change.bytes_allocated, change.bytes_deallocated);
}

#[test]
#[ignore = "allocation probe; run alone in release"]
fn key_help_cache_position_updates_allocate_nothing() {
    let (mut engine, mut backend, log) = visible_normal_overlay();
    engine.overlay.key_help_visible = true;
    engine.refresh_overlay(&mut backend).unwrap();
    backend.accept_position_updates = true;
    engine.cursor = Point::new(100.0, 100.0);
    engine.refresh_overlay_positions(&mut backend).unwrap();
    log.lock().unwrap().positions.clear();
    let labels = engine.overlay.last_scene.as_ref().unwrap().labels.clone();
    let region = Region::new(TEST_ALLOCATOR);
    for index in 0..20_000 {
        engine.cursor.x = 100.0 + (index % 500) as f64;
        engine.refresh_overlay_positions(&mut backend).unwrap();
        log.lock().unwrap().positions.clear();
    }
    let change = region.change();
    assert_eq!(change.allocations, 0, "{change:?}");
    assert_eq!(change.deallocations, 0, "{change:?}");
    assert!(labels.shares_storage_with(&engine.overlay.last_scene.as_ref().unwrap().labels));
}

fn character_down(physical: &str, character: char) -> BackendEvent {
    let BackendEvent::Input(mut input) = key_down(physical) else { unreachable!() };
    input.character = Some(character);
    BackendEvent::Input(input)
}

#[test]
fn character_bindings_use_reported_text_without_assuming_a_keyboard_gesture() {
    for character in ['?', ':', '@', '+', '€'] {
        let mut engine = engine_with_normal_binding(&character.to_string(), "key_help");
        engine.registry.active = ModeId::normal();
        let (mut backend, _) = FakeBackend::new(Vec::new());
        engine.screens = backend.screens().unwrap();
        // Deliberately use an unrelated physical key: the parser/resolver must
        // never guess a layout or require Shift for any particular symbol.
        engine.handle_backend_event(character_down("x", character), &mut backend).unwrap();
        assert!(engine.overlay.key_help_visible, "{character}");
        engine.handle_backend_event(key_up("x"), &mut backend).unwrap();
        engine.handle_backend_event(key_down("x"), &mut backend).unwrap();
        assert!(engine.overlay.key_help_visible, "raw x must not act as {character}");
    }
}

#[test]
fn literal_character_held_action_releases_by_physical_key() {
    let mut engine = engine_with_normal_binding(":", "move_left");
    engine.registry.active = ModeId::normal();
    let (mut backend, _) = FakeBackend::new(Vec::new());
    engine.handle_backend_event(key_down("left_shift"), &mut backend).unwrap();
    engine.handle_backend_event(character_down(";", ':'), &mut backend).unwrap();
    assert!(engine.input.active_gestures.get(&Key::new(";").unwrap()).is_some());
    engine.handle_backend_event(key_up("left_shift"), &mut backend).unwrap();
    engine.handle_backend_event(key_up(";"), &mut backend).unwrap();
    assert!(engine.input.active_gestures.is_empty());
}

#[test]
fn character_capture_policy_tracks_startup_and_reload_removal() {
    let mut config = Config::default();
    config.normal.bindings.insert(":".into(), Binding::parse("key_help").unwrap());
    config.normal.bindings.insert("ctrl+@".into(), Binding::parse("key_help").unwrap());
    let mut engine = Engine::from_plan(crate::app::configuration::compile(&config).unwrap(), Appearance::Dark).unwrap();
    let (mut backend, log) = FakeBackend::new(Vec::new());
    engine.run(&mut backend).unwrap();
    assert!(engine.registry.character_keys.contains_key(&':'));
    assert!(!engine.registry.character_keys.contains_key(&'@'), "a modified chord is not a literal character binding");
    let captured = log.lock().unwrap().character_bindings[0].clone();
    assert!(captured.contains(&":".to_string()));
    assert!(!captured.contains(&"@".to_string()));

    config.normal.bindings.remove(":");
    engine.apply_runtime_plan(crate::app::configuration::compile(&config).unwrap(), &mut backend).unwrap();
    assert!(!engine.registry.character_keys.contains_key(&':'));
    assert!(!log.lock().unwrap().character_bindings.last().unwrap().contains(&":".to_string()));
}

#[test]
fn character_capture_policy_tracks_effective_app_overrides_only() {
    let mut config = Config::default();
    config.normal.app_configs.push(crate::config::AppOverride {
        bundle_id: "com.example.editor".into(),
        bindings: Bindings::from([(":".into(), Binding::parse("key_help").unwrap())]),
    });
    let app = |title: &str| BackendEvent::FocusChanged(Some(FocusedApp {
        bundle_id: "com.example.editor".into(), window_title: title.into(), process_id: 7,
    }));
    let mut engine = Engine::from_plan(crate::app::configuration::compile(&config).unwrap(), Appearance::Dark).unwrap();
    let (mut backend, log) = FakeBackend::new(vec![app("one"), app("two"), BackendEvent::FocusChanged(None)]);
    engine.run(&mut backend).unwrap();
    let policies = &log.lock().unwrap().character_bindings;
    assert_eq!(policies.len(), 3, "initial, matching override, and override removal; title-only changes reuse the policy");
    assert!(!policies[0].contains(&":".to_string()));
    assert!(policies[1].contains(&":".to_string()));
    assert!(!policies[2].contains(&":".to_string()));
}

#[test]
fn character_index_preserves_local_disabled_binding_over_inheritance() {
    let mut config = Config::default();
    config.normal.bindings.insert("?".into(), Binding::parse("key_help").unwrap());
    config.grid.bindings.insert("?".into(), Binding::Disabled);
    let mut engine = Engine::new(config.clone(), Appearance::Dark);
    for mode in crate::app::mode_catalog::built_in(&config) { engine.register(mode); }
    let (mut backend, _) = FakeBackend::new(Vec::new());
    engine.activate(ModeId::grid(), Some(ModeId::normal()), &mut backend).unwrap();
    engine.handle_backend_event(character_down("/", '?'), &mut backend).unwrap();
    assert!(!engine.overlay.key_help_visible);
}
