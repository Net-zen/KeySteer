#![cfg(test)]

use super::*;
use crate::api::overlay::{OverlayScene, OverlayShape};

use crate::api::geometry::{Point, Screen};
use crate::config::{Config, GridLayer};

/// Cells of the current area, paired with their keys.
fn cells(mode: &GridMode) -> Vec<(char, Rect)> {
    if mode.controller.session.terminal {
        return Vec::new();
    }
    let Some(area) = mode.controller.session.current() else {
        return Vec::new();
    };
    let layout = mode.controller.layout_at(mode.controller.session.depth());
    layout
        .keys
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, key)| {
            area.subdivision(layout.rows, layout.cols, index)
                .map(|rect| (key, rect))
        })
        .collect()
}

struct Env {
    screens: Vec<Screen>,
    cursor: Point,
    palette: Palette,
    config: Config,
}

fn legacy_config() -> Config {
    let mut config = Config::default();
    config.recursive_grid.grid_cols = 3;
    config.recursive_grid.grid_rows = 3;
    config.recursive_grid.keys = "rtyfghvbn".into();
    config.recursive_grid.max_depth = 10;
    config.recursive_grid.ui.label.font_size = 20;
    config
}

impl Env {
    fn new() -> Self {
        Self::with(legacy_config())
    }
    fn with(config: Config) -> Self {
        Self {
            screens: vec![Screen {
                bounds: Rect::new(0.0, 0.0, 900.0, 900.0),
                work_area: Rect::new(0.0, 0.0, 900.0, 900.0),
                is_primary: true,
                scale: 1.0,
                name: None,
            }],
            cursor: Point::new(450.0, 450.0),
            palette: Palette::default(),
            config,
        }
    }
    fn ctx(&self) -> HostContext<'_> {
        HostContext {
            presenter: &crate::presentation::COMPOSER,
            screens: &self.screens,
            cursor: self.cursor,
            focused_app: None,
            palette: &self.palette,
        }
    }
}

fn activate(mode: &mut GridMode, env: &Env) -> Vec<Command> {
    mode.handle(&ModeEvent::Activated { previous: None }, &env.ctx())
        .into_iter()
        .collect()
}

fn press(mode: &mut GridMode, env: &Env, name: &str) -> Vec<Command> {
    mode.handle(
        &ModeEvent::Key {
            key: Key::new(name).unwrap(),
            state: KeyState::Down,
            repeat: false,
        },
        &env.ctx(),
    )
    .into_iter()
    .collect()
}

fn toggle_follow(mode: &mut GridMode, env: &Env) -> Vec<Command> {
    mode.handle(
        &ModeEvent::Binding {
            binding: Binding::ToggleCursorFollowSelection.into(),
            state: KeyState::Down,
            key: Key::new("`").unwrap(),
        },
        &env.ctx(),
    )
    .into_iter()
    .collect()
}

fn scene_of<'a>(commands: impl IntoIterator<Item = &'a Command>) -> &'a OverlayScene {
    commands
        .into_iter()
        .find_map(|c| match c {
            Command::ShowOverlay(s) => Some(s),
            _ => None,
        })
        .expect("expected an overlay")
}

#[test]
fn default_letters_scale_with_recursive_cells() {
    let env = Env::with(Config::default());
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    let initial = activate(&mut mode, &env);
    let initial_size = scene_of(&initial).labels[0].style.font_size;
    assert_eq!(initial_size, 300.0 * 0.40);
    assert!(!scene_of(&initial).labels[0].style.bold);
    let nested = press(&mut mode, &env, "s");
    let nested_size = scene_of(&nested).labels[0].style.font_size;
    assert_eq!(nested_size, 100.0 * 0.40);
}

#[test]
fn configured_font_size_reaches_recursive_grid_letters() {
    for size in [17, 20, 36] {
        let config = Config::parse(&format!("[recursive_grid.ui]\nfont_size = {size}")).unwrap();
        let env = Env::with(config);
        let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
        let out = activate(&mut mode, &env);
        let labels = &scene_of(&out).labels;
        assert_eq!(labels.len(), 9);
        assert!(
            labels
                .iter()
                .all(|label| label.style.font_size == f64::from(size))
        );
    }
}

