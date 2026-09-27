use super::*;
use crate::api::window_tabs::{TabGroup, TabGroupId, TabState};

#[test]
fn indexed_ring_refresh_preserves_order_through_churn_and_large_inventories() {
    let access = Fake::new(300);
    let all: Vec<_> = access.windows.values().map(|w| w.info.clone()).collect();
    let mut session = Session::default();
    let mut expected = Vec::new();
    let mut seed = 37u64;
    for count in [
        4, 7, 8, 9, 15, 16, 17, 23, 24, 25, 31, 32, 33, 48, 96, 300, 129, 128, 16, 3, 0, 96, 8,
    ] {
        for iteration in 0..12 {
            let mut windows: Vec<_> = all
                .iter()
                .cycle()
                .skip(iteration * 7)
                .take(count)
                .cloned()
                .collect();
            for i in (1..windows.len()).rev() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                windows.swap(i, seed as usize % (i + 1));
            }
            if iteration % 3 == 0 && !windows.is_empty() {
                windows.push(windows[0].clone());
            }
            expected.retain(|id| windows.iter().any(|w| w.id == *id));
            for window in &windows {
                if !expected.contains(&window.id) {
                    expected.push(window.id);
                }
            }
            session.refresh_cycle(&windows);
            assert_eq!(
                session.cycle, expected,
                "count={count}, iteration={iteration}"
            );
        }
    }
}

fn grouped_fixture() -> Fake {
    grouped_fixture_for_count(5)
}

fn grouped_fixture_for_count(count: u64) -> Fake {
    let mut access = Fake::new(count);
    for window in access.windows.values_mut() {
        window.info.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
    }
    access.selected.set(Some(WindowId(1)));
    access.tabs = Some(TabState {
        groups: vec![TabGroup {
            id: TabGroupId(1),
            members: vec![WindowId(1), WindowId(3), WindowId(2)],
            active: WindowId(1),
        }],
        ..TabState::default()
    });
    access
}

#[test]
fn overlap_missing_anchor_preserves_ring_history_and_revalidates_geometry() {
    let mut access = Fake::new(4);
    let new_window = access.windows.remove(&WindowId(4)).unwrap();
    access.selected.set(Some(WindowId(1)));
    let mut session = Session::default();
    run(
        &mut session,
        &mut access,
        WindowOperation::CycleOverlapping { backwards: false },
    );
    let returning = access.windows.remove(&WindowId(2)).unwrap();
    access.windows.insert(WindowId(4), new_window);
    access.selected.set(None);
    access.windows.get_mut(&WindowId(3)).unwrap().info.bounds.x = 2000.0;
    let result = run(
        &mut session,
        &mut access,
        WindowOperation::CycleOverlapping { backwards: false },
    );
    assert!(result.target.is_none() && result.pointer.is_none() && result.message.is_none());
    assert_eq!(session.cycle, [WindowId(1), WindowId(3), WindowId(4)]);
    access.windows.insert(WindowId(2), returning);
    access.selected.set(Some(WindowId(1)));
    let result = run(
        &mut session,
        &mut access,
        WindowOperation::CycleOverlapping { backwards: false },
    );
    // Window 3 moved away while focus was absent; 4 must precede reappearing 2.
    assert_eq!(result.target.unwrap().id, WindowId(4));
    assert_eq!(
        session.cycle,
        [WindowId(1), WindowId(3), WindowId(4), WindowId(2)]
    );
    assert!(!session.overlap.as_ref().unwrap().contains(WindowId(3)));
}

#[test]
fn overlap_group_priority_fallback_and_invalid_members_in_both_directions() {
    for count in [
        5, 7, 8, 9, 15, 16, 17, 23, 24, 25, 31, 32, 33, 48, 96, 128, 129,
    ] {
        for backwards in [false, true] {
            for unavailable in 0..8u8 {
                let mut access = grouped_fixture_for_count(count);
                if unavailable & 1 != 0 {
                    access.unavailable.push(WindowId(3));
                }
                if unavailable & 2 != 0 {
                    access.windows.get_mut(&WindowId(2)).unwrap().info.minimized = true;
                }
                if unavailable & 4 != 0 {
                    access.windows.remove(&WindowId(3));
                }
                let group_order = if backwards { [2, 3] } else { [3, 2] };
                let expected = group_order
                    .into_iter()
                    .find(|id| {
                        !access.unavailable.contains(&WindowId(*id))
                            && access
                                .windows
                                .get(&WindowId(*id))
                                .is_some_and(|w| !w.info.minimized)
                    })
                    .unwrap_or(if backwards { count } else { 4 });
                let result = run(
                    &mut Session::default(),
                    &mut access,
                    WindowOperation::CycleOverlapping { backwards },
                );
                assert_eq!(
                    result.target.unwrap().id,
                    WindowId(expected),
                    "count={count}, backwards={backwards}, mask={unavailable}"
                );
                assert!(result.message.is_none());
            }
        }
    }
}

