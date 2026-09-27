//! Only borrows a bounded luma frame; no native APIs or retained screen pixels.

use crate::api::command::MAX_UI_SCAN_TARGETS;
use crate::api::{Rect, SemanticRole, UiTarget};

pub(crate) const MAX_PIXELS: usize = 2_073_600;
const MAX_COMPONENTS: usize = 32_768;
const LOW: u16 = 70;
const HIGH: u16 = 220;

mod regions;

pub(crate) fn sources(
    strategy: crate::api::UiScanStrategy,
    options: &crate::api::VisionOptions,
) -> (bool, bool) {
    use crate::api::UiScanStrategy;
    match strategy {
        UiScanStrategy::Hybrid => (true, true),
        UiScanStrategy::Contour => (false, true),
        UiScanStrategy::Vision => (options.detect_text, options.detect_rectangles),
        UiScanStrategy::AxTree => (false, false),
    }
}

/// Hybrid includes every source up to the shared scan budget, even when an
/// existing config still carries the old 100-candidate visual-only default.
pub(crate) fn candidate_limit(
    strategy: crate::api::UiScanStrategy,
    options: &crate::api::VisionOptions,
) -> usize {
    if strategy == crate::api::UiScanStrategy::Hybrid {
        MAX_UI_SCAN_TARGETS
    } else {
        options.rectangle_max_candidates.min(MAX_UI_SCAN_TARGETS)
    }
}

#[derive(Clone, Copy, Debug)]
struct Component {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    parent: Option<usize>,
}

impl Component {
    fn area(self) -> usize {
        (self.right - self.left + 1) * (self.bottom - self.top + 1)
    }

    fn rect(self, width: usize, height: usize, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x + self.left as f64 * bounds.width / width as f64,
            bounds.y + self.top as f64 * bounds.height / height as f64,
            (self.right - self.left + 1) as f64 * bounds.width / width as f64,
            (self.bottom - self.top + 1) as f64 * bounds.height / height as f64,
        )
    }
}

