//! Conservative retained-scene damage and paint queries, without native APIs.
use super::spatial_index::SpatialIndex;
use crate::api::{
    Rect,
    overlay::{OverlayLabel, OverlayScene, OverlayShape},
};
use smallvec::SmallVec;
use std::sync::Arc;

pub(crate) fn label_bounds(label: &OverlayLabel) -> Rect {
    // Include antialiasing, strokes and the optical text baseline shift.
    let mut bounds = label.rect;
    // DirectWrite clips left-aligned/editing text to its viewport. Centred and
    // right-aligned text can overflow the estimated Hint container, especially
    // with long codes or a wide configured font. Include a generous glyph
    // envelope instead of leaving those pixels outside the damage rectangle.
    if label.style.text_alignment != crate::api::overlay::TextAlignment::Left
        && label.edit.is_none()
        && !label.scroll_to_cursor
    {
        let width = label.text.chars().count() as f64 * label.style.font_size.max(0.0) * 2.0;
        let x = if label.style.text_alignment == crate::api::overlay::TextAlignment::Right {
            label.rect.right() - width
        } else {
            label.rect.center().x - width / 2.0
        };
        bounds = bounds.union(&Rect::new(x, label.rect.y, width, label.rect.height));
    }
    bounds.inset(
        -label
            .style
            .border_width
            .max(0.0)
            .max(label.style.font_size * 0.5)
            - 2.0,
        -label
            .style
            .border_width
            .max(0.0)
            .max(label.style.font_size * 0.5)
            - 2.0,
    )
}

pub(crate) fn shape_bounds(shape: &OverlayShape) -> Rect {
    match shape {
        OverlayShape::Rect {
            rect, stroke_width, ..
        } => rect.inset(-stroke_width.max(0.0) - 2.0, -stroke_width.max(0.0) - 2.0),
        OverlayShape::Line {
            from, to, width, ..
        } => Rect::new(
            from.x.min(to.x),
            from.y.min(to.y),
            (to.x - from.x).abs(),
            (to.y - from.y).abs(),
        )
        .inset(-width.max(0.0) - 2.0, -width.max(0.0) - 2.0),
    }
}

/// Call only after `next.retains_static_content_from(previous)`: a plain
/// previous scene is the entire background, with no old foreground to erase.
pub(crate) fn damage(
    previous: &OverlayScene,
    next: &OverlayScene,
    area: Rect,
) -> SmallVec<[Rect; 8]> {
    let mut regions = SmallVec::new();
    let old_omitted = previous
        .retained
        .as_ref()
        .and_then(|layer| layer.omitted_label);
    let new_omitted = next.retained.as_ref().and_then(|layer| layer.omitted_label);
    if old_omitted != new_omitted {
        for scene in [previous, next] {
            if let Some(retained) = &scene.retained
                && let Some(label) = retained
                    .omitted_label
                    .and_then(|i| retained.scene.labels.get(i))
            {
                add_region(&mut regions, label_bounds(label), area);
            }
        }
    }
    let old_shapes = if previous.retained.is_some() {
        &previous.shapes[..]
    } else {
        &[]
    };
    let old_labels = if previous.retained.is_some() {
        &previous.labels[..]
    } else {
        &[]
    };
    for i in 0..old_shapes.len().max(next.shapes.len()) {
        let old = old_shapes.get(i);
        let new = next.shapes.get(i);
        if old != new {
            for shape in old.into_iter().chain(new) {
                add_region(&mut regions, shape_bounds(shape), area);
            }
        }
    }
    for i in 0..old_labels.len().max(next.labels.len()) {
        let old = old_labels.get(i);
        let new = next.labels.get(i);
        if old != new {
            for label in old.into_iter().chain(new) {
                add_region(&mut regions, label_bounds(label), area);
            }
        }
    }
    regions
}

fn add_region(regions: &mut SmallVec<[Rect; 8]>, rect: Rect, area: Rect) {
    let Some(mut rect) = rect.intersect(&area) else {
        return;
    };
    let mut index = 0;
    while index < regions.len() {
        if regions[index].intersect(&rect).is_some() {
            rect = rect.union(&regions.remove(index));
            index = 0;
        } else {
            index += 1;
        }
    }
    regions.push(rect);
}

