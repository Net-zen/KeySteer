mod geometry;
pub(crate) use geometry::{
    BisectGeometry, Geometry, GridGeometry, LetterGeometry, RecursiveGeometry,
};

use crate::api::binding::Direction;
use smallvec::SmallVec;

use crate::api::{
    Command, CommandBatch, FinishCause, Key, LifecycleAction, ModeId, Point, Rect,
    TargetingLifecycle,
};

/// Shared selection/lifecycle state. Geometry belongs to TargetingController;
/// rendering remains owned by each visible mode.
pub(crate) struct TargetingSession {
    pub(crate) default_cursor_follow_selection: bool,
    pub(crate) cursor_follow_selection: bool,
    pub(crate) stack: SmallVec<[Rect; 12]>,
    pub(crate) path: SmallVec<[usize; 12]>,
    pub(crate) terminal: bool,
    pub(crate) finished: bool,
    pub(crate) lifecycle: TargetingLifecycle,
    pub(crate) return_mode: ModeId,
}

/// Precompiled geometry, shared by visible grids and blind Normal positioning.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layout {
    pub rows: usize,
    pub cols: usize,
    pub keys: Vec<char>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerSettings {
    pub depth: u32,
    pub grid_cols: Option<u32>,
    pub grid_rows: Option<u32>,
    pub keys: Option<String>,
}

pub(crate) enum Selection {
    Ignored,
    Changed,
    Cancel,
    Follow(Point),
    Commit(Point),
}

pub(crate) struct TargetingController<G: Geometry> {
    pub geometry: G,
    pub session: TargetingSession,
    pub max_depth: u32,
}

impl<G: Geometry> TargetingController<G> {
    pub fn new(geometry: G, max_depth: u32, follow: bool, lifecycle: TargetingLifecycle) -> Self {
        let max_depth = max_depth.max(1);
        let mut session = TargetingSession::new(follow, lifecycle);
        // Reserve deep paths at construction, not on input or backtracking.
        session.stack.reserve(max_depth as usize + 1);
        session.path.reserve(max_depth as usize);
        Self {
            geometry,
            session,
            max_depth,
        }
    }

    pub fn cut(&mut self, direction: Direction) -> Selection {
        if self.session.terminal || self.session.finished {
            return Selection::Ignored;
        }
        let Some((index, cell)) = self
            .session
            .current()
            .and_then(|area| self.geometry.direction_cell(area, direction))
        else {
            return Selection::Ignored;
        };
        self.select(index, cell)
    }

    pub fn claims_key(&self, key: &Key) -> bool {
        self.geometry.claims_key(key)
    }

    /// Stateless absolute positioning for a one-level silent grid.
    pub fn root_point(&self, key: &Key, bounds: Rect) -> Option<Point> {
        self.geometry
            .key_cell(bounds, 0, key)
            .map(|(_, cell)| cell.center())
    }

    pub fn can_descend(&self) -> bool {
        self.geometry.can_descend(&self.session, self.max_depth)
    }

    pub fn reset(&mut self, bounds: Rect) {
        self.session.reset(bounds);
    }

    pub fn retarget(&mut self, bounds: Rect, preserve: bool) -> Point {
        if !preserve {
            self.reset(bounds);
        } else {
            // Replay in place: retain the path and stack capacities, with no
            // temporary path clone (including paths beyond inline capacity).
            self.session.stack.clear();
            self.session.stack.push(bounds);
            for offset in 0..self.session.path.len() {
                let index = self.session.path[offset];
                let Some(cell) = self
                    .session
                    .current()
                    .and_then(|area| self.geometry.cell_at(area, self.session.depth(), index))
                else {
                    self.session.path.truncate(offset);
                    break;
                };
                self.session.stack.push(cell);
            }
            self.session.terminal = !self.can_descend();
        }
        self.session.current().unwrap_or(bounds).center()
    }

    pub fn input(&mut self, key: &Key, bounds: Rect) -> Selection {
        match key.as_str() {
            "esc" => return Selection::Cancel,
            "enter" => {
                return self
                    .session
                    .current()
                    .map_or(Selection::Cancel, |area| Selection::Commit(area.center()));
            }
            // Compatibility for explicit `targeting_key` bindings.
            "backspace" | "tab" => return self.parent(),
            "space" => return self.reset_selection(bounds),
            _ => {}
        }
        if self.session.terminal || self.session.finished {
            return Selection::Ignored;
        }
        let Some((index, cell)) = self
            .session
            .current()
            .and_then(|area| self.geometry.key_cell(area, self.session.depth(), key))
        else {
            return Selection::Ignored;
        };
        self.select(index, cell)
    }

    fn select(&mut self, index: usize, cell: Rect) -> Selection {
        self.session.stack.push(cell);
        self.session.path.push(index);
        self.session.terminal = !self.can_descend();
        self.selection_result(cell)
    }