/// Cancellation includes the caller's generation and deadline. An interrupted
/// pass returns no boxes; already published targets from other sources survive.
pub(crate) fn detect_fragments(
    gray: &[u8],
    width: usize,
    height: usize,
    bounds: Rect,
    limit: usize,
    cancelled: impl Fn() -> bool,
) -> Vec<UiTarget> {
    if width < 3
        || height < 3
        || width.checked_mul(height) != Some(gray.len())
        || gray.len() > MAX_PIXELS
        || limit == 0
        || bounds.is_empty()
        || ![bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .all(|v| v.is_finite())
        || cancelled()
    {
        return Vec::new();
    }
    let Some(edges) = tiled_edges(gray, width, height, &cancelled) else {
        return Vec::new();
    };
    let Some(dilated) = dilate(&edges, width, height, bounds, &cancelled) else {
        return Vec::new();
    };
    drop(edges);
    let Some(mut components) = components(dilated, width, height, &cancelled) else {
        return Vec::new();
    };
    if !hierarchy(&mut components, &cancelled) {
        return Vec::new();
    }
    filter(
        &components,
        width,
        height,
        bounds,
        limit.min(MAX_UI_SCAN_TARGETS),
        &cancelled,
    )
}

pub(crate) fn detect(
    gray: &[u8],
    width: usize,
    height: usize,
    bounds: Rect,
    limit: usize,
    cancelled: impl Fn() -> bool,
) -> Vec<UiTarget> {
    let fragments = detect_fragments(gray, width, height, bounds, limit, &cancelled);
    if cancelled() {
        return Vec::new();
    }
    let owners = regions::detect(gray, width, height, bounds, &cancelled);
    if cancelled() {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(owners.len() + fragments.len());
    for rect in &owners {
        output.push(UiTarget {
            rect: *rect,
            name: String::new(),
            role: SemanticRole::Image,
        });
    }
    output.extend(fragments.into_iter().filter(|t| {
        !owners.iter().any(|r| {
            t.rect
                .intersect(r)
                .is_some_and(|i| i.width * i.height >= 0.9 * t.rect.width * t.rect.height)
        })
    }));
    output.truncate(limit.min(MAX_UI_SCAN_TARGETS));
    output
}

// Four pixels cover blur radius 2, Sobel radius 1 and nonmaximum radius 1.
// Only tile cores are copied back; hysteresis remains global across every seam.
const TILE_SIDE: usize = 256;
const TILE_HALO: usize = 4;

#[derive(Default)]
struct EdgeScratch {
    gray: Vec<u8>,
    tmp: Vec<u8>,
    blurred: Vec<u8>,
    magnitude: Vec<u16>,
    state: Vec<u8>,
}

fn tiled_edges(gray: &[u8], w: usize, h: usize, stop: &impl Fn() -> bool) -> Option<Vec<u8>> {
    let mut states = vec![0u8; gray.len()];
    let mut scratch = EdgeScratch::default();
    for tile in super::image_tiles::tiles(w, h, TILE_SIDE, TILE_HALO) {
        if stop() {
            return None;
        }
        let tw = tile.right_halo - tile.left_halo;
        let th = tile.bottom_halo - tile.top_halo;
        scratch.gray.clear();
        for y in tile.top_halo..tile.bottom_halo {
            scratch
                .gray
                .extend_from_slice(&gray[y * w + tile.left_halo..y * w + tile.right_halo]);
        }
        threshold_edges(&mut scratch, tw, th, stop)?;
        for y in tile.y..tile.bottom {
            let offset = (y - tile.top_halo) * tw + tile.x - tile.left_halo;
            states[y * w + tile.x..y * w + tile.right]
                .copy_from_slice(&scratch.state[offset..offset + tile.right - tile.x]);
        }
    }
    drop(scratch);
    hysteresis(&mut states, w, h, stop)?;
    Some(states)
}

fn threshold_edges(
    s: &mut EdgeScratch,
    w: usize,
    h: usize,
    stop: &impl Fn() -> bool,
) -> Option<()> {
    let n = w * h;
    s.tmp.resize(n, 0);
    s.blurred.resize(n, 0);
    s.magnitude.resize(n, 0);
    s.magnitude.fill(0);
    s.state.resize(n, 0);
    s.state.fill(0);
    for y in 0..h {
        if stop() {
            return None;
        }
        let row = &s.gray[y * w..(y + 1) * w];
        let out = &mut s.tmp[y * w..(y + 1) * w];
        let boundary = |x: usize| {
            ((u16::from(row[x.saturating_sub(2)])
                + 4 * u16::from(row[x.saturating_sub(1)])
                + 6 * u16::from(row[x])
                + 4 * u16::from(row[(x + 1).min(w - 1)])
                + u16::from(row[(x + 2).min(w - 1)])
                + 8)
                >> 4) as u8
        };
        if w < 5 {
            for (x, value) in out.iter_mut().enumerate() {
                *value = boundary(x);
            }
        } else {
            for x in [0, 1, w - 2, w - 1] {
                out[x] = boundary(x);
            }
            for (value, p) in out[2..w - 2].iter_mut().zip(row.windows(5)) {
                *value = ((u16::from(p[0])
                    + 4 * u16::from(p[1])
                    + 6 * u16::from(p[2])
                    + 4 * u16::from(p[3])
                    + u16::from(p[4])
                    + 8)
                    >> 4) as u8;
            }
        }
    }
    for y in 0..h {
        if stop() {
            return None;
        }
        let row = |yy: usize| &s.tmp[yy * w..(yy + 1) * w];
        let a = row(y.saturating_sub(2));
        let b = row(y.saturating_sub(1));
        let c = row(y);
        let d = row((y + 1).min(h - 1));
        let e = row((y + 2).min(h - 1));
        for (((((out, a), b), c), d), e) in s.blurred[y * w..(y + 1) * w]
            .iter_mut()
            .zip(a)
            .zip(b)
            .zip(c)
            .zip(d)
            .zip(e)
        {
            *out = ((u16::from(*a)
                + 4 * u16::from(*b)
                + 6 * u16::from(*c)
                + 4 * u16::from(*d)
                + u16::from(*e)
                + 8)
                >> 4) as u8;
        }
    }
    for y in 1..h - 1 {
        if stop() {
            return None;
        }
        let top = s.blurred[(y - 1) * w..y * w].windows(3);
        let middle = s.blurred[y * w..(y + 1) * w].windows(3);
        let bottom = s.blurred[(y + 1) * w..(y + 2) * w].windows(3);
        let outputs = s.magnitude[y * w + 1..(y + 1) * w - 1]
            .iter_mut()
            .zip(&mut s.state[y * w + 1..(y + 1) * w - 1]);
        for (((t, m), b), (magnitude, direction)) in top.zip(middle).zip(bottom).zip(outputs) {
            let gx = i32::from(t[2]) + 2 * i32::from(m[2]) + i32::from(b[2])
                - i32::from(t[0])
                - 2 * i32::from(m[0])
                - i32::from(b[0]);
            let gy = i32::from(b[0]) + 2 * i32::from(b[1]) + i32::from(b[2])
                - i32::from(t[0])
                - 2 * i32::from(t[1])
                - i32::from(t[2]);
            let (ax, ay) = (gx.abs(), gy.abs());
            *magnitude = (ax + ay) as u16;
            *direction = if ay * 1024 <= ax * 424 {
                0
            } else if ay * 1024 >= ax * 2472 {
                2
            } else if (gx > 0 && gy > 0) || (gx < 0 && gy < 0) {
                1
            } else {
                3
            };
        }
    }
    for y in 1..h - 1 {
        if stop() {
            return None;
        }
        for x in 1..w - 1 {
            let i = y * w + x;
            let (a, b) = match s.state[i] {
                0 => (i - 1, i + 1),
                1 => (i - w + 1, i + w - 1),
                2 => (i - w, i + w),
                _ => (i - w - 1, i + w + 1),
            };
            let m = s.magnitude[i];
            s.state[i] = if m >= LOW && m >= s.magnitude[a] && m >= s.magnitude[b] {
                if m >= HIGH { 2 } else { 1 }
            } else {
                0
            };
        }
    }
    Some(())
}

fn hysteresis(states: &mut [u8], w: usize, h: usize, stop: &impl Fn() -> bool) -> Option<()> {
    let mut stack = Vec::<u32>::new();
    for i in 0..states.len() {
        if i.is_multiple_of(1024) && stop() {
            return None;
        }
        if states[i] != 2 {
            continue;
        }
        states[i] = 255;
        stack.push(i as u32);
        let mut visited = 0usize;
        while let Some(index) = stack.pop() {
            visited += 1;
            if visited.is_multiple_of(1024) && stop() {
                return None;
            }
            neighbors(index as usize, w, h, |next| {
                if states[next] == 1 || states[next] == 2 {
                    states[next] = 255;
                    stack.push(next as u32);
                }
            });
        }
    }
    for pixel in states {
        if *pixel != 255 {
            *pixel = 0;
        }
    }
    Some(())
}

fn neighbors(i: usize, w: usize, h: usize, mut visit: impl FnMut(usize)) {
    let (x, y) = (i % w, i / w);
    for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
        for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
            if nx != x || ny != y {
                visit(ny * w + nx);
            }
        }
    }
}

