//! Text preview uses UIHint's input, panel positioning and compiled appearance.
use crate::api::overlay::{Color, OverlayLabel};
use crate::api::presentation::TextSelectView;
use crate::api::{HostContext, OverlayScene, OverlayShape, Point, Rect};
use unicode_segmentation::UnicodeSegmentation;

impl TextSelectView<'_> {
    pub(crate) fn scene(self, ctx: &HostContext<'_>) -> OverlayScene {
        let (input, _) = super::hint::input_panel(
            self.hints.content.style.search_input_ui,
            *self.hints.window_bounds,
            ctx,
        );
        let scale = super::label_scale(ctx.scale());
        let styles = self.ui.for_appearance(ctx.palette.appearance);
        let base = &styles.panel;
        let font = base.font_size * scale;
        let line = font * 1.4;
        let padding = (
            base.padding_x.max(10.0) * scale,
            // The compact text preview needs half the multi-field info panel's
            // vertical inset, while still following its configured spacing.
            base.padding_y * 0.5 * scale,
        );
        let preview = if self.text.is_empty() {
            self.status
        } else {
            self.text
        };
        let preliminary = self.ui.place(
            ctx.active_bounds(),
            *self.hints.window_bounds,
            Some(input),
            line,
            scale,
        );
        let width = (preliminary.width - padding.0 * 2.0).max(1.0);
        // Reserve a bounded preview area; only native typography decides where
        // the prose wraps. Estimated character widths must not insert line breaks.
        let preview_height = line * preview_lines(preview, width / font, 4).len() as f64;
        let code_height = font;
        let word_height = font * 1.9;
        let chip_height = code_height + word_height + 8.0 * scale;
        let summary_height = if self.summary.is_empty() {
            0.0
        } else {
            line * 0.8
        };
        let pages_height = if self.page.1 > 1 && !self.chips.is_empty() {
            line + 6.0 * scale
        } else {
            0.0
        };
        let divider_height = if self.chips.is_empty() {
            0.0
        } else {
            16.0 * scale
        };
        let fixed_height =
            preview_height + summary_height + divider_height + pages_height + padding.1 * 2.0;
        let max_rows = ((ctx.active_bounds().height - input.height - fixed_height - 12.0 * scale)
            / chip_height)
            .floor()
            .max(1.0) as usize;
        let cells = chip_layout(
            self.chips,
            width,
            font,
            scale,
            self.grid_min_width,
            chip_height,
            max_rows,
        );
        let grid_height = cells.last().map_or(0.0, |rect| rect.bottom());
        let height = preview_height
            + summary_height
            + divider_height
            + grid_height
            + pages_height
            + padding.1 * 2.0;
        let panel = self.ui.place(
            ctx.active_bounds(),
            *self.hints.window_bounds,
            Some(input),
            height,
            scale,
        );
        let highlight = self.hints.content.style.search_highlight;
        let mut scene = self.hints.scene(ctx);
        for rect in self
            .selected_bounds
            .into_iter()
            .filter(|_| self.selection_ui.border_width > 0.0)
        {
            let [pad_x, pad_y] = self
                .selection_ui
                .padding
                .map(|value| f64::from(value) * scale);
            let rect = Rect::new(
                rect.x - pad_x,
                rect.y - pad_y,
                rect.width + pad_x * 2.0,
                rect.height + pad_y * 2.0,
            );
            scene.push_shape(OverlayShape::Rect {
                rect,
                fill: Color::TRANSPARENT,
                stroke: self
                    .selection_ui
                    .border_color
                    .and_then(|color| color.resolve(ctx.palette.appearance))
                    .unwrap_or(ctx.palette.accent),
                stroke_width: self.selection_ui.border_width * scale,
                corner_radius: 2.0 * scale,
                z_index: 0,
            });
        }
        scene.push_shape(OverlayShape::Rect {
            rect: panel,
            fill: base.background,
            stroke: base.border_color,
            stroke_width: base.border_width * scale,
            corner_radius: base.border_radius * scale,
            z_index: 10_000,
        });
        let x = panel.x + padding.0;
        let mut y = panel.y + padding.1;
        let text_style = self.ui.input_style(ctx.palette.appearance).clone();
        let mut preview_style = (*text_style).clone();
        preview_style.wrap = true;
        // Keep shaping bounded even when the clipboard selection is very large.
        let preview = bounded_preview(preview);
        scene.push_label(
            OverlayLabel::new(
                preview,
                Rect::new(x, y, width, preview_height),
                preview_style,
            )
            .with_fixed_bounds()
            .with_z_index(10_002),
        );
        y += preview_height;
        if !self.summary.is_empty() {
            let mut style = (*text_style).clone();
            style.font_size *= 0.8;
            scene.push_label(
                OverlayLabel::new(self.summary, Rect::new(x, y, width, summary_height), style)
                    .with_fixed_bounds()
                    .with_z_index(10_002),
            );
            y += summary_height;
        }
        if divider_height > 0.0 {
            scene.push_shape(OverlayShape::Line {
                from: Point::new(x, y + divider_height * 0.5),
                to: Point::new(x + width, y + divider_height * 0.5),
                color: base.border_color.with_alpha(100),
                width: scale,
                z_index: 10_001,
                placement_group: None,
            });
            y += divider_height;
        }
        // Each word takes its own width; one long word never widens the whole grid.
        for (chip, cell) in self.chips.iter().zip(cells) {
            let rect = Rect::new(
                x + cell.x,
                y + cell.y,
                cell.width - 4.0 * scale,
                chip_height - 4.0 * scale,
            );
            let mut style = (*text_style).clone();
            style.background = base.background;
            if chip.selected {
                highlight.apply(&mut style, ctx.palette);
            }
            scene.push_shape(OverlayShape::Rect {
                rect,
                fill: style.background,
                stroke: style.border_color,
                stroke_width: style.border_width.max(1.0) * scale,
                corner_radius: 3.0 * scale,
                z_index: 10_001,
            });
            style.padding_x = 0.0;
            style.padding_y = 0.0;
            style.background = Color::TRANSPARENT;
            style.border_width = 0.0;
            let mut code_style = style.clone();
            code_style.font_size *= 0.7;
            scene.push_label(
                OverlayLabel::new(
                    if chip.selected {
                        format!("✓ {}", chip.code.to_uppercase())
                    } else {
                        chip.code.to_uppercase()
                    },
                    Rect::new(
                        rect.x + 4.0 * scale,
                        rect.y + 2.0 * scale,
                        rect.width - 8.0 * scale,
                        code_height,
                    ),
                    code_style,
                )
                .with_fixed_bounds()
                .with_z_index(10_002),
            );
            style.font_size *= 1.35;
            scene.push_label(
                OverlayLabel::new(
                    preview_lines(chip.text, (rect.width - 8.0 * scale) / (font * 1.35), 1)
                        .pop()
                        .unwrap_or_default(),
                    Rect::new(
                        rect.x + 4.0 * scale,
                        rect.y + 2.0 * scale + code_height,
                        rect.width - 8.0 * scale,
                        word_height,
                    ),
                    style,
                )
                .with_fixed_bounds()
                .with_z_index(10_002),
            );
        }
        if pages_height > 0.0 {
            scene.push_label(
                OverlayLabel::new(
                    if self.page_keys.is_empty() {
                        format!("Page {} / {}", self.page.0, self.page.1)
                    } else {
                        format!(
                            "Page {} / {}  ·  {}: next page",
                            self.page.0, self.page.1, self.page_keys
                        )
                    },
                    Rect::new(x, y + grid_height + 6.0 * scale, width, line),
                    text_style,
                )
                .with_fixed_bounds()
                .with_z_index(10_002),
            );
        }
        scene
    }
}

