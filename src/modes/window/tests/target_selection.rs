use super::*;
use crate::api::window::WindowTarget;
use std::sync::Arc;

fn window(id: u64) -> WindowInfo {
    WindowInfo {
        id: WindowId(id),
        title: format!("Window {id}"),
        app: "test".into(),
        bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
        screen: 0,
        resizable: true,
        maximized: false,
        minimized: false,
        fullscreen: false,
    }
}

fn binding(text: &str, state: KeyState, key: &str) -> ModeEvent {
    ModeEvent::Binding {
        binding: Arc::new(Binding::parse(text).unwrap()),
        state,
        key: Key::new(key).unwrap(),
    }
}

fn response(
    request: &crate::api::window::WindowRequest,
    target: Option<WindowInfo>,
    error: bool,
) -> ModeEvent {
    ModeEvent::WindowResult(Box::new(WindowResult {
        tabs: None,
        session: request.session,
        id: request.id,
        target,
        windows: None,
        closed: vec![],
        pointer: None,
        changed: 0,
        skipped: 0,
        message: error.then(|| "lookup failed".into()),
        edit: None,
    }))
}

fn request(commands: &CommandBatch) -> crate::api::window::WindowRequest {
    commands
        .iter()
        .find_map(|c| {
            if let Command::WindowRequest(r) = c {
                Some((**r).clone())
            } else {
                None
            }
        })
        .unwrap()
}

#[test]
fn targeted_gesture_resolves_once_handles_early_release_and_reselects_next_press() {
    let config = crate::config::Config::default();
    let palette = config.palette(Appearance::Dark);
    let ctx = HostContext {
        presenter: &crate::presentation::COMPOSER,
        screens: &[],
        cursor: Point::default(),
        focused_app: None,
        palette: &palette,
    };
    for early_release in [false, true] {
        let mut mode = crate::app::mode_catalog::window(&config);
        mode.session = 1;
        mode.request = 10;
        mode.result = 10;
        mode.target = Some(window(1));
        let down = binding("window_left mouse", KeyState::Down, "h");
        let up = binding("window_left mouse", KeyState::Up, "h");
        let lookup = request(&mode.handle(&down, &ctx));
        assert_eq!(
            lookup.operation,
            WindowOperation::ResolveTarget(WindowTarget::Mouse)
        );
        assert!(
            !mode
                .handle(&down, &ctx)
                .iter()
                .any(|c| matches!(c, Command::WindowRequest(_)))
        );
        if early_release {
            mode.handle(&up, &ctx);
        }
        let commands = mode.handle(&response(&lookup, Some(window(2)), false), &ctx);
        assert!(commands.iter().any(|c| matches!(c, Command::WindowRequest(r) if matches!(r.operation, WindowOperation::Adjust { target: WindowId(2), .. }))));
        assert_eq!(mode.held.is_empty(), early_release);
        if !early_release {
            let mut frames = CommandBatch::new();
            mode.motion(Some(0.016), &ctx, &mut frames);
            assert!(frames.iter().all(|c| !matches!(c, Command::WindowRequest(r) if matches!(r.operation, WindowOperation::ResolveTarget(_)))));
            mode.handle(&up, &ctx);
        }
        assert!(mode.selection.is_none());
        assert_eq!(
            request(&mode.handle(&down, &ctx)).operation,
            WindowOperation::ResolveTarget(WindowTarget::Mouse)
        );
    }
}

#[test]
fn targeted_close_does_not_use_old_target_on_failure_or_after_exit() {
    let config = crate::config::Config::default();
    let palette = config.palette(Appearance::Dark);
    let ctx = HostContext {
        presenter: &crate::presentation::COMPOSER,
        screens: &[],
        cursor: Point::default(),
        focused_app: None,
        palette: &palette,
    };
    for (failed, exit) in [(true, false), (false, true), (false, false)] {
        let mut mode = crate::app::mode_catalog::window(&config);
        mode.session = 1;
        mode.request = 10;
        mode.result = 10;
        mode.target = Some(window(1));
        let lookup =
            request(&mode.handle(&binding("window_close active", KeyState::Down, "x"), &ctx));
        if exit {
            mode.handle(&ModeEvent::Deactivated, &ctx);
        }
        let commands = mode.handle(&response(&lookup, Some(window(2)), failed), &ctx);
        let close = commands.iter().find_map(|c| match c {
            Command::WindowRequest(r) => match r.operation {
                WindowOperation::Close(id) => Some(id),
                _ => None,
            },
            _ => None,
        });
        assert_eq!(close, (!failed && !exit).then_some(WindowId(2)));
    }
}

