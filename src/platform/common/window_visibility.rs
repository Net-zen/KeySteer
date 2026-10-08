//! Match native visibility metadata without depending on platform handles.
use crate::api::{Point, Rect};

pub(crate) struct Visible {
    pub(crate) pid: i32,
    pub(crate) layer: i64,
    pub(crate) bounds: Rect,
    pub(crate) title: Option<String>,
    pub(crate) number: Option<isize>,
}

/// Quartz supplies front-to-back records. A raised menu/panel blocks the
/// ordinary window behind it; never skip a failed hit to another application.
pub(crate) fn pointer_window(windows: &[Visible], point: Point) -> Option<&Visible> {
    let shown = windows
        .iter()
        .find(|window| window.bounds.contains(&point))?;
    (shown.layer == 0).then_some(shown)
}

/// Select one AX window of the hit process, rejecting coincident identities.
pub(crate) fn pointer_candidate(
    candidates: &[(Rect, &str)],
    shown: &Visible,
    point: Point,
) -> Option<usize> {
    if shown.layer != 0 || !shown.bounds.contains(&point) {
        return None;
    }
    let matches = visible_candidates(candidates, shown, std::slice::from_ref(shown));
    match matches.as_slice() {
        [index] if candidates[*index].0.contains(&point) => Some(*index),
        _ => None,
    }
}

pub(crate) fn visible_candidates(
    candidates: &[(Rect, &str)],
    shown: &Visible,
    remaining: &[Visible],
) -> Vec<usize> {
    let matches: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter(|(_, (bounds, _))| same_rect(*bounds, shown.bounds))
        .map(|(index, _)| index)
        .collect();
    if matches.len() <= 1 {
        return matches;
    }
    // Titles are asynchronous, optional metadata, not window identities. Use
    // them only to disambiguate coincident windows in this process.
    if let Some(title) = shown.title.as_deref().filter(|title| !title.is_empty()) {
        let named: Vec<_> = matches
            .iter()
            .copied()
            .filter(|index| candidates[*index].1 == title)
            .collect();
        if named.len() == 1 {
            return named;
        }
    }
    // Require enough on-screen records that each could describe every AX
    // candidate. Otherwise an indistinguishable window may be on another Space.
    let count = remaining
        .iter()
        .filter(|record| {
            record.pid == shown.pid
                && matches.iter().all(|index| {
                    let (bounds, _) = candidates[*index];
                    same_rect(bounds, record.bounds)
                })
        })
        .count();
    if count == matches.len() {
        matches
    } else {
        Vec::new()
    }
}

