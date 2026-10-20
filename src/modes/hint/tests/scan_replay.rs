//! Replay the actual worker, mailbox, mode and scene together, without native
//! providers or private application data. No timing-dependent sleeps.
use super::*;
use crate::platform::common::{scan_fusion, scan_mailbox, spatial_index::TargetSource};
use std::sync::Arc;
use std::time::Instant;

#[test]
fn page_ax_and_contour_labels_are_visible_before_ocr_completes() {
    let env = Env::new();
    let mut mode = crate::app::mode_catalog::hint(&env.config);
    activate(&mut mode, &env);
    let id = mode.session.scan_id;
    let output = Arc::new(scan_mailbox::ScanMailbox::default());
    let generation = output.begin(id);
    let mut worker = scan_fusion::Worker::default();
    let sink = worker
        .prepare(generation)
        .unwrap()
        .begin(scan_fusion::Options {
            id,
            generation,
            threshold: 0.5,
            output: Arc::clone(&output),
            wake: Arc::new(|| {}),
            accepts: Arc::new(|_| true),
            current: Arc::new(|| true),
        });
    let link = UiTarget {
        rect: Rect::new(100., 200., 100., 24.),
        role: SemanticRole::Link,
        name: "中文链接".into(),
        details: None,
    };
    let contour = UiTarget {
        rect: Rect::new(400., 200., 80., 24.),
        role: SemanticRole::Control,
        name: String::new(),
        details: None,
    };
    // OCR has supplied neither targets nor a terminal status. Each ready source
    // must already reach ShowOverlay while the scan remains active.
    for (source, target) in [
        (TargetSource::Accessibility, link.clone()),
        (TargetSource::Contour, contour),
    ] {
        sink.submit(source, vec![target.clone()]);
        sink.flush();
        let result = output.take().unwrap();
        assert_eq!(result.status, UiScanStatus::Partial);
        let out = mode.handle_owned(ModeEvent::UiScanned(result), &env.ctx());
        assert_fresh_scene(&mode, &env, &out);
        assert!(mode.session.scanning);
        assert!(mode.session.scanned.iter().any(|t| t.rect == target.rect));
    }
    let labels: Vec<_> = mode.session.hints.iter().map(|h| h.label.clone()).collect();
    sink.submit(
        TargetSource::SystemOcr,
        vec![UiTarget::recognized_text(link.rect, "中文链接".into())],
    );
    sink.flush();
    mode.handle_owned(ModeEvent::UiScanned(output.take().unwrap()), &env.ctx());
    assert_eq!(mode.session.scanned.len(), 2);
    assert_eq!(
        mode.session
            .hints
            .iter()
            .map(|h| h.label.clone())
            .collect::<Vec<_>>(),
        labels
    );
    assert!(
        mode.session
            .scanned
            .iter()
            .any(|t| t.role == SemanticRole::Link && t.ocr_text() == "中文链接")
    );
    sink.finish(UiScanStatus::Success);
    sink.flush();
    mode.handle_owned(ModeEvent::UiScanned(output.take().unwrap()), &env.ctx());
    assert!(!mode.session.scanning);
    worker
        .stop_until(Instant::now() + Duration::from_secs(5))
        .unwrap();
}