#[test]
fn overlap_group_fallback_does_not_escape_component_or_retry_denied_focus() {
    for backwards in [false, true] {
        let mut access = grouped_fixture();
        access.windows.get_mut(&WindowId(2)).unwrap().info.bounds.x = 2000.0;
        access.windows.get_mut(&WindowId(3)).unwrap().info.bounds.x = 2000.0;
        // Group members are valid, but outside the anchor's connected component.
        let result = run(
            &mut Session::default(),
            &mut access,
            WindowOperation::CycleOverlapping { backwards },
        );
        assert_eq!(
            result.target.unwrap().id,
            WindowId(if backwards { 5 } else { 4 })
        );

        let mut access = grouped_fixture();
        access.refuse_focus = true;
        let result = run(
            &mut Session::default(),
            &mut access,
            WindowOperation::CycleOverlapping { backwards },
        );
        assert_eq!(result.message.as_deref(), Some("focus denied"));
        assert_eq!(
            access.selected.get(),
            Some(WindowId(if backwards { 2 } else { 3 }))
        );
        assert_eq!(result.skipped, 0);
    }
}

#[test]
fn overlap_cancelled_before_group_fallback_never_activates() {
    for backwards in [false, true] {
        // Sweep cancellation boundaries, including the transition between passes.
        for limit in 1..=24 {
            let mut access = grouped_fixture();
            access.unavailable.extend([WindowId(2), WindowId(3)]);
            let calls = std::cell::Cell::new(0);
            let mut session = Session::default();
            let result = session.execute(
                &mut access,
                WindowRequest {
                    scope: None,
                    session: 0,
                    id: 0,
                    operation: WindowOperation::CycleOverlapping { backwards },
                },
                &screens(),
                &|| {
                    calls.set(calls.get() + 1);
                    calls.get() >= limit
                },
            );
            if let Some(target) = result.target {
                assert!(calls.get() < limit, "activation after cancellation");
                assert_eq!(target.id, WindowId(if backwards { 5 } else { 4 }));
            } else {
                assert_eq!(access.selected.get(), Some(WindowId(1)));
                assert!(result.pointer.is_none());
            }
        }
    }
}

#[test]
#[ignore = "process-wide allocator; run alone with --ignored --test-threads=1"]
fn cycle_scratch_storage_refresh_stays_inline_through_128_windows() {
    for count in [
        0, 1, 7, 8, 9, 15, 16, 17, 23, 24, 25, 31, 32, 33, 48, 96, 128, 129,
    ] {
        let access = Fake::new(count);
        let windows: Vec<_> = access.windows.values().map(|w| w.info.clone()).collect();
        let mut session = Session::default();
        session.refresh_cycle(&windows); // Reserve persistent ring storage first.
        let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
        session.refresh_cycle(&windows);
        let stats = region.change();
        assert_eq!(stats.allocations, usize::from(count > 128), "count={count}");
        assert_eq!(stats.reallocations, 0, "count={count}");
        assert_eq!(session.cycle.len(), count as usize);
    }
}

#[test]
fn ring_refresh_equal_counts_require_surviving_identities() {
    for count in [8, 9, 16, 24, 32, 48, 96, 128, 129] {
        let access = Fake::new(count + 1);
        let mut windows: Vec<_> = access.windows.values().map(|w| w.info.clone()).collect();
        windows.sort_by_key(|w| w.id.0);
        let newcomer = windows.pop().unwrap();
        let mut session = Session::default();
        session.refresh_cycle(&windows);
        let original = session.cycle.clone();
        windows.reverse();
        session.refresh_cycle(&windows);
        assert_eq!(
            session.cycle, original,
            "enumeration order must not reset history"
        );
        let removed = windows[0].id;
        windows[0] = newcomer.clone();
        session.refresh_cycle(&windows);
        let mut expected: Vec<_> = original.into_iter().filter(|id| *id != removed).collect();
        expected.push(newcomer.id);
        assert_eq!(session.cycle, expected, "same count with a replacement");
        windows[1] = newcomer;
        expected.retain(|id| windows.iter().any(|w| w.id == *id));
        session.refresh_cycle(&windows);
        assert_eq!(
            session.cycle, expected,
            "duplicate inventory must not duplicate ring IDs"
        );
    }
}
