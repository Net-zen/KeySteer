use super::*;

#[test]
fn hybrid_always_combines_sources_and_ignores_legacy_visual_caps() {
    use crate::api::{UiScanStrategy, VisionOptions};
    for detect_text in [false, true] {
        for detect_rectangles in [false, true] {
            let options = VisionOptions {
                detect_text,
                detect_rectangles,
                rectangle_max_candidates: 100,
                ..Default::default()
            };
            assert_eq!(sources(UiScanStrategy::Hybrid, &options), (true, true));
            assert_eq!(
                candidate_limit(UiScanStrategy::Hybrid, &options),
                MAX_UI_SCAN_TARGETS
            );
            assert_eq!(
                sources(UiScanStrategy::Vision, &options),
                (detect_text, detect_rectangles)
            );
            assert_eq!(candidate_limit(UiScanStrategy::Vision, &options), 100);
            assert_eq!(sources(UiScanStrategy::Contour, &options), (false, true));
            assert_eq!(candidate_limit(UiScanStrategy::Contour, &options), 100);
            assert_eq!(sources(UiScanStrategy::AxTree, &options), (false, false));
        }
    }
}

fn scene(w: usize, h: usize) -> Vec<u8> {
    let mut gray = vec![245; w * h];
    for (x, y, bw, bh) in [(30, 20, 60, 25), (210, 220, 110, 40), (480, 60, 50, 35)] {
        for row in y..(y + bh).min(h) {
            for col in x..(x + bw).min(w) {
                gray[row * w + col] = 10;
            }
        }
    }
    gray
}

#[test]
fn tiled_edges_equal_full_frame_including_seams_and_outer_edges() {
    for (w, h) in [(513, 517), (257, 259), (40, 40)] {
        let gray = scene(w, h);
        let mut reference = EdgeScratch {
            gray: gray.clone(),
            ..Default::default()
        };
        threshold_edges(&mut reference, w, h, &|| false).unwrap();
        hysteresis(&mut reference.state, w, h, &|| false).unwrap();
        assert_eq!(
            tiled_edges(&gray, w, h, &|| false).unwrap(),
            reference.state
        );
    }
}

#[test]
fn cross_tile_button_is_one_target_and_maps_to_negative_desktop_coordinates() {
    let (w, h) = (513, 517);
    let gray = scene(w, h);
    let local = detect(
        &gray,
        w,
        h,
        Rect::new(0.0, 0.0, w as f64, h as f64),
        2000,
        || false,
    );
    assert_eq!(local.len(), 3, "{local:?}");
    assert_eq!(
        local
            .iter()
            .filter(|t| t.rect.contains(&crate::api::Point::new(265.0, 240.0)))
            .count(),
        1
    );
    let global = detect(
        &gray,
        w,
        h,
        Rect::new(-1000.0, -200.0, w as f64, h as f64),
        2000,
        || false,
    );
    for (a, b) in local.iter().zip(&global) {
        assert_eq!(b.rect.x, a.rect.x - 1000.0);
        assert_eq!(b.rect.y, a.rect.y - 200.0);
        assert_eq!(b.rect.width, a.rect.width);
        assert!(b.name.is_empty());
    }
    assert_eq!(
        detect(
            &gray,
            w,
            h,
            Rect::new(0.0, 0.0, w as f64, h as f64),
            1,
            || false
        )
        .len(),
        1
    );
}

#[test]
fn flat_and_invalid_frames_have_no_targets() {
    let bounds = Rect::new(0.0, 0.0, 40.0, 40.0);
    for value in [0, 80, 255] {
        assert!(detect(&vec![value; 1600], 40, 40, bounds, 100, || false).is_empty());
    }
    assert!(detect(&[], usize::MAX, 10, bounds, 100, || false).is_empty());
    assert!(
        detect(
            &[0; 9],
            3,
            3,
            Rect::new(f64::NAN, 0.0, 1.0, 1.0),
            100,
            || false
        )
        .is_empty()
    );
}

#[test]
fn interruption_during_each_stage_discards_incomplete_contours() {
    use std::cell::Cell;
    let gray = scene(513, 517);
    let calls = Cell::new(0);
    let stop = || {
        calls.set(calls.get() + 1);
        false
    };
    assert!(
        !detect(
            &gray,
            513,
            517,
            Rect::new(0.0, 0.0, 513.0, 517.0),
            100,
            stop
        )
        .is_empty()
    );
    let total = calls.get();
    for checkpoint in [0, total / 4, total / 2, 3 * total / 4, total - 1] {
        calls.set(0);
        assert!(
            detect(
                &gray,
                513,
                517,
                Rect::new(0.0, 0.0, 513.0, 517.0),
                100,
                || {
                    let n = calls.get();
                    calls.set(n + 1);
                    n >= checkpoint
                }
            )
            .is_empty()
        );
    }
}

