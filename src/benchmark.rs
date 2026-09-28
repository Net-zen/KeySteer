/// Thin handles to the production queue; no probes or alternate algorithms.
pub struct NotificationSender(crate::platform::common::event_queue::Sender);
pub struct NotificationReceiver(crate::platform::common::event_queue::Receiver);

pub fn notification_channel() -> (NotificationSender, NotificationReceiver) {
    let (sender, receiver) = crate::platform::common::event_queue::channel();
    (NotificationSender(sender), NotificationReceiver(receiver))
}

impl NotificationSender {
    pub fn send(&self, event: BackendEvent) -> Option<bool> {
        self.0.send(event).ok()
    }
}
impl NotificationReceiver {
    pub fn try_recv(&self) -> Option<BackendEvent> {
        self.0.try_recv().ok()
    }
}

pub use crate::api::{
    Appearance, Backend, BackendEvent, Binding, ButtonAction, Command, CommandBatch, Direction,
    FocusedApp, HostContext, InputEvent, Key, KeyDisposition, KeyState, LabelDirection, Mode,
    ModeEvent, MouseButton, OverlayScene, Point, Rect, Screen, SemanticRole, UiScanRequest,
    UiScanResult, UiScanStatus, UiTarget,
};
pub use crate::config::Config;
pub use crate::presentation::COMPOSER;

#[cfg(target_os = "windows")]
pub use crate::platform::windows::{CharacterCapture, observe_character_unfiltered};

#[cfg(target_os = "macos")]
pub use crate::platform::macos::{CharacterCaptureProbe, observe_character_unfiltered};

pub fn engine(config: &Config) -> Result<crate::app::runtime::Engine, String> {
    crate::app::runtime::Engine::from_plan(
        crate::app::configuration::compile(config)?,
        Appearance::Dark,
    )
}

pub fn hint(config: &Config) -> crate::modes::HintMode {
    crate::app::mode_catalog::hint(config)
}

pub fn normal(config: &Config) -> crate::modes::NormalMode {
    crate::app::mode_catalog::normal(config)
}

/// Exercise the real scanner index without native UIA/AX provider costs.
pub fn scan_index(rects: &[Rect]) -> usize {
    use crate::platform::common::spatial_index::{SpatialIndex, rectangles_match};
    let mut index = SpatialIndex::new(64.0, 8.0, 2.0);
    for &rect in rects {
        index.insert_if_unique(rect, |a, b| rectangles_match(a, b, 0.5, 8.0));
    }
    index.len()
}

pub use crate::api::{command, input, overlay};
#[cfg(target_os = "macos")]
pub use crate::platform::macos::MacOsBackend;
