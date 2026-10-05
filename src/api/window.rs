//! Window management vocabulary. Native handles never cross this boundary.
use super::command::WindowScreenTarget;
use super::{Direction, Point, Rect};

/// Normalized placement rectangles, shared by the preview and native placement.
pub const LAYOUTS: [(&str, Rect); 12] = [
    ("Full", Rect::new(0.0, 0.0, 1.0, 1.0)),
    ("Left half", Rect::new(0.0, 0.0, 0.5, 1.0)),
    ("Right half", Rect::new(0.5, 0.0, 0.5, 1.0)),
    ("Top half", Rect::new(0.0, 0.0, 1.0, 0.5)),
    ("Bottom half", Rect::new(0.0, 0.5, 1.0, 0.5)),
    ("Top left", Rect::new(0.0, 0.0, 0.5, 0.5)),
    ("Top right", Rect::new(0.5, 0.0, 0.5, 0.5)),
    ("Bottom left", Rect::new(0.0, 0.5, 0.5, 0.5)),
    ("Bottom right", Rect::new(0.5, 0.5, 0.5, 0.5)),
    ("Left third", Rect::new(0.0, 0.0, 1.0 / 3.0, 1.0)),
    ("Center third", Rect::new(1.0 / 3.0, 0.0, 1.0 / 3.0, 1.0)),
    ("Right third", Rect::new(2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0)),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub id: WindowId,
    pub title: String,
    pub app: String,
    pub bounds: Rect,
    pub screen: usize,
    pub resizable: bool,
    pub maximized: bool,
    pub minimized: bool,
    pub fullscreen: bool,
}

/// Allocate new numbers in application batches while preserving every existing
/// number. The acquired application's existing number keeps its batch first.
pub(crate) fn application_number_order<'a>(
    windows: impl IntoIterator<Item = &'a WindowInfo>,
    numbers: impl IntoIterator<Item = (WindowId, u32)>,
) -> Vec<&'a WindowInfo> {
    let mut windows: Vec<_> = windows.into_iter().collect();
    let numbers: std::collections::BTreeMap<_, _> = numbers.into_iter().collect();
    let mut first = std::collections::BTreeMap::<String, u32>::new();
    for window in &windows {
        if let Some(number) = numbers.get(&window.id) {
            first
                .entry(window.app.to_lowercase())
                .and_modify(|value| *value = (*value).min(*number))
                .or_insert(*number);
        }
    }
    windows.sort_by_cached_key(|window| {
        let app = window.app.to_lowercase();
        (first.get(&app).copied().unwrap_or(u32::MAX), app)
    });
    windows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Left,
    Down,
    Up,
    Right,
    Size,
    Navigate(Direction),
    Split(Direction),
    Ratio(Direction),
    Tile,
    NextScreen,
    PreviousScreen,
    CycleState,
    ToggleMaximize,
    ToggleMinimize,
    Center,
    Close,
    VolumeDown,
    VolumeUp,
    VolumeMute,
    AudioPrevious,
    AudioNext,
    SystemVolumeDown,
    SystemVolumeUp,
    SystemVolumeMute,
    SystemAudioPrevious,
    SystemAudioNext,
    Select,
    SelectPrevious,
    MultiSelect,
    MultiConfirm,
    ClearMulti,
    Undo,
    Redo,
    ResetInitial,
    RemoveRegion,
    SaveLayout,
    DeletePreset,
    Confirm,
    TabEnd,
    TabPrefix,
    TabSeparator,
    TabRemove,
    TabDissolve,
    TabNext,
    TabPrevious,
    TabMoveLeft,
    TabMoveRight,
}

impl WindowAction {
    /// History, presets and system audio have their own explicit scope, not a window anchor.
    pub fn accepts_target(self) -> bool {
        !matches!(
            self,
            Self::MultiSelect
                | Self::MultiConfirm
                | Self::ClearMulti
                | Self::Undo
                | Self::Redo
                | Self::ResetInitial
                | Self::SaveLayout
                | Self::DeletePreset
                | Self::Confirm
                | Self::TabEnd
                | Self::TabPrefix
                | Self::TabSeparator
                | Self::SystemVolumeDown
                | Self::SystemVolumeUp
                | Self::SystemVolumeMute
                | Self::SystemAudioPrevious
                | Self::SystemAudioNext
        )
    }