fn same_rect(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 2.0
        && (a.y - b.y).abs() < 2.0
        && (a.width - b.width).abs() < 2.0
        && (a.height - b.height).abs() < 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unique_geometry_does_not_require_synchronized_browser_titles() {
        let internal = Rect::new(0.0, 30.0, 1400.0, 900.0);
        let external = Rect::new(-1920.0, -200.0, 1920.0, 1080.0);
        let candidates = [
            (internal, "New tab - Google Chrome"),
            (external, "Document"),
        ];
        for title in [None, Some(""), Some("Old page"), Some("New tab")] {
            let shown = Visible {
                pid: 10,
                layer: 0,
                bounds: internal,
                title: title.map(str::to_owned),
                number: Some(42),
            };
            assert_eq!(shown.number, Some(42));
            assert_eq!(
                visible_candidates(&candidates, &shown, std::slice::from_ref(&shown)),
                vec![0]
            );
        }
        let shown = Visible {
            pid: 10,
            layer: 0,
            bounds: external,
            title: Some("Stale document".into()),
            number: None,
        };
        assert_eq!(
            visible_candidates(&candidates, &shown, std::slice::from_ref(&shown)),
            vec![1]
        );
    }

    #[test]
    fn title_mismatch_does_not_guess_between_spaces_or_processes() {
        let bounds = Rect::new(100.0, 100.0, 400.0, 300.0);
        let candidates = [(bounds, "A"), (bounds, "B")];
        let records = [10, 20].map(|pid| Visible {
            pid,
            layer: 0,
            bounds,
            title: Some("Stale title".into()),
            number: None,
        });
        assert!(visible_candidates(&candidates, &records[0], &records).is_empty());
        let other = [(Rect::new(900.0, 100.0, 400.0, 300.0), "Stale title")];
        assert!(visible_candidates(&other, &records[0], &records).is_empty());
    }

    #[test]
    fn coincident_windows_are_kept_only_when_the_entire_cohort_is_on_screen() {
        let bounds = Rect::new(100.0, 100.0, 400.0, 300.0);
        let candidates = [(bounds, "First"), (bounds, "Second")];
        let records = [0, 1].map(|number| Visible {
            pid: 10,
            layer: 0,
            bounds,
            title: None,
            number: Some(number),
        });
        assert_eq!(
            visible_candidates(&candidates, &records[0], &records),
            vec![0, 1]
        );
        assert!(visible_candidates(&candidates, &records[0], &records[..1]).is_empty());
        let named = Visible {
            pid: 10,
            layer: 0,
            bounds,
            title: Some("Second".into()),
            number: Some(1),
        };
        assert_eq!(visible_candidates(&candidates, &named, &records), vec![1]);
    }

    fn shown(pid: i32, layer: i64, bounds: Rect, title: Option<&str>) -> Visible {
        Visible {
            pid,
            layer,
            bounds,
            title: title.map(str::to_owned),
            number: None,
        }
    }

    #[test]
    fn failed_ax_hit_falls_back_to_the_topmost_pointer_window_of_the_hit_process() {
        use crate::platform::common::accessibility_window::with_inventory_fallback;
        let bounds = Rect::new(-1200.0, 50.0, 900.0, 700.0);
        let cursor = Point::new(-800.0, 300.0);
        let windows = [
            shown(20, 0, bounds, Some("Telegram")),
            shown(10, 0, bounds, Some("Finder")),
        ];
        for direct in [Ok(None), Err("AXCannotComplete")] {
            let selected = with_inventory_fallback(direct, || {
                let hit = pointer_window(&windows, cursor).unwrap();
                // The native caller reads AXWindows for this process only.
                assert_eq!(hit.pid, 20);
                let ax = [
                    (Rect::new(-400.0, 90.0, 200.0, 100.0), "Other"),
                    (bounds, "Telegram"),
                ];
                Ok(pointer_candidate(&ax, hit, cursor).map(|index| (hit.pid, index)))
            });
            assert_eq!(selected, Ok(Some((20, 1))));
        }
        assert!(pointer_window(&windows, Point::new(10.0, 10.0)).is_none());
        // A stale/different AX rectangle must not cause selection of Finder behind it.
        let hit = pointer_window(&windows, cursor).unwrap();
        assert!(
            pointer_candidate(
                &[(Rect::new(0.0, 0.0, 900.0, 700.0), "Telegram")],
                hit,
                cursor
            )
            .is_none()
        );
    }

    #[test]
    fn raised_menu_blocks_pointer_fallback_to_the_window_behind_it() {
        let bounds = Rect::new(100.0, 100.0, 500.0, 400.0);
        let windows = [
            shown(20, 101, Rect::new(150.0, 150.0, 100.0, 100.0), None),
            shown(10, 0, bounds, None),
        ];
        assert!(pointer_window(&windows, Point::new(180.0, 180.0)).is_none());
        assert_eq!(
            pointer_window(&windows, Point::new(120.0, 120.0))
                .unwrap()
                .pid,
            10
        );
        assert!(
            pointer_candidate(
                &[(windows[0].bounds, "")],
                &windows[0],
                Point::new(180.0, 180.0)
            )
            .is_none()
        );
    }

    #[test]
    fn pointer_fallback_requires_one_identity_and_the_actual_ax_frame_to_contain_the_point() {
        let bounds = Rect::new(100.0, 100.0, 500.0, 400.0);
        let cursor = bounds.center();
        let ax = [(bounds, "Telegram"), (bounds, "Preferences")];
        for title in [None, Some(""), Some("Stale title")] {
            assert!(pointer_candidate(&ax, &shown(20, 0, bounds, title), cursor).is_none());
        }
        assert_eq!(
            pointer_candidate(&ax, &shown(20, 0, bounds, Some("Telegram")), cursor),
            Some(0)
        );
        assert_eq!(
            pointer_candidate(&ax[..1], &shown(20, 0, bounds, None), cursor),
            Some(0)
        );
        // Geometry tolerance must not treat the one-point strip outside AX as a hit.
        let shifted = Rect::new(101.0, 100.0, 500.0, 400.0);
        assert!(
            pointer_candidate(
                &[(shifted, "Telegram")],
                &shown(20, 0, bounds, None),
                Point::new(100.5, 200.0)
            )
            .is_none()
        );
    }
}