fn dilate(
    edges: &[u8],
    w: usize,
    h: usize,
    bounds: Rect,
    stop: &impl Fn() -> bool,
) -> Option<Vec<u8>> {
    let kw = (3.5 * w as f64 / bounds.width).round().clamp(1.0, 32.0) as usize;
    let kh = (2.5 * h as f64 / bounds.height).round().clamp(1.0, 32.0) as usize;
    let mut tmp = vec![0u8; edges.len()];
    let mut out = vec![0u8; edges.len()];
    for y in 0..h {
        if stop() {
            return None;
        }
        for x in 0..w {
            if (x.saturating_sub(kw / 2)..=(x + kw - 1 - kw / 2).min(w - 1))
                .any(|xx| edges[y * w + xx] != 0)
            {
                tmp[y * w + x] = 255;
            }
        }
    }
    for y in 0..h {
        if stop() {
            return None;
        }
        for x in 0..w {
            if (y.saturating_sub(kh / 2)..=(y + kh - 1 - kh / 2).min(h - 1))
                .any(|yy| tmp[yy * w + x] != 0)
            {
                out[y * w + x] = 255;
            }
        }
    }
    Some(out)
}

fn components(
    mut pixels: Vec<u8>,
    w: usize,
    h: usize,
    stop: &impl Fn() -> bool,
) -> Option<Vec<Component>> {
    let mut result = Vec::new();
    let mut stack = Vec::<u32>::new();
    for i in 0..pixels.len() {
        if i.is_multiple_of(1024) && stop() {
            return None;
        }
        if pixels[i] == 0 {
            continue;
        }
        // Bound hierarchy cost on noisy photographs. Refuse an incomplete
        // hierarchy rather than emit children of a discarded parent.
        if result.len() == MAX_COMPONENTS {
            return Some(Vec::new());
        }
        let mut c = Component {
            left: i % w,
            right: i % w,
            top: i / w,
            bottom: i / w,
            parent: None,
        };
        pixels[i] = 0;
        stack.push(i as u32);
        let mut visited = 0usize;
        while let Some(index) = stack.pop() {
            visited += 1;
            if visited.is_multiple_of(1024) && stop() {
                return None;
            }
            let index = index as usize;
            let (x, y) = (index % w, index / w);
            c.left = c.left.min(x);
            c.right = c.right.max(x);
            c.top = c.top.min(y);
            c.bottom = c.bottom.max(y);
            neighbors(index, w, h, |next| {
                if pixels[next] != 0 {
                    pixels[next] = 0;
                    stack.push(next as u32);
                }
            });
        }
        result.push(c);
    }
    Some(result)
}

