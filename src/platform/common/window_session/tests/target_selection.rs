use super::*;
use crate::api::window::WindowTarget;

#[test]
fn explicit_cycle_sources_are_prioritized_and_bidirectional() {
    for overlapping in [false, true] {
        for backwards in [false, true] {
            for source in [WindowTarget::Active, WindowTarget::Mouse] {
                let mut access = Fake::new(4);
                access.selected.set(Some(WindowId(1)));
                access.pointer_target = Some(WindowId(3));
                let mut session = Session::default();
                let operation = WindowOperation::CycleFrom {
                    backwards,
                    overlapping,
                    source,
                };
                let result = run(&mut session, &mut access, operation.clone());
                assert!(result.message.is_none(), "{result:?}");
                let anchor = if source == WindowTarget::Active { 1 } else { 3 };
                let expected = if backwards {
                    (anchor + 2) % 4 + 1
                } else {
                    anchor % 4 + 1
                };
                assert_eq!(result.target.unwrap().id, WindowId(expected));
                // Missing, stale and minimized preferred anchors use the other valid source.
                for unavailable in [None, Some(WindowId(99)), Some(WindowId(2))] {
                    access.windows.get_mut(&WindowId(2)).unwrap().info.minimized = true;
                    if source == WindowTarget::Active {
                        access.selected.set(unavailable);
                        access.pointer_target = Some(WindowId(3));
                    } else {
                        access.selected.set(Some(WindowId(1)));
                        access.pointer_target = unavailable;
                    }
                    let result = run(&mut session, &mut access, operation.clone());
                    assert!(result.message.is_none(), "{result:?}");
                    // Window 2 is minimized. The remaining cycle is 1 -> 3 -> 4.
                    let expected = match (source, backwards) {
                        (WindowTarget::Active, false) | (WindowTarget::Mouse, true) => 4,
                        (WindowTarget::Active, true) => 1,
                        (WindowTarget::Mouse, false) => 3,
                    };
                    assert_eq!(result.target.unwrap().id, WindowId(expected));
                    assert!(result.pointer.is_some());
                }
            }
        }
    }
}

#[test]
fn resolve_does_not_activate_and_entry_uses_the_preferred_available_source() {
    let mut access = Fake::new(3);
    access.selected.set(Some(WindowId(1)));
    access.pointer_target = Some(WindowId(2));
    let mut session = Session::default();
    let result = run(
        &mut session,
        &mut access,
        WindowOperation::ResolveTarget(WindowTarget::Mouse),
    );
    assert_eq!(result.target.unwrap().id, WindowId(2));
    assert_eq!(access.selected.get(), Some(WindowId(1)));
    let restored = run(
        &mut session,
        &mut access,
        WindowOperation::RestoreTarget(Some(WindowId(1))),
    );
    assert_eq!(restored.target.unwrap().id, WindowId(1));
    assert!(restored.pointer.is_none());
    let inventory = run(&mut session, &mut access, WindowOperation::Enumerate);
    assert_eq!(inventory.target.unwrap().id, WindowId(1));
    let result = run(
        &mut session,
        &mut access,
        WindowOperation::AcquireFrom(WindowTarget::Active),
    );
    assert_eq!(result.target.unwrap().id, WindowId(1));
    access.pointer_target = None;
    let result = run(
        &mut session,
        &mut access,
        WindowOperation::Retarget(WindowTarget::Mouse),
    );
    assert!(result.message.is_none(), "{result:?}");
    assert_eq!(result.target.unwrap().id, WindowId(1));
    assert_eq!(access.selected.get(), Some(WindowId(1)));
}

#[test]
fn explicit_target_cancellation_never_activates() {
    let mut access = Fake::new(3);
    access.selected.set(Some(WindowId(1)));
    access.pointer_target = Some(WindowId(2));
    let mut session = Session::default();
    let result = session.execute(
        &mut access,
        WindowRequest {
            scope: None,
            session: 1,
            id: 1,
            operation: WindowOperation::AcquireFrom(WindowTarget::Mouse),
        },
        &screens(),
        &|| true,
    );
    assert!(result.target.is_none());
    assert_eq!(access.selected.get(), Some(WindowId(1)));
}