#[test]
fn product_defaults_keep_the_three_by_three_recursive_grid_with_follow_enabled() {
    let env = Env::with(Config::default());
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    let out = activate(&mut mode, &env);
    assert_eq!(scene_of(&out).labels.len(), 9);
    assert!(mode.controller.session.cursor_follow_selection);

    toggle_follow(&mut mode, &env);
    assert!(!mode.controller.session.cursor_follow_selection);
    let out = press(&mut mode, &env, "q");
    assert!(
        !out.iter()
            .any(|command| matches!(command, Command::WarpPointer { .. }))
    );
}

#[test]
fn enabling_follow_immediately_warps_to_the_current_recursive_cell_centre() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    toggle_follow(&mut mode, &env);
    press(&mut mode, &env, "g");
    let selected = mode.controller.session.current().unwrap();
    let depth = mode.controller.session.depth();

    let out = toggle_follow(&mut mode, &env);

    assert!(mode.controller.session.cursor_follow_selection);
    assert_eq!(
        mode.controller.session.depth(),
        depth,
        "toggle must not select another layer"
    );
    assert!(out.contains(&Command::warp_to(selected.center())));
    assert!(
        out.iter()
            .any(|command| matches!(command, Command::ShowOverlay(_)))
    );
    assert!(!out.iter().any(|command| matches!(
        command,
        Command::MouseButton { .. } | Command::SwitchMode(_)
    )));
}

#[test]
fn activation_starts_at_the_full_screen() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    assert_eq!(
        mode.controller.session.current(),
        Some(env.screens[0].bounds)
    );
    assert_eq!(mode.controller.session.depth(), 0);
}

#[test]
fn default_grid_is_three_by_three_with_nine_keys() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    assert_eq!(cells(&mode).len(), 9);
}

#[test]
fn selecting_a_cell_narrows_the_area() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);

    // "rtyfghvbn": 'g' is index 4, the centre cell of a 3x3 grid.
    let out = press(&mut mode, &env, "g");
    assert!(out.iter().any(|c| matches!(c, Command::ShowOverlay(_))));
    assert_eq!(scene_of(&out).clip, Some(env.screens[0].bounds));
    assert_eq!(mode.controller.session.depth(), 1);
    assert_eq!(
        mode.controller.session.current(),
        Some(Rect::new(300.0, 300.0, 300.0, 300.0))
    );
}

#[test]
fn each_level_multiplies_precision() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    press(&mut mode, &env, "r"); // top-left
    assert_eq!(
        mode.controller.session.current(),
        Some(Rect::new(0.0, 0.0, 300.0, 300.0))
    );
    press(&mut mode, &env, "r");
    assert_eq!(
        mode.controller.session.current(),
        Some(Rect::new(0.0, 0.0, 100.0, 100.0))
    );
    assert_eq!(mode.controller.session.depth(), 2);
}

#[test]
fn screen_retarget_replays_or_resets_each_recursive_layer() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    press(&mut mode, &env, "t");
    press(&mut mode, &env, "g");
    assert_eq!(mode.controller.session.path.as_slice(), [1, 4]);
    let target = Screen {
        bounds: Rect::new(-1200.0, 100.0, 1200.0, 800.0),
        work_area: Rect::new(-1200.0, 100.0, 1200.0, 800.0),
        is_primary: false,
        scale: 1.0,
        name: None,
    };

    let out = mode.handle(
        &ModeEvent::ScreenRetargeted {
            screen: target.clone(),
            preserve: true,
        },
        &env.ctx(),
    );
    assert_eq!(mode.controller.session.path.as_slice(), [1, 4]);
    assert_eq!(mode.controller.session.depth(), 2);
    assert_eq!(scene_of(&out).clip, Some(target.bounds));
    assert!(out.contains(&Command::warp_to(
        mode.controller.session.current().unwrap().center()
    )));

    mode.handle(
        &ModeEvent::ScreenRetargeted {
            screen: target.clone(),
            preserve: false,
        },
        &env.ctx(),
    );
    assert!(mode.controller.session.path.is_empty());
    assert_eq!(mode.controller.session.depth(), 0);
    assert_eq!(mode.controller.session.current(), Some(target.bounds));
}