#[test]
fn metadata_refresh_keeps_plain_scene_and_updates_search() {
    for searching in [false, true] {
        let env = Env::new();
        let mut mode = crate::app::mode_catalog::hint(&env.config);
        activate(&mut mode, &env);
        mode.handle_owned(
            ModeEvent::UiScanned(UiScanResult {
                id: mode.session.scan_id,
                targets: vec![target("old name", 20.), target("other", 100.)],
                retired: Vec::new(),
                status: UiScanStatus::Partial,
            }),
            &env.ctx(),
        );
        mode.session.ensure_search_names();
        if searching {
            press(&mut mode, &env, "/");
            mode.handle(&ModeEvent::TextChanged("updated".into()), &env.ctx());
            assert!(mode.session.hints.is_empty());
        }
        let mut updated = mode.session.scanned[0].clone();
        updated.name = "updated name".into();
        let out = mode.handle_owned(
            ModeEvent::UiScanned(UiScanResult {
                id: mode.session.scan_id,
                retired: vec![updated.rect],
                targets: vec![updated.clone()],
                status: UiScanStatus::Partial,
            }),
            &env.ctx(),
        );
        if searching {
            assert_eq!(mode.session.hints.len(), 1);
            assert_eq!(mode.session.scanned[mode.session.hints[0].value], updated);
            assert_fresh_scene(&mode, &env, &out);
        } else {
            assert!(!out.iter().any(|c| matches!(c, Command::ShowOverlay(_))));
            assert!(out.iter().any(|c| matches!(c, Command::SetTimer { id, .. }
                if id == SEARCH_PREWARM_TIMER_ID)));
            mode.handle(
                &ModeEvent::Timer {
                    id: SEARCH_PREWARM_TIMER_ID.into(),
                    elapsed: Duration::ZERO,
                },
                &env.ctx(),
            );
            assert!(mode.session.search_names_initialized);
            assert!(mode.session.search_text[0].matches("updated", ""));
        }
    }
}

fn sources(scale: f64) -> [Vec<UiTarget>; 3] {
    let rect = |x, y, w, h| Rect::new((x - 300.) * scale, (y - 200.) * scale, w * scale, h * scale);
    let target = |rect, role, name: &str| UiTarget {
        rect,
        role,
        name: name.into(),
        details: None,
    };
    let fields = [
        (rect(180., 112., 220., 20.), "Group"),
        (rect(488., 112., 68., 20.), "20:21"),
        (rect(180., 148., 90., 20.), "sender:"),
        (rect(280., 148., 180., 20.), "Preview message"),
        (rect(510., 148., 46., 20.), "6"),
    ];
    let mut contours = vec![target(rect(108., 112., 56., 56.), SemanticRole::Image, "")];
    for (r, _) in fields {
        for x in [r.x + 2. * scale, r.right() - 14. * scale] {
            contours.push(target(
                Rect::new(x, r.y + 2. * scale, 10. * scale, 16. * scale),
                SemanticRole::Control,
                "",
            ));
        }
    }
    let text = fields
        .into_iter()
        .map(|(r, name)| UiTarget::recognized_text(r, name.into()))
        .collect();
    let native = vec![
        target(
            rect(100., 100., 480., 88.),
            SemanticRole::ListItem,
            "Group, sender: Preview message, 6 unread, 20:21",
        ),
        target(rect(110., 170., 16., 16.), SemanticRole::Checkbox, "select"),
        target(rect(640., 100., 80., 40.), SemanticRole::Button, "Go"),
    ];
    [contours, text, native]
}

