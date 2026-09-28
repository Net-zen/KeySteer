#![forbid(unsafe_code)]

//! Cache-friendly bounded spatial index shared by native UI scanners.

use rustc_hash::FxHashMap;

use smallvec::SmallVec;

use crate::api::geometry::{Rect, SemanticRole, UiTarget};

const MAX_CELLS_PER_RECT: i64 = 32;

/// Stores each rectangle once and keeps only compact indices in grid cells.
///
/// The matching policy remains provider-specific: Windows combines IoU,
/// containment and text-baseline proximity, while macOS Vision currently uses
/// IoU only. Queries allocate no temporary candidate list.
pub(crate) struct SpatialIndex {
    cell_size: f64,
    padding: f64,
    minimum_side: f64,
    cells: FxHashMap<(i32, i32), SmallVec<[u32; 4]>>,
    oversize: SmallVec<[u32; 16]>,
    rects: Vec<Rect>,
    marks: Vec<u32>,
    query_generation: u32,
}

impl SpatialIndex {
    pub(crate) fn new(cell_size: f64, padding: f64, minimum_side: f64) -> Self {
        Self {
            cell_size: cell_size.max(1.0),
            padding: padding.max(0.0),
            minimum_side: minimum_side.max(0.0),
            cells: FxHashMap::default(),
            oversize: SmallVec::new(),
            rects: Vec::new(),
            marks: Vec::new(),
            query_generation: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.rects.len()
    }

    /// Insert `rect` unless `matches` considers an indexed rectangle equal.
    pub(crate) fn insert_if_unique(
        &mut self,
        rect: Rect,
        mut matches: impl FnMut(Rect, Rect) -> bool,
    ) -> bool {
        self.insert_with_index(rect, |_, a, b| matches(a, b))
    }

    fn insert_with_index(
        &mut self,
        rect: Rect,
        matches: impl FnMut(usize, Rect, Rect) -> bool,
    ) -> bool {
        if !self.usable(rect) || self.any_match(rect, matches) {
            return false;
        }
        self.store(rect)
    }

    pub(super) fn any_match(
        &mut self,
        rect: Rect,
        mut matches: impl FnMut(usize, Rect, Rect) -> bool,
    ) -> bool {
        let range = self.covered_cells(rect);
        let oversize = Self::oversize_range(range);
        self.next_query_generation();
        let generation = self.query_generation;
        let rects = &self.rects;
        let marks = &mut self.marks;
        let mut inspect = |candidate: u32| {
            let candidate = candidate as usize;
            if marks[candidate] == generation {
                return false;
            }
            marks[candidate] = generation;
            matches(candidate, rects[candidate], rect)
        };
        if oversize {
            (0..rects.len()).any(|index| inspect(index as u32))
        } else {
            self.oversize.iter().copied().any(&mut inspect)
                || (range.1..=range.3).any(|y| {
                    (range.0..=range.2).any(|x| {
                        self.cells
                            .get(&(x, y))
                            .is_some_and(|entries| entries.iter().copied().any(&mut inspect))
                    })
                })
        }
    }

    fn oversize_range(range: (i32, i32, i32, i32)) -> bool {
        // Widen first: finite native coordinates may saturate opposite i32 ends.
        let columns = i64::from(range.2)
            .saturating_sub(i64::from(range.0))
            .saturating_add(1);
        let rows = i64::from(range.3)
            .saturating_sub(i64::from(range.1))
            .saturating_add(1);
        columns.saturating_mul(rows) > MAX_CELLS_PER_RECT
    }

    pub(super) fn store(&mut self, rect: Rect) -> bool {
        let Ok(index) = u32::try_from(self.rects.len()) else {
            return false;
        };
        let range = self.covered_cells(rect);
        self.rects.push(rect);
        self.marks.push(0);
        if Self::oversize_range(range) {
            self.oversize.push(index);
        } else {
            for y in range.1..=range.3 {
                for x in range.0..=range.2 {
                    self.cells.entry((x, y)).or_default().push(index);
                }
            }
        }
        true
    }

    fn usable(&self, rect: Rect) -> bool {
        rect.x.is_finite()
            && rect.y.is_finite()
            && rect.width.is_finite()
            && rect.height.is_finite()
            && rect.width >= self.minimum_side
            && rect.height >= self.minimum_side
    }

    fn covered_cells(&self, rect: Rect) -> (i32, i32, i32, i32) {
        let cell = |value: f64| (value / self.cell_size).floor() as i32;
        (
            cell(rect.x - self.padding),
            cell(rect.y - self.padding),
            cell(rect.right() + self.padding),
            cell(rect.bottom() + self.padding),
        )
    }

    fn next_query_generation(&mut self) {
        self.query_generation = self.query_generation.wrapping_add(1);
        if self.query_generation == 0 {
            self.marks.fill(0);
            self.query_generation = 1;
        }
    }
}

/// Batch provenance stays in the scanner, without enlarging public UiTarget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetSource {
    Accessibility,
    #[cfg(any(target_os = "windows", test))]
    SystemOcr,
    #[cfg(target_os = "macos")]
    NativeVision,
    #[cfg(target_os = "windows")]
    WechatOcr,
    Contour,
}