#[test]
fn backspace_widens_and_then_dismisses_at_the_root() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    press(&mut mode, &env, "g");
    assert_eq!(mode.controller.session.depth(), 1);

    press(&mut mode, &env, "backspace");
    assert_eq!(mode.controller.session.depth(), 0);

    let out = press(&mut mode, &env, "backspace");
    assert_eq!(out, Command::dismiss_to_idle());
}

#[test]
fn space_resets_to_the_root() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    press(&mut mode, &env, "g");
    press(&mut mode, &env, "g");
    assert_eq!(mode.controller.session.depth(), 2);

    press(&mut mode, &env, "space");
    assert_eq!(mode.controller.session.depth(), 0);
    assert_eq!(
        mode.controller.session.current(),
        Some(env.screens[0].bounds)
    );
}

#[test]
fn enter_moves_to_the_centre_without_clicking() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    press(&mut mode, &env, "g");

    let out = press(&mut mode, &env, "enter");
    assert!(
        out.iter()
            .any(|c| matches!(c, Command::WarpPointer { x, y } if *x == 450.0 && *y == 450.0)),
        "{out:?}"
    );
    assert!(
        !out.iter()
            .any(|command| matches!(command, Command::MouseButton { .. }))
    );
    assert!(
        !out.iter()
            .any(|command| matches!(command, Command::SwitchMode(_)))
    );
}

#[test]
fn max_depth_marks_the_selected_cell_as_terminal() {
    let mut config = legacy_config();
    config.recursive_grid.max_depth = 1;
    let env = Env::with(config);
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);

    let out = press(&mut mode, &env, "g");
    assert!(out.iter().any(|c| matches!(c, Command::WarpPointer { .. })));
    assert!(mode.controller.session.terminal);
    assert!(!out.contains(&Command::SwitchMode(ModeId::idle())));
}

#[test]
fn min_cell_size_stops_further_subdivision() {
    let mut config = legacy_config();
    config.recursive_grid.min_size_width = 200;
    config.recursive_grid.min_size_height = 200;
    let env = Env::with(config);
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);

    // The selected 300×300 cell cannot produce another 3×3 layer whose
    // cells meet the 200px minimum, so it is terminal immediately.
    let out = press(&mut mode, &env, "g");
    assert!(out.iter().any(|c| matches!(c, Command::WarpPointer { .. })));
    assert_eq!(mode.controller.session.depth(), 1);
    assert!(mode.controller.session.terminal);
}

#[test]
fn layers_override_the_shape_at_a_given_depth() {
    let mut config = legacy_config();
    config.recursive_grid.layers = vec![GridLayer {
        depth: 0,
        grid_cols: Some(2),
        grid_rows: Some(2),
        keys: Some("crtn".into()),
    }];
    let env = Env::with(config);
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);

    assert_eq!(cells(&mode).len(), 4);
    press(&mut mode, &env, "c");
    assert_eq!(
        mode.controller.session.current(),
        Some(Rect::new(0.0, 0.0, 450.0, 450.0))
    );
    // Depth 1 has no override, so it reverts to the base 3x3.
    assert_eq!(cells(&mode).len(), 9);
}

#[test]
fn unknown_keys_are_ignored() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    let out = press(&mut mode, &env, "z");
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(mode.controller.session.depth(), 0);
}

#[test]
fn label_char_replaces_every_cell_key() {
    let mut config = legacy_config();
    config.recursive_grid.ui.label_char = "\u{B7}".into();
    let env = Env::with(config);
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    let out = activate(&mut mode, &env);

    let scene = scene_of(&out);
    assert!(!scene.labels.is_empty());
    assert!(scene.labels.iter().all(|l| l.text == "\u{B7}"));
}

