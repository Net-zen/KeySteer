#![cfg(test)]

use super::*;
use crate::api::{Palette, Point, Screen};

struct Env {
    screens: Vec<Screen>,
    palette: Palette,
}
impl Env {
    fn new() -> Self {
        Self {
            screens: vec![Screen {
                bounds: Rect::new(0., 0., 1200., 900.),
                work_area: Rect::new(0., 0., 1200., 900.),
                is_primary: true,
                scale: 1.,
                name: None,
            }],
            palette: Palette::default(),
        }
    }
    fn ctx(&self) -> HostContext<'_> {
        HostContext {
            presenter: &crate::presentation::COMPOSER,
            screens: &self.screens,
            cursor: Point::new(100., 100.),
            focused_app: None,
            palette: &self.palette,
        }
    }
}
fn mode() -> TextSelectMode {
    crate::app::mode_catalog::text_select(&crate::config::Config::default())
}
fn settle(mode: &mut TextSelectMode, env: &Env) {
    for _ in 0..50 {
        if mode.search.search_names_initialized && (!mode.source.pending_relabel || mode.frozen) {
            return;
        }
        mode.maintain(&env.ctx());
    }
    panic!("text search preparation did not converge");
}
fn deliver(mode: &mut TextSelectMode, env: &Env, status: UiScanStatus, text: &str) -> CommandBatch {
    let commands = mode.handle_owned(
        ModeEvent::UiScanned(UiScanResult {
            id: mode.scan_id,
            retired: Vec::new(),
            targets: vec![UiTarget::recognized_text(
                Rect::new(10., 10., 300., 20.),
                text.into(),
            )],
            status,
        }),
        &env.ctx(),
    );
    settle(mode, env);
    commands
}
fn words(mode: &mut TextSelectMode, env: &Env, text: &str, ranges: Vec<std::ops::Range<usize>>) {
    mode.start(&env.ctx());
    deliver(mode, env, UiScanStatus::Success, text);
    mode.insert("a", &env.ctx());
    mode.action(Action::Refine, &env.ctx());
    mode.segmented(
        Segmented {
            id: mode.segment_id,
            words: ranges,
        },
        &env.ctx(),
    );
    settle(mode, env);
}

#[test]
fn first_hints_and_early_input_paint_before_bounded_search_prewarm() {
    let env = Env::new();
    let mut mode = mode();
    mode.start(&env.ctx());
    mode.insert("hello", &env.ctx());
    assert!(!mode.frozen);
    let commands = mode.scan(
        UiScanResult {
            id: mode.scan_id,
            retired: vec![],
            status: UiScanStatus::Partial,
            targets: vec![UiTarget::recognized_text(
                Rect::new(10., 10., 300., 20.),
                "hello world".into(),
            )],
        },
        &env.ctx(),
    );
    assert_eq!(mode.search.search_query, "hello");
    assert!(mode.search.search_text.is_empty());
    assert!(mode.search.search_pending);
    let paint = commands
        .iter()
        .position(|c| matches!(c, Command::ShowOverlay(_)))
        .unwrap();
    let warm = commands
        .iter()
        .position(|c| matches!(c, Command::SetTimer { .. }))
        .unwrap();
    assert!(paint < warm);
    settle(&mut mode, &env);
    assert_eq!(mode.output, "hello world");
    assert!(mode.frozen);
}

#[test]
fn an_early_unmatched_query_receives_matching_late_ocr() {
    let env = Env::new();
    let mut mode = mode();
    mode.start(&env.ctx());
    mode.insert("hello", &env.ctx());
    deliver(&mut mode, &env, UiScanStatus::Partial, "unrelated text");
    assert!(mode.output.is_empty());
    assert!(!mode.frozen);
    mode.scan(
        UiScanResult {
            id: mode.scan_id,
            retired: vec![],
            status: UiScanStatus::Success,
            targets: vec![UiTarget::recognized_text(
                Rect::new(600., 100., 200., 20.),
                "hello world".into(),
            )],
        },
        &env.ctx(),
    );
    settle(&mut mode, &env);
    assert_eq!(mode.search.search_query, "hello");
    assert_eq!(mode.output, "hello world");
    assert!(mode.frozen);
}