    fn selection_result(&self, cell: Rect) -> Selection {
        if self.session.terminal {
            Selection::Commit(cell.center())
        } else if self.session.cursor_follow_selection {
            Selection::Follow(cell.center())
        } else {
            Selection::Changed
        }
    }
}

impl<G: Geometry> TargetingController<G> {
    pub fn parent(&mut self) -> Selection {
        if self.session.stack.len() <= 1 {
            return Selection::Cancel;
        }
        self.session.stack.pop();
        self.session.path.pop();
        self.session.terminal = false;
        self.session.finished = false;
        Selection::Changed
    }

    pub fn reset_selection(&mut self, bounds: Rect) -> Selection {
        let root = if G::RESET_TO_ACTIVE_SCREEN {
            Some(bounds)
        } else {
            self.session.root()
        };
        let Some(root) = root else {
            return Selection::Cancel;
        };
        self.reset(root);
        Selection::Changed
    }
}

impl<G: LetterGeometry> TargetingController<G> {
    pub fn layout_at(&self, depth: u32) -> &Layout {
        self.geometry.layout_at(depth)
    }
}

impl TargetingSession {
    pub(crate) fn new(cursor_follow_selection: bool, lifecycle: TargetingLifecycle) -> Self {
        Self {
            default_cursor_follow_selection: cursor_follow_selection,
            cursor_follow_selection,
            stack: SmallVec::new(),
            path: SmallVec::new(),
            terminal: false,
            finished: false,
            lifecycle,
            return_mode: ModeId::idle(),
        }
    }

    pub(crate) fn depth(&self) -> u32 {
        self.stack.len().saturating_sub(1) as u32
    }

    pub(crate) fn current(&self) -> Option<Rect> {
        self.stack.last().copied()
    }

    pub(crate) fn root(&self) -> Option<Rect> {
        self.stack.first().copied()
    }

    pub(crate) fn reset(&mut self, bounds: Rect) {
        self.stack.clear();
        self.stack.push(bounds);
        self.path.clear();
        self.terminal = false;
        self.finished = false;
        self.cursor_follow_selection = self.default_cursor_follow_selection;
    }

    pub(crate) fn toggle_cursor_follow(&mut self) -> Option<Rect> {
        self.cursor_follow_selection = !self.cursor_follow_selection;
        self.cursor_follow_selection
            .then(|| self.current())
            .flatten()
    }
}

pub(crate) fn lifecycle_commands(action: &LifecycleAction, return_mode: &ModeId) -> CommandBatch {
    match action {
        LifecycleAction::Keep => CommandBatch::new(),
        LifecycleAction::Finish => CommandBatch::one(Command::FinishMode {
            cause: FinishCause::Click,
        }),
        LifecycleAction::Restart => CommandBatch::one(Command::RestartMode),
        LifecycleAction::Return => CommandBatch::two(
            Command::HideOverlay,
            Command::SwitchMode(return_mode.clone()),
        ),
        LifecycleAction::Mode(mode) => {
            CommandBatch::two(Command::HideOverlay, Command::SwitchMode(mode.clone()))
        }
        LifecycleAction::Click { button, action } => CommandBatch::one(Command::MouseButton {
            button: *button,
            action: *action,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ButtonAction, MouseButton};

    #[test]
    fn lifecycle_actions_do_not_implicitly_reactivate_the_mode() {
        let return_mode = ModeId::normal();
        assert!(lifecycle_commands(&LifecycleAction::Keep, &return_mode).is_empty());
        assert_eq!(
            lifecycle_commands(&LifecycleAction::Finish, &return_mode),
            vec![Command::FinishMode {
                cause: FinishCause::Click,
            }]
        );
        assert_eq!(
            lifecycle_commands(&LifecycleAction::Restart, &return_mode),
            vec![Command::RestartMode]
        );
        assert_eq!(
            lifecycle_commands(&LifecycleAction::Return, &return_mode),
            vec![
                Command::HideOverlay,
                Command::SwitchMode(return_mode.clone())
            ]
        );

        let plugin = ModeId::new("example:picker").unwrap();
        assert_eq!(
            lifecycle_commands(&LifecycleAction::Mode(plugin.clone()), &return_mode),
            vec![Command::HideOverlay, Command::SwitchMode(plugin)]
        );

        for (button, action) in [
            (MouseButton::Left, ButtonAction::Click),
            (MouseButton::Right, ButtonAction::Click),
            (MouseButton::Middle, ButtonAction::Click),
            (MouseButton::Left, ButtonAction::DoubleClick),
        ] {
            assert_eq!(
                lifecycle_commands(&LifecycleAction::Click { button, action }, &return_mode),
                vec![Command::MouseButton { button, action }]
            );
        }
    }
}