#[test]
fn explicit_cycle_with_two_invalid_sources_is_a_noop() {
    for source in [WindowTarget::Active, WindowTarget::Mouse] {
        for overlapping in [false, true] {
            for backwards in [false, true] {
                for invalid in [None, Some(WindowId(99)), Some(WindowId(2))] {
                    let mut access = Fake::new(4);
                    access.selected.set(invalid);
                    access.pointer_target = invalid;
                    access.windows.get_mut(&WindowId(2)).unwrap().info.minimized = true;
                    let mut session = Session::default();
                    let result = run(
                        &mut session,
                        &mut access,
                        WindowOperation::CycleFrom {
                            source,
                            overlapping,
                            backwards,
                        },
                    );
                    assert!(
                        result.target.is_none()
                            && result.pointer.is_none()
                            && result.message.is_none()
                    );
                    assert_eq!(access.selected.get(), invalid);
                    assert_eq!(session.cycle, [WindowId(1), WindowId(3), WindowId(4)]);
                    assert_eq!(access.tab_reads.get(), 0);
                }
            }
        }
    }
}

#[test]
fn explicit_isolated_overlap_anchor_never_uses_the_other_source() {
    for source in [WindowTarget::Active, WindowTarget::Mouse] {
        let mut access = Fake::new(4);
        access.selected.set(Some(WindowId(1)));
        access.pointer_target = Some(WindowId(3));
        let anchor = WindowId(if source == WindowTarget::Active { 1 } else { 3 });
        access.windows.get_mut(&anchor).unwrap().info.bounds = Rect::new(5000.0, 0.0, 100.0, 100.0);
        let result = run(
            &mut Session::default(),
            &mut access,
            WindowOperation::CycleFrom {
                source,
                overlapping: true,
                backwards: false,
            },
        );
        assert_eq!(result.target.unwrap().id, anchor);
        assert_eq!(result.pointer, Some(Point::new(5050.0, 50.0)));
        assert_eq!(access.selected.get(), Some(WindowId(1)));
    }
}

#[test]
fn entry_and_gesture_priorities_fall_back_only_for_unavailable_sources() {
    for source in [WindowTarget::Active, WindowTarget::Mouse] {
        for minimized in [false, true] {
            for operation in [
                WindowOperation::ResolveTarget(source),
                WindowOperation::AcquireFrom(source),
                WindowOperation::Retarget(source),
            ] {
                let mut access = Fake::new(3);
                access.selected.set(Some(WindowId(1)));
                access.pointer_target = Some(WindowId(2));
                let (preferred, fallback) = if source == WindowTarget::Active {
                    (1, 2)
                } else {
                    (2, 1)
                };
                if minimized {
                    access
                        .windows
                        .get_mut(&WindowId(preferred))
                        .unwrap()
                        .info
                        .minimized = true;
                } else if source == WindowTarget::Active {
                    access.selected.set(None);
                } else {
                    access.pointer_target = None;
                }
                let old_focus = access.selected.get();
                let activate = !matches!(operation, WindowOperation::ResolveTarget(_));
                let result = run(&mut Session::default(), &mut access, operation);
                assert!(result.message.is_none(), "{result:?}");
                assert_eq!(result.target.unwrap().id, WindowId(fallback));
                assert_eq!(
                    access.selected.get(),
                    if activate {
                        Some(WindowId(fallback))
                    } else {
                        old_focus
                    }
                );
            }
        }
    }
}

#[test]
fn entry_errors_and_denied_activation_do_not_redirect_to_the_fallback() {
    for snapshot_error in [false, true] {
        let mut access = Fake::new(3);
        access.selected.set(Some(WindowId(1)));
        access.pointer_target = Some(WindowId(2));
        access.snapshot_unavailable = snapshot_error;
        access.refuse_focus = !snapshot_error;
        let result = run(
            &mut Session::default(),
            &mut access,
            WindowOperation::AcquireFrom(WindowTarget::Active),
        );
        assert!(result.message.is_some());
        assert_eq!(access.selected.get(), Some(WindowId(1)));
    }
}
