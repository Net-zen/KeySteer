//! Shared pixel partitioning for OCR and contour analysis.

pub(crate) const MAX_OCR_IN_FLIGHT: usize = 16;

/// Rotate ready frame tiles without creating a separate concurrency budget for
/// each frame. Empty frames are removed while preserving display order.
#[cfg(any(windows, test))]
pub(crate) fn round_robin<I: Iterator>(
    inputs: impl IntoIterator<Item = I>,
) -> impl Iterator<Item = (usize, I::Item)> {
    let mut pending: smallvec::SmallVec<[_; 2]> = inputs.into_iter().enumerate().collect();
    let mut cursor = 0;
    std::iter::from_fn(move || {
        while !pending.is_empty() {
            if cursor == pending.len() {
                cursor = 0;
            }
            let (frame, input) = &mut pending[cursor];
            if let Some(tile) = input.next() {
                let frame = *frame;
                cursor += 1;
                return Some((frame, tile));
            }
            pending.remove(cursor);
        }
        None
    })
}

pub(crate) fn partition(value: u32, index: u32, divisions: u32) -> u32 {
    ((u64::from(value) * u64::from(index)) / u64::from(divisions)) as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tile {
    pub x: usize,
    pub y: usize,
    pub right: usize,
    pub bottom: usize,
    pub left_halo: usize,
    pub top_halo: usize,
    pub right_halo: usize,
    pub bottom_halo: usize,
}

impl Tile {
    pub(crate) fn with_halo(
        width: usize,
        height: usize,
        x: usize,
        y: usize,
        right: usize,
        bottom: usize,
        halo: usize,
    ) -> Self {
        Self {
            x,
            y,
            right,
            bottom,
            left_halo: x.saturating_sub(halo),
            top_halo: y.saturating_sub(halo),
            right_halo: right.saturating_add(halo).min(width),
            bottom_halo: bottom.saturating_add(halo).min(height),
        }
    }
}

/// The original Windows OCR layout, with provider-specific resource bounds.
/// Tile cores own results; halos supply context without resampling pixels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OcrLayout {
    pub(crate) grid: u32,
    pub(crate) tile_count: usize,
    pub(crate) max_in_flight: usize,
}

impl OcrLayout {
    pub(crate) fn new(
        width: u32,
        height: u32,
        parallelism: usize,
        minimum_side: u32,
        maximum_in_flight: usize,
    ) -> Self {
        let cpu_grid = parallelism.max(1).isqrt().saturating_add(1);
        let size_grid = (width / minimum_side.max(1))
            .min(height / minimum_side.max(1))
            .max(1) as usize;
        let grid = cpu_grid.min(size_grid).max(1) as u32;
        let tile_count = (grid as usize).saturating_mul(grid as usize);
        Self {
            grid,
            tile_count,
            max_in_flight: tile_count
                .min(parallelism.max(1))
                .clamp(1, maximum_in_flight.max(1)),
        }
    }

    pub(crate) fn tiles(self, width: u32, height: u32, halo: usize) -> impl Iterator<Item = Tile> {
        (0..self.grid).flat_map(move |row| {
            (0..self.grid).map(move |column| {
                Tile::with_halo(
                    width as usize,
                    height as usize,
                    partition(width, column, self.grid) as usize,
                    partition(height, row, self.grid) as usize,
                    partition(width, column + 1, self.grid) as usize,
                    partition(height, row + 1, self.grid) as usize,
                    halo,
                )
            })
        })
    }
}

