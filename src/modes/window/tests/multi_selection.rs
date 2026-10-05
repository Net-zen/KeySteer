use super::*;

fn window(id: u64) -> WindowInfo {
    WindowInfo {
        id: WindowId(id),
        title: format!("Window {id}"),
        app: format!("app{id}"),
        bounds: Rect::new(id as f64 * 10.0, 0.0, 300.0, 200.0),
        screen: 0,
        resizable: true,
        maximized: false,
        minimized: false,
        fullscreen: false,
    }
}

fn session(count: u64) -> WindowSession {
    let mut mode = crate::app::mode_catalog::window(&crate::config::Config::default());
    mode.session = 1;
    mode.target = Some(window(1));
    for id in 1..=count {
        mode.inventory.insert(WindowId(id), window(id));
        mode.numbers.insert(WindowId(id), id as u32);
    }
    mode.rebuild_numbers();
    mode
}

fn type_input(mode: &mut WindowSession, text: &str) {
    for c in text.chars() {
        let key = if c == ' ' {
            Key::new("space").unwrap()
        } else {
            Key::new(c.to_string()).unwrap()
        };
        assert!(mode.multi_input_key(&key, false));
    }
}

#[test]
fn multi_selection_input_is_immediate_and_uses_spaces_for_complete_labels() {
    let mut mode = session(123);
    let mut out = CommandBatch::new();
    mode.begin_multi(&mut out);
    type_input(&mut mode, "234567");
    assert_eq!(mode.multi, (1..=7).map(WindowId).collect::<Vec<_>>());
    type_input(&mut mode, " 12 23 123 ");
    assert_eq!(mode.multi, [1, 2, 3, 4, 5, 6, 7, 12, 23, 123].map(WindowId));
    mode.multi_input_key(&Key::new("backspace").unwrap(), false);
    mode.multi_input_key(&Key::new("backspace").unwrap(), false);
    type_input(&mut mode, " ");
    assert_eq!(
        mode.multi,
        [1, 2, 3, 4, 5, 6, 7, 23].map(WindowId),
        "editing 123 to 12 toggles 12 a second time"
    );
    mode.begin_multi(&mut out); // Same entry binding confirms.
    assert!(mode.multi_input.is_none());
    mode.begin_multi(&mut out);
    type_input(&mut mode, "3 23 ");
    assert_eq!(mode.multi, [1, 2, 4, 5, 6, 7].map(WindowId));
    mode.multi_input_key(&Key::new("esc").unwrap(), false);
    assert_eq!(mode.multi, [1, 2, 3, 4, 5, 6, 7, 23].map(WindowId));
}

#[test]
fn multi_selection_continuous_digits_toggle_and_clear_restores_captured_identity() {
    let mut mode = session(9);
    let mut out = CommandBatch::new();
    mode.begin_multi(&mut out);
    type_input(&mut mode, "2345676");
    assert_eq!(
        mode.multi,
        [
            WindowId(1),
            WindowId(2),
            WindowId(3),
            WindowId(4),
            WindowId(5),
            WindowId(7)
        ]
    );
    type_input(&mut mode, "3");
    assert!(!mode.multi.contains(&WindowId(3)));
    mode.settings.target = Some(crate::api::window::WindowTarget::Active);
    mode.clear_multi(&mut out);
    assert_eq!(
        mode.settings.target,
        Some(crate::api::window::WindowTarget::Active)
    );
    assert!(out.iter().any(|c| matches!(c, Command::WindowRequest(r) if r.operation == WindowOperation::Select(WindowId(1)))));
    assert_eq!(mode.operation_targets().as_slice(), &[WindowId(1)]);
}

