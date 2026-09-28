//! Shared fusion and count-driven publication for Windows UIA and macOS AX.

use smallvec::SmallVec;

use crate::api::command::{MAX_UI_SCAN_TARGETS, remove_retired_targets};
use crate::api::{Rect, SemanticRole, UiTarget};

use super::partial_batcher::PartialBatcher;
use super::spatial_index::{TargetIndex, TargetSource};

pub(crate) struct ScanAccumulator {
    index: TargetIndex,
    batches: PartialBatcher<UiTarget>,
    published_any: bool,
    visual_text: Vec<UiTarget>,
    images: Vec<UiTarget>,
    search_targets: Vec<Option<UiTarget>>,
    search_spatial: super::spatial_index::SpatialIndex,
}

pub(crate) struct ScanUpdate {
    pub(crate) accepted: usize,
    pub(crate) batches: SmallVec<[Vec<UiTarget>; 2]>,
    pub(crate) retired: Vec<Rect>,
}

impl ScanAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            index: TargetIndex::new(),
            batches: PartialBatcher::new(24, MAX_UI_SCAN_TARGETS),
            published_any: false,
            visual_text: Vec::new(),
            images: Vec::new(),
            search_targets: Vec::new(),
            search_spatial: super::spatial_index::SpatialIndex::new(64.0, 8.0, 2.0),
        }
    }

    /// Each input batch has one provenance. Semantic owners can retire an
    /// earlier text descendant in the same batch; prune before publication.
    /// Call and publish the returned delta under the same platform session lock.
    pub(crate) fn push(
        &mut self,
        source: TargetSource,
        mut targets: Vec<UiTarget>,
        threshold: f64,
    ) -> ScanUpdate {
        for target in &mut targets {
            if source != TargetSource::Accessibility && target.details.is_none() {
                target.details = Some(Box::new(crate::api::geometry::UiTargetDetails {
                    ocr: if source != TargetSource::Contour
                        && target.role == SemanticRole::StaticText
                    {
                        target.name.clone()
                    } else {
                        String::new()
                    },
                    ..Default::default()
                }));
            }
        }
        let text_start = self.visual_text.len();
        let has_text = source != TargetSource::Accessibility
            && source != TargetSource::Contour
            && targets.iter().any(|t| t.role == SemanticRole::StaticText);
        if has_text {
            for t in &targets {
                if t.role == SemanticRole::StaticText
                    && !t.name.is_empty()
                    && self.visual_text.len() < MAX_UI_SCAN_TARGETS
                {
                    self.visual_text.push(t.clone());
                }
            }
        }
        if source == TargetSource::Contour {
            for t in &mut targets {
                if t.role == SemanticRole::Image {
                    image_name(t, &self.visual_text);
                }
            }
        }
        targets.retain_mut(|target| {
            self.index.stitch_text(target, source);
            self.index.insert(target, source, threshold)
        });
        debug_assert!(self.index.len() <= MAX_UI_SCAN_TARGETS);
        let mut retired = self.index.take_retired();
        if !retired.is_empty() {
            let descriptions: Vec<_> = targets
                .iter()
                .filter(|t| !t.name.is_empty() && retired.contains(&t.rect))
                .cloned()
                .collect();
            self.index.retain_active(&mut targets, source);
            crate::api::command::enrich_replacements(&mut targets, &descriptions, &retired);
        }
        if !retired.is_empty() {
            self.index
                .retain_active(&mut self.images, TargetSource::Contour);
        }
        if source == TargetSource::Contour {
            self.images.extend(
                targets
                    .iter()
                    .filter(|t| t.role == SemanticRole::Image)
                    .cloned(),
            );
        } else if has_text {
            // A late OCR description updates the existing image in place via
            // an atomic same-geometry replacement. It must not create a second
            // hint or disappear just because the image arrived first.
            for image in &mut self.images {
                if !self.visual_text[text_start..].iter().any(|t| {
                    t.rect
                        .intersect(&image.rect)
                        .is_some_and(|i| i.width * i.height >= 0.9 * t.rect.width * t.rect.height)
                }) {
                    continue;
                }
                let previous = image.name.clone();
                image_name(image, &self.visual_text);
                if image.name != previous {
                    retired.push(image.rect);
                    targets.push(image.clone());
                }
            }
        }
        crate::api::command::enrich_replacements(
            &mut targets,
            self.batches.pending_mut(),
            &retired,
        );
        let accepted = targets.len();
        remove_retired_targets(self.batches.pending_mut(), &retired);
        let mut batches: SmallVec<[Vec<UiTarget>; 2]> = SmallVec::new();
        for target in targets {
            if let Some(batch) = self.batches.push_one(target) {
                batches.push(batch);
            }
        }
        // Never wait for another provider to complete a small first batch.
        // A replacement also flushes its tail so removal and addition travel
        // together, including when the replacement has identical geometry.
        if (!retired.is_empty() || (accepted != 0 && batches.is_empty() && !self.published_any))
            && let Some(batch) = self.batches.flush_pending()
        {
            batches.push(batch);
        }
        self.published_any |= !batches.is_empty();
        if batches.is_empty() && !retired.is_empty() {
            batches.push(Vec::new());
        }
        // A retirement and ALL replacements form one observable transaction.
        // Native adapters attach retirements to the first batch; splitting the
        // replacements lets the engine paint an intermediate incomplete frame.
        if !retired.is_empty() && batches.len() > 1 {
            let mut transaction = Vec::with_capacity(batches.iter().map(Vec::len).sum());
            for batch in batches.drain(..) {
                transaction.extend(batch);
            }
            batches.push(transaction);
        }
        for &rect in &retired {
            self.search_spatial.any_match(rect, |index, old, _| {
                if old == rect {
                    self.search_targets[index] = None;
                }
                false
            });
        }
        // Metadata is a same-geometry delta, so OCR arriving after AX enriches
        // the original semantic target rather than creating an extra label.
        if has_text {
            let mut changed = std::collections::BTreeSet::new();
            for text in &self.visual_text[text_start..] {
                self.search_spatial.any_match(text.rect, |index, _, _| {
                    if let Some(target) = &mut self.search_targets[index]
                        && attach_ocr(target, std::slice::from_ref(text))
                    {
                        changed.insert(index);
                    }
                    false
                });
            }
            for index in changed {
                if let Some(target) = &self.search_targets[index] {
                    retired.push(target.rect);
                    if batches.is_empty() {
                        batches.push(Vec::new());
                    }
                    batches[0].push(target.clone());
                }
            }
        }
        for batch in &mut batches {
            for target in batch {
                attach_ocr(target, &self.visual_text);
                let mut existing = None;
                self.search_spatial
                    .any_match(target.rect, |index, rect, _| {
                        if rect == target.rect
                            && self.search_targets[index]
                                .as_ref()
                                .is_some_and(|old| old.role == target.role)
                        {
                            existing = Some(index);
                            true
                        } else {
                            false
                        }
                    });
                if let Some(index) = existing {
                    self.search_targets[index] = Some(target.clone());
                } else if self.search_targets.len() < MAX_UI_SCAN_TARGETS * 2
                    && self.search_spatial.store(target.rect)
                {
                    self.search_targets.push(Some(target.clone()));
                }
            }
        }
        ScanUpdate {
            accepted,
            batches,
            retired,
        }
    }

    pub(crate) fn finish(&mut self) -> Option<Vec<UiTarget>> {
        let mut pending = self.batches.finish();
        if let Some(targets) = &mut pending {
            for target in targets {
                attach_ocr(target, &self.visual_text);
            }
        }
        // Fusion evidence is no longer needed after terminal publication.
        // Release it even if a native completion token still owns the session.
        *self = Self::new();
        pending
    }
}

