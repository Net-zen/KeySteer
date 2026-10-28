//! Pixel geometry shared by native overlay renderers, after DPI layout.
//!
//! macOS passes its panel's backing pixels per point. Windows scenes already
//! use physical pixels, so Windows passes 1.0 regardless of monitor DPI.

use crate::api::geometry::{Point, Rect};
use crate::api::overlay::Indicator;

/// Align bitmap origins and sizes independently: moving a cached text bitmap
/// must not resize it or introduce fractional-pixel compositing translations.
/// Sizes round outward to avoid clipping; positions move at most half a pixel.
/// Call in the native layer's untransformed local coordinate system.
pub(crate) fn pixel_aligned_rect(rect: Rect, pixels_per_unit: f64) -> Rect {
    Rect::new(
        (rect.x * pixels_per_unit).round() / pixels_per_unit,
        (rect.y * pixels_per_unit).round() / pixels_per_unit,
        pixel_extent(rect.width, pixels_per_unit),
        pixel_extent(rect.height, pixels_per_unit),
    )
}

fn pixel_extent(value: f64, pixels_per_unit: f64) -> f64 {
    (value.max(0.0) * pixels_per_unit).ceil() / pixels_per_unit
}

pub(crate) struct IndicatorLayout {
    pub bounds: Rect,
    pub main: Rect,
    pub held: Option<Rect>,
}

/// Both badges share an aligned right edge. `position` remains the requested
/// right/top anchor; no font scaling, native objects or caches live here.
pub(crate) fn indicator_layout(
    indicator: &Indicator,
    position: Point,
    pixels_per_unit: f64,
) -> IndicatorLayout {
    let label_width = |text: &str| {
        pixel_extent(
            Indicator::label_size(text.chars().count(), &indicator.style).0,
            pixels_per_unit,
        )
    };
    let line_height = pixel_extent(
        Indicator::label_size(0, &indicator.style).1,
        pixels_per_unit,
    );
    let main_width = label_width(&indicator.text);
    let held_width = indicator.held_text.as_deref().map(label_width);
    let gap = held_width.map_or(0.0, |_| pixel_extent(4.0, pixels_per_unit));
    let width = main_width.max(held_width.unwrap_or_default());
    let height = line_height + held_width.map_or(0.0, |_| line_height + gap);
    // Sizes are already aligned; only snap the common anchor here.
    let right = (position.x * pixels_per_unit).round() / pixels_per_unit;
    let top = (position.y * pixels_per_unit).round() / pixels_per_unit;
    IndicatorLayout {
        bounds: Rect::new(right - width, top, width, height),
        main: Rect::new(right - main_width, top, main_width, line_height),
        held: held_width
            .map(|width| Rect::new(right - width, top + line_height + gap, width, line_height)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::overlay::LabelStyle;

    fn assert_pixel_aligned(value: f64, scale: f64) {
        let pixels = value * scale;
        assert!(
            (pixels - pixels.round()).abs() < 1e-9,
            "{value} at {scale}x"
        );
    }

    #[test]
    fn moving_text_keeps_pixel_aligned_origins_and_stable_backing_sizes() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let mut previous_size = None;
            for step in 0..32 {
                let rect = Rect::new(
                    -1800.2 + f64::from(step) * 0.1,
                    80.3 - f64::from(step) * 0.1,
                    54.2,
                    15.4,
                );
                let aligned = pixel_aligned_rect(rect, scale);
                for value in [aligned.x, aligned.y, aligned.width, aligned.height] {
                    assert_pixel_aligned(value, scale);
                }
                assert!((rect.x - aligned.x).abs() * scale <= 0.5 + 1e-9);
                assert!((rect.y - aligned.y).abs() * scale <= 0.5 + 1e-9);
                assert!((0.0..1.0).contains(&((aligned.width - rect.width) * scale)));
                assert!((0.0..1.0).contains(&((aligned.height - rect.height) * scale)));
                let size = (aligned.width, aligned.height);
                if let Some(previous_size) = previous_size {
                    assert_eq!(
                        size, previous_size,
                        "moving text must not resize its bitmap"
                    );
                }
                previous_size = Some(size);
            }
        }
    }

    #[test]
    fn badges_keep_aligned_right_edges_for_retina_and_pre_scaled_windows_scenes() {
        let mut indicator = Indicator {
            text: "Normal".into(),
            held_text: None,
            position: Point::new(-900.3, 81.7),
            style: LabelStyle {
                font_size: 11.0,
                padding_x: 4.0,
                padding_y: 2.0,
                ..LabelStyle::default()
            },
        };
        assert_eq!(
            indicator_layout(&indicator, indicator.position, 1.0)
                .bounds
                .width,
            55.0
        );
        assert_eq!(
            indicator_layout(&indicator, indicator.position, 2.0)
                .bounds
                .width,
            54.5
        );
        let style = indicator.style.clone();
        for held in [None, Some("HELD: SHIFT"), Some("A")] {
            indicator.held_text = held.map(str::to_owned);
            // macOS scales backing pixels; Windows scales styles before this
            // helper. Passing Windows DPI again would incorrectly double-scale.
            for (dpi, backing) in [(1.0, 1.0), (1.0, 2.0), (1.25, 1.0), (1.5, 1.0), (2.0, 1.0)] {
                indicator.style = LabelStyle {
                    font_size: (style.font_size * dpi).round(),
                    padding_x: (style.padding_x * dpi).round(),
                    padding_y: (style.padding_y * dpi).round(),
                    ..style.clone()
                };
                let mut previous_size = None;
                for offset in [0.0, 0.1, 0.5, 1.25, 3000.7] {
                    let anchor =
                        Point::new(indicator.position.x + offset, indicator.position.y - offset);
                    let layout = indicator_layout(&indicator, anchor, backing);
                    assert!((layout.bounds.right() - anchor.x).abs() * backing <= 0.5 + 1e-9);
                    assert!((layout.bounds.top() - anchor.y).abs() * backing <= 0.5 + 1e-9);
                    let size = (layout.bounds.width, layout.bounds.height);
                    if let Some(previous_size) = previous_size {
                        assert_eq!(size, previous_size);
                    }
                    previous_size = Some(size);
                    for badge in std::iter::once(layout.main).chain(layout.held) {
                        assert!((badge.right() - layout.bounds.right()).abs() < 1e-9);
                        for value in [badge.x, badge.y, badge.right(), badge.bottom()] {
                            assert_pixel_aligned(value, backing);
                        }
                    }
                    if let Some(held) = layout.held {
                        assert!(held.y >= layout.main.bottom() + 4.0);
                        assert!((held.bottom() - layout.bounds.bottom()).abs() < 1e-9);
                    }
                }
            }
        }
    }
}
