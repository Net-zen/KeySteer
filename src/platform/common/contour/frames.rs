//! Small, empty control frames that the blurred Canny pass can miss.
//! Inspired by Neru's low step threshold and four-side coverage, restricted
//! to near-square frames so text and photographs keep their existing owners.
use super::MAX_PIXELS;
use crate::api::{Rect, SemanticRole, UiTarget};

pub(super) fn detect(
    gray: &[u8],
    w: usize,
    h: usize,
    bounds: Rect,
    stop: &impl Fn() -> bool,
) -> Vec<UiTarget> {
    if w < 3
        || h < 3
        || w.checked_mul(h) != Some(gray.len())
        || gray.len() > MAX_PIXELS
        || bounds.is_empty()
        || ![bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .all(|v| v.is_finite())
        || stop()
    {
        return Vec::new();
    }
    let edge = |x: usize, y: usize| {
        let p = gray[y * w + x];
        (x + 1 < w && p.abs_diff(gray[y * w + x + 1]) >= 8)
            || (y + 1 < h && p.abs_diff(gray[(y + 1) * w + x]) >= 8)
    };
    let mut mask = vec![0; gray.len()];
    for y in 0..h - 1 {
        if stop() {
            return Vec::new();
        }
        let row = &gray[y * w..][..w];
        let below = &gray[(y + 1) * w..][..w - 1];
        for ((out, pair), &bottom) in mask[y * w..][..w - 1]
            .iter_mut()
            .zip(row.windows(2))
            .zip(below)
        {
            *out = u8::from(pair[0].abs_diff(pair[1]) >= 8 || pair[0].abs_diff(bottom) >= 8);
        }
        mask[(y + 1) * w - 1] = u8::from(edge(w - 1, y));
    }
    for x in 0..w - 1 {
        mask[(h - 1) * w + x] = u8::from(edge(x, h - 1));
    }
    // Reject impossible frame sizes as components finish, before storing the
    // many text/photo fragments that will never need four-side validation.
    let Some(candidates) = super::components(mask, w, h, stop, |c| {
        let rect = c.rect(w, h, bounds);
        (18.0..=96.0).contains(&rect.width)
            && (18.0..=96.0).contains(&rect.height)
            && (0.7..=1.4).contains(&(rect.width / rect.height))
            && c.right - c.left + 1 >= 8
            && c.bottom - c.top + 1 >= 8
            && c.left != 0
            && c.top != 0
            && c.right + 1 != w
            && c.bottom + 1 != h
    }) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for c in candidates {
        if stop() {
            return Vec::new();
        }
        let rw = c.right - c.left + 1;
        let rh = c.bottom - c.top + 1;
        let rect = c.rect(w, h, bounds);
        let top = (c.left..=c.right)
            .filter(|&x| edge(x, c.top) || edge(x, c.top + 1))
            .count();
        let bottom = (c.left..=c.right)
            .filter(|&x| edge(x, c.bottom) || edge(x, c.bottom - 1))
            .count();
        let left = (c.top..=c.bottom)
            .filter(|&y| edge(c.left, y) || edge(c.left + 1, y))
            .count();
        let right = (c.top..=c.bottom)
            .filter(|&y| edge(c.right, y) || edge(c.right - 1, y))
            .count();
        if top * 10 < rw * 7 || bottom * 10 < rw * 7 || left * 10 < rh * 7 || right * 10 < rh * 7 {
            continue;
        }
        let centre = gray[((c.top + c.bottom) / 2) * w + (c.left + c.right) / 2];
        let (mut flat, mut total) = (0, 0);
        for y in c.top + 3..c.bottom - 2 {
            if stop() {
                return Vec::new();
            }
            for &pixel in &gray[y * w + c.left + 3..y * w + c.right - 2] {
                total += 1;
                flat += usize::from(centre.abs_diff(pixel) <= 8);
            }
        }
        if total != 0 && flat * 10 >= total * 9 {
            result.push(UiTarget {
                rect,
                role: SemanticRole::Control,
                name: String::new(),
                details: None,
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_frames_require_four_sides_and_flat_interiors() {
        let (w, h) = (180, 80);
        let mut gray = vec![250; w * h];
        for left in [10, 65, 120] {
            for y in 15..55 {
                for x in left..left + 40 {
                    if y == 15 || y == 54 || x == left || x == left + 39 {
                        gray[y * w + x] = 225;
                    }
                }
            }
        }
        // Open right side, and a closed square with a textured interior.
        for y in 15..55 {
            gray[y * w + 104] = 250;
        }
        for y in 20..50 {
            for x in 125..155 {
                gray[y * w + x] = if (x + y) % 2 == 0 { 80 } else { 220 };
            }
        }
        let targets = detect(&gray, w, h, Rect::new(-50., 70., 180., 80.), &|| false);
        assert_eq!(targets.len(), 1);
        assert!(
            targets[0]
                .rect
                .contains(&crate::api::Point::new(-20., 105.))
        );
        assert!(detect(&gray, w, h, Rect::new(0., 0., 180., 80.), &|| true).is_empty());
    }
}