#[test]
fn sub_key_preview_adds_a_second_label_per_cell() {
    let mut config = legacy_config();
    config.recursive_grid.ui.sub_key_preview = true;
    let env = Env::with(config);
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    let out = activate(&mut mode, &env);

    let scene = scene_of(&out);
    // Nine cell keys plus nine previews.
    assert_eq!(scene.labels.len(), 18, "{:?}", scene.labels.len());
    assert!(scene.labels.iter().any(|l| l.text == "rtyfghvbn"));
}

#[test]
fn crossing_displays_restarts_recursive_grid_on_the_cursor_screen() {
    let mut env = Env::new();
    env.screens.push(Screen {
        bounds: Rect::new(900.0, 0.0, 1200.0, 800.0),
        work_area: Rect::new(900.0, 0.0, 1200.0, 800.0),
        is_primary: false,
        scale: 2.0,
        name: None,
    });
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    press(&mut mode, &env, "g");
    assert_eq!(mode.controller.session.depth(), 1);

    env.cursor = Point::new(1500.0, 400.0);
    let out = mode.handle(&ModeEvent::PointerMoved(env.cursor), &env.ctx());

    assert_eq!(
        mode.controller.session.stack.as_slice(),
        [env.screens[1].bounds]
    );
    assert_eq!(mode.controller.session.depth(), 0);
    assert_eq!(scene_of(&out).clip, Some(env.screens[1].bounds));
}

#[test]
fn labels_shrink_before_they_reach_the_autohide_threshold() {
    let mut config = legacy_config();
    config.recursive_grid.ui.label.font_size = 20;
    config.recursive_grid.ui.label_min_font_size = 6;
    let env = Env::with(config);
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    let initial = activate(&mut mode, &env);
    let initial_font = scene_of(&initial).labels[0].style.font_size;

    press(&mut mode, &env, "g");
    let out = press(&mut mode, &env, "g");
    let labels = &scene_of(&out).labels;
    assert!(!labels.is_empty());
    assert!(labels.iter().all(|label| {
        label.style.font_size < initial_font
            && label.style.font_size >= env.config.recursive_grid.ui.label_min_font_size as f64
    }));
}

#[test]
fn labels_autohide_once_fitting_would_make_them_too_small() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    press(&mut mode, &env, "g");
    press(&mut mode, &env, "g");
    let out = press(&mut mode, &env, "g");
    assert!(
        scene_of(&out).labels.is_empty(),
        "tiny cells should hide their labels"
    );
}

#[test]
fn scene_draws_interior_rulings() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    let out = activate(&mut mode, &env);
    let lines = scene_of(&out)
        .shapes
        .iter()
        .filter(|s| matches!(s, OverlayShape::Line { .. }))
        .count();
    // A 3x3 grid has two vertical and two horizontal rulings.
    assert_eq!(lines, 4);
}

#[test]
fn click_keeps_the_recursive_grid_live_for_further_subdivision() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    activate(&mut mode, &env);
    press(&mut mode, &env, "g");
    let selected = mode.controller.session.current();

    let clicked = mode.handle(
        &ModeEvent::Clicked {
            button: crate::api::MouseButton::Left,
            action: crate::api::ButtonAction::Click,
        },
        &env.ctx(),
    );
    assert!(clicked.is_empty());
    assert!(!mode.controller.session.finished);
    assert_eq!(mode.controller.session.current(), selected);

    let continued = press(&mut mode, &env, "g");
    assert!(!mode.controller.session.finished);
    assert_eq!(mode.controller.session.depth(), 2);
    assert!(
        continued
            .iter()
            .any(|command| matches!(command, Command::ShowOverlay(_)))
    );
}

#[test]
fn restart_resets_the_session_but_preserves_its_return_mode() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::recursive_grid(&env.config);
    mode.handle(
        &ModeEvent::Activated {
            previous: Some(ModeId::normal()),
        },
        &env.ctx(),
    );
    press(&mut mode, &env, "g");
    mode.handle(&ModeEvent::Restarted, &env.ctx());

    assert_eq!(mode.controller.session.depth(), 0);
    assert!(!mode.controller.session.finished);
    assert_eq!(mode.controller.session.return_mode, ModeId::normal());
}