#[test]
fn explicit_labels_work_without_preparing_names_and_plain_c_remains_input() {
    let env = Env::new();
    let mut mode = mode();
    mode.start(&env.ctx());
    mode.scan(
        UiScanResult {
            id: mode.scan_id,
            retired: vec![],
            status: UiScanStatus::Success,
            targets: vec![UiTarget::recognized_text(
                Rect::new(10., 10., 300., 20.),
                "copy".into(),
            )],
        },
        &env.ctx(),
    );
    mode.insert("@a", &env.ctx());
    assert_eq!(mode.output, "copy");
    assert!(mode.search.search_text.is_empty());
    let c = Key::new("c").unwrap();
    assert!(
        mode.keyboard_prompt_binding(&c, std::slice::from_ref(&c))
            .is_none()
    );
    mode.action(Action::Search, &env.ctx());
    mode.insert("c", &env.ctx());
    assert_eq!(mode.search.search_query, "c");
    assert_eq!(mode.frame.level, Level::Regions);
    settle(&mut mode, &env);
    assert_eq!(mode.output, "copy");
}

#[test]
fn ocr_only_entry_uses_english_indicator_and_text_only_panel_without_clicking() {
    let env = Env::new();
    let mut mode = mode();
    let commands = mode.start(&env.ctx());
    let request = commands
        .iter()
        .find_map(|c| {
            if let Command::ScanUi(r) = c {
                Some(r)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(request.strategy, UiScanStrategy::Vision);
    assert!(request.vision.detect_text && request.vision.text_positions);
    assert!(!request.vision.detect_rectangles);
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, Command::OpenTextPrompt(_)))
    );
    assert_eq!(mode.display_name(), "Text Select");
    assert_eq!(mode.indicator_detail().as_deref(), Some("Regions"));
    deliver(&mut mode, &env, UiScanStatus::Partial, "键盘选择文字");
    let selected = mode.insert("a", &env.ctx());
    assert_eq!(mode.output, "键盘选择文字");
    assert!(
        selected
            .iter()
            .any(|c| matches!(c, Command::WarpPointer { .. }))
    );
    assert!(
        !selected
            .iter()
            .any(|c| matches!(c, Command::MouseButton { .. } | Command::SwitchMode(_)))
    );
    let scene = selected
        .iter()
        .find_map(|c| {
            if let Command::ShowOverlay(s) = c {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    assert!(
        scene
            .labels
            .iter()
            .any(|l| l.text.as_str() == "键盘选择文字")
    );
    assert!(
        !scene
            .labels
            .iter()
            .any(|l| l.text.as_str().contains("文字选择") || l.text.as_str().contains("Ctrl+1"))
    );
    deliver(&mut mode, &env, UiScanStatus::Success, "迟到的识别结果");
    assert_eq!(mode.output, "键盘选择文字");
}

#[test]
fn space_separated_hints_and_search_terms_use_the_uihint_union_and_editing() {
    let env = Env::new();
    let mut mode = mode();
    words(&mut mode, &env, "one two three", vec![0..3, 4..7, 8..13]);
    mode.insert("d a d", &env.ctx());
    assert_eq!(
        mode.output, "one three",
        "deduplicated selection uses source reading order"
    );
    mode.action(Action::Search, &env.ctx());
    mode.insert("@a @d", &env.ctx());
    assert_eq!(mode.output, "one three");
    mode.handle(&ModeEvent::TextEdit(EditAction::Backspace), &env.ctx());
    assert_eq!(mode.search.search_query, "@a @");
    assert_eq!(mode.output, "one");
    assert_eq!(
        mode.search.hints.len(),
        3,
        "standalone @ reveals all hints like UIHint"
    );
    mode.action(Action::Search, &env.ctx());
    mode.insert("one two", &env.ctx());
    assert_eq!(mode.output, "one two");
    mode.handle(&ModeEvent::TextEdit(EditAction::SelectAll), &env.ctx());
    mode.handle(&ModeEvent::TextEdit(EditAction::Backspace), &env.ctx());
    assert_eq!(
        mode.output, "one two three",
        "empty input restores inherited selection"
    );
}

#[test]
fn word_hints_use_original_positions_and_character_level_alone_enlarges_in_panel() {
    let env = Env::new();
    let mut mode = mode();
    words(
        &mut mode,
        &env,
        "使用键盘选择文字",
        vec![0..6, 6..12, 12..18, 18..24],
    );
    assert_eq!(mode.search.hints.len(), 4);
    assert!(
        mode.search
            .hints
            .windows(2)
            .all(|pair| pair[0].bounds.right() <= pair[1].bounds.x + 0.001)
    );
    let Command::ShowOverlay(scene) = mode.render(&env.ctx()) else {
        panic!()
    };
    let scene = scene.materialized();
    assert!(
        scene
            .labels
            .iter()
            .any(|label| label.text.as_str().eq_ignore_ascii_case("a") && label.rect.y < 100.)
    );
    mode.insert("a d", &env.ctx());
    assert_eq!(mode.output, "使用选择");
    mode.action(Action::Refine, &env.ctx());
    settle(&mut mode, &env);
    assert_eq!(mode.frame.level, Level::Characters);
    let Command::ShowOverlay(scene) = mode.render(&env.ctx()) else {
        panic!()
    };
    let scene = scene.materialized();
    assert!(
        !scene
            .labels
            .iter()
            .any(|label| label.text.as_str().eq_ignore_ascii_case("a") && label.rect.y < 100.)
    );
    mode.insert("s f", &env.ctx());
    assert_eq!(mode.output, "用择");
    mode.navigate(false, &env.ctx());
    assert_eq!(mode.output, "使用选择");
    mode.navigate(false, &env.ctx());
    assert_eq!(mode.output, "使用键盘选择文字");
    mode.action(Action::Refine, &env.ctx());
    let stale = mode.segment_id;
    mode.navigate(true, &env.ctx());
    assert!(
        mode.segmented(
            Segmented {
                id: stale,
                words: std::iter::once(0..24).collect()
            },
            &env.ctx()
        )
        .is_empty()
    );
    assert_eq!(mode.frame.level, Level::Regions);
    assert!(mode.output.is_empty());
}

#[test]
fn input_caret_reuses_hint_scene_and_changed_results_replace_it() {
    let env = Env::new();
    let mut mode = mode();
    words(&mut mode, &env, "one two three", vec![0..3, 4..7, 8..13]);
    let Command::ShowOverlay(first) = mode.render(&env.ctx()) else {
        panic!()
    };
    mode.handle(&ModeEvent::TextEdit(EditAction::Home), &env.ctx());
    let Command::ShowOverlay(moved) = mode.render(&env.ctx()) else {
        panic!()
    };
    assert!(Arc::ptr_eq(
        &first.retained.as_ref().unwrap().scene,
        &moved.retained.as_ref().unwrap().scene
    ));
    mode.insert("two", &env.ctx());
    let Command::ShowOverlay(filtered) = mode.render(&env.ctx()) else {
        panic!()
    };
    assert!(!Arc::ptr_eq(
        &first.retained.as_ref().unwrap().scene,
        &filtered.retained.as_ref().unwrap().scene
    ));
    let labels = &filtered.retained.as_ref().unwrap().scene.labels;
    assert_eq!(labels.len(), 1);
    assert!(labels[0].text.as_str().eq_ignore_ascii_case("s"));
}

#[test]
fn pinyin_ranking_and_copy_preview_match_uihint_and_wait_for_copy_acknowledgement() {
    let env = Env::new();
    let mut mode = mode();
    words(&mut mode, &env, "设置搜索", vec![0..6, 6..12]);
    mode.insert("static_text", &env.ctx());
    assert!(
        mode.output.is_empty(),
        "text selection must not search synthetic role metadata"
    );
    mode.action(Action::Search, &env.ctx());
    mode.insert("sz", &env.ctx());
    assert_eq!(mode.output, "设置");
    let commands = mode.action(Action::Copy, &env.ctx());
    assert_eq!(
        commands,
        CommandBatch::one(Command::CopyText("设置".into()))
    );
    assert!(mode.active);
    assert!(
        mode.handle(&ModeEvent::TextCopied, &env.ctx())
            .iter()
            .any(|c| matches!(c, Command::SwitchMode(_)))
    );
}

#[test]
fn late_batches_coalesce_and_deactivation_cancels_maintenance() {
    let env = Env::new();
    let mut mode = mode();
    mode.start(&env.ctx());
    deliver(&mut mode, &env, UiScanStatus::Partial, "first");
    let commands = mode.scan(
        UiScanResult {
            id: mode.scan_id,
            retired: vec![],
            status: UiScanStatus::Success,
            targets: vec![UiTarget::recognized_text(
                Rect::new(600., 100., 200., 20.),
                "second".into(),
            )],
        },
        &env.ctx(),
    );
    assert_eq!(mode.frame.units.len(), 1);
    assert!(
        commands
            .iter()
            .all(|c| matches!(c, Command::SetTimer { .. }))
    );
    settle(&mut mode, &env);
    assert_eq!(mode.frame.units.len(), 2);
    let commands = mode.handle(&ModeEvent::Deactivated, &env.ctx());
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, Command::CancelTimer { id } if id == MAINTENANCE))
    );
    assert!(
        mode.handle(
            &ModeEvent::Timer {
                id: MAINTENANCE.into(),
                elapsed: Duration::ZERO
            },
            &env.ctx()
        )
        .is_empty()
    );
}
