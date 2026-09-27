//! Shared pixel partitioning for OCR and contour analysis.

pub(crate) fn partition(value: u32, index: u32, divisions: u32) -> u32 {
    ((u64::from(value) * u64::from(index)) / u64::from(divisions)) as u32
}

#[derive(Clone, Copy)]
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
