#![allow(dead_code, unused_imports, non_snake_case)]
use std::ffi::c_void;
use std::ptr::NonNull;
mod core_foundation {
    pub mod base {
        use std::sync::atomic::{AtomicUsize, Ordering};
        pub static RELEASES: AtomicUsize = AtomicUsize::new(0);
        pub unsafe fn CFRelease(value: *const std::ffi::c_void) {
            assert!(
                !value.is_null(),
                "CFRelease(NULL): the native macOS call would abort"
            );
            RELEASES.fetch_add(1, Ordering::Relaxed);
        }
    }
}
// PRODUCTION_CF_OWNER

#[test]
fn empty_ax_output_and_owned_transfer() {
    // The production owner is compiled verbatim above; only CFRelease is mocked.
    // The mock never dereferences the marker, and rejects null as Apple does.
    let null_owner = unsafe { OwnedCf::from_create_rule(std::ptr::null()) };
    assert!(null_owner.is_none());
    assert_eq!(std::mem::size_of::<OwnedCf>(), std::mem::size_of::<usize>());
    assert_eq!(
        std::mem::size_of::<Option<OwnedCf>>(),
        std::mem::size_of::<usize>()
    );
    assert_eq!(
        core_foundation::base::RELEASES.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    let marker = std::ptr::dangling::<u8>().cast::<c_void>();
    let owner = unsafe { OwnedCf::from_create_rule(marker) }.unwrap();
    assert_eq!(owner.as_ptr(), marker);
    drop(owner);
    assert_eq!(
        core_foundation::base::RELEASES.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    let owner = unsafe { OwnedCf::from_create_rule(marker) }.unwrap();
    let transferred = owner.into_raw();
    assert_eq!(transferred, marker);
    assert_eq!(
        core_foundation::base::RELEASES.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    drop(unsafe { OwnedCf::from_create_rule(transferred) }.unwrap());
    assert_eq!(
        core_foundation::base::RELEASES.load(std::sync::atomic::Ordering::Relaxed),
        2
    );
}