#[test]
fn multi_selection_tab_members_toggle_one_unit_and_move_resize_once_per_unit() {
    use crate::api::window_tabs::{TabGroup, TabGroupId};
    let mut mode = session(4);
    mode.tabs.state.groups.push(TabGroup {
        id: TabGroupId(1),
        members: vec![WindowId(2), WindowId(3)],
        active: WindowId(3),
    });
    let mut out = CommandBatch::new();
    mode.begin_multi(&mut out);
    type_input(&mut mode, "2");
    assert_eq!(
        mode.operation_targets().as_slice(),
        &[WindowId(1), WindowId(3)]
    );
    for change in [
        WindowChange::Move { dx: 20.0, dy: 0.0 },
        WindowChange::Resize { dw: 20.0, dh: 0.0 },
        WindowChange::ToggleMaximize,
        WindowChange::ToggleMinimize,
        WindowChange::Center,
    ] {
        out = CommandBatch::new();
        mode.adjust(change.clone(), &mut out);
        let targets: Vec<_> = out
            .iter()
            .filter_map(|c| match c {
                Command::WindowRequest(r) => match &r.operation {
                    WindowOperation::Adjust {
                        target,
                        change: actual,
                        ..
                    } => {
                        assert_eq!(actual, &change);
                        Some(*target)
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(targets, [WindowId(3), WindowId(1)]);
    }
    type_input(&mut mode, "3");
    assert_eq!(mode.operation_targets().as_slice(), &[WindowId(1)]);
}

#[test]
fn multi_selection_peer_confirmation_updates_border_without_replacing_anchor() {
    let config = crate::config::Config::default();
    let palette = config.palette(Appearance::Dark);
    let screens = [crate::api::Screen {
        bounds: Rect::new(0.0, 0.0, 1200.0, 900.0),
        work_area: Rect::new(0.0, 0.0, 1200.0, 900.0),
        scale: 1.0,
        is_primary: true,
        name: None,
    }];
    let ctx = HostContext {
        presenter: &crate::presentation::COMPOSER,
        screens: &screens,
        cursor: Point::default(),
        focused_app: None,
        palette: &palette,
    };
    let mut mode = session(3);
    mode.begin_multi(&mut CommandBatch::new());
    type_input(&mut mode, "2");
    let mut peer = window(2);
    peer.bounds.x += 25.0;
    mode.window_result(
        WindowResult {
            session: 1,
            id: 1,
            target: Some(peer.clone()),
            windows: None,
            closed: vec![],
            tabs: None,
            pointer: None,
            changed: 1,
            skipped: 0,
            message: None,
            edit: None,
        },
        &ctx,
    );
    assert_eq!(mode.target.as_ref().unwrap().id, WindowId(1));
    assert_eq!(mode.inventory[&WindowId(2)].bounds, peer.bounds);
    let crate::api::presentation::View::Window(view) = mode.view() else {
        panic!()
    };
    assert_eq!(view.selected, [WindowId(1), WindowId(2)]);
    assert_eq!(view.target.unwrap().id, WindowId(1));
}

#[test]
fn multi_selection_spaced_prefix_never_removes_a_single_digit_until_committed() {
    let mut mode = session(23);
    mode.begin_multi(&mut CommandBatch::new());
    type_input(&mut mode, "123 1");
    assert_eq!(mode.multi, [1, 2, 3].map(WindowId));
    type_input(&mut mode, "2");
    assert_eq!(mode.multi, [1, 2, 3].map(WindowId));
    mode.finish_multi();
    assert_eq!(mode.multi, [1, 2, 3, 12].map(WindowId));
    mode.begin_multi(&mut CommandBatch::new());
    type_input(&mut mode, " 1");
    assert!(mode.multi.contains(&WindowId(1)));
    type_input(&mut mode, " ");
    assert!(!mode.multi.contains(&WindowId(1)));
    mode.clear_multi(&mut CommandBatch::new());
    assert_eq!(mode.operation_targets().as_slice(), &[WindowId(1)]);
}

#[test]
fn multi_selection_editor_scopes_initial_layout_and_held_motion_reaches_every_window() {
    let config = crate::config::Config::default();
    let palette = config.palette(Appearance::Dark);
    let screens = [crate::api::Screen {
        bounds: Rect::new(0.0, 0.0, 1200.0, 900.0),
        work_area: Rect::new(0.0, 0.0, 1200.0, 900.0),
        scale: 1.0,
        is_primary: true,
        name: None,
    }];
    let ctx = HostContext {
        presenter: &crate::presentation::COMPOSER,
        screens: &screens,
        cursor: Point::default(),
        focused_app: None,
        palette: &palette,
    };
    let mut mode = session(4);
    let mut out = CommandBatch::new();
    mode.begin_multi(&mut out);
    type_input(&mut mode, "23");
    mode.finish_multi();
    for resizing in [false, true] {
        mode.size = resizing;
        mode.held.insert(Key::new("h").unwrap(), W::Left);
        out = CommandBatch::new();
        mode.motion(Some(0.1), &ctx, &mut out);
        let targets: Vec<_> = out
            .iter()
            .filter_map(|c| match c {
                Command::WindowRequest(r) => match &r.operation {
                    WindowOperation::Adjust { target, change, .. } => {
                        assert!(if resizing {
                            matches!(change, WindowChange::Resize { dw, .. } if *dw < 0.0)
                        } else {
                            matches!(change, WindowChange::Move { dx, .. } if *dx < 0.0)
                        });
                        Some(*target)
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(targets, [3, 2, 1].map(WindowId));
    }
    mode.kind = WindowKind::Editor;
    out = CommandBatch::new();
    mode.start_edit(true, &ctx, &mut out);
    assert!(out.iter().any(|c| matches!(c, Command::WindowRequest(r) if matches!(&r.operation, WindowOperation::BeginEdit { targets, screen: None, .. } if targets == &[WindowId(1), WindowId(2), WindowId(3)]))));
    let transaction = mode.edit.as_ref().unwrap().transaction;
    out = CommandBatch::new();
    mode.edit_result(
        &WindowEditResult::Started {
            transaction,
            minimums: [1, 2, 3]
                .map(|id| (WindowId(id), Point::new(40.0, 40.0)))
                .to_vec(),
            gap_scale: 1.0,
            screen_scales: vec![1.0],
            full_inventory: false,
        },
        &ctx,
        &mut out,
    );
    let EditModel::Tree(tree) = &mode.edit.as_ref().unwrap().model else {
        panic!()
    };
    let mut targets: Vec<_> = tree.slots().iter().filter_map(|s| s.window).collect();
    targets.sort();
    assert_eq!(targets, [1, 2, 3].map(WindowId));
}

#[test]
fn multi_selection_tab_entry_uses_explicit_selection_instead_of_auto_grouping() {
    use crate::api::window_tabs::TabOperation;
    let mut mode = session(4);
    mode.begin_multi(&mut CommandBatch::new());
    type_input(&mut mode, "23");
    mode.finish_multi();
    mode.kind = WindowKind::Tab;
    let mut out = CommandBatch::new();
    mode.enter_tabs(&mut out);
    assert!(out.iter().any(|c| matches!(c, Command::WindowRequest(r) if r.operation == WindowOperation::Tabs(TabOperation::EnterSelection(vec![WindowId(1), WindowId(2), WindowId(3)])))));
    assert!(!out.iter().any(|c| matches!(c, Command::WindowRequest(r) if matches!(r.operation, WindowOperation::Tabs(TabOperation::Enter { .. })))));
}
