//! Concrete geometry policies selected by the catalog, never by an input event.

use super::{LayerSettings, Layout, TargetingSession};
use crate::api::{Direction, Key, Rect};
use smallvec::SmallVec;

pub(crate) trait Geometry: Send + 'static {
    const RESET_TO_ACTIVE_SCREEN: bool;

    fn cell_at(&self, area: Rect, depth: u32, index: usize) -> Option<Rect>;
    fn can_descend(&self, session: &TargetingSession, max_depth: u32) -> bool;

    fn claims_key(&self, _key: &Key) -> bool {
        false
    }
    fn key_cell(&self, _area: Rect, _depth: u32, _key: &Key) -> Option<(usize, Rect)> {
        None
    }
    fn direction_cell(&self, _area: Rect, _direction: Direction) -> Option<(usize, Rect)> {
        None
    }
}

pub(crate) trait LetterGeometry: Geometry {
    fn layout_at(&self, depth: u32) -> &Layout;
}

pub(crate) struct GridGeometry {
    layouts: SmallVec<[Layout; 1]>,
}

impl GridGeometry {
    pub(crate) fn new(base: Layout, layers: &[LayerSettings], max_depth: u32) -> Self {
        // Resolve layer overrides once; uniform layouts stay inline.
        let layouts = if layers.is_empty() {
            smallvec::smallvec![base]
        } else {
            (0..=max_depth.max(1))
                .map(|depth| {
                    let mut layout = base.clone();
                    if let Some(layer) = layers.iter().find(|layer| layer.depth == depth) {
                        if let Some(cols) = layer.grid_cols {
                            layout.cols = cols.max(1) as usize;
                        }
                        if let Some(rows) = layer.grid_rows {
                            layout.rows = rows.max(1) as usize;
                        }
                        if let Some(keys) = &layer.keys {
                            layout.keys = keys.chars().collect();
                        }
                    }
                    layout
                })
                .collect()
        };
        Self { layouts }
    }
}

impl LetterGeometry for GridGeometry {
    fn layout_at(&self, depth: u32) -> &Layout {
        &self.layouts[(depth as usize).min(self.layouts.len() - 1)]
    }
}

impl Geometry for GridGeometry {
    const RESET_TO_ACTIVE_SCREEN: bool = false;

    fn cell_at(&self, area: Rect, depth: u32, index: usize) -> Option<Rect> {
        let layout = self.layout_at(depth);
        area.subdivision(layout.rows, layout.cols, index)
    }
    fn can_descend(&self, session: &TargetingSession, max_depth: u32) -> bool {
        session.depth() < max_depth
    }
    fn claims_key(&self, key: &Key) -> bool {
        key.as_char()
            .is_some_and(|ch| self.layouts.iter().any(|layout| layout.keys.contains(&ch)))
    }
    fn key_cell(&self, area: Rect, depth: u32, key: &Key) -> Option<(usize, Rect)> {
        let ch = key.as_char()?;
        let layout = self.layout_at(depth);
        let index = layout.keys.iter().position(|candidate| *candidate == ch)?;
        area.subdivision(layout.rows, layout.cols, index)
            .map(|cell| (index, cell))
    }
}

pub(crate) struct RecursiveGeometry {
    pub(crate) grid: GridGeometry,
    pub(crate) min_size: (f64, f64),
}

impl LetterGeometry for RecursiveGeometry {
    fn layout_at(&self, depth: u32) -> &Layout {
        self.grid.layout_at(depth)
    }
}

impl Geometry for RecursiveGeometry {
    const RESET_TO_ACTIVE_SCREEN: bool = true;

    fn cell_at(&self, area: Rect, depth: u32, index: usize) -> Option<Rect> {
        self.grid.cell_at(area, depth, index)
    }
    fn can_descend(&self, session: &TargetingSession, max_depth: u32) -> bool {
        if session.depth() + 1 >= max_depth {
            return false;
        }
        let Some(area) = session.current() else {
            return false;
        };
        let layout = self.layout_at(session.depth());
        area.width / layout.cols as f64 >= self.min_size.0
            && area.height / layout.rows as f64 >= self.min_size.1
    }
    fn claims_key(&self, key: &Key) -> bool {
        self.grid.claims_key(key)
    }
    fn key_cell(&self, area: Rect, depth: u32, key: &Key) -> Option<(usize, Rect)> {
        self.grid.key_cell(area, depth, key)
    }
}

pub(crate) struct BisectGeometry {
    pub(crate) min_size: (f64, f64),
}

impl BisectGeometry {
    pub(crate) fn axes(&self, session: &TargetingSession, max_depth: u32) -> (bool, bool) {
        if session.depth() >= max_depth {
            return (false, false);
        }
        let Some(area) = session.current() else {
            return (false, false);
        };
        (
            area.width / 2.0 >= self.min_size.0,
            area.height / 2.0 >= self.min_size.1,
        )
    }
}

impl Geometry for BisectGeometry {
    const RESET_TO_ACTIVE_SCREEN: bool = true;

    fn cell_at(&self, area: Rect, _depth: u32, index: usize) -> Option<Rect> {
        match index {
            0 | 1 if area.width / 2.0 >= self.min_size.0 => area.subdivision(1, 2, index),
            2 | 3 if area.height / 2.0 >= self.min_size.1 => area.subdivision(2, 1, index - 2),
            _ => None,
        }
    }
    fn can_descend(&self, session: &TargetingSession, max_depth: u32) -> bool {
        let (horizontal, vertical) = self.axes(session, max_depth);
        horizontal || vertical
    }
    fn direction_cell(&self, area: Rect, direction: Direction) -> Option<(usize, Rect)> {
        let index = match direction {
            Direction::Left => 0,
            Direction::Right => 1,
            Direction::Up => 2,
            Direction::Down => 3,
        };
        self.cell_at(area, 0, index).map(|cell| (index, cell))
    }
}