fn hierarchy(components: &mut [Component], stop: &impl Fn() -> bool) -> bool {
    // Sorting pays off on dense/photo-heavy frames; keep the cheaper original
    // scan for ordinary UI frames (measured with the local screenshot corpus).
    if components.len() <= 2048 {
        return exhaustive_hierarchy(components, stop);
    }
    // Smallest containing area wins. Equal-area ties keep original raster order.
    // Sorting once skips smaller/equal components and lets each search stop at
    // its first parent instead of repeatedly comparing all component pairs.
    let mut by_area: Vec<_> = components
        .iter()
        .enumerate()
        .map(|(i, c)| (c.area(), i))
        .collect();
    by_area.sort_unstable();
    for i in 0..components.len() {
        if stop() {
            return false;
        }
        let c = components[i];
        let start = by_area.partition_point(|&(area, _)| area <= c.area());
        let mut parent = None;
        for (n, &(_, j)) in by_area[start..].iter().enumerate() {
            if n.is_multiple_of(1024) && stop() {
                return false;
            }
            let p = components[j];
            if p.left <= c.left + 1
                && p.right + 1 >= c.right
                && p.top <= c.top + 1
                && p.bottom + 1 >= c.bottom
            {
                parent = Some(j);
                break;
            }
        }
        components[i].parent = parent;
    }
    true
}

fn exhaustive_hierarchy(components: &mut [Component], stop: &impl Fn() -> bool) -> bool {
    for i in 0..components.len() {
        if stop() {
            return false;
        }
        let c = components[i];
        let mut parent = None;
        let mut area = usize::MAX;
        for (j, p) in components.iter().enumerate() {
            if j.is_multiple_of(1024) && stop() {
                return false;
            }
            if p.area() > c.area()
                && p.area() < area
                && p.left <= c.left + 1
                && p.right + 1 >= c.right
                && p.top <= c.top + 1
                && p.bottom + 1 >= c.bottom
            {
                area = p.area();
                parent = Some(j);
            }
        }
        components[i].parent = parent;
    }
    true
}

fn filter(
    components: &[Component],
    w: usize,
    h: usize,
    bounds: Rect,
    limit: usize,
    stop: &impl Fn() -> bool,
) -> Vec<UiTarget> {
    let rects: Vec<_> = components.iter().map(|c| c.rect(w, h, bounds)).collect();
    let mut filtered: Vec<_> = rects
        .iter()
        .map(|r| r.width <= 7.0 || r.height <= 3.0 || r.width >= 650.0 || r.height >= 160.0)
        .collect();
    // Parent before child, without the repeated recursive walk in the Go port.
    let mut order: Vec<_> = (0..components.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(components[i].area()));
    for &i in &order {
        if stop() {
            return Vec::new();
        }
        let Some(p) = components[i].parent else {
            continue;
        };
        if filtered[i] || filtered[p] {
            continue;
        }
        let (c, p) = (rects[i], rects[p]);
        if c.height <= 6.0
            || ((c.x + c.width / 2.0 - p.x - p.width / 2.0).abs() < 8.0
                && (c.y + c.height / 2.0 - p.y - p.height / 2.0).abs() < 8.0)
            || ((p.height - p.width).abs() < 5.0 && p.height < 40.0 && p.width < 40.0)
        {
            filtered[i] = true;
        }
    }
    for (i, c) in components.iter().enumerate() {
        if stop() {
            return Vec::new();
        }
        if !filtered[i]
            && rects[i].height < 50.0
            && rects[i].height > 6.0
            && let Some(p) = c.parent
            && rects[p].height >= 50.0
        {
            filtered[p] = true;
        }
    }
    if stop() {
        return Vec::new();
    }
    rects
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !filtered[*i])
        .take(limit)
        .map(|(_, rect)| UiTarget {
            rect,
            name: String::new(),
            role: SemanticRole::Control,
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests;