pub(crate) struct PaintIndex {
    pub source: Arc<OverlayScene>,
    index: SpatialIndex,
    pub hits: Vec<usize>,
}
impl PaintIndex {
    pub fn new(source: Arc<OverlayScene>) -> Self {
        let mut index = SpatialIndex::new(128.0, 0.0, 0.0);
        for shape in &source.shapes {
            index.store(shape_bounds(shape));
        }
        for label in &source.labels {
            index.store(label_bounds(label));
        }
        Self {
            source,
            index,
            hits: Vec::new(),
        }
    }
    pub fn query(&mut self, region: Rect) {
        self.hits.clear();
        self.index.any_match(region, |i, bounds, query| {
            if bounds.intersect(&query).is_some() {
                self.hits.push(i);
            }
            false
        });
        self.hits.sort_unstable();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::overlay::{LabelStyle, OverlayLabel, RetainedOverlay};
    #[test]
    fn damage_keeps_distant_selection_regions_separate_and_queries_overlap_in_order() {
        let mut base = OverlayScene::new();
        for x in [10.0, 10.0, 900.0] {
            base.labels.push(OverlayLabel::new(
                "a",
                Rect::new(x, 10.0, 20.0, 20.0),
                LabelStyle::default(),
            ));
        }
        let source = Arc::new(base);
        let mut previous = OverlayScene::new();
        previous.retained = Some(RetainedOverlay {
            scene: source.clone(),
            omitted_label: Some(0),
        });
        let mut next = previous.clone();
        next.retained.as_mut().unwrap().omitted_label = Some(2);
        let area = Rect::new(0.0, 0.0, 1000.0, 500.0);
        let regions = damage(&previous, &next, area);
        assert_eq!(regions.len(), 2);
        let mut index = PaintIndex::new(source);
        index.query(regions[0]);
        assert_eq!(index.hits, [0, 1]);
        index.query(regions[1]);
        assert_eq!(index.hits, [2]);
        assert!(next.retains_static_content_from(&previous));
        next.retained.as_mut().unwrap().scene =
            Arc::new(next.retained.as_ref().unwrap().scene.as_ref().clone());
        assert!(!next.retains_static_content_from(&previous));
    }

    #[test]
    fn first_retained_frame_only_damages_the_new_foreground_and_omission() {
        let mut base = OverlayScene::new();
        for x in [10.0, 900.0] {
            base.labels.push(OverlayLabel::new(
                "a",
                Rect::new(x, 10.0, 20.0, 20.0),
                LabelStyle::default(),
            ));
        }
        base.shapes.push(OverlayShape::fill(
            Rect::new(700.0, 300.0, 40.0, 40.0),
            crate::api::Color::rgb(10, 20, 30),
        ));
        let mut next = OverlayScene::new();
        next.retained = Some(RetainedOverlay {
            scene: Arc::new(base.clone()),
            omitted_label: Some(0),
        });
        let area = Rect::new(0.0, 0.0, 1000.0, 500.0);
        assert!(next.retains_static_content_from(&base));
        assert_eq!(
            damage(&base, &next, area).as_slice(),
            [label_bounds(&base.labels[0]).intersect(&area).unwrap()]
        );
        next.retained.as_mut().unwrap().omitted_label = None;
        assert!(damage(&base, &next, area).is_empty());
        next.labels.push(base.labels[1].clone());
        assert_eq!(
            damage(&base, &next, area).as_slice(),
            [label_bounds(&base.labels[1]).intersect(&area).unwrap()]
        );

        let mut changed = base.clone();
        changed.labels[0].text = "changed".into();
        assert!(!next.retains_static_content_from(&changed));
        changed = base.clone();
        changed.shapes.clear();
        assert!(!next.retains_static_content_from(&changed));
        changed = base.clone();
        changed.clip = Some(area);
        assert!(!next.retains_static_content_from(&changed));
        changed = base.clone();
        changed.backdrop = Some(crate::api::Color::rgb(0, 0, 0));
        assert!(!next.retains_static_content_from(&changed));
        next.retained.as_mut().unwrap().scene = Arc::new(next.clone());
        assert!(
            !next.retains_static_content_from(&base),
            "nested layers require full drawing"
        );
        assert!(!base.retains_static_content_from(&base));
    }
}