fn attach_ocr(target: &mut UiTarget, text: &[UiTarget]) -> bool {
    let mut changed = false;
    for item in text {
        if item.name.is_empty()
            || !target
                .rect
                .intersect(&item.rect)
                .is_some_and(|r| r.width * r.height >= item.rect.width * item.rect.height * 0.9)
        {
            continue;
        }
        let name = target.name.clone();
        let details = target.details.get_or_insert_with(|| {
            Box::new(crate::api::geometry::UiTargetDetails {
                accessibility: name,
                ..Default::default()
            })
        });
        if !details.ocr.contains(&item.name) {
            if !details.ocr.is_empty() {
                details.ocr.push(' ');
            }
            details.ocr.push_str(&item.name);
            changed = true;
        }
    }
    changed
}

fn image_name(image: &mut UiTarget, text: &[UiTarget]) {
    let mut words: Vec<_> = text
        .iter()
        .filter(|t| {
            t.rect
                .intersect(&image.rect)
                .is_some_and(|i| i.width * i.height >= 0.9 * t.rect.width * t.rect.height)
        })
        .collect();
    words.sort_by(|a, b| {
        a.rect
            .y
            .total_cmp(&b.rect.y)
            .then(a.rect.x.total_cmp(&b.rect.x))
            .then(a.name.cmp(&b.name))
    });
    image.name.clear();
    for word in words {
        if !image.name.contains(&word.name) {
            if !image.name.is_empty() {
                image.name.push(' ');
            }
            image.name.push_str(&word.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::geometry::SemanticRole;

    #[test]
    fn search_details_keep_ocr_and_accessibility_in_both_arrival_orders() {
        for ocr_first in [false, true] {
            let mut scan = ScanAccumulator::new();
            let control = UiTarget {
                rect: Rect::new(0.0, 0.0, 100.0, 40.0),
                name: "Copy button".into(),
                role: SemanticRole::Button,
                details: None,
            };
            let ocr = UiTarget {
                rect: Rect::new(20.0, 10.0, 40.0, 20.0),
                name: "复制".into(),
                role: SemanticRole::StaticText,
                details: None,
            };
            let mut sources = [
                (TargetSource::Accessibility, control),
                (TargetSource::SystemOcr, ocr),
            ];
            if ocr_first {
                sources.reverse();
            }
            let mut visible = Vec::new();
            for (source, target) in sources {
                let update = scan.push(source, vec![target], 0.5);
                remove_retired_targets(&mut visible, &update.retired);
                visible.extend(update.batches.into_iter().flatten());
            }
            visible.extend(scan.finish().into_iter().flatten());
            let control = visible
                .iter()
                .find(|t| t.role == SemanticRole::Button)
                .unwrap();
            assert_eq!(control.ocr_text(), "复制");
            assert_eq!(control.accessibility_text(), "Copy button");
            assert_eq!(
                visible
                    .iter()
                    .filter(|t| t.role == SemanticRole::Button)
                    .count(),
                1
            );
            assert_eq!(scan.search_targets.capacity(), 0);
        }
    }

    #[test]
    fn finish_releases_fusion_evidence_and_description_caches() {
        let mut scan = ScanAccumulator::new();
        scan.push(
            TargetSource::SystemOcr,
            vec![UiTarget {
                details: None,
                rect: Rect::new(0., 0., 20., 20.),
                name: "text".into(),
                role: SemanticRole::StaticText,
            }],
            0.5,
        );
        scan.images.reserve(1000);
        scan.finish();
        assert_eq!(scan.index.len(), 0);
        assert_eq!(scan.images.capacity(), 0);
        assert_eq!(scan.visual_text.capacity(), 0);
        assert!(!scan.published_any);
    }

    #[test]
    fn image_ocr_and_native_action_converge_in_every_source_order() {
        let image = UiTarget {
            details: None,
            rect: Rect::new(0., 0., 240., 160.),
            name: String::new(),
            role: SemanticRole::Image,
        };
        let text = UiTarget {
            details: None,
            rect: Rect::new(30., 30., 80., 20.),
            name: "caption".into(),
            role: SemanticRole::StaticText,
        };
        let button = UiTarget {
            details: None,
            rect: Rect::new(150., 90., 30., 25.),
            name: "action".into(),
            role: SemanticRole::Button,
        };
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let groups = [
                (TargetSource::Contour, image.clone()),
                (TargetSource::SystemOcr, text.clone()),
                (TargetSource::Accessibility, button.clone()),
            ];
            let mut scan = ScanAccumulator::new();
            let mut visible = Vec::new();
            for i in order {
                let update = scan.push(groups[i].0, vec![groups[i].1.clone()], 0.5);
                remove_retired_targets(&mut visible, &update.retired);
                visible.extend(update.batches.into_iter().flatten());
            }
            if let Some(tail) = scan.finish() {
                visible.extend(tail);
            }
            assert_eq!(visible.len(), 2, "{order:?}");
            assert!(
                visible
                    .iter()
                    .any(|t| t.rect == image.rect && t.name == "caption"),
                "{order:?}"
            );
            assert!(visible.iter().any(|t| t.rect == button.rect));
        }
    }

    #[test]
    fn large_replacement_is_one_publication() {
        let mut scan = ScanAccumulator::new();
        let boxes: Vec<_> = (0..100)
            .map(|i| UiTarget {
                details: None,
                rect: Rect::new(f64::from(i) * 100., 0., 40., 20.),
                name: String::new(),
                role: SemanticRole::Control,
            })
            .collect();
        scan.push(TargetSource::Contour, boxes.clone(), 0.5);
        let stronger = boxes
            .into_iter()
            .map(|t| UiTarget {
                role: SemanticRole::Button,
                ..t
            })
            .collect();
        let update = scan.push(TargetSource::Accessibility, stronger, 0.5);
        assert_eq!(update.retired.len(), 100);
        assert_eq!(update.batches.len(), 1);
        assert_eq!(update.batches[0].len(), 100);
    }

    fn target(x: f64, width: f64, role: SemanticRole) -> UiTarget {
        UiTarget {
            details: None,
            rect: Rect::new(x, 0.0, width, 20.0),
            name: String::new(),
            role,
        }
    }

    #[test]
    fn overlapping_ocr_tile_runs_join_without_joining_adjacent_controls_or_rows() {
        for reverse in [false, true] {
            let mut scan = ScanAccumulator::new();
            let mut left = target(0.0, 100.0, SemanticRole::StaticText);
            left.name = "left phrase".into();
            let mut right = target(80.0, 100.0, SemanticRole::StaticText);
            right.name = "right phrase".into();
            let input = if reverse {
                vec![right, left]
            } else {
                vec![left, right]
            };
            let update = scan.push(TargetSource::SystemOcr, input, 0.5);
            assert_eq!(scan.index.len(), 1);
            let joined = &update.batches[0][0];
            assert_eq!(joined.rect, Rect::new(0.0, 0.0, 180.0, 20.0));
            assert!(joined.name.contains("left phrase"));
            assert!(joined.name.contains("right phrase"));
            let disjoint = target(190.0, 100.0, SemanticRole::StaticText);
            let mut next_row = target(0.0, 100.0, SemanticRole::StaticText);
            next_row.rect.y = 24.0;
            scan.push(TargetSource::SystemOcr, vec![disjoint, next_row], 0.5);
            assert_eq!(scan.index.len(), 3);
            let button = target(50.0, 20.0, SemanticRole::Button);
            scan.push(TargetSource::Accessibility, vec![button], 0.5);
            assert_eq!(scan.index.len(), 4);
        }
    }

    #[test]
    fn small_first_batch_is_immediate_and_replacement_prunes_pending_fragments() {
        let mut scan = ScanAccumulator::new();
        let a = target(0.0, 20.0, SemanticRole::Control);
        let b = target(50.0, 20.0, SemanticRole::Control);
        let first = scan.push(TargetSource::Contour, vec![a.clone()], 0.5);
        assert_eq!(first.batches[0][0].rect, a.rect);
        assert_eq!(first.batches[0][0].accessibility_text(), "");
        assert!(
            scan.push(TargetSource::Contour, vec![b.clone()], 0.5)
                .batches
                .is_empty()
        );
        let row = target(0.0, 100.0, SemanticRole::ListItem);
        let update = scan.push(TargetSource::Accessibility, vec![row.clone()], 0.5);
        assert_eq!(update.accepted, 1);
        assert_eq!(update.batches.as_slice(), &[vec![row]]);
        assert_eq!(update.retired.len(), 2);
        assert!(update.retired.contains(&a.rect));
        assert!(update.retired.contains(&b.rect));
        assert!(scan.finish().is_none());
    }

    #[test]
    fn description_before_owner_in_one_batch_is_not_published_again() {
        let mut scan = ScanAccumulator::new();
        let text = target(10.0, 20.0, SemanticRole::StaticText);
        let row = target(0.0, 100.0, SemanticRole::ListItem);
        let update = scan.push(TargetSource::Accessibility, vec![text, row.clone()], 0.5);
        assert_eq!(update.accepted, 1);
        assert_eq!(update.batches.as_slice(), &[vec![row]]);
    }

    #[test]
    fn identical_geometry_replacement_keeps_new_target() {
        let mut scan = ScanAccumulator::new();
        let visual = target(0.0, 20.0, SemanticRole::Control);
        scan.push(TargetSource::Contour, vec![visual.clone()], 0.5);
        let button = target(0.0, 20.0, SemanticRole::Button);
        let update = scan.push(TargetSource::Accessibility, vec![button.clone()], 0.5);
        assert_eq!(update.retired, vec![visual.rect]);
        assert_eq!(update.batches.as_slice(), &[vec![button]]);
    }
}
