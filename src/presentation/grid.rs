//! Ordinary and recursive grid composition from borrowed semantic views.
use crate::api::overlay::{
    Color, LabelStyle, OverlayLabel, OverlayScene, OverlayShape, OverlayText, SharedLabelStyle,
};
use crate::api::presentation::{BisectView, GridView, RecursiveGridView};
use crate::api::{Palette, Point, Rect};

// Each view decides whether its interior rulings belong in the scene.
fn push_rulings(
    scene: &mut OverlayScene,
    area: Rect,
    rows: usize,
    cols: usize,
    color: Color,
    width: f64,
    z_index: i32,
) {
    let cell_width = area.width / cols as f64;
    let cell_height = area.height / rows as f64;
    let shapes = &mut *scene.shapes;
    for column in 1..cols {
        let x = area.x + column as f64 * cell_width;
        shapes.push(OverlayShape::Line {
            placement_group: None,
            from: Point::new(x, area.top()),
            to: Point::new(x, area.bottom()),
            color,
            width,
            z_index,
        });
    }
    for row in 1..rows {
        let y = area.y + row as f64 * cell_height;
        shapes.push(OverlayShape::Line {
            placement_group: None,
            from: Point::new(area.left(), y),
            to: Point::new(area.right(), y),
            color,
            width,
            z_index,
        });
    }
}

impl GridView<'_> {
    pub(crate) fn style(&self, palette: &Palette) -> LabelStyle {
        self.ui.label.resolve(
            palette,
            palette.surface_cell(),
            palette.text,
            palette.accent_border(),
        )
    }

    fn previews_second_layer(&self) -> bool {
        !self.terminal && self.depth == 0 && self.max_depth > 1
    }

    fn push_second_layer_preview(
        &self,
        scene: &mut OverlayScene,
        area: Rect,
        style: &LabelStyle,
        palette: &Palette,
    ) {
        // Because every depth uses the same layout, the nested rulings align
        // into one fine grid. Draw only the fine lines that are not already an
        // outer boundary, avoiding per-cell duplicate edges and alpha blending.
        let preview_rows = self.layout.rows.saturating_mul(self.layout.rows);
        let preview_cols = self.layout.cols.saturating_mul(self.layout.cols);
        let line_color = crate::api::style::resolve(
            self.ui.matched_border_color.as_ref(),
            palette.appearance,
            style.border_color,
        )
        .with_opacity(0.35);
        let line_width = style.border_width * 0.7;
        if line_width > 0.0 && !line_color.is_transparent() {
            let cell_width = area.width / preview_cols as f64;
            let cell_height = area.height / preview_rows as f64;
            let shapes = &mut *scene.shapes;
            for column in 1..preview_cols {
                if column % self.layout.cols == 0 {
                    continue;
                }
                let x = area.x + column as f64 * cell_width;
                shapes.push(OverlayShape::Line {
                    placement_group: None,
                    from: Point::new(x, area.top()),
                    to: Point::new(x, area.bottom()),
                    color: line_color,
                    width: line_width,
                    z_index: 1,
                });
            }
            for row in 1..preview_rows {
                if row % self.layout.rows == 0 {
                    continue;
                }
                let y = area.y + row as f64 * cell_height;
                shapes.push(OverlayShape::Line {
                    placement_group: None,
                    from: Point::new(area.left(), y),
                    to: Point::new(area.right(), y),
                    color: line_color,
                    width: line_width,
                    z_index: 1,
                });
            }
        }

        let preview_color = style.text_color.with_opacity(0.62);
        let preview_style = LabelStyle {
            background: Color::TRANSPARENT,
            text_color: preview_color,
            matched_text_color: preview_color,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            border_radius: 0.0,
            bold: false,
            ..style.clone()
        };
        let Some(first_outer) = area.subdivision(self.layout.rows, self.layout.cols, 0) else {
            return;
        };
        let Some(first_inner) = first_outer.subdivision(self.layout.rows, self.layout.cols, 0)
        else {
            return;
        };
        // Subdivision preserves cell sizes, and every key is one Unicode scalar.
        // Fit once per layer and share the style across all of its labels.
        let preview_style: SharedLabelStyle = preview_style.fit_to("X", first_inner).into();
        let labels = &mut *scene.labels;
        for outer_index in 0..self.layout.keys.len() {
            let Some(outer) = area.subdivision(self.layout.rows, self.layout.cols, outer_index)
            else {
                break;
            };
            for (inner_index, suffix) in self.layout.keys.iter().copied().enumerate() {
                let Some(rect) = outer.subdivision(self.layout.rows, self.layout.cols, inner_index)
                else {
                    break;
                };
                labels.push(
                    OverlayLabel::new(suffix, rect, preview_style.clone())
                        .fitted()
                        .with_z_index(2),
                );
            }
        }

        // The large prefix sits above the faint suffix grid, matching the
        // visual hierarchy of nested decorations. Its size is
        // relative to the outer cell rather than capped by the ordinary label
        // font, so the first key remains readable at a glance on any display.
        let primary_color = style.matched_text_color.with_opacity(0.92);
        let primary_style: SharedLabelStyle = LabelStyle {
            font_size: style
                .font_size
                .max(first_outer.width.min(first_outer.height) * 0.62),
            text_color: primary_color,
            matched_text_color: primary_color,
            background: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            border_radius: 0.0,
            padding_x: 0.0,
            padding_y: 0.0,
            bold: false,
            ..style.clone()
        }
        .fit_to("X", first_outer)
        .into();
        for (outer_index, prefix) in self.layout.keys.iter().copied().enumerate() {
            let Some(outer) = area.subdivision(self.layout.rows, self.layout.cols, outer_index)
            else {
                break;
            };
            labels.push(
                OverlayLabel::new(prefix, outer, primary_style.clone())
                    .fitted()
                    .with_z_index(3),
            );
        }
    }

    pub(crate) fn scene(&self, palette: &Palette) -> OverlayScene {
        let preview_second_layer = self.previews_second_layer();
        let outer_rulings = self.layout.rows.saturating_sub(1) + self.layout.cols.saturating_sub(1);
        let preview_rulings = if preview_second_layer {
            self.layout
                .rows
                .saturating_mul(self.layout.rows.saturating_sub(1))
                + self
                    .layout
                    .cols
                    .saturating_mul(self.layout.cols.saturating_sub(1))
        } else {
            0
        };
        let shape_capacity = if self.terminal {
            1
        } else {
            1 + outer_rulings + preview_rulings
        };
        let label_capacity = if preview_second_layer {
            self.layout
                .keys
                .len()
                .saturating_mul(self.layout.keys.len())
                .saturating_add(self.layout.keys.len())
        } else {
            usize::from(!self.terminal) * self.layout.keys.len()
        };
        let mut scene = OverlayScene::with_capacity(shape_capacity, label_capacity);
        let style = self.style(palette);

        if self.terminal {
            if let Some(rect) = self.current {
                scene.push_shape(OverlayShape::Rect {
                    rect,
                    fill: palette.highlight().with_opacity(0.55),
                    stroke: palette.accent_border(),
                    stroke_width: style.border_width.max(1.0),
                    corner_radius: 0.0,
                    z_index: 1,
                });
            }
        } else {
            let Some(area) = self.current else {
                return scene;
            };

            // Paint the common cell background once and each ruling once.
            // Drawing a filled/stroked rectangle per cell makes the software
            // Windows backend traverse the whole overlay multiple times and
            // alpha-blend shared edges twice. A grid is the same geometry as
            // one background rectangle plus O(rows + columns) straight lines.
            scene.push_shape(OverlayShape::Rect {
                rect: area,
                fill: style.background.with_opacity(0.55),
                stroke: style.border_color,
                stroke_width: style.border_width,
                corner_radius: 0.0,
                z_index: 0,
            });

            if !(style.border_width <= 0.0 || style.border_color.is_transparent()) {
                push_rulings(
                    &mut scene,
                    area,
                    self.layout.rows,
                    self.layout.cols,
                    style.border_color,
                    style.border_width,
                    1,
                );
            }

            if preview_second_layer {
                self.push_second_layer_preview(&mut scene, area, &style, palette);
            } else if let Some(cell) = area.subdivision(self.layout.rows, self.layout.cols, 0) {
                let text_style: SharedLabelStyle = LabelStyle {
                    background: Color::TRANSPARENT,
                    border_color: Color::TRANSPARENT,
                    border_width: 0.0,
                    border_radius: 0.0,
                    ..style
                }
                .fit_to("X", cell)
                .into();
                let labels = &mut *scene.labels;
                for (index, key) in self.layout.keys.iter().copied().enumerate() {
                    let Some(rect) = area.subdivision(self.layout.rows, self.layout.cols, index)
                    else {
                        break;
                    };
                    labels.push(
                        OverlayLabel::new(key, rect, text_style.clone())
                            .fitted()
                            .with_z_index(2),
                    );
                }
            }
        }

        scene.backdrop = Some(Color::rgba(0, 0, 0, 0x40));
        scene.clip = self.root;
        scene
    }
}

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

        push_recursive_region(
            &mut scene,
            area,
            highlight,
            line_color,
            line_width,
            self.terminal,
        );
        if self.terminal {
            return scene;
        }
        push_rulings(
            &mut scene,
            area,
            layout.rows,
            layout.cols,
            line_color,
            line_width,
            1,
        );

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

