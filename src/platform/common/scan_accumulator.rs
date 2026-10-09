//! Shared fusion; each ready input produces an immediate atomic delta.

use smallvec::SmallVec;

use crate::api::command::MAX_UI_SCAN_TARGETS;
use crate::api::{Rect, SemanticRole, UiTarget};

use super::spatial_index::{TargetIndex, TargetSource};

pub(crate) struct ScanAccumulator {
    index: TargetIndex,
    visual_text: Vec<UiTarget>,
    text_spatial: super::spatial_index::SpatialIndex,
    images: Vec<UiTarget>,
    search_targets: Vec<Option<UiTarget>>,
    ocr_scratch: OcrScratch,
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
            visual_text: Vec::new(),
            text_spatial: super::spatial_index::SpatialIndex::new(64.0, 0.0, 0.0),
            images: Vec::new(),
            search_targets: Vec::new(),
            ocr_scratch: OcrScratch::default(),
            search_spatial: super::spatial_index::SpatialIndex::new(64.0, 8.0, 2.0),
        }
    }

    /// Each input batch has one provenance. Semantic owners can retire an
    /// earlier text descendant in the same batch; prune before publication.
    /// The single-writer fusion worker publishes each returned delta atomically.
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
                    // Evidence needs only geometry and normalized text, not a
                    // second copy of the provider's metadata/boxed strings.
                    let mut name = String::with_capacity(t.name.len());
                    let _ = crate::api::presentation::write_ocr_text(&mut name, &t.name);
                    self.text_spatial.store(t.rect);
                    self.visual_text.push(UiTarget {
                        rect: t.rect,
                        role: t.role,
                        name,
                        details: None,
                    });
                }
            }
        }
        if source == TargetSource::Contour {
            for t in &mut targets {
                if t.role == SemanticRole::Image {
                    image_name(
                        t,
                        &self.visual_text,
                        &mut self.ocr_scratch,
                        &mut self.text_spatial,
                    );
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
                image_name(
                    image,
                    &self.visual_text,
                    &mut self.ocr_scratch,
                    &mut self.text_spatial,
                );
                if image.name != previous {
                    retired.push(image.rect);
                    targets.push(image.clone());
                }
            }
        }
        let accepted = targets.len();
        // The asynchronous fusion scheduler owns processing quanta. Never hold
        // ready data for another source or a cumulative count boundary here.
        // Retirements and replacements stay in the same observable transaction.
        let mut batches: SmallVec<[Vec<UiTarget>; 2]> = SmallVec::new();
        if !targets.is_empty() || !retired.is_empty() {
            batches.push(targets);
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
                    if !changed.contains(&index)
                        && let Some(target) = &mut self.search_targets[index]
                        && attach_ocr(
                            target,
                            &self.visual_text,
                            &mut self.ocr_scratch,
                            &mut self.text_spatial,
                        )
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
                attach_ocr(
                    target,
                    &self.visual_text,
                    &mut self.ocr_scratch,
                    &mut self.text_spatial,
                );
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
        // Every push already published its tail. Release scan-only evidence.
        *self = Self::new();
        None
    }
}

#[derive(Default)]
struct OcrScratch {
    text: String,
    // Store indices rather than references so overflow capacity can be reused.
    runs: SmallVec<[usize; 16]>,
}