#[derive(Clone, Copy)]
struct Evidence {
    source: TargetSource,
    role: SemanticRole,
    active: bool,
}

impl Evidence {
    fn priority(self) -> u8 {
        match self.source {
            TargetSource::Accessibility
                if !matches!(
                    self.role,
                    SemanticRole::StaticText
                        | SemanticRole::Image
                        | SemanticRole::Control
                        | SemanticRole::Unknown
                ) =>
            {
                5
            }
            TargetSource::Contour if self.role == SemanticRole::Image => 3,
            TargetSource::Contour => 1,
            _ => 2,
        }
    }
}

/// Incremental source-priority suppression. A late semantic control can retire
/// multiple visual fragments; the public delta preserves asynchronous delivery.
/// This is a deterministic evidence tier, not a calibrated probability score.
pub(crate) struct TargetIndex {
    spatial: SpatialIndex,
    evidence: Vec<Evidence>,
    active: usize,
    retired: Vec<Rect>,
}

impl TargetIndex {
    pub(crate) fn new() -> Self {
        Self {
            spatial: SpatialIndex::new(64.0, 8.0, 2.0),
            evidence: Vec::new(),
            active: 0,
            retired: Vec::new(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.active
    }

    pub(crate) fn take_retired(&mut self) -> Vec<Rect> {
        std::mem::take(&mut self.retired)
    }

    /// Join only genuinely overlapping text runs (typically OCR tile seams).
    /// No proximity grouping, widget rules, or native actions participate.
    pub(crate) fn stitch_text(&mut self, target: &mut UiTarget, source: TargetSource) {
        if source == TargetSource::Accessibility || target.role != SemanticRole::StaticText {
            return;
        }
        // Bounded fixed point: one long tile can bridge multiple earlier runs.
        for _ in 0..4 {
            let old_rect = target.rect;
            let mut rect = old_rect;
            let evidence = &self.evidence;
            self.spatial.any_match(old_rect, |i, a, _| {
                let e = evidence[i];
                if e.active
                    && e.source == source
                    && e.role == SemanticRole::StaticText
                    && overlapping_text_runs(a, old_rect)
                {
                    let x = rect.x.min(a.x);
                    let y = rect.y.min(a.y);
                    rect = Rect::new(
                        x,
                        y,
                        rect.right().max(a.right()) - x,
                        rect.bottom().max(a.bottom()) - y,
                    );
                }
                false
            });
            target.rect = rect;
            if rect == old_rect {
                break;
            }
        }
    }

    pub(crate) fn retain_active(&mut self, targets: &mut Vec<UiTarget>, source: TargetSource) {
        let evidence = &self.evidence;
        targets.retain(|target| {
            self.spatial.any_match(target.rect, |index, old, new| {
                let entry = evidence[index];
                entry.active && entry.source == source && entry.role == target.role && old == new
            })
        });
    }

    pub(crate) fn insert(
        &mut self,
        target: &UiTarget,
        source: TargetSource,
        threshold: f64,
    ) -> bool {
        let rect = target.rect;
        if !self.spatial.usable(rect) {
            return false;
        }
        let candidate = Evidence {
            source,
            role: target.role,
            active: true,
        };
        let evidence = &self.evidence;
        let mut suppressed = SmallVec::<[usize; 8]>::new();
        let blocked = self.spatial.any_match(rect, |index, a, b| {
            let old = evidence[index];
            if !old.active || !evidence_matches(a, old, b, candidate, threshold) {
                return false;
            }
            if old.priority() >= candidate.priority() {
                let expands_text = old.source != TargetSource::Accessibility
                    && source != TargetSource::Accessibility
                    && old.role == SemanticRole::StaticText
                    && candidate.role == SemanticRole::StaticText
                    && a != b
                    && b.x <= a.x
                    && b.y <= a.y
                    && b.right() >= a.right()
                    && b.bottom() >= a.bottom()
                    && overlapping_text_runs(a, b);
                // Different OCR engines may segment one line as words vs a
                // phrase. Pick a stable representative instead of first arrival.
                let preferred_text = old.priority() == candidate.priority()
                    && old.source != TargetSource::Accessibility
                    && source != TargetSource::Accessibility
                    && old.role == SemanticRole::StaticText
                    && candidate.role == SemanticRole::StaticText
                    && (b.width * b.height)
                        .total_cmp(&(a.width * a.height))
                        .then_with(|| a.y.total_cmp(&b.y))
                        .then_with(|| a.x.total_cmp(&b.x))
                        .is_gt();
                if !expands_text && !preferred_text {
                    return true;
                }
            }
            suppressed.push(index);
            false
        });
        if blocked {
            return false;
        }
        // Public retirements identify geometry. Do not withdraw an AX text
        // rectangle if an independent AX action shares those exact bounds.
        // Keeping the description is preferable to deleting that action.
        if source == TargetSource::Accessibility {
            suppressed.retain(|index| {
                let index = *index;
                let old = self.evidence[index];
                if old.source != source || old.role != SemanticRole::StaticText {
                    return true;
                }
                let evidence = &self.evidence;
                !self
                    .spatial
                    .any_match(self.spatial.rects[index], |other, a, b| {
                        let entry = evidence[other];
                        other != index
                            && entry.active
                            && entry.source == source
                            && entry.role != SemanticRole::StaticText
                            && a == b
                    })
            });
        }
        if self.active.saturating_sub(suppressed.len()) >= crate::api::command::MAX_UI_SCAN_TARGETS
        {
            return false;
        }
        if !self.spatial.store(rect) {
            return false;
        }
        for index in suppressed {
            self.evidence[index].active = false;
            self.retired.push(self.spatial.rects[index]);
            self.active -= 1;
        }
        self.evidence.push(candidate);
        self.active += 1;
        // Bound stale grid entries during long, heavily refined scans. No
        // published ID depends on these internal indices.
        if self.evidence.len() > 512 && self.evidence.len() > self.active.saturating_mul(2) {
            let mut spatial = SpatialIndex::new(64.0, 8.0, 2.0);
            let mut index = 0;
            self.evidence.retain(|entry| {
                let rect = self.spatial.rects[index];
                index += 1;
                if entry.active {
                    spatial.store(rect);
                }
                entry.active
            });
            self.spatial = spatial;
        }
        true
    }
}

fn evidence_matches(a: Rect, ae: Evidence, b: Rect, be: Evidence, threshold: f64) -> bool {
    let iw = (a.right().min(b.right()) - a.x.max(b.x)).max(0.0);
    let ih = (a.bottom().min(b.bottom()) - a.y.max(b.y)).max(0.0);
    let intersection = iw * ih;
    let aa = a.width * a.height;
    let ba = b.width * b.height;
    let image = |e: Evidence| e.source == TargetSource::Contour && e.role == SemanticRole::Image;
    if image(ae) || image(be) {
        let (owner, other, other_e) = if image(ae) { (a, b, be) } else { (b, a, ae) };
        // Native actions inside an image remain independent; visual evidence
        // cannot revoke them. This predicate is symmetric in arrival order.
        if other_e.source == TargetSource::Accessibility {
            return false;
        }
        if image(ae) && image(be) {
            return intersection >= 0.85 * (aa + ba - intersection);
        }
        return owner.width * owner.height > other.width * other.height
            && intersection >= 0.9 * other.width * other.height;
    }
    if ae.source == be.source {
        if ae.source == TargetSource::Accessibility {
            // Static text describes its containing control; it
            // is not another action. Keep independently actionable children
            // (checkboxes, links, cells, buttons) even inside a selectable row.
            let description = |role| role == SemanticRole::StaticText;
            let owned = if ae.priority() == 5 && description(be.role) {
                intersection >= ba * 0.9
            } else if be.priority() == 5 && description(ae.role) {
                intersection >= aa * 0.9
            } else {
                false
            };
            if intersection > 0.0 && owned {
                return true;
            }
            // A row and its independent checkbox/button must both survive.
            return ae.role == be.role
                && intersection > 0.0
                && intersection >= threshold.clamp(0.0, 1.0) * (aa + ba - intersection);
        }
        return rectangles_match(a, b, threshold, 8.0);
    }
    if intersection <= 0.0 {
        return false;
    }
    if ae.source != TargetSource::Accessibility
        && be.source != TargetSource::Accessibility
        && ae.role == SemanticRole::StaticText
        && be.role == SemanticRole::StaticText
    {
        // Symmetric coverage: a short OCR word contained in a phrase must
        // match regardless of which provider supplied the phrase first.
        let small_height = a.height.min(b.height);
        return a.height.max(b.height) <= small_height * 1.6
            && (a.center().y - b.center().y).abs() <= small_height * 0.35
            && intersection >= 0.8 * aa.min(ba);
    }
    if (ae.source == TargetSource::Accessibility && ae.priority() < 3)
        || (be.source == TargetSource::Accessibility && be.priority() < 3)
    {
        // A generic accessible container is not evidence that everything
        // inside it shares one click action.
        return intersection >= threshold.clamp(0.0, 1.0) * (aa + ba - intersection);
    }
    let (high, he, low, low_area) = if ae.priority() >= be.priority() {
        (a, ae, b, ba)
    } else {
        (b, be, a, aa)
    };
    // OCR and contour see different stroke extents. A small glyph fragment
    // can protrude beyond a text line without being a separate button. Require
    // its center and almost all of its width inside the text band, with only
    // a font-relative margin (bounded to four screen-coordinate units).
    if he.source != TargetSource::Accessibility
        && he.role == SemanticRole::StaticText
        && (ae.source == TargetSource::Contour || be.source == TargetSource::Contour)
    {
        let margin = (high.height * 0.2).min(4.0);
        let center = low.center();
        let covered_width = (high.right() + margin).min(low.right()) - (high.x - margin).max(low.x);
        // A tight edge envelope can also surround the OCR box (especially a
        // short word). Symmetric coverage handles that direction without
        // promoting an enclosing panel to the same text target.
        if low.height <= high.height * 1.6
            && low.width <= high.width * 1.6
            && intersection >= high.width * high.height * 0.9
            && low_area <= high.width * high.height * 2.0
        {
            return true;
        }
        if low.height <= high.height * 1.6
            && center.x >= high.x - margin
            && center.x <= high.right() + margin
            && center.y >= high.y - margin
            && center.y <= high.bottom() + margin
            && covered_width >= low.width * 0.8
        {
            return true;
        }
    }
    if he.priority() >= 3 {
        // Native actions own their fragments. Generic containers/static text
        // cannot absorb an entire region based on appearance alone.
        return intersection >= 0.8 * low_area
            || intersection >= threshold.clamp(0.0, 1.0) * (aa + ba - intersection);
    }
    // OCR may own jittered glyph contours, but not a nearby disjoint icon or
    // an enclosing panel. Require substantial coverage of the visual target.
    let aligned = (high.center().y - low.center().y).abs() <= high.height.max(low.height) * 0.5;
    aligned && intersection >= 0.6 * low_area
}

fn overlapping_text_runs(a: Rect, b: Rect) -> bool {
    let small = a.height.min(b.height);
    let tall = a.height.max(b.height);
    a.width >= a.height * 2.0
        && b.width >= b.height * 2.0
        && small > 0.0
        && tall <= small * 1.6
        && (a.center().y - b.center().y).abs() <= tall * 0.3
        && a.intersect(&b)
            .is_some_and(|r| r.height >= small * 0.7 && r.width >= small * 0.5)
}

/// Shared visual duplicate predicate used after independent native providers
/// have already validated their rectangles.
pub(crate) fn rectangles_match(a: Rect, b: Rect, iou_threshold: f64, minimum_spacing: f64) -> bool {
    let ac = a.center();
    let bc = b.center();
    let dx = ac.x - bc.x;
    let dy = ac.y - bc.y;
    let spacing = minimum_spacing.max(1.0);
    let same_baseline = dy.abs() <= (a.height.min(b.height) * 0.35).max(2.0);
    let near = same_baseline && dx * dx + dy * dy < spacing * spacing;

    let intersection_width = a.right().min(b.right()) - a.x.max(b.x);
    let intersection_height = a.bottom().min(b.bottom()) - a.y.max(b.y);
    if intersection_width <= 0.0 || intersection_height <= 0.0 {
        return near;
    }
    let intersection = intersection_width * intersection_height;
    let a_area = a.width * a.height;
    let b_area = b.width * b.height;
    let union = a_area + b_area - intersection;
    let iou_match = union > 0.0 && intersection >= iou_threshold.clamp(0.0, 1.0) * union;
    let containment_match = intersection >= 0.8 * a_area.min(b_area).max(1.0);
    iou_match || containment_match || near
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(rect: Rect, role: SemanticRole) -> UiTarget {
        UiTarget {
            details: None,
            rect,
            role,
            name: String::new(),
        }
    }

    #[test]
    fn description_sharing_geometry_with_an_independent_action_is_not_retired() {
        let mut index = TargetIndex::new();
        let rect = Rect::new(10.0, 5.0, 30.0, 20.0);
        index.insert(
            &target(rect, SemanticRole::StaticText),
            TargetSource::Accessibility,
            0.5,
        );
        index.insert(
            &target(rect, SemanticRole::Image),
            TargetSource::Accessibility,
            0.5,
        );
        index.insert(
            &target(Rect::new(0.0, 0.0, 300.0, 32.0), SemanticRole::ListItem),
            TargetSource::Accessibility,
            0.5,
        );
        assert_eq!(index.len(), 3);
        assert!(index.take_retired().is_empty());
    }

    #[test]
    fn semantic_control_owns_description_but_not_independent_actions() {
        let row = target(Rect::new(0.0, 0.0, 400.0, 32.0), SemanticRole::ListItem);
        for owner_first in [false, true] {
            let mut index = TargetIndex::new();
            let text = target(Rect::new(80.0, 5.0, 60.0, 20.0), SemanticRole::StaticText);
            if owner_first {
                assert!(index.insert(&row, TargetSource::Accessibility, 0.5));
            }
            assert_eq!(
                index.insert(&text, TargetSource::Accessibility, 0.5),
                !owner_first
            );
            if !owner_first {
                assert!(index.insert(&row, TargetSource::Accessibility, 0.5));
            }
            assert_eq!(index.len(), 1);
            for role in [
                SemanticRole::Checkbox,
                SemanticRole::Button,
                SemanticRole::Link,
                SemanticRole::Cell,
                SemanticRole::Image,
            ] {
                assert!(index.insert(&target(text.rect, role), TargetSource::Accessibility, 0.5));
            }
        }
    }

    #[test]
    fn text_band_absorbs_protruding_strokes_but_keeps_adjacent_icons_and_rows() {
        let text = target(Rect::new(30.0, 10.0, 100.0, 20.0), SemanticRole::StaticText);
        let fragment = target(Rect::new(45.0, 21.0, 18.0, 20.0), SemanticRole::Control);
        for text_first in [false, true] {
            let mut index = TargetIndex::new();
            if text_first {
                index.insert(&text, TargetSource::SystemOcr, 0.5);
            }
            assert_eq!(
                index.insert(&fragment, TargetSource::Contour, 0.5),
                !text_first
            );
            if !text_first {
                index.insert(&text, TargetSource::SystemOcr, 0.5);
            }
            assert_eq!(index.len(), 1);
            for rect in [
                Rect::new(5.0, 10.0, 20.0, 20.0),
                Rect::new(40.0, 39.0, 20.0, 20.0),
            ] {
                assert!(index.insert(
                    &target(rect, SemanticRole::Control),
                    TargetSource::Contour,
                    0.5
                ));
            }
        }
    }

    #[test]
    fn semantic_row_replaces_jittered_visual_fragments_in_either_arrival_order() {
        let row = target(Rect::new(10.0, 100.0, 900.0, 32.0), SemanticRole::ListItem);
        let fragments = [
            target(Rect::new(20.0, 103.0, 70.0, 18.0), SemanticRole::Control),
            target(Rect::new(600.0, 108.0, 100.0, 20.0), SemanticRole::Control),
        ];
        for row_first in [false, true] {
            let mut index = TargetIndex::new();
            if row_first {
                assert!(index.insert(&row, TargetSource::Accessibility, 0.5));
            }
            for fragment in &fragments {
                assert_eq!(
                    index.insert(fragment, TargetSource::Contour, 0.5),
                    !row_first
                );
            }
            if !row_first {
                assert!(index.insert(&row, TargetSource::Accessibility, 0.5));
            }
            assert_eq!(index.len(), 1);
            assert_eq!(index.take_retired().len(), if row_first { 0 } else { 2 });
            // Same row does not imply same action: an accessible checkbox survives.
            assert!(index.insert(
                &target(Rect::new(14.0, 106.0, 16.0, 16.0), SemanticRole::Checkbox),
                TargetSource::Accessibility,
                0.5
            ));
            assert!(index.insert(
                &target(Rect::new(10.0, 135.0, 900.0, 32.0), SemanticRole::ListItem),
                TargetSource::Accessibility,
                0.5
            ));
        }
    }

    #[test]
    fn text_envelope_is_symmetric_but_does_not_absorb_a_panel_or_native_button() {
        let text = target(Rect::new(12.0, 13.0, 10.0, 14.0), SemanticRole::StaticText);
        let edge = target(Rect::new(10.0, 10.0, 14.0, 20.0), SemanticRole::Control);
        for text_first in [false, true] {
            let mut index = TargetIndex::new();
            if text_first {
                index.insert(&text, TargetSource::SystemOcr, 0.5);
            }
            assert_eq!(index.insert(&edge, TargetSource::Contour, 0.5), !text_first);
            if !text_first {
                assert!(index.insert(&text, TargetSource::SystemOcr, 0.5));
            }
            assert_eq!(index.len(), 1);
            let panel = target(Rect::new(0.0, 0.0, 300.0, 150.0), SemanticRole::Control);
            assert!(index.insert(&panel, TargetSource::Contour, 0.5));
            let mut button = edge.clone();
            button.role = SemanticRole::Button;
            assert!(index.insert(&button, TargetSource::Accessibility, 0.5));
        }
    }

    #[test]
    fn ocr_replaces_contour_fragments_but_does_not_merge_disjoint_neighbors() {
        let mut index = TargetIndex::new();
        let glyph = target(Rect::new(12.0, 12.0, 20.0, 12.0), SemanticRole::Control);
        let text = target(Rect::new(10.0, 8.0, 120.0, 22.0), SemanticRole::Control);
        assert!(index.insert(&glyph, TargetSource::Contour, 0.5));
        assert!(index.insert(&text, TargetSource::SystemOcr, 0.5));
        assert_eq!(index.len(), 1);
        assert_eq!(index.take_retired(), vec![glyph.rect]);
        assert!(index.insert(
            &target(Rect::new(134.0, 10.0, 20.0, 20.0), SemanticRole::Control),
            TargetSource::Contour,
            0.5
        ));
        assert!(index.insert(
            &target(Rect::new(10.0, 34.0, 120.0, 22.0), SemanticRole::Control),
            TargetSource::SystemOcr,
            0.5
        ));
    }

    #[test]
    fn higher_priority_replacement_is_allowed_at_the_active_target_limit() {
        let mut index = TargetIndex::new();
        for i in 0..crate::api::command::MAX_UI_SCAN_TARGETS {
            assert!(index.insert(
                &target(
                    Rect::new((i % 100) as f64 * 80.0, (i / 100) as f64 * 40.0, 20.0, 20.0),
                    SemanticRole::Control
                ),
                TargetSource::Contour,
                0.5
            ));
        }
        assert!(index.insert(
            &target(Rect::new(0.0, 0.0, 24.0, 24.0), SemanticRole::Button),
            TargetSource::Accessibility,
            0.5
        ));
        assert_eq!(index.len(), crate::api::command::MAX_UI_SCAN_TARGETS);
        assert_eq!(index.take_retired().len(), 1);
        assert!(!index.insert(
            &target(Rect::new(-100.0, -100.0, 20.0, 20.0), SemanticRole::Button),
            TargetSource::Accessibility,
            0.5
        ));
    }

    #[test]
    fn grid_deduplication_matches_exhaustive_scan_across_distributions() {
        for spacing in [4.0, 30.0, 70.0, 256.0] {
            let mut index = SpatialIndex::new(64.0, 8.0, 2.0);
            let mut accepted = Vec::<Rect>::new();
            for i in 0..1_000 {
                let cell = i / 2;
                let oversized = cell % 97 == 0;
                let rect = Rect::new(
                    (cell % 29) as f64 * spacing - 800.0,
                    (cell / 29) as f64 * spacing - 300.0,
                    if oversized { 600.0 } else { 64.0 },
                    if oversized { 400.0 } else { 24.0 },
                );
                let unique = !accepted
                    .iter()
                    .any(|&old| rectangles_match(old, rect, 0.5, 8.0));
                assert_eq!(
                    index.insert_if_unique(rect, |a, b| rectangles_match(a, b, 0.5, 8.0)),
                    unique
                );
                if unique {
                    accepted.push(rect);
                }
                assert_eq!(index.len(), accepted.len());
            }
        }
    }

    #[test]
    #[ignore = "run alone with --test-threads=1 to isolate allocation counts"]
    fn scan_index_allocation_profile() {
        for count in [100, 2_000] {
            let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
            let mut index = SpatialIndex::new(64.0, 8.0, 2.0);
            for i in 0..count {
                let rect = Rect::new(
                    (i % 50) as f64 * 70.0 - 800.0,
                    (i / 50) as f64 * 70.0 - 300.0,
                    64.0,
                    24.0,
                );
                assert!(index.insert_if_unique(rect, |a, b| rectangles_match(a, b, 0.5, 8.0)));
            }
            std::hint::black_box(&index);
            println!("scan_index targets={count} {:?}", region.change());
        }
    }

    #[test]
    fn indexes_negative_coordinates_and_oversize_rectangles() {
        let mut index = SpatialIndex::new(64.0, 8.0, 2.0);
        let same = |a: Rect, b: Rect| a.intersect(&b).is_some();
        assert!(index.insert_if_unique(Rect::new(-500.0, -200.0, 400.0, 300.0), same));
        assert!(!index.insert_if_unique(Rect::new(-200.0, -100.0, 8.0, 8.0), same));
        assert!(index.insert_if_unique(Rect::new(100.0, 100.0, 8.0, 8.0), same));
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn queries_without_building_a_candidate_container() {
        let mut index = SpatialIndex::new(64.0, 0.0, 1.0);
        let close = |a: Rect, b: Rect| {
            let ac = a.center();
            let bc = b.center();
            (ac.x - bc.x).abs() < 4.0 && (ac.y - bc.y).abs() < 4.0
        };
        assert!(index.insert_if_unique(Rect::new(10.0, 10.0, 10.0, 10.0), close));
        assert!(!index.insert_if_unique(Rect::new(12.0, 12.0, 10.0, 10.0), close));
        assert!(index.insert_if_unique(Rect::new(80.0, 12.0, 10.0, 10.0), close));
    }

    #[test]
    fn shared_matcher_covers_iou_containment_and_same_baseline_proximity() {
        assert!(rectangles_match(
            Rect::new(0.0, 0.0, 20.0, 20.0),
            Rect::new(2.0, 2.0, 20.0, 20.0),
            0.5,
            8.0,
        ));
        assert!(rectangles_match(
            Rect::new(0.0, 0.0, 30.0, 30.0),
            Rect::new(4.0, 4.0, 5.0, 5.0),
            0.9,
            8.0,
        ));
        assert!(rectangles_match(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(5.0, 1.0, 10.0, 10.0),
            0.9,
            8.0,
        ));
        assert!(!rectangles_match(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(5.0, 20.0, 10.0, 10.0),
            0.9,
            8.0,
        ));
    }

    #[test]
    fn extreme_finite_coordinates_cannot_overflow_cell_accounting() {
        let mut index = SpatialIndex::new(64.0, 8.0, 1.0);
        assert!(index.insert_if_unique(
            Rect::new(-f64::MAX / 4.0, 0.0, f64::MAX / 2.0, 10.0),
            |_, _| false,
        ));
        assert_eq!(index.len(), 1);
    }
}