#[test]
fn hierarchy_keeps_dialog_buttons_and_drops_centered_icon_artwork() {
    let c = |left, top, right, bottom| Component {
        left,
        top,
        right,
        bottom,
        parent: None,
    };
    let mut comps = vec![
        c(0, 0, 400, 200),
        c(20, 30, 120, 60),
        c(200, 30, 230, 60),
        c(210, 40, 220, 50),
    ];
    assert!(hierarchy(&mut comps, &|| false));
    let targets = filter(
        &comps,
        500,
        300,
        Rect::new(0.0, 0.0, 500.0, 300.0),
        100,
        &|| false,
    );
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().all(|t| t.rect.width >= 30.0));
}

#[test]
fn hysteresis_connects_weak_edges_across_a_tile_boundary() {
    let mut pixels = vec![0; 520 * 8];
    for x in 240..280 {
        pixels[3 * 520 + x] = 1;
    }
    pixels[3 * 520 + 240] = 2;
    pixels[6 * 520 + 400] = 1;
    hysteresis(&mut pixels, 520, 8, &|| false).unwrap();
    assert!(
        pixels[3 * 520 + 240..3 * 520 + 280]
            .iter()
            .all(|&p| p == 255)
    );
    assert_eq!(pixels[6 * 520 + 400], 0);
}

#[test]
#[ignore = "release timing probe; not an end-to-end capture benchmark"]
fn contour_analysis_timing() {
    for (w, h) in [(1280, 720), (1920, 1080)] {
        let mut gray = vec![245; w * h];
        for y in (20..h - 40).step_by(70) {
            for x in (20..w - 90).step_by(130) {
                for row in y..y + 28 {
                    gray[row * w + x..row * w + x + 75].fill(10);
                }
            }
        }
        let bounds = Rect::new(0.0, 0.0, w as f64, h as f64);
        let mut samples = Vec::new();
        for _ in 0..7 {
            let start = std::time::Instant::now();
            let targets = detect(&gray, w, h, bounds, 2000, || false);
            assert!(!targets.is_empty());
            samples.push(start.elapsed());
        }
        samples.sort();
        println!(
            "contour {w}x{h}: median {:?}, max {:?}",
            samples[3], samples[6]
        );
    }
}

fn reference_hierarchy(components: &mut [Component], stop: &impl Fn() -> bool) -> bool {
    for i in 0..components.len() {
        if stop() {
            return false;
        }
        let c = components[i];
        let mut parent = None;
        let mut area = usize::MAX;
        for (j, p) in components.iter().enumerate() {
            if j.is_multiple_of(1024) && stop() {
                return false;
            }
            if p.area() > c.area()
                && p.area() < area
                && p.left <= c.left + 1
                && p.right + 1 >= c.right
                && p.top <= c.top + 1
                && p.bottom + 1 >= c.bottom
            {
                area = p.area();
                parent = Some(j);
            }
        }
        components[i].parent = parent;
    }
    true
}

#[test]
fn hierarchy_matches_reference_for_nested_disjoint_and_equal_area_boxes() {
    let mut seed = 0xdeadbeefu64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 32) as usize
    };
    for count in [0, 1, 32, 256, 2048] {
        let mut actual: Vec<_> = (0..count)
            .map(|_| {
                let left = next() % 512;
                let top = next() % 512;
                Component {
                    left,
                    top,
                    right: left + next() % 128,
                    bottom: top + next() % 128,
                    parent: None,
                }
            })
            .collect();
        // Include duplicate parents, a nested chain and boundary-slack cases.
        for i in 0..32 {
            let c = Component {
                left: i,
                top: i,
                right: 512 - i,
                bottom: 512 - i,
                parent: None,
            };
            actual.extend([c, c]);
        }
        let mut expected = actual.clone();
        assert!(reference_hierarchy(&mut expected, &|| false));
        assert!(hierarchy(&mut actual, &|| false));
        assert_eq!(
            actual.iter().map(|c| c.parent).collect::<Vec<_>>(),
            expected.iter().map(|c| c.parent).collect::<Vec<_>>()
        );
    }
}

// Same pixel stages and filtering, with the original all-pairs parent search.
// Kept test-only for balanced, same-process screenshot A/B measurements.
#[cfg(target_os = "windows")]
pub(crate) fn reference_detect(
    gray: &[u8],
    w: usize,
    h: usize,
    bounds: Rect,
) -> (Vec<UiTarget>, usize) {
    let edges = tiled_edges(gray, w, h, &|| false).unwrap();
    let dilated = dilate(&edges, w, h, bounds, &|| false).unwrap();
    drop(edges);
    let mut c = components(dilated, w, h, &|| false).unwrap();
    assert!(reference_hierarchy(&mut c, &|| false));
    (filter(&c, w, h, bounds, 2000, &|| false), c.len())
}

