//! Cached display selection from backend-owned geometry, without native queries.

use std::cell::Cell;

use smallvec::SmallVec;

use crate::api::{Point, Rect};

#[derive(Default)]
pub(crate) struct DisplayTargetCache {
    bounds: SmallVec<[Rect; 4]>,
    selected: Cell<Option<usize>>,
}

impl DisplayTargetCache {
    /// Called only when the backend replaces its display or surface snapshot.
    pub(crate) fn replace(&mut self, bounds: impl IntoIterator<Item = Rect>) {
        self.bounds.clear();
        self.bounds.extend(bounds);
        self.selected.set(None);
    }

    /// Stay on the cached display while its rectangle contains the point.
    /// Gaps use the nearest rectangle; an absent point uses the first display.
    pub(crate) fn select(&self, point: Option<Point>) -> Option<usize> {
        let selected = match point.filter(|point| point.x.is_finite() && point.y.is_finite()) {
            Some(point) => {
                if let Some(index) = self.selected.get()
                    && self
                        .bounds
                        .get(index)
                        .is_some_and(|area| area.contains(&point))
                {
                    return Some(index);
                }
                self.bounds
                    .iter()
                    .position(|area| area.contains(&point))
                    .or_else(|| {
                        self.bounds
                            .iter()
                            .enumerate()
                            .filter(|(_, area)| !area.is_empty())
                            .min_by(|(_, a), (_, b)| {
                                distance_squared(**a, point)
                                    .total_cmp(&distance_squared(**b, point))
                            })
                            .map(|(index, _)| index)
                    })
            }
            None => self.bounds.iter().position(|area| !area.is_empty()),
        };
        self.selected.set(selected);
        selected
    }
}

fn distance_squared(area: Rect, point: Point) -> f64 {
    let dx = (area.x - point.x).max(0.0).max(point.x - area.right());
    let dy = (area.y - point.y).max(0.0).max(point.y - area.bottom());
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_selection_tracks_negative_origins_and_exact_screen_edges() {
        let mut cache = DisplayTargetCache::default();
        cache.replace([
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
            Rect::new(-1280.0, -200.0, 1280.0, 1024.0),
            Rect::new(0.0, -1440.0, 2560.0, 1440.0),
        ]);
        assert_eq!(cache.select(Some(Point::new(0.0, 0.0))), Some(0));
        for x in 0..1920 {
            assert_eq!(cache.select(Some(Point::new(x as f64, 500.0))), Some(0));
        }
        assert_eq!(cache.select(Some(Point::new(-0.01, 0.0))), Some(1));
        assert_eq!(cache.select(Some(Point::new(0.0, -0.01))), Some(2));
        assert_eq!(cache.select(Some(Point::new(1919.0, 1079.0))), Some(0));
    }

    #[test]
    fn gaps_and_offscreen_points_use_cached_nearest_geometry() {
        let mut cache = DisplayTargetCache::default();
        cache.replace([
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Rect::new(300.0, -100.0, 200.0, 200.0),
        ]);
        assert_eq!(cache.select(Some(Point::new(120.0, 50.0))), Some(0));
        assert_eq!(cache.select(Some(Point::new(280.0, 50.0))), Some(1));
        assert_eq!(cache.select(Some(Point::new(-500.0, -500.0))), Some(0));
        assert_eq!(cache.select(Some(Point::new(800.0, -500.0))), Some(1));
    }

    #[test]
    fn replacing_geometry_invalidates_indices_even_if_bounds_are_unchanged() {
        let mut cache = DisplayTargetCache::default();
        let bounds = [
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Rect::new(100.0, 0.0, 100.0, 100.0),
        ];
        cache.replace(bounds);
        assert_eq!(cache.select(Some(Point::new(150.0, 20.0))), Some(1));
        cache.replace(bounds);
        assert_eq!(cache.selected.get(), None);
        cache.replace([bounds[1]]);
        assert_eq!(cache.select(Some(Point::new(150.0, 20.0))), Some(0));
        cache.replace([]);
        assert_eq!(cache.select(Some(Point::new(150.0, 20.0))), None);
    }

    #[test]
    fn empty_and_unknown_positions_do_not_reuse_a_stale_index() {
        let mut cache = DisplayTargetCache::default();
        assert_eq!(cache.select(None), None);
        cache.replace([Rect::default(), Rect::new(100.0, 0.0, 100.0, 100.0)]);
        assert_eq!(cache.select(None), Some(1));
        assert_eq!(cache.select(Some(Point::new(f64::NAN, 0.0))), Some(1));
    }
}