// Rebuild from bounded, original scan evidence, never from previously joined text.
// Scratch capacity is retained for this scan and released by finish/drop.
fn attach_ocr(
    target: &mut UiTarget,
    text: &[UiTarget],
    scratch: &mut OcrScratch,
    spatial: &mut super::spatial_index::SpatialIndex,
) -> bool {
    let OcrScratch {
        text: scratch,
        runs,
    } = scratch;
    runs.clear();
    // Evidence is append-only for this scan. Query overlapping cells rather
    // than repeatedly walking all previous OCR lines for each small quantum.
    spatial.any_match(target.rect, |index, _, _| {
        let item = &text[index];
        if !item.name.is_empty()
            && target
                .rect
                .intersect(&item.rect)
                .is_some_and(|r| r.width * r.height >= item.rect.width * item.rect.height * 0.9)
        {
            runs.push(index);
        }
        false
    });
    if runs.is_empty() {
        return false;
    }
    runs.sort_unstable_by(|&a, &b| {
        let (a, b) = (&text[a], &text[b]);
        a.rect
            .y
            .total_cmp(&b.rect.y)
            .then(a.rect.x.total_cmp(&b.rect.x))
            .then(a.name.cmp(&b.name))
    });
    // Anchor each row before ordering by x; a fuzzy sort comparator would not
    // be transitive when three rows have different heights.
    let mut start = 0;
    while start < runs.len() {
        let anchor = text[runs[start]].rect;
        let mut end = start + 1;
        while end < runs.len() && same_text_row(anchor, text[runs[end]].rect) {
            end += 1;
        }
        runs[start..end].sort_unstable_by(|&a, &b| {
            let (a, b) = (&text[a], &text[b]);
            a.rect
                .x
                .total_cmp(&b.rect.x)
                .then(a.rect.right().total_cmp(&b.rect.right()))
                .then(a.name.cmp(&b.name))
        });
        start = end;
    }
    scratch.clear();
    let mut previous: Option<&UiTarget> = None;
    for &index in runs.iter() {
        let run = &text[index];
        let value = run.name.trim();
        let overlap = previous.is_some_and(|old| {
            same_text_row(old.rect, run.rect)
                && old
                    .rect
                    .intersect(&run.rect)
                    .is_some_and(|r| r.width >= old.rect.height.min(run.rect.height) * 0.5)
        });
        if overlap
            && (scratch.ends_with(value)
                || previous.is_some_and(|old| {
                    old.rect.x <= run.rect.x
                        && old.rect.right() >= run.rect.right()
                        && old.name.contains(value)
                }))
        {
            continue;
        }
        let skip = if overlap {
            text_overlap(scratch, value)
        } else {
            0
        };
        if !scratch.is_empty() && skip == 0 {
            scratch.push(' ');
        }
        scratch.push_str(&value[skip..]);
        previous = Some(run);
    }
    let details = target.details.get_or_insert_with(|| {
        Box::new(crate::api::geometry::UiTargetDetails {
            accessibility: target.name.clone(),
            ..Default::default()
        })
    });
    let changed = details.ocr != *scratch;
    if changed {
        std::mem::swap(&mut details.ocr, scratch);
    }
    if target.role == SemanticRole::StaticText && details.accessibility.is_empty() {
        target.name.clone_from(&details.ocr);
    }
    changed
}

fn same_text_row(a: Rect, b: Rect) -> bool {
    let small = a.height.min(b.height);
    small > 0.0
        && a.height.max(b.height) <= small * 1.6
        && (a.center().y - b.center().y).abs() <= small * 0.3
}

// Only inspect a bounded seam, on UTF-8 boundaries. Never discard a single
// coincident character; uncertain recognition is preserved rather than guessed.
fn text_overlap(left: &str, right: &str) -> usize {
    const LIMIT: usize = 512;
    let n = left.len().min(right.len()).min(LIMIT);
    if n == 0 {
        return 0;
    }
    // KMP: linear time even for repeated characters. Fixed stack storage,
    // independent of the length of a page or a long OCR paragraph.
    let pattern = &right.as_bytes()[..n];
    let mut failure = [0u16; LIMIT];
    let mut matched = 0;
    for i in 1..n {
        while matched > 0 && pattern[i] != pattern[matched] {
            matched = usize::from(failure[matched - 1]);
        }
        if pattern[i] == pattern[matched] {
            matched += 1;
        }
        failure[i] = matched as u16;
    }
    matched = 0;
    for &byte in &left.as_bytes()[left.len() - n..] {
        while matched > 0 && (matched == n || pattern[matched] != byte) {
            matched = usize::from(failure[matched - 1]);
        }
        if pattern[matched] == byte {
            matched += 1;
        }
    }
    if right.is_char_boundary(matched) && right[..matched].chars().take(2).count() == 2 {
        matched
    } else {
        0
    }
}

