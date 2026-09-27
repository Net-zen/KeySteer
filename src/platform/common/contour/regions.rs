//! Bounded, pure Rust image-region proposals inspired by UIED's separation of
//! text/containers from non-text regions. Not a UIED port or clickability model.
//! Keeps image interiors from becoming hundreds of independent contour targets.
use super::{MAX_PIXELS, components};
use crate::api::Rect;

pub(super) fn detect(
    gray: &[u8],
    width: usize,
    height: usize,
    bounds: Rect,
    stop: &impl Fn() -> bool,
) -> Vec<Rect> {
    if width < 3
        || height < 3
        || width.checked_mul(height) != Some(gray.len())
        || gray.len() > MAX_PIXELS
        || bounds.is_empty()
        || ![bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .all(|v| v.is_finite())
        || stop()
    {
        return Vec::new();
    }
    let scale = (960. / width.max(height) as f64).min(1.);
    let w = ((width as f64 * scale) as usize).max(3);
    let h = ((height as f64 * scale) as usize).max(3);
    let columns: Vec<_> = (0..w).map(|x| x * width / w).collect();
    let mut luma = vec![0u8; w * h];
    for y in 0..h {
        if y % 32 == 0 && stop() {
            return Vec::new();
        }
        let row = &gray[(y * height / h) * width..][..width];
        for (out, &x) in luma[y * w..][..w].iter_mut().zip(&columns) {
            *out = row[x];
        }
    }
    let mut edges = vec![0u8; w * h];
    for y in 0..h - 1 {
        if y % 32 == 0 && stop() {
            return Vec::new();
        }
        for x in 0..w - 1 {
            let i = y * w + x;
            edges[i] = u8::from(
                u16::from(luma[i].abs_diff(luma[i + 1])) + u16::from(luma[i].abs_diff(luma[i + w]))
                    > 20,
            );
        }
    }
    // 3x3 closing connects textured interiors without a full-screen kernel.
    let Some(grown) = morphology(&edges, w, h, true, stop) else {
        return Vec::new();
    };
    let Some(closed) = morphology(&grown, w, h, false, stop) else {
        return Vec::new();
    };
    drop(grown);
    let Some(parts) = components(closed, w, h, stop) else {
        return Vec::new();
    };
    let mut owners = Vec::new();
    for c in parts {
        if stop() {
            return Vec::new();
        }
        let r = c.rect(w, h, bounds);
        if r.width < 48.
            || r.height < 48.
            || r.width > bounds.width * 0.85
            || r.height > bounds.height * 0.75
            || r.width > r.height * 6.
            || r.height > r.width * 6.
        {
            continue;
        }
        let mut histogram = [0usize; 16];
        let (mut samples, mut ink) = (0usize, 0usize);
        for y in (c.top..=c.bottom).step_by(2) {
            if stop() {
                return Vec::new();
            }
            for x in (c.left..=c.right).step_by(2) {
                histogram[usize::from(luma[y * w + x] >> 4)] += 1;
                ink += usize::from(edges[y * w + x]);
                samples += 1;
            }
        }
        // Flat backgrounds (including selected rows and code panes) cannot
        // become owners just because their border encloses many children.
        if samples > 0
            && ink * 100 >= samples * 28
            && histogram.into_iter().max().unwrap_or(0) * 100 < samples * 45
        {
            owners.push(r);
        }
    }
    owners
}

// Binary 3x3 morphology. Zip equal-length row windows so the hot loop has
// no per-pixel row slicing, short-circuit searches, or coordinate arithmetic.
fn morphology(
    input: &[u8],
    w: usize,
    h: usize,
    grow: bool,
    stop: &impl Fn() -> bool,
) -> Option<Vec<u8>> {
    let mut output = vec![0; w * h];
    for y in 1..h - 1 {
        if y % 32 == 0 && stop() {
            return None;
        }
        let rows = input[(y - 1) * w..][..w]
            .windows(3)
            .zip(input[y * w..][..w].windows(3))
            .zip(input[(y + 1) * w..][..w].windows(3));
        for (out, ((a, b), c)) in output[y * w + 1..y * w + w - 1].iter_mut().zip(rows) {
            *out = if grow {
                a[0] | a[1] | a[2] | b[0] | b[1] | b[2] | c[0] | c[1] | c[2]
            } else {
                a[0] & a[1] & a[2] & b[0] & b[1] & b[2] & c[0] & c[1] & c[2]
            };
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morphology_matches_reference_at_borders_and_varied_density() {
        for (w, h) in [(3, 3), (7, 11), (64, 32), (127, 65)] {
            for density in [0, 1, 3, 7, 16] {
                let input: Vec<_> = (0..w * h)
                    .map(|i| u8::from((i * 71 + i * i * 13) % 17 < density))
                    .collect();
                for grow in [false, true] {
                    let actual = morphology(&input, w, h, grow, &|| false).unwrap();
                    let mut expected = vec![0; w * h];
                    for y in 1..h - 1 {
                        for x in 1..w - 1 {
                            let ones = (y - 1..=y + 1)
                                .flat_map(|yy| (x - 1..=x + 1).map(move |xx| yy * w + xx))
                                .filter(|&i| input[i] == 1)
                                .count();
                            expected[y * w + x] =
                                u8::from(if grow { ones != 0 } else { ones == 9 });
                        }
                    }
                    assert_eq!(actual, expected, "{w}x{h} density={density} grow={grow}");
                }
            }
        }
    }

    #[test]
    fn texture_owner_but_not_flat_selected_background() {
        let (w, h) = (600, 400);
        let bounds = Rect::new(-600., 100., 600., 400.);
        let mut frame = vec![245u8; w * h];
        for y in 100..200 {
            for x in 100..280 {
                frame[y * w + x] = ((x * 71 + y * 137 + x * y * 13) % 240) as u8;
            }
        }
        let owners = detect(&frame, w, h, bounds, &|| false);
        assert!(owners.iter().any(|r| r.width > 150. && r.height > 80.));
        for y in 100..200 {
            for x in 100..280 {
                frame[y * w + x] = 180;
            }
        }
        assert!(detect(&frame, w, h, bounds, &|| false).is_empty());
        assert!(detect(&frame, w, h, bounds, &|| true).is_empty());
        assert!(detect(&[], w, h, bounds, &|| false).is_empty());
    }
}