fn bounded_preview(text: &str) -> &str {
    text.grapheme_indices(true)
        .nth(4096)
        .map_or(text, |(end, _)| &text[..end])
}

/// Ordered row packing, bounded to the screen's available height. On short
/// screens only oversized cells are capped; their text is elided when painted.
fn chip_layout(
    chips: &[crate::api::presentation::TextChip<'_>],
    width: f64,
    font: f64,
    scale: f64,
    min_width: f64,
    height: f64,
    max_rows: usize,
) -> Vec<Rect> {
    let advance = |text: &str| {
        text.graphemes(true)
            .map(|g| if g.is_ascii() { 0.75 } else { 1.0 })
            .sum::<f64>()
    };
    let widths: Vec<_> = chips
        .iter()
        .map(|chip| {
            // Reserve the checkmark even for unchecked cells, so toggling is stable.
            ((advance(chip.text) * font * 1.35).max((advance(chip.code) + 1.5) * font * 0.7)
                + 16.0 * scale)
                .max(min_width * scale)
                .min(width)
        })
        .collect();
    let pack = |cap: f64| {
        let mut x = 0.0;
        let mut y = 0.0;
        widths
            .iter()
            .map(|&natural| {
                let cell_width = natural.min(cap);
                if x > 0.0 && x + cell_width > width + 0.001 {
                    x = 0.0;
                    y += height;
                }
                let rect = Rect::new(x, y, cell_width, height);
                x += cell_width;
                rect
            })
            .collect::<Vec<_>>()
    };
    let cells = pack(width);
    if cells
        .last()
        .is_some_and(|rect| rect.bottom() > height * max_rows as f64 + 0.001)
    {
        pack(width / chips.len().div_ceil(max_rows.max(1)).max(1) as f64)
    } else {
        cells
    }
}

/// Bounded preview with real paragraph breaks and intact grapheme clusters.
/// The mode retains the full buffer for copying.
fn preview_lines(text: &str, capacity: f64, max_lines: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut width = 0.0;
    for g in text.graphemes(true) {
        let newline = g.contains('\n') || g == "\r";
        let advance = if g == "\t" {
            3.0
        } else if g.is_ascii() {
            0.75
        } else {
            1.0
        };
        if newline || width + advance > capacity && lines.last().is_some_and(|s| !s.is_empty()) {
            if lines.len() == max_lines {
                if let Some(last) = lines.last_mut() {
                    last.push('…');
                }
                break;
            }
            lines.push(String::new());
            width = 0.0;
        }
        if !newline {
            if let Some(last) = lines.last_mut() {
                last.push_str(g);
            }
            width += advance;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::presentation::TextChip;

    #[test]
    fn word_cards_pack_individually_and_checks_do_not_move_them() {
        let mut chips: Vec<_> = [
            "a",
            "to",
            "of",
            "in",
            "on",
            "is",
            "longerword",
            "a",
            "中",
            "e\u{301}",
        ]
        .into_iter()
        .map(|text| TextChip {
            code: "a",
            text,
            selected: true,
        })
        .collect();
        let layout = chip_layout(&chips, 480., 14., 1., 48., 58., 8);
        assert!(layout.iter().take_while(|cell| cell.y == 0.).count() > 3);
        assert!(layout[6].width > layout[0].width * 2.);
        for cell in &layout {
            assert!(cell.x >= 0. && cell.right() <= 480.001);
        }
        chips[0].selected = false;
        assert_eq!(layout, chip_layout(&chips, 480., 14., 1., 48., 58., 8));
    }

    #[test]
    fn character_density_scales_and_short_screens_keep_every_card() {
        let chips: Vec<_> = (0..24)
            .map(|_| TextChip {
                code: "a",
                text: "中",
                selected: true,
            })
            .collect();
        let columns = |layout: &[Rect]| layout.iter().take_while(|cell| cell.y == 0.).count();
        let compact = chip_layout(&chips, 480., 14., 1., 40., 58., 8);
        assert_eq!(columns(&compact), 12);
        let roomy = chip_layout(&chips, 480., 14., 1., 80., 58., 8);
        assert_eq!(columns(&roomy), 6);
        let scaled = chip_layout(&chips, 960., 28., 2., 40., 116., 8);
        assert_eq!(columns(&scaled), columns(&compact));
        let short = chip_layout(&chips, 480., 14., 1., 240., 58., 2);
        assert_eq!(short.len(), 24);
        assert!(
            short
                .iter()
                .all(|cell| cell.right() <= 480.001 && cell.bottom() <= 116.001)
        );
    }
    #[test]
    fn preview_keeps_paragraph_breaks_and_does_not_split_graphemes() {
        assert_eq!(
            preview_lines("first\n\nsecond", 50., 4),
            ["first", "", "second"]
        );
        assert_eq!(preview_lines("e\u{301}👨‍👩‍👧‍👦a", 1., 2), ["e\u{301}", "👨‍👩‍👧‍👦…"]);
    }
}