fn image_name(
    image: &mut UiTarget,
    text: &[UiTarget],
    scratch: &mut OcrScratch,
    spatial: &mut super::spatial_index::SpatialIndex,
) {
    attach_ocr(image, text, scratch, spatial);
    if let Some(details) = &image.details {
        image.name.clone_from(&details.ocr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{command::remove_retired_targets, geometry::SemanticRole};

    fn evidence_index(text: &[UiTarget]) -> super::super::spatial_index::SpatialIndex {
        let mut index = super::super::spatial_index::SpatialIndex::new(64.0, 0.0, 0.0);
        for item in text {
            assert!(index.store(item.rect));
        }
        index
    }
    #[test]
    #[ignore = "allocation counter requires isolated execution"]
    fn warmed_seam_assembly_reuses_strings_and_fragment_storage() {
        let text: Vec<_> = (0..32)
            .map(|i| {
                UiTarget::recognized_text(
                    Rect::new(f64::from(i) * 140.0, 0.0, 120.0, 20.0),
                    format!("fragment {i}"),
                )
            })
            .collect();
        let mut target =
            UiTarget::recognized_text(Rect::new(0.0, 0.0, 5000.0, 30.0), String::new());
        let mut scratch = OcrScratch::default();
        let mut spatial = evidence_index(&text);
        attach_ocr(&mut target, &text, &mut scratch, &mut spatial);
        attach_ocr(&mut target, &text, &mut scratch, &mut spatial);
        let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
        for _ in 0..1000 {
            assert!(!attach_ocr(&mut target, &text, &mut scratch, &mut spatial));
        }
        let stats = region.change();
        assert_eq!(stats.allocations + stats.reallocations, 0, "{stats:?}");
    }

    #[test]
    fn seam_text_converges_across_batches_and_arrival_orders() {
        for reverse in [false, true] {
            for separate in [false, true] {
                let mut scan = ScanAccumulator::new();
                let mut input = vec![
                    UiTarget::recognized_text(
                        Rect::new(0.0, 0.0, 140.0, 20.0),
                        "支持 搜 索 和 复 制".into(),
                    ),
                    UiTarget::recognized_text(
                        Rect::new(40.0, 0.0, 140.0, 20.0),
                        "搜索和复制结果".into(),
                    ),
                ];
                if reverse {
                    input.reverse();
                }
                let mut visible = Vec::new();
                let batches = if separate {
                    input.into_iter().map(|t| vec![t]).collect()
                } else {
                    vec![input]
                };
                for batch in batches {
                    let update = scan.push(TargetSource::SystemOcr, batch, 0.5);
                    let mut targets: Vec<_> = update.batches.into_iter().flatten().collect();
                    // The mailbox and hint mode also enrich replacements.
                    crate::api::command::enrich_replacements(
                        &mut targets,
                        &visible,
                        &update.retired,
                    );
                    remove_retired_targets(&mut visible, &update.retired);
                    visible.extend(targets);
                }
                visible.extend(scan.finish().into_iter().flatten());
                assert_eq!(visible.len(), 1);
                assert_eq!(visible[0].name, "支持搜索和复制结果");
                assert_eq!(visible[0].ocr_text(), "支持搜索和复制结果");
                assert_eq!(scan.ocr_scratch.text.capacity(), 0);
                assert!(!scan.ocr_scratch.runs.spilled());
                assert_eq!(scan.visual_text.capacity(), 0);
            }
        }
    }

    #[test]
    fn seam_text_requires_geometry_and_preserves_uncertain_content() {
        let mut scratch = OcrScratch::default();
        for (x, y, right, expected) in [
            (80.0, 0.0, "copy results", "please copy results"),
            (140.0, 0.0, "copy results", "please copy copy results"),
            (0.0, 30.0, "copy results", "please copy copy results"),
            (80.0, 0.0, "different OCR", "please copy different OCR"),
        ] {
            let text = vec![
                UiTarget::recognized_text(Rect::new(0.0, 0.0, 120.0, 20.0), "please copy".into()),
                UiTarget::recognized_text(Rect::new(x, y, 120.0, 20.0), right.into()),
            ];
            let mut owner =
                UiTarget::recognized_text(Rect::new(0.0, 0.0, 300.0, 60.0), String::new());
            let mut spatial = evidence_index(&text);
            attach_ocr(&mut owner, &text, &mut scratch, &mut spatial);
            assert_eq!(owner.ocr_text(), expected);
        }
        assert_eq!(text_overlap("重复的字符字", "字结束"), 0);
        assert_eq!(text_overlap("🦀支持搜索", "支持搜索结果"), "支持搜索".len());
        assert_eq!(text_overlap(&"a".repeat(10_000), &"a".repeat(10_000)), 512);
    }

    #[test]
    fn indexed_evidence_matches_full_geometry_filter_for_small_and_oversized_owners() {
        let text: Vec<_> = (0..1024)
            .map(|i| {
                UiTarget::recognized_text(
                    Rect::new(
                        (i % 32 * 40) as f64 - 200.,
                        (i / 32 * 30) as f64 - 200.,
                        30.,
                        20.,
                    ),
                    i.to_string(),
                )
            })
            .collect();
        let mut spatial = evidence_index(&text);
        for rect in [
            Rect::new(-200., -200., 5000., 5000.),
            Rect::new(-200., -200., 30., 20.),
            Rect::new(10., 10., 251., 105.),
            Rect::new(4000., 4000., 30., 20.),
        ] {
            let mut target = UiTarget::recognized_text(rect, String::new());
            let mut scratch = OcrScratch::default();
            attach_ocr(&mut target, &text, &mut scratch, &mut spatial);
            let mut actual = scratch.runs.to_vec();
            actual.sort_unstable();
            let expected: Vec<_> = text
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    rect.intersect(&item.rect)
                        .is_some_and(|r| {
                            r.width * r.height >= item.rect.width * item.rect.height * 0.9
                        })
                        .then_some(i)
                })
                .collect();
            assert_eq!(actual, expected, "{rect:?}");
        }
    }

    #[test]
    fn search_details_keep_ocr_and_accessibility_in_both_arrival_orders() {
        for (ocr_source, ocr_first) in [
            TargetSource::SystemOcr,
            #[cfg(target_os = "macos")]
            TargetSource::NativeVision,
        ]
        .into_iter()
        .flat_map(|source| [false, true].map(|first| (source, first)))
        {
            let mut scan = ScanAccumulator::new();
            let control = UiTarget {
                rect: Rect::new(0.0, 0.0, 100.0, 40.0),
                name: "Copy button".into(),
                role: SemanticRole::Button,
                details: None,
            };
            let ocr = UiTarget::recognized_text(Rect::new(20.0, 10.0, 40.0, 20.0), "复制".into());
            let mut sources = [(TargetSource::Accessibility, control), (ocr_source, ocr)];
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
        assert_eq!(scan.search_targets.capacity(), 0);
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
    fn every_ready_batch_is_immediate_and_replacement_retires_fragments() {
        let mut scan = ScanAccumulator::new();
        let a = target(0.0, 20.0, SemanticRole::Control);
        let b = target(50.0, 20.0, SemanticRole::Control);
        let first = scan.push(TargetSource::Contour, vec![a.clone()], 0.5);
        assert_eq!(first.batches[0][0].rect, a.rect);
        assert_eq!(first.batches[0][0].accessibility_text(), "");
        let second = scan.push(TargetSource::Contour, vec![b.clone()], 0.5);
        assert_eq!(second.batches.len(), 1);
        assert_eq!(second.batches[0].len(), 1);
        assert_eq!(second.batches[0][0].rect, b.rect);
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
