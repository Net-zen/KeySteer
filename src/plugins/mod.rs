#![forbid(unsafe_code)]

//! Bundled example plugins.
//!
//! These are ordinary [`Plugin`] implementations built
//! against the public API. They prove the point of the architecture: a plugin
//! composes the same primitives the built-in modes use, so it can create its
//! own grid or full-screen overlay without any special support from the host.

mod screen_selector;
mod window_mover;

pub use screen_selector::ScreenSelector;
pub use window_mover::WindowMover;

use crate::api::Plugin;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledSettings {
    pub key_aliases: BTreeMap<String, String>,
    pub screen_selector_preserve: bool,
}

/// Instantiate the bundled plugins.
pub fn bundled(settings: BundledSettings) -> Result<Vec<Box<dyn Plugin>>, String> {
    Ok(vec![
        Box::new(ScreenSelector::with_settings(
            &settings.key_aliases,
            settings.screen_selector_preserve,
        )?),
        Box::new(WindowMover::with_aliases(&settings.key_aliases)?),
    ])
}