#[test]
fn complete_scan_preserves_fields_and_labels_across_orders_chunks_and_mailbox_coalescing() {
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for scale in [1., 2.] {
        for order in orders {
            for chunk in [1, 4, 32] {
                for coalesce in [false, true] {
                    let mut config = Config::default();
                    config.ui_hint.hint_characters = "asdfghjklqwertyuiopzxcvbnm".into();
                    let mut env = Env::with(config);
                    env.screens[0].bounds = Rect::new(-1000., -1000., 6000., 6000.);
                    env.screens[0].work_area = env.screens[0].bounds;
                    env.screens[0].scale = scale;
                    let mut mode = crate::app::mode_catalog::hint(&env.config);
                    activate(&mut mode, &env);
                    let id = mode.session.scan_id;
                    let output = Arc::new(scan_mailbox::ScanMailbox::default());
                    let generation = output.begin(id);
                    let mut worker = scan_fusion::Worker::default();
                    let sink = worker
                        .prepare(generation)
                        .unwrap()
                        .begin(scan_fusion::Options {
                            id,
                            generation,
                            threshold: 0.5,
                            output: Arc::clone(&output),
                            wake: Arc::new(|| {}),
                            accepts: Arc::new(|_| true),
                            current: Arc::new(|| true),
                        });
                    let sources = sources(scale);
                    let provenance = [
                        TargetSource::Contour,
                        TargetSource::SystemOcr,
                        TargetSource::Accessibility,
                    ];
                    let consume = |mode: &mut HintMode| {
                        if let Some(result) = output.take() {
                            let old: Vec<_> = mode
                                .session
                                .hints
                                .iter()
                                .map(|h| (mode.session.scanned[h.value].rect, h.label.clone()))
                                .collect();
                            let out = mode.handle_owned(ModeEvent::UiScanned(result), &env.ctx());
                            for hint in &mode.session.hints {
                                if let Some((_, code)) = old.iter().find(|(rect, _)| {
                                    *rect == mode.session.scanned[hint.value].rect
                                }) {
                                    assert_eq!(
                                        &hint.label, code,
                                        "scale={scale} order={order:?} chunk={chunk} coalesce={coalesce}"
                                    );
                                }
                            }
                            if out.iter().any(|c| matches!(c, Command::ShowOverlay(_))) {
                                assert_fresh_scene(mode, &env, &out);
                            }
                        }
                    };
                    for lane in order {
                        for batch in sources[lane].chunks(chunk) {
                            assert_eq!(sink.submit(provenance[lane], batch.to_vec()), batch.len());
                            sink.flush();
                            if !coalesce {
                                consume(&mut mode);
                            }
                        }
                        consume(&mut mode);
                        assert!(
                            !mode.session.hints.is_empty(),
                            "ready data must precede completion"
                        );
                    }
                    sink.finish(UiScanStatus::Success);
                    sink.flush();
                    consume(&mut mode);
                    assert!(!mode.session.scanning);
                    assert_eq!(mode.session.scanned.len(), 9);
                    for field in &sources[1] {
                        assert!(
                            mode.session
                                .scanned
                                .iter()
                                .any(|t| t.rect == field.rect && t.name == field.name)
                        );
                    }
                    for action in &sources[2] {
                        assert!(
                            mode.session
                                .scanned
                                .iter()
                                .any(|t| t.rect == action.rect && t.role == action.role)
                        );
                    }
                    let labels: Vec<_> = mode
                        .session
                        .hints
                        .iter()
                        .map(|h| (h.label.clone(), h.bounds, h.value))
                        .collect();
                    mode.handle_owned(
                        ModeEvent::UiScanned(UiScanResult {
                            id: id.wrapping_add(1),
                            targets: sources[0].clone(),
                            retired: Vec::new(),
                            status: UiScanStatus::Partial,
                        }),
                        &env.ctx(),
                    );
                    assert_eq!(
                        mode.session
                            .hints
                            .iter()
                            .map(|h| (h.label.clone(), h.bounds, h.value))
                            .collect::<Vec<_>>(),
                        labels,
                        "stale scan must not alter identities"
                    );
                    mode.handle(&ModeEvent::Deactivated, &env.ctx());
                    assert!(mode.session.scanned.is_empty());
                    worker
                        .stop_until(Instant::now() + Duration::from_secs(5))
                        .unwrap();
                }
            }
        }
    }
}

#[test]
fn visual_only_scan_keeps_the_same_regions_without_native_evidence() {
    for order in [[0, 1], [1, 0]] {
        let env = Env::new();
        let mut mode = crate::app::mode_catalog::hint(&env.config);
        activate(&mut mode, &env);
        let mut accumulator = crate::platform::common::scan_accumulator::ScanAccumulator::new();
        let mut input = sources(1.);
        // The same layout moved into the default display bounds.
        for lane in &mut input {
            for target in lane {
                target.rect.x += 500.;
                target.rect.y += 500.;
            }
        }
        for lane in order {
            let source = if lane == 0 {
                TargetSource::Contour
            } else {
                TargetSource::SystemOcr
            };
            for target in input[lane].chunks(1) {
                let update = accumulator.push(source, target.to_vec(), 0.5);
                mode.handle_owned(
                    ModeEvent::UiScanned(UiScanResult {
                        id: mode.session.scan_id,
                        targets: update.batches.into_iter().flatten().collect(),
                        retired: update.retired,
                        status: UiScanStatus::Partial,
                    }),
                    &env.ctx(),
                );
            }
        }
        assert_eq!(mode.session.scanned.len(), 6);
        assert_eq!(
            mode.session
                .scanned
                .iter()
                .filter(|t| t.role == SemanticRole::StaticText)
                .count(),
            5
        );
    }
}