#[test]
fn ten_thousand_candidates_survive_component_and_output_budgets() {
    // Separate connected components exercise the old 8192 cutoff as well as
    // filtering above 2000, without making normal tests run full-image Sobel.
    let side = 303;
    let mut pixels = vec![0; side * side];
    for i in 0..10_001 {
        pixels[(i / 101 * 3) * side + i % 101 * 3] = 255;
    }
    let mut found = components(pixels, side, side, &|| false).unwrap();
    assert_eq!(found.len(), 10_001);
    for c in &mut found {
        c.right = c.left + 9;
        c.bottom = c.top + 7;
    }
    let targets = filter(
        &found,
        side,
        side,
        Rect::new(0.0, 0.0, side as f64, side as f64),
        MAX_UI_SCAN_TARGETS,
        &|| false,
    );
    assert_eq!(targets.len(), 10_000);
}

fn reference_threshold_edges(
    s: &mut EdgeScratch,
    w: usize,
    h: usize,
    stop: &impl Fn() -> bool,
) -> Option<()> {
    let n = w * h;
    s.tmp.resize(n, 0);
    s.blurred.resize(n, 0);
    s.magnitude.resize(n, 0);
    s.magnitude.fill(0);
    s.state.resize(n, 0);
    s.state.fill(0);
    for y in 0..h {
        if stop() {
            return None;
        }
        for x in 0..w {
            let row = &s.gray[y * w..(y + 1) * w];
            s.tmp[y * w + x] = ((u16::from(row[x.saturating_sub(2)])
                + 4 * u16::from(row[x.saturating_sub(1)])
                + 6 * u16::from(row[x])
                + 4 * u16::from(row[(x + 1).min(w - 1)])
                + u16::from(row[(x + 2).min(w - 1)])
                + 8)
                >> 4) as u8;
        }
    }
    for y in 0..h {
        if stop() {
            return None;
        }
        for x in 0..w {
            s.blurred[y * w + x] = ((u16::from(s.tmp[y.saturating_sub(2) * w + x])
                + 4 * u16::from(s.tmp[y.saturating_sub(1) * w + x])
                + 6 * u16::from(s.tmp[y * w + x])
                + 4 * u16::from(s.tmp[(y + 1).min(h - 1) * w + x])
                + u16::from(s.tmp[(y + 2).min(h - 1) * w + x])
                + 8)
                >> 4) as u8;
        }
    }
    for y in 1..h - 1 {
        if stop() {
            return None;
        }
        for x in 1..w - 1 {
            let i = y * w + x;
            let p = |offset: isize| i32::from(s.blurred[i.wrapping_add_signed(offset)]);
            let stride = w as isize;
            let gx = p(-stride + 1) + 2 * p(1) + p(stride + 1)
                - p(-stride - 1)
                - 2 * p(-1)
                - p(stride - 1);
            let gy = p(stride - 1) + 2 * p(stride) + p(stride + 1)
                - p(-stride - 1)
                - 2 * p(-stride)
                - p(-stride + 1);
            let (ax, ay) = (gx.abs(), gy.abs());
            s.magnitude[i] = (ax + ay) as u16;
            s.state[i] = if ay * 1024 <= ax * 424 {
                0
            } else if ay * 1024 >= ax * 2472 {
                2
            } else if (gx > 0 && gy > 0) || (gx < 0 && gy < 0) {
                1
            } else {
                3
            };
        }
    }
    for y in 1..h - 1 {
        if stop() {
            return None;
        }
        for x in 1..w - 1 {
            let i = y * w + x;
            let (a, b) = match s.state[i] {
                0 => (i - 1, i + 1),
                1 => (i - w + 1, i + w - 1),
                2 => (i - w, i + w),
                _ => (i - w - 1, i + w + 1),
            };
            let m = s.magnitude[i];
            s.state[i] = if m >= LOW && m >= s.magnitude[a] && m >= s.magnitude[b] {
                if m >= HIGH { 2 } else { 1 }
            } else {
                0
            };
        }
    }
    Some(())
}

#[test]
fn row_kernels_match_original_pixels_including_small_frames_and_tile_halos() {
    let mut seed = 17u64;
    for (w, h) in [(3, 3), (4, 17), (5, 5), (17, 19), (263, 261)] {
        let gray: Vec<_> = (0..w * h)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                (seed >> 32) as u8
            })
            .collect();
        let mut expected = EdgeScratch {
            gray: gray.clone(),
            ..Default::default()
        };
        let mut actual = EdgeScratch {
            gray,
            ..Default::default()
        };
        reference_threshold_edges(&mut expected, w, h, &|| false).unwrap();
        threshold_edges(&mut actual, w, h, &|| false).unwrap();
        assert_eq!(actual.tmp, expected.tmp);
        assert_eq!(actual.blurred, expected.blurred);
        assert_eq!(actual.magnitude, expected.magnitude);
        assert_eq!(actual.state, expected.state);
    }
}
