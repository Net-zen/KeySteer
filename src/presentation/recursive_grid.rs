//! Central recursive grid composition from a borrowed semantic view.
use crate::api::overlay::{
    Color, LabelStyle, OverlayLabel, OverlayScene, OverlayShape, OverlayText, SharedLabelStyle,
};
use crate::api::presentation::RecursiveGridView;
use crate::api::{Palette, Point, Rect};

impl RecursiveGridView<'_> {
    /// A label is hidden once its cell is too small to read it in.
    fn label_fits(&self, cell: Rect, font_size: f64, multiplier: f64) -> bool {
        if multiplier <= 0.0 {
            return true;
        }
        let need = font_size * multiplier;
        cell.width >= need && cell.height >= need
    }

    pub(crate) fn scene(&self, palette: &Palette) -> OverlayScene {
        let Some(area) = self.current else {
            return OverlayScene::new();
        };
        let appearance = palette.appearance;
        let resolve = crate::api::style::resolve;

        let line_color = resolve(self.ui.line_color.as_ref(), appearance, palette.accent);
        let highlight = resolve(
            self.ui.highlight_color.as_ref(),
            appearance,
            palette.highlight(),
        );
        let label_bg = if self.ui.label_background {
            resolve(
                self.ui.label_background_color.as_ref(),
                appearance,
                palette.surface_label(),
            )
        } else {
            Color::TRANSPARENT
        };

        let mut base_style = self.ui.label.resolve(
            palette,
            label_bg,
            palette.text,
            if self.ui.label_background {
                palette.accent_border()
            } else {
                Color::TRANSPARENT
            },
        );
        // Keep auto-sized keys readable without dominating the cell.
        // Positive configured sizes remain an explicit upper bound.
        if self.ui.label.font_size == 0 {
            base_style.font_size = (area.width / self.layout.cols.max(1) as f64)
                .min(area.height / self.layout.rows.max(1) as f64)
                * 0.40;
        }
        base_style.bold = false;
        let line_width = self.ui.line_width.max(0) as f64;
        let layout = self.layout;
        let shape_capacity = if self.terminal {
            1
        } else {
            1 + layout.rows.saturating_sub(1) + layout.cols.saturating_sub(1)
        };
        let label_capacity = if self.terminal {
            0
        } else if self.ui.sub_key_preview && self.can_descend {
            layout.keys.len().saturating_mul(2)
        } else {
            layout.keys.len()
        };
        let mut scene = OverlayScene::with_capacity(shape_capacity, label_capacity);
        // The native overlay remains scoped to the original screen at every depth.
        scene.clip = self.root;

        if self.terminal {
            scene.push_shape(OverlayShape::Rect {
                rect: area,
                fill: highlight.with_opacity(0.55),
                stroke: line_color,
                stroke_width: line_width.max(1.0),
                corner_radius: 0.0,
                z_index: 0,
            });
            scene.backdrop = Some(Color::rgba(0, 0, 0, 0x40));
            return scene;
        }

        // Highlight the narrowed area and outline it.
        scene.push_shape(OverlayShape::Rect {
            rect: area,
            fill: highlight,
            stroke: line_color,
            stroke_width: line_width.max(1.0),
            corner_radius: 0.0,
            z_index: 0,
        });

        // Interior rulings.
        let cell_w = area.width / layout.cols as f64;
        let cell_h = area.height / layout.rows as f64;
        for c in 1..layout.cols {
            let x = area.x + c as f64 * cell_w;
            scene.push_shape(OverlayShape::Line {
                placement_group: None,
                from: Point::new(x, area.top()),
                to: Point::new(x, area.bottom()),
                color: line_color,
                width: line_width,
                z_index: 1,
            });
        }
        for r in 1..layout.rows {
            let y = area.y + r as f64 * cell_h;
            scene.push_shape(OverlayShape::Line {
                placement_group: None,
                from: Point::new(area.left(), y),
                to: Point::new(area.right(), y),
                color: line_color,
                width: line_width,
                z_index: 1,
            });
        }

        // Cell keys, plus an optional preview of the next level's keys.
        let mut next_keys = OverlayText::default();
        if self.ui.sub_key_preview && self.can_descend {
            let replacement = self.ui.label_char.chars().next();
            for &key in self.next_layout.keys {
                next_keys.push(replacement.unwrap_or(key));
            }
        }

        let Some(first_cell) = area.subdivision(layout.rows, layout.cols, 0) else {
            return scene;
        };
        // Every cell has the same size and label length, including a custom
        // multi-character label. Resolve fitting and visibility once per scene.
        let sample_text = if self.ui.label_char.is_empty() {
            "X"
        } else {
            &self.ui.label_char
        };
        let scale = base_style.fit_scale(sample_text, first_cell);
        let fitted_font_size = base_style.font_size * scale;
        let label_style = base_style.scaled(scale);
        let label_style: Option<SharedLabelStyle> = (fitted_font_size
            >= self.ui.label_min_font_size.max(1) as f64
            && self.label_fits(
                first_cell,
                label_style.font_size,
                self.ui.label_autohide_multiplier,
            ))
        .then(|| label_style.into());

        let preview_size = Rect::new(0.0, 0.0, first_cell.width, first_cell.height * 0.3);
        let sub_style: Option<SharedLabelStyle> = if next_keys.is_empty() {
            None
        } else {
            let sub_style = LabelStyle {
                font_size: self.ui.sub_key_preview_font_size.max(1) as f64,
                text_color: resolve(
                    self.ui.sub_key_preview_text_color.as_ref(),
                    appearance,
                    palette.text.with_opacity(0.6),
                ),
                background: Color::TRANSPARENT,
                border_color: Color::TRANSPARENT,
                border_width: 0.0,
                bold: false,
                ..base_style
            };
            let scale = sub_style.fit_scale(&next_keys, preview_size);
            let fitted_font_size = sub_style.font_size * scale;
            let sub_style = sub_style.scaled(scale);
            (fitted_font_size >= 4.0
                && self.label_fits(
                    preview_size,
                    sub_style.font_size,
                    self.ui.sub_key_preview_autohide_multiplier,
                ))
            .then(|| sub_style.into())
        };

        for (index, key) in layout.keys.iter().copied().enumerate() {
            let Some(cell) = area.subdivision(layout.rows, layout.cols, index) else {
                break;
            };
            if let Some(style) = &label_style {
                let text = if self.ui.label_char.is_empty() {
                    OverlayText::from(key)
                } else {
                    OverlayText::from(self.ui.label_char.as_str())
                };
                scene.push_label(
                    OverlayLabel::new(text, cell, style.clone())
                        .fitted()
                        .with_z_index(3),
                );
            }
            if let Some(style) = &sub_style {
                // Sit the preview in the lower part of the cell.
                let preview = Rect::new(
                    cell.x,
                    cell.center().y + cell.height * 0.15,
                    preview_size.width,
                    preview_size.height,
                );
                scene.push_label(
                    OverlayLabel::new(next_keys.clone(), preview, style.clone())
                        .fitted()
                        .with_z_index(2),
                );
            }
        }

        scene
    }
}
