//! Text preview uses UIHint's input, panel positioning and compiled appearance.
use crate::api::overlay::{Color, OverlayLabel};
use crate::api::presentation::TextSelectView;
use crate::api::{HostContext, OverlayScene, OverlayShape, Rect};
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
        let line = font * 1.8;
        let padding = (
            base.padding_x.max(10.0) * scale,
            base.padding_y.max(8.0) * scale,
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
        let lines = preview_lines(preview, width / font, 4);
        let columns = ((width / (font * 3.8)) as usize).clamp(1, 8);
        let rows = self.chips.len().div_ceil(columns);
        let chip_height = line * 2.3;
        let summary_height = if self.summary.is_empty() { 0.0 } else { line };
        let pages_height = if self.page.1 > 1 && !self.chips.is_empty() {
            line
        } else {
            0.0
        };
        let height = line * lines.len() as f64
            + summary_height
            + rows as f64 * chip_height
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
        for rect in self.selected_bounds {
            scene.push_shape(OverlayShape::Rect {
                rect,
                fill: Color::TRANSPARENT,
                stroke: ctx.palette.accent,
                stroke_width: 1.5 * scale,
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
        for text in lines {
            scene.push_label(
                OverlayLabel::new(text, Rect::new(x, y, width, line), text_style.clone())
                    .with_fixed_bounds()
                    .with_z_index(10_002),
            );
            y += line;
        }
        if !self.summary.is_empty() {
            let mut style = (*text_style).clone();
            style.font_size *= 0.8;
            scene.push_label(
                OverlayLabel::new(self.summary, Rect::new(x, y, width, line), style)
                    .with_fixed_bounds()
                    .with_z_index(10_002),
            );
            y += line;
        }
        // Only the final character level needs enlargement. Region and word
        // labels stay attached to their original screen positions.
        let cell_width = width / columns as f64;
        for (index, chip) in self.chips.iter().enumerate() {
            let rect = Rect::new(
                x + (index % columns) as f64 * cell_width,
                y + (index / columns) as f64 * chip_height,
                cell_width - 4.0 * scale,
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
                    chip.code.to_uppercase(),
                    Rect::new(
                        rect.x + 4.0 * scale,
                        rect.y,
                        rect.width - 8.0 * scale,
                        line * 0.8,
                    ),
                    code_style,
                )
                .with_fixed_bounds()
                .with_z_index(10_002),
            );
            style.font_size *= 1.35;
            scene.push_label(
                OverlayLabel::new(
                    chip.text,
                    Rect::new(
                        rect.x + 4.0 * scale,
                        rect.y + line * 0.8,
                        rect.width - 8.0 * scale,
                        line * 1.3,
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
                    format!("{} / {}", self.page.0, self.page.1),
                    Rect::new(x, y + rows as f64 * chip_height, width, line),
                    text_style,
                )
                .with_fixed_bounds()
                .with_z_index(10_002),
            );
        }
        scene
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
    #[test]
    fn preview_keeps_paragraph_breaks_and_does_not_split_graphemes() {
        assert_eq!(
            preview_lines("first\n\nsecond", 50., 4),
            ["first", "", "second"]
        );
        assert_eq!(preview_lines("e\u{301}👨‍👩‍👧‍👦a", 1., 2), ["e\u{301}", "👨‍👩‍👧‍👦…"]);
    }
}