pub(crate) fn tiles(
    width: usize,
    height: usize,
    side: usize,
    halo: usize,
) -> impl Iterator<Item = Tile> {
    let nx = width.div_ceil(side) as u32;
    let ny = height.div_ceil(side) as u32;
    (0..ny).flat_map(move |row| {
        (0..nx).map(move |col| {
            let x = partition(width as u32, col, nx) as usize;
            let right = partition(width as u32, col + 1, nx) as usize;
            let y = partition(height as u32, row, ny) as usize;
            let bottom = partition(height as u32, row + 1, ny) as usize;
            Tile {
                x,
                y,
                right,
                bottom,
                left_halo: x.saturating_sub(halo),
                top_halo: y.saturating_sub(halo),
                right_halo: (right + halo).min(width),
                bottom_halo: (bottom + halo).min(height),
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracted_layout_preserves_windows_grid_credits_and_every_crop_pixel() {
        for (width, height) in [(200, 100), (320, 200), (3841, 2161), (7680, 4320)] {
            for parallelism in 0usize..=65 {
                let cpu_grid = parallelism.max(1).isqrt().saturating_add(1);
                let size_grid = (width / 64u32).min(height / 64).max(1) as usize;
                let old_grid = cpu_grid.min(size_grid).max(1) as u32;
                let count = (old_grid as usize).saturating_mul(old_grid as usize);
                let layout = OcrLayout::new(width, height, parallelism, 64, 16);
                assert_eq!(layout.grid, old_grid);
                assert_eq!(layout.tile_count, count);
                assert_eq!(
                    layout.max_in_flight,
                    count.min(parallelism.max(1)).clamp(1, 16)
                );
                let mut actual = layout.tiles(width, height, 64);
                for row in 0..old_grid {
                    for column in 0..old_grid {
                        let x = partition(width, column, old_grid);
                        let y = partition(height, row, old_grid);
                        let right = partition(width, column + 1, old_grid);
                        let bottom = partition(height, row + 1, old_grid);
                        assert_eq!(
                            actual.next(),
                            Some(Tile {
                                x: x as usize,
                                y: y as usize,
                                right: right as usize,
                                bottom: bottom as usize,
                                left_halo: x.saturating_sub(64) as usize,
                                top_halo: y.saturating_sub(64) as usize,
                                right_halo: right.saturating_add(64).min(width) as usize,
                                bottom_halo: bottom.saturating_add(64).min(height) as usize,
                            })
                        );
                    }
                }
                assert!(actual.next().is_none());
            }
        }
        let frames = [(3840, 2160), (1200, 180), (2160, 3840)];
        let layouts: Vec<_> = frames
            .iter()
            .map(|&(w, h)| OcrLayout::new(w, h, 2, 384, 16))
            .collect();
        let actual: Vec<_> = round_robin(
            frames
                .iter()
                .zip(&layouts)
                .map(|(&(w, h), &layout)| layout.tiles(w, h, 64)),
        )
        .collect();
        assert_eq!(
            actual
                .iter()
                .take(3)
                .map(|&(frame, _)| frame)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        for (index, (&(w, h), &layout)) in frames.iter().zip(&layouts).enumerate() {
            assert_eq!(
                actual
                    .iter()
                    .filter_map(|&(frame, tile)| (frame == index).then_some(tile))
                    .collect::<Vec<_>>(),
                layout.tiles(w, h, 64).collect::<Vec<_>>()
            );
        }
        assert!(
            round_robin(std::iter::empty::<std::iter::Empty<Tile>>())
                .next()
                .is_none()
        );
    }

    #[test]
    fn macos_tiles_cover_the_capture_with_bounded_overlap_and_concurrency() {
        for (width, height) in [(1200, 180), (1440, 900), (2880, 1800), (4096, 2048)] {
            for parallelism in [1, 2, 4, 8, 16] {
                let layout = OcrLayout::new(width, height, parallelism, 384, MAX_OCR_IN_FLIGHT);
                let tiles: Vec<_> = layout.tiles(width, height, 64).collect();
                assert_eq!(tiles.len(), layout.tile_count);
                assert!((1..=parallelism).contains(&layout.max_in_flight));
                assert!(layout.tile_count <= 25);
                if height < 384 {
                    assert_eq!(
                        layout.tile_count, 1,
                        "small captures need no extra OCR requests"
                    );
                }
                let mut area = 0;
                for tile in &tiles {
                    assert!(tile.left_halo <= tile.x && tile.top_halo <= tile.y);
                    assert!(tile.right_halo >= tile.right && tile.bottom_halo >= tile.bottom);
                    assert!(
                        tile.right_halo <= width as usize && tile.bottom_halo <= height as usize
                    );
                    assert!(tile.x - tile.left_halo <= 64 && tile.y - tile.top_halo <= 64);
                    assert!(
                        tile.right_halo - tile.right <= 64 && tile.bottom_halo - tile.bottom <= 64
                    );
                    area += (tile.right - tile.x) * (tile.bottom - tile.y);
                }
                assert_eq!(area, width as usize * height as usize);
            }
        }
    }
}