#[test]
fn target_queue_preserves_rapid_independent_shortcuts() {
    let config = crate::config::Config::default();
    let palette = config.palette(Appearance::Dark);
    let ctx = HostContext {
        presenter: &crate::presentation::COMPOSER,
        screens: &[],
        cursor: Point::default(),
        focused_app: None,
        palette: &palette,
    };
    let mut mode = crate::app::mode_catalog::window(&config);
    mode.session = 1;
    mode.request = 10;
    mode.result = 10;
    mode.target = Some(window(1));
    let first = request(&mode.handle(&binding("window_center active", KeyState::Down, "a"), &ctx));
    assert!(
        !mode
            .handle(&binding("window_close mouse", KeyState::Down, "b"), &ctx)
            .iter()
            .any(|c| matches!(c, Command::WindowRequest(_)))
    );
    let commands = mode.handle(&response(&first, Some(window(2)), false), &ctx);
    let second = commands
        .iter()
        .find_map(|c| match c {
            Command::WindowRequest(r)
                if r.operation == WindowOperation::ResolveTarget(WindowTarget::Mouse) =>
            {
                Some((**r).clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(commands.iter().any(|c| matches!(c, Command::WindowRequest(r) if matches!(r.operation, WindowOperation::Adjust {target: WindowId(2), ..}))));
    let commands = mode.handle(&response(&second, Some(window(3)), false), &ctx);
    assert!(commands.iter().any(|c| matches!(c, Command::WindowRequest(r) if r.operation == WindowOperation::Close(WindowId(3)))));
}

#[test]
fn targeted_application_volume_repeats_keep_identity_without_requerying() {
    use crate::api::audio::AudioTarget;
    let config = crate::config::Config::default();
    let palette = config.palette(Appearance::Dark);
    let ctx = HostContext {
        presenter: &crate::presentation::COMPOSER,
        screens: &[],
        cursor: Point::default(),
        focused_app: None,
        palette: &palette,
    };
    let mut mode = crate::app::mode_catalog::window(&config);
    mode.session = 1;
    mode.request = 10;
    mode.result = 10;
    mode.target = Some(window(1));
    let down = binding("window_volume_down mouse", KeyState::Down, "j");
    let lookup = request(&mode.handle(&down, &ctx));
    mode.handle(&response(&lookup, Some(window(2)), false), &ctx);
    mode.target = Some(window(3));
    for _ in 0..3 {
        let commands = mode.handle(&down, &ctx);
        assert!(
            !commands
                .iter()
                .any(|c| matches!(c, Command::WindowRequest(_)))
        );
        assert!(commands.iter().any(|c| matches!(c, Command::AudioRequest(r) if r.target == AudioTarget::Application(WindowId(2)))));
    }
    mode.handle(
        &binding("window_volume_down mouse", KeyState::Up, "j"),
        &ctx,
    );
    assert!(mode.targeted_audio.is_none());
    assert_eq!(
        request(&mode.handle(&down, &ctx)).operation,
        WindowOperation::ResolveTarget(WindowTarget::Mouse)
    );
}

#[test]
fn suspended_target_lookup_cannot_replace_the_preserved_window() {
    let config = crate::config::Config::default();
    let palette = config.palette(Appearance::Dark);
    let ctx = HostContext {
        presenter: &crate::presentation::COMPOSER,
        screens: &[],
        cursor: Point::default(),
        focused_app: None,
        palette: &palette,
    };
    let mut mode = crate::app::mode_catalog::window(&config);
    mode.session = 1;
    mode.request = 10;
    mode.result = 10;
    mode.target = Some(window(1));
    let lookup = request(&mode.handle(&binding("window_left mouse", KeyState::Down, "h"), &ctx));
    mode.handle(&ModeEvent::Suspended, &ctx);
    let commands = mode.handle(&response(&lookup, Some(window(2)), false), &ctx);
    assert!(commands.is_empty());
    assert_eq!(mode.target.as_ref().unwrap().id, WindowId(1));
    assert!(mode.ignored_selections.is_none());
}
