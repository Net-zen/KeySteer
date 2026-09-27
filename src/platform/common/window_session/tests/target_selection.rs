use super::*;
use crate::api::window::WindowTarget;

#[test]
fn explicit_cycle_sources_are_independent_strict_and_bidirectional() {
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
                // Missing, stale and minimized anchors cannot silently use the other source.
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
                    assert!(result.target.is_none() && result.pointer.is_none());
                }
            }
        }
    }
}

#[test]
fn resolve_does_not_activate_and_entry_acquires_only_the_requested_source() {
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
    assert!(result.message.is_some() && result.target.is_none());
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
