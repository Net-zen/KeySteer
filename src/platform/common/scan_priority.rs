//! Generation-local spatial scheduling. Priority changes work order, never scope.

use crate::api::{Point, Rect};

use super::image_tiles::{OcrLayout, Tile};

#[derive(Clone, Copy, Default)]
pub(crate) struct ScanPriority {
    pointer: Option<Point>,
    window: Option<Rect>,
}

impl ScanPriority {
    pub(crate) fn new(pointer: Option<Point>, window: Option<Rect>) -> Self {
        Self {
            pointer: pointer.filter(|p| p.x.is_finite() && p.y.is_finite()),
            window,
        }
    }

    pub(crate) fn sort<T>(&self, items: &mut [T], bounds: impl Fn(&T) -> Rect) {
        let Some(pointer) = self.pointer else {
            return;
        };
        let rank = |rect: Rect| {
            let group = if rect.contains(&pointer) {
                0
            } else if self.window.is_some_and(|w| w.intersect(&rect).is_some()) {
                1
            } else {
                2
            };
            let dx = (rect.x - pointer.x).max(pointer.x - rect.right()).max(0.0);
            let dy = (rect.y - pointer.y).max(pointer.y - rect.bottom()).max(0.0);
            (group, dx * dx + dy * dy)
        };
        // Stable ties retain native Z order and the original raster order.
        items.sort_by(|a, b| {
            let a = rank(bounds(a));
            let b = rank(bounds(b));
            a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1))
        });
    }

    /// Reorder the provider's existing crops without changing any core or halo.
    pub(crate) fn ocr_tiles(
        &self,
        layout: OcrLayout,
        width: u32,
        height: u32,
        halo: usize,
        bounds: Rect,
    ) -> Vec<Tile> {
        let mut tiles: Vec<_> = layout.tiles(width, height, halo).collect();
        self.sort(&mut tiles, |tile| {
            Rect::new(
                bounds.x + tile.x as f64 / f64::from(width) * bounds.width,
                bounds.y + tile.y as f64 / f64::from(height) * bounds.height,
                (tile.right - tile.x) as f64 / f64::from(width) * bounds.width,
                (tile.bottom - tile.y) as f64 / f64::from(height) * bounds.height,
            )
        });
        tiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_screen_and_window_precede_distant_work_with_stable_z_order() {
        let pointer = Point::new(-600.0, 250.0);
        let window = Rect::new(-1000.0, 100.0, 800.0, 600.0);
        let priority = ScanPriority::new(Some(pointer), Some(window));
        let mut screens = [Rect::new(0.0, 0.0, 1920.0, 1080.0), window];
        priority.sort(&mut screens, |r| *r);
        assert_eq!(screens[0], window);
        let mut work = [
            (0, Rect::new(100.0, 100.0, 100.0, 100.0)),
            (1, window),
            (2, window),
            (3, Rect::new(-900.0, 400.0, 100.0, 100.0)),
        ];
        priority.sort(&mut work, |(_, r)| *r);
        assert_eq!(work.map(|(id, _)| id), [1, 2, 3, 0]);
        let before = work;
        ScanPriority::default().sort(&mut work, |(_, r)| *r);
        assert_eq!(work, before);
    }

    #[test]
    fn reordered_windows_keep_their_original_occlusion_prefixes() {
        use crate::platform::common::window_visibility::visible_part;
        let bounds = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let front = Rect::new(0.0, 0.0, 400.0, 800.0);
        let occluders = [front, bounds];
        let mut order = [0, 1];
        ScanPriority::new(Some(Point::new(700.0, 200.0)), Some(bounds))
            .sort(&mut order, |&index| occluders[index]);
        assert_eq!(order, [1, 0]);
        let visible: Vec<_> = order
            .iter()
            .map(|&index| visible_part(occluders[index], bounds, &occluders[..index], &[]).unwrap())
            .collect();
        assert_eq!(visible, [Rect::new(400.0, 0.0, 600.0, 800.0), front]);
    }

    #[test]
    fn tile_priority_preserves_the_existing_crops_and_puts_the_pointer_core_first() {
        for (width, height) in [
            (200, 100),
            (2880, 1800),
            (3841, 2161),
            (4096, 2048),
            (4096, 180),
        ] {
            let bounds = Rect::new(-1920.0, -100.0, width as f64 / 2.0, height as f64 / 2.0);
            for lanes in [1, 2, 8, 16] {
                // Exercise both platforms' minimum tile sizes.
                for minimum in [64, 384] {
                    let layout = OcrLayout::new(width, height, lanes, minimum, 16);
                    for (px, py) in [(0.0, 0.0), (0.5, 0.5), (0.63, 0.72), (0.999, 0.999)] {
                        let pointer =
                            Point::new(bounds.x + bounds.width * px, bounds.y + bounds.height * py);
                        let priority = ScanPriority::new(Some(pointer), Some(bounds));
                        let tiles = priority.ocr_tiles(layout, width, height, 64, bounds);
                        assert_eq!(tiles.len(), layout.tile_count);
                        let original: Vec<_> = layout.tiles(width, height, 64).collect();
                        assert!(tiles.iter().all(|tile| original.contains(tile)));
                        let first = tiles[0];
                        let x = (px * width as f64) as usize;
                        let y = (py * height as f64) as usize;
                        assert!(
                            first.x <= x && x < first.right && first.y <= y && y < first.bottom
                        );
                        let mut area = 0;
                        for (i, tile) in tiles.iter().enumerate() {
                            area += (tile.right - tile.x) * (tile.bottom - tile.y);
                            assert_eq!(
                                *tile,
                                Tile::with_halo(
                                    width as usize,
                                    height as usize,
                                    tile.x,
                                    tile.y,
                                    tile.right,
                                    tile.bottom,
                                    64
                                )
                            );
                            for other in &tiles[..i] {
                                assert!(
                                    tile.right <= other.x
                                        || other.right <= tile.x
                                        || tile.bottom <= other.y
                                        || other.bottom <= tile.y
                                );
                            }
                        }
                        assert_eq!(area, width as usize * height as usize);
                        assert!(layout.max_in_flight <= lanes);
                    }
                    let original: Vec<_> = layout.tiles(width, height, 64).collect();
                    for pointer in [None, Some(Point::new(f64::NAN, 0.0))] {
                        assert_eq!(
                            ScanPriority::new(pointer, None)
                                .ocr_tiles(layout, width, height, 64, bounds),
                            original
                        );
                    }
                }
            }
        }
    }
}