    pub const fn is_held(self) -> bool {
        matches!(
            self,
            Self::Left
                | Self::Down
                | Self::Up
                | Self::Right
                | Self::Ratio(_)
                | Self::VolumeDown
                | Self::VolumeUp
                | Self::SystemVolumeDown
                | Self::SystemVolumeUp
        )
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Left => "window_left",
            Self::Down => "window_down",
            Self::Up => "window_up",
            Self::Right => "window_right",
            Self::Size => "window_size",
            Self::Navigate(Direction::Left) => "window_layout_left",
            Self::Navigate(Direction::Down) => "window_layout_down",
            Self::Navigate(Direction::Up) => "window_layout_up",
            Self::Navigate(Direction::Right) => "window_layout_right",
            Self::Split(Direction::Left) => "window_split_left",
            Self::Split(Direction::Down) => "window_split_down",
            Self::Split(Direction::Up) => "window_split_up",
            Self::Split(Direction::Right) => "window_split_right",
            Self::Ratio(Direction::Left) => "window_ratio_left",
            Self::Ratio(Direction::Down) => "window_ratio_down",
            Self::Ratio(Direction::Up) => "window_ratio_up",
            Self::Ratio(Direction::Right) => "window_ratio_right",
            Self::Tile => "window_tile",
            Self::NextScreen => "window_screen_next",
            Self::PreviousScreen => "window_screen_previous",
            Self::CycleState => "size_cycle",
            Self::ToggleMaximize => "window_maximize",
            Self::ToggleMinimize => "window_minimize",
            Self::Center => "window_center",
            Self::Close => "window_close",
            Self::VolumeDown => "window_volume_down",
            Self::VolumeUp => "window_volume_up",
            Self::VolumeMute => "window_volume_mute",
            Self::AudioPrevious => "window_audio_previous",
            Self::AudioNext => "window_audio_next",
            Self::SystemVolumeDown => "window_system_volume_down",
            Self::SystemVolumeUp => "window_system_volume_up",
            Self::SystemVolumeMute => "window_system_volume_mute",
            Self::SystemAudioPrevious => "window_system_audio_previous",
            Self::SystemAudioNext => "window_system_audio_next",
            Self::MultiConfirm => "window_multi_confirm",
            Self::MultiSelect => "window_multi_select",
            Self::ClearMulti => "window_multi_clear",
            Self::Select => "window_select",
            Self::SelectPrevious => "window_select_previous",
            Self::Undo => "window_undo",
            Self::Redo => "window_redo",
            Self::ResetInitial => "window_reset_initial",
            Self::RemoveRegion => "window_remove_region",
            Self::SaveLayout => "window_save_layout",
            Self::DeletePreset => "window_delete",
            Self::Confirm => "window_confirm",
            Self::TabEnd => "window_tab_end",
            Self::TabPrefix => "window_tab_group",
            Self::TabSeparator => "window_number_end",
            Self::TabRemove => "window_tab_remove",
            Self::TabDissolve => "window_tab_dissolve",
            Self::TabNext => "window_tab_next",
            Self::TabPrevious => "window_tab_previous",
            Self::TabMoveLeft => "window_tab_move_left",
            Self::TabMoveRight => "window_tab_move_right",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::MultiConfirm,
            Self::MultiSelect,
            Self::ClearMulti,
            Self::Left,
            Self::Down,
            Self::Up,
            Self::Right,
            Self::Size,
            Self::Navigate(Direction::Left),
            Self::Navigate(Direction::Down),
            Self::Navigate(Direction::Up),
            Self::Navigate(Direction::Right),
            Self::Split(Direction::Left),
            Self::Split(Direction::Down),
            Self::Split(Direction::Up),
            Self::Split(Direction::Right),
            Self::Ratio(Direction::Left),
            Self::Ratio(Direction::Down),
            Self::Ratio(Direction::Up),
            Self::Ratio(Direction::Right),
            Self::Tile,
            Self::NextScreen,
            Self::PreviousScreen,
            Self::CycleState,
            Self::ToggleMaximize,
            Self::ToggleMinimize,
            Self::Center,
            Self::Close,
            Self::VolumeDown,
            Self::VolumeUp,
            Self::VolumeMute,
            Self::AudioPrevious,
            Self::AudioNext,
            Self::SystemVolumeDown,
            Self::SystemVolumeUp,
            Self::SystemVolumeMute,
            Self::SystemAudioPrevious,
            Self::SystemAudioNext,
            Self::Select,
            Self::SelectPrevious,
            Self::Undo,
            Self::Redo,
            Self::ResetInitial,
            Self::RemoveRegion,
            Self::SaveLayout,
            Self::DeletePreset,
            Self::Confirm,
            Self::TabEnd,
            Self::TabPrefix,
            Self::TabSeparator,
            Self::TabRemove,
            Self::TabDissolve,
            Self::TabNext,
            Self::TabPrevious,
            Self::TabMoveLeft,
            Self::TabMoveRight,
        ]
        .into_iter()
        .find(|action| action.name() == value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WindowChange {
    /// Place the window center at an absolute targeting position without warping the pointer.
    MoveTo(Point),
    Move {
        dx: f64,
        dy: f64,
    },
    Resize {
        dw: f64,
        dh: f64,
    },
    Place {
        index: usize,
        gap: f64,
    },
    Center,
    CycleState,
    Screen(WindowScreenTarget),
    ToggleMaximize,
    ToggleMinimize,
}

/// Preferred source for window selection; an absent target falls back to the other source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowTarget {
    Active,
    Mouse,
}

impl WindowTarget {
    /// Query the preferred source lazily. Native errors are not absence and must
    /// not redirect an action to a different window after an operation fails.
    #[inline]
    pub(crate) fn with_fallback<T, E>(
        self,
        mut lookup: impl FnMut(Self) -> Result<Option<T>, E>,
    ) -> Result<Option<T>, E> {
        if let Some(target) = lookup(self)? {
            return Ok(Some(target));
        }
        lookup(match self {
            Self::Active => Self::Mouse,
            Self::Mouse => Self::Active,
        })
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "mouse" => Ok(Self::Mouse),
            _ => Err("window target must be active or mouse".into()),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Mouse => "mouse",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WindowOperation {
    CycleFrom {
        backwards: bool,
        overlapping: bool,
        source: WindowTarget,
    },
    /// Resolve once on the worker; subsequent gesture frames use the returned identity.
    ResolveTarget(WindowTarget),
    /// Restore a cancelled lookup's session anchor without activating a window.
    RestoreTarget(Option<WindowId>),
    Retarget(WindowTarget),
    AcquireFrom(WindowTarget),
    /// Cycle peers of the focused window (pointer fallback), preferring tabs.
    /// An isolated anchor only centers the pointer; no valid anchor is a no-op.
    CycleOverlapping {
        backwards: bool,
    },
    /// Standalone focus cycling; session/id are unused and no WindowResult is emitted.
    CycleActive {
        backwards: bool,
    },
    Tabs(super::window_tabs::TabOperation),
    /// Lock and activate the ordinary window under the physical pointer.
    Acquire(Point),
    /// Cancel older queued/in-flight operations while preserving target/history.
    CancelPending,
    Enumerate,
    Select(WindowId),
    /// Bring an input preview to the foreground without moving the pointer.
    Activate(WindowId),
    /// Ask the application to close this window, preserving save/cancel dialogs.
    Close(WindowId),
    /// Close a selected tab group as one unit, preserving application confirmation dialogs.
    CloseGroup(WindowId),
    /// Query eligible windows and immediately lock/activate the next one.
    Cycle,
    CyclePrevious,
    /// Capture native restore state and constraints before a live edit.
    BeginEdit {
        transaction: u64,
        targets: Vec<WindowId>,
        /// When present, refresh the inventory and capture every eligible
        /// resizable window on this screen instead of relying on cached targets.
        screen: Option<usize>,
        group: u64,
    },
    /// Latest absolute geometry, in normalized screen work-area coordinates.
    ApplyLayout {
        /// Keep successful placements when individual applications reject tiling.
        best_effort: bool,
        additional_screens: Vec<WindowScreenLayout>,
        transaction: u64,
        revision: u64,
        screen: usize,
        placements: Vec<(WindowId, Rect)>,
        gap: f64,
        strict: bool,
    },
    EndEdit {
        transaction: u64,
        commit: bool,
    },
    Adjust {
        target: WindowId,
        change: WindowChange,
        group: u64,
    },
    Tile {
        target: WindowId,
        gap: f64,
        group: u64,
    },
    TileSelection {
        targets: Vec<WindowId>,
        gap: f64,
        group: u64,
    },
    Undo,
    Redo,
    /// Restore windows changed in this session to their first observed native state.
    ResetInitial {
        group: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowScreenLayout {
    pub screen: usize,
    pub placements: Vec<(WindowId, Rect)>,
}

impl WindowOperation {
    pub(crate) fn is_standalone_cycle(&self) -> bool {
        matches!(
            self,
            Self::CycleActive { .. } | Self::CycleOverlapping { .. } | Self::CycleFrom { .. }
        )
    }
    pub fn precedes_inventory(&self) -> bool {
        matches!(
            self,
            Self::Retarget(_)
                | Self::ResolveTarget(_)
                | Self::AcquireFrom(_)
                | Self::Tabs(_)
                | Self::BeginEdit { .. }
                | Self::ApplyLayout { .. }
                | Self::EndEdit { .. }
                | Self::Select(_)
                | Self::Activate(_)
                | Self::Close(_)
                | Self::CloseGroup(_)
                | Self::Cycle
                | Self::CyclePrevious
                | Self::Tile { .. }
                | Self::TileSelection { .. }
                | Self::Undo
                | Self::Redo
                | Self::ResetInitial { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowRequest {
    pub scope: Option<WindowScope>,
    pub session: u64,
    pub id: u64,
    pub operation: WindowOperation,
}

/// Inventory selection shared by numbering, cycling and layout operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowScope {
    pub screen: Option<usize>,
    pub include_minimized: bool,
}
impl WindowScope {
    pub fn contains(self, window: &WindowInfo) -> bool {
        self.screen.is_none_or(|screen| screen == window.screen)
            && (self.include_minimized || !window.minimized)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowResult {
    pub tabs: Option<super::window_tabs::TabState>,
    pub session: u64,
    pub id: u64,
    pub target: Option<WindowInfo>,
    pub windows: Option<Vec<WindowInfo>>,
    /// Retired native identities, distinct from hidden/minimized windows.
    pub closed: Vec<WindowId>,
    pub pointer: Option<Point>,
    pub changed: usize,
    pub skipped: usize,
    pub message: Option<String>,
    pub edit: Option<Box<WindowEditResult>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WindowEditResult {
    Started {
        transaction: u64,
        minimums: Vec<(WindowId, Point)>,
        gap_scale: f64,
        /// Layout units per configured unit, indexed by the request screen list.
        screen_scales: Vec<f64>,
        full_inventory: bool,
    },
    Applied {
        skipped_windows: Vec<WindowId>,
        transaction: u64,
        revision: u64,
        accepted: bool,
        minimums: Vec<(WindowId, Point)>,
    },
    Ended {
        transaction: u64,
        committed: bool,
    },
}

#[cfg(test)]
mod state_cycle_tests {
    use super::WindowAction;
    #[test]
    fn separate_state_toggles_and_legacy_cycle_parse() {
        assert_eq!(
            WindowAction::parse("size_cycle"),
            Some(WindowAction::CycleState)
        );
        assert_eq!(
            WindowAction::parse("window_maximize"),
            Some(WindowAction::ToggleMaximize)
        );
        assert_eq!(
            WindowAction::parse("window_minimize"),
            Some(WindowAction::ToggleMinimize)
        );
        assert_eq!(WindowAction::parse("window_cycle_state"), None);
        assert_eq!(WindowAction::CycleState.name(), "size_cycle");
    }
}

#[cfg(test)]
mod target_priority_tests {
    use super::{WindowId, WindowTarget};

    #[test]
    fn target_priority_is_lazy_and_does_not_hide_errors() {
        for source in [WindowTarget::Active, WindowTarget::Mouse] {
            let mut queries = Vec::new();
            let preferred = source.with_fallback(|current| {
                queries.push(current);
                Ok::<_, &str>(Some(WindowId(1)))
            });
            assert_eq!(preferred, Ok(Some(WindowId(1))));
            assert_eq!(queries, [source]);
            queries.clear();
            let fallback = source.with_fallback(|current| {
                queries.push(current);
                Ok::<_, &str>((current != source).then_some(WindowId(2)))
            });
            assert_eq!(fallback, Ok(Some(WindowId(2))));
            assert_eq!(queries.len(), 2);
            assert_eq!(queries[0], source);
            assert_ne!(queries[1], source);
            queries.clear();
            let missing = source.with_fallback(|current| {
                queries.push(current);
                Ok::<Option<WindowId>, &str>(None)
            });
            assert_eq!(missing, Ok(None));
            assert_eq!(queries.len(), 2);
            queries.clear();
            let error = source.with_fallback(|current| {
                queries.push(current);
                Err::<Option<WindowId>, _>("native query failed")
            });
            assert_eq!(error, Err("native query failed"));
            assert_eq!(queries, [source]);
        }
    }

    #[test]
    #[ignore = "process-wide allocator; run alone with --ignored --test-threads=1"]
    fn target_priority_selection_allocates_nothing() {
        let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
        let mut queries = 0;
        for _ in 0..10_000 {
            for source in [WindowTarget::Active, WindowTarget::Mouse] {
                for missing in [false, true] {
                    let result = std::hint::black_box(source).with_fallback(|current| {
                        queries += 1;
                        Ok::<_, ()>((!missing || current != source).then_some(WindowId(1)))
                    });
                    assert_eq!(result, Ok(Some(WindowId(1))));
                }
            }
        }
        let stats = region.change();
        assert_eq!(queries, 60_000);
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.reallocations, 0);
    }
}