/// The label-free Bisect view shares the recursive grid's region styling.
impl BisectView<'_> {
    pub(crate) fn scene(&self, palette: &Palette) -> OverlayScene {
        let Some(area) = self.current else {
            return OverlayScene::new();
        };
        let line = crate::api::style::resolve(
            self.ui.line_color.as_ref(),
            palette.appearance,
            palette.accent,
        );
        let highlight = crate::api::style::resolve(
            self.ui.highlight_color.as_ref(),
            palette.appearance,
            palette.highlight(),
        );
        let width = self.ui.line_width.max(0) as f64;
        let mut scene = OverlayScene::with_capacity(3, 0);
        scene.clip = self.root;
        push_recursive_region(&mut scene, area, highlight, line, width, self.terminal);
        if !self.terminal {
            push_rulings(
                &mut scene,
                area,
                if self.axes.1 { 2 } else { 1 },
                if self.axes.0 { 2 } else { 1 },
                line,
                width,
                1,
            );
        }
        scene
    }
}

fn push_recursive_region(
    scene: &mut OverlayScene,
    area: Rect,
    highlight: Color,
    line: Color,
    width: f64,
    terminal: bool,
) {
    scene.push_shape(OverlayShape::Rect {
        rect: area,
        fill: if terminal {
            highlight.with_opacity(0.55)
        } else {
            highlight
        },
        stroke: line,
        stroke_width: width.max(1.0),
        corner_radius: 0.0,
        z_index: 0,
    });
    if terminal {
        scene.backdrop = Some(Color::rgba(0, 0, 0, 0x40));
    }
}
