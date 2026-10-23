//! Bounded luma preparation shared by native BGRA capture providers.
#![forbid(unsafe_code)]

use crate::api::Rect;

// Bound only the contour analysis image, together with MAX_PIXELS. Capture
// and OCR keep their own resolution; detected bounds map back to the desktop.
const MAX_EDGE: f64 = 2560.0;

pub(crate) struct Input {
    pub(crate) gray: Vec<u8>,
    // Only downsampled captures need this second bounded plane. Keep the
    // original luma byte-for-byte for existing fragment and image detection.
    pub(super) frame_gray: Option<Vec<u8>>,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) desktop_bounds: Rect,
}

pub(crate) fn from_bgra(
    pixels: &[u8],
    (source_width, source_height, stride): (usize, usize, usize),
    desktop_bounds: Rect,
    mut cancelled: impl FnMut() -> Result<bool, String>,
) -> Result<Option<Input>, String> {
    let expected = stride
        .checked_mul(source_height)
        .ok_or_else(|| "captured image dimensions overflow".to_string())?;
    if source_width == 0
        || source_height == 0
        || source_width.checked_mul(4).is_none_or(|row| row > stride)
        || pixels.len() != expected
    {
        return Err("captured BGRA length does not match its geometry".into());
    }
    let edge_scale = (MAX_EDGE / source_width.max(source_height) as f64).min(1.0);
    let pixel_scale = (super::MAX_PIXELS as f64 / (source_width * source_height) as f64)
        .sqrt()
        .min(1.0);
    let analysis_scale = edge_scale.min(pixel_scale);
    let width = (source_width as f64 * analysis_scale).floor().max(2.0) as usize;
    let height = (source_height as f64 * analysis_scale).floor().max(2.0) as usize;
    let mut gray = Vec::with_capacity(width * height);
    let mut frame_gray = None;
    if width == source_width && height == source_height {
        // 1080p and smaller captures need no coordinate tables or zero-fill.
        for (y, row) in pixels.chunks_exact(stride).enumerate() {
            if y.is_multiple_of(32) && cancelled()? {
                return Ok(None);
            }
            gray.extend(
                row[..source_width * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(bgra_luma),
            );
        }
    } else {
        let source_x_offsets = nearest_offsets(source_width, width);
        let source_y_offsets = nearest_offsets(source_height, height);
        gray.resize(width * height, 0);
        let mut mean = vec![0; width * height];
        for (y, source_y) in source_y_offsets.into_iter().enumerate() {
            if y.is_multiple_of(32) && cancelled()? {
                return Ok(None);
            }
            let row = pixels[source_y * stride..][..source_width * 4]
                .as_chunks::<4>()
                .0;
            let bottom = pixels[(source_y + 1).min(source_height - 1) * stride..]
                [..source_width * 4]
                .as_chunks::<4>()
                .0;
            for ((out, averaged), &x) in gray[y * width..][..width]
                .iter_mut()
                .zip(&mut mean[y * width..][..width])
                .zip(&source_x_offsets)
            {
                let first = bgra_luma(&row[x]);
                *out = first;
                let right = (x + 1).min(source_width - 1);
                *averaged = ((u16::from(first)
                    + u16::from(bgra_luma(&row[right]))
                    + u16::from(bgra_luma(&bottom[x]))
                    + u16::from(bgra_luma(&bottom[right]))
                    + 2)
                    / 4) as u8;
            }
        }
        frame_gray = Some(mean);
    }
    Ok(Some(Input {
        gray,
        frame_gray,
        width,
        height,
        desktop_bounds,
    }))
}

#[inline]
fn bgra_luma(pixel: &[u8; 4]) -> u8 {
    ((u16::from(pixel[2]) * 77 + u16::from(pixel[1]) * 150 + u16::from(pixel[0]) * 29) >> 8) as u8
}

/// Return the exact `floor(output * source / destination)` mapping without a
/// division in the per-pixel conversion loop.
pub(crate) fn nearest_offsets(source: usize, destination: usize) -> Vec<usize> {
    debug_assert!(source != 0 && destination != 0);
    let step = source / destination;
    let remainder_step = source % destination;
    let mut source_index = 0usize;
    let mut remainder = 0usize;
    let mut offsets = Vec::with_capacity(destination);
    for _ in 0..destination {
        offsets.push(source_index.min(source - 1));
        source_index += step;
        remainder += remainder_step;
        if remainder >= destination {
            source_index += 1;
            remainder -= destination;
        }
    }
    offsets
}
