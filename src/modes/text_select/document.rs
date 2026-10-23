use crate::api::{Rect, UiTarget};
use std::{ops::Range, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Level {
    Regions,
    Words,
    Characters,
}
impl Level {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Regions => "Regions",
            Self::Words => "Words",
            Self::Characters => "Characters",
        }
    }
}

#[derive(Clone)]
pub(super) struct Unit {
    pub range: Range<usize>,
    pub bounds: Rect,
}

#[derive(Clone)]
pub(super) struct Frame {
    pub text: Arc<str>,
    pub units: Vec<Unit>,
    /// Original screen positions survive selection and word segmentation.
    positions: Vec<Unit>,
    pub selected: Vec<bool>,
    pub level: Level,
    pub inherited: bool,
    pub anchor: Option<usize>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            text: Arc::from(""),
            units: Vec::new(),
            positions: Vec::new(),
            selected: Vec::new(),
            level: Level::Regions,
            inherited: false,
            anchor: None,
        }
    }
}

impl Frame {
    pub fn from_targets(targets: &[UiTarget]) -> Self {
        let mut lines: Vec<_> = targets
            .iter()
            .filter(|t| {
                !(t.name.trim().is_empty() && t.ocr_text().trim().is_empty()) && !t.rect.is_empty()
            })
            .collect();
        lines.sort_by(|a, b| {
            a.rect
                .y
                .total_cmp(&b.rect.y)
                .then(a.rect.x.total_cmp(&b.rect.x))
        });
        // Join aligned nearby lines, keeping distinct columns separate. This is
        // layout grouping of OCR evidence, not a second recognition pass.
        let mut groups: Vec<(Rect, Vec<&UiTarget>)> = Vec::new();
        for line in lines {
            let candidate = groups
                .iter()
                .enumerate()
                .filter_map(|(i, (_, items))| {
                    let last = items.last()?;
                    let gap = line.rect.y - last.rect.bottom();
                    let overlap =
                        line.rect.right().min(last.rect.right()) - line.rect.x.max(last.rect.x);
                    (gap >= -last.rect.height * 0.25
                        && gap <= last.rect.height.max(line.rect.height) * 0.9
                        && overlap > last.rect.width.min(line.rect.width) * 0.5
                        && (line.rect.x - last.rect.x).abs()
                            < last.rect.height.max(line.rect.height) * 2.0)
                        .then_some((i, gap.abs()))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            if let Some(index) = candidate {
                groups[index].0 = groups[index].0.union(&line.rect);
                groups[index].1.push(line);
            } else {
                groups.push((line.rect, vec![line]));
            }
        }
        groups.sort_by(|a, b| a.0.y.total_cmp(&b.0.y).then(a.0.x.total_cmp(&b.0.x)));
        let mut text = String::new();
        let mut units = Vec::with_capacity(groups.len());
        let mut positions = Vec::new();
        for (bounds, lines) in groups {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            let start = text.len();
            for (i, line) in lines.iter().enumerate() {
                if i != 0 {
                    text.push('\n');
                }
                let source = if line.ocr_text().is_empty() {
                    &line.name
                } else {
                    line.ocr_text()
                };
                let offset = text.len();
                let _ = crate::api::presentation::write_ocr_text(&mut text, source.trim());
                let content = &text[offset..];
                let mut line_positions = projected_graphemes(content, line.rect);
                let mut cursor = 0;
                for span in line.details.iter().flat_map(|details| &details.ocr_spans) {
                    let mut value = String::new();
                    let _ = crate::api::presentation::write_ocr_text(&mut value, span.text.trim());
                    if value.is_empty() || span.rect.is_empty() {
                        continue;
                    }
                    let Some(start) = content[cursor..].find(&value).map(|i| cursor + i) else {
                        continue;
                    };
                    let end = start + value.len();
                    let precise = projected_graphemes(&content[start..end], span.rect);
                    for (unit, native) in line_positions
                        .iter_mut()
                        .filter(|u| u.range.start >= start && u.range.end <= end)
                        .zip(precise)
                    {
                        unit.bounds = native.bounds;
                    }
                    cursor = end;
                }
                positions.extend(line_positions.into_iter().map(|mut unit| {
                    unit.range = unit.range.start + offset..unit.range.end + offset;
                    unit
                }));
            }
            units.push(Unit {
                range: start..text.len(),
                bounds,
            });
        }
        let selected = vec![false; units.len()];
        Self {
            text: text.into(),
            units,
            positions,
            selected,
            ..Self::default()
        }
    }

    pub fn toggle(&mut self, index: usize, range: bool) {
        if index >= self.units.len() {
            return;
        }
        if self.inherited {
            self.selected.fill(false);
            self.inherited = false;
        }
        if range && let Some(anchor) = self.anchor {
            self.selected[anchor.min(index)..=anchor.max(index)].fill(true);
        } else {
            self.selected[index] = !self.selected[index];
            self.anchor = Some(index);
        }
    }

    pub fn selected_text(&self) -> String {
        let mut text = String::new();
        let mut previous: Option<&Unit> = None;
        let mut previous_index = None;
        for (index, unit) in self
            .units
            .iter()
            .enumerate()
            .filter(|(i, _)| self.selected[*i])
        {
            if let Some(last) = previous {
                if previous_index == index.checked_sub(1) {
                    text.push_str(&self.text[last.range.end..unit.range.start]);
                } else if self.level == Level::Regions {
                    text.push_str("\n\n");
                } else if self.level == Level::Words
                    && text.ends_with(|c: char| c.is_ascii_alphanumeric())
                    && self.text[unit.range.clone()]
                        .starts_with(|c: char| c.is_ascii_alphanumeric())
                {
                    text.push(' ');
                }
            }
            text.push_str(&self.text[unit.range.clone()]);
            previous = Some(unit);
            previous_index = Some(index);
        }
        text
    }

    pub fn bounds(&self) -> Option<Rect> {
        self.units
            .iter()
            .enumerate()
            .filter(|(i, _)| self.selected[*i])
            .map(|(_, unit)| unit.bounds)
            .reduce(|a, b| a.union(&b))
    }

    pub fn descend(&self, level: Level) -> Self {
        let text: Arc<str> = self.selected_text().into();
        let mut positions = Vec::new();
        let mut cursor = 0;
        for unit in self
            .units
            .iter()
            .enumerate()
            .filter(|(i, _)| self.selected[*i])
            .map(|(_, u)| u)
        {
            let value = &self.text[unit.range.clone()];
            let Some(offset) = text[cursor..].find(value).map(|i| cursor + i) else {
                continue;
            };
            let start = self
                .positions
                .partition_point(|p| p.range.end <= unit.range.start);
            positions.extend(
                self.positions[start..]
                    .iter()
                    .take_while(|p| p.range.start < unit.range.end)
                    .map(|p| Unit {
                        range: offset + p.range.start - unit.range.start
                            ..offset + p.range.end - unit.range.start,
                        bounds: p.bounds,
                    }),
            );
            cursor = offset + value.len();
        }
        let units = graphemes(&text, 0)
            .into_iter()
            .map(|range| Unit {
                bounds: range_bounds(&positions, &range),
                range,
            })
            .collect::<Vec<_>>();
        let selected = vec![true; units.len()];
        Self {
            text,
            units,
            positions,
            selected,
            level,
            inherited: true,
            anchor: None,
        }
    }

    pub fn apply_words(&mut self, ranges: &[Range<usize>]) {
        let unit = |range: Range<usize>| Unit {
            bounds: range_bounds(&self.positions, &range),
            range,
        };
        let mut units = Vec::new();
        let boundaries: Vec<_> = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain([self.text.len()])
            .collect();
        let mut end = 0;
        for range in ranges {
            if range.start < end
                || range.start >= range.end
                || boundaries.binary_search(&range.start).is_err()
                || boundaries.binary_search(&range.end).is_err()
            {
                continue;
            }
            units.extend(
                fallback(&self.text[end..range.start], end)
                    .into_iter()
                    .map(&unit),
            );
            if !self.text[range.clone()].trim().is_empty() {
                units.push(unit(range.clone()));
            }
            end = range.end;
        }
        units.extend(fallback(&self.text[end..], end).into_iter().map(unit));
        self.selected = vec![true; units.len()];
        self.units = units;
        self.inherited = true;
    }
}

fn range_bounds(positions: &[Unit], range: &Range<usize>) -> Rect {
    let start = positions.partition_point(|p| p.range.end <= range.start);
    positions[start..]
        .iter()
        .take_while(|p| p.range.start < range.end)
        .map(|p| p.bounds)
        .reduce(|a, b| a.union(&b))
        .unwrap_or_default()
}

// OCR word boxes are authoritative. Only sub-word ranges and providers without
// word boxes use a proportional estimate within their original OCR line/run.
fn projected_graphemes(text: &str, bounds: Rect) -> Vec<Unit> {
    let width = |g: &str| if g.is_ascii() { 0.6 } else { 1.0 };
    let total: f64 = text.graphemes(true).map(width).sum();
    let mut x = 0.0;
    text.grapheme_indices(true)
        .filter_map(|(i, g)| {
            let left = x;
            x += width(g);
            (!g.trim().is_empty() && total > 0.0).then(|| Unit {
                range: i..i + g.len(),
                bounds: Rect::new(
                    bounds.x + bounds.width * left / total,
                    bounds.y,
                    bounds.width * (x - left) / total,
                    bounds.height,
                ),
            })
        })
        .collect()
}

fn graphemes(text: &str, offset: usize) -> Vec<Range<usize>> {
    text.grapheme_indices(true)
        .filter(|(_, g)| !g.trim().is_empty())
        .map(|(i, g)| (offset + i)..(offset + i + g.len()))
        .collect()
}

/// System tokenizers omit punctuation. Preserve it as selectable units;
/// unavailable Chinese rules fall back to graphemes, ASCII runs remain words.
fn fallback(text: &str, offset: usize) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut ascii = false;
    for (i, g) in text.grapheme_indices(true) {
        let current = g.chars().all(|c| c.is_ascii_alphanumeric());
        if !g.trim().is_empty() {
            if ascii
                && current
                && let Some(last) = ranges.last_mut()
            {
                last.end = offset + i + g.len();
            } else {
                ranges.push((offset + i)..(offset + i + g.len()));
            }
        }
        ascii = current;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_word_positions_survive_multiline_and_disjoint_selection() {
        let first = Rect::new(10., 10., 50., 20.);
        let second = Rect::new(150., 10., 80., 20.);
        let mut root = Frame::from_targets(&[
            UiTarget::recognized_text(Rect::new(10., 10., 220., 20.), "one two".into())
                .with_ocr_spans(vec![
                    crate::api::OcrTextSpan {
                        text: "one".into(),
                        rect: first,
                    },
                    crate::api::OcrTextSpan {
                        text: "two".into(),
                        rect: second,
                    },
                ]),
            UiTarget::recognized_text(Rect::new(10., 35., 220., 20.), "three".into()),
        ]);
        root.toggle(0, false);
        let mut words = root.descend(Level::Words);
        words.apply_words(&[0..3, 4..7, 8..13]);
        assert_eq!(words.units[0].bounds, first);
        assert_eq!(words.units[1].bounds, second);
        assert_eq!(words.units[2].bounds.y, 35.);
        assert_eq!(words.selected_text(), "one two\nthree");
        words.toggle(0, false);
        words.toggle(2, false);
        let chars = words.descend(Level::Characters);
        assert_eq!(chars.text.as_ref(), "one three");
        assert_eq!(chars.units[0].bounds.x, first.x);
        assert_eq!(chars.units[3].bounds.y, 35.);
    }
    #[test]
    fn selection_preserves_order_whitespace_and_graphemes() {
        let root = Frame::from_targets(&[UiTarget::recognized_text(
            Rect::new(0., 0., 300., 20.),
            "使用 键盘，选择 e\u{301} 👨‍👩‍👧‍👦".into(),
        )]);
        let mut root = root;
        root.toggle(0, false);
        let mut words = root.descend(Level::Words);
        words.apply_words(&[0..6, 6..12, 15..21]);
        assert_eq!(words.selected_text(), root.selected_text());
        let chars = words.descend(Level::Characters);
        assert!(
            chars
                .units
                .iter()
                .any(|u| &chars.text[u.range.clone()] == "👨‍👩‍👧‍👦")
        );
        assert!(
            chars
                .units
                .iter()
                .any(|u| &chars.text[u.range.clone()] == "e\u{301}")
        );
        words.toggle(2, false);
        words.toggle(0, false);
        assert_eq!(words.selected_text(), "使用，");
    }
    #[test]
    fn paragraphs_group_lines_but_keep_columns_apart() {
        let frame = Frame::from_targets(&[
            UiTarget::recognized_text(Rect::new(10., 10., 100., 20.), "第一行".into()),
            UiTarget::recognized_text(Rect::new(10., 35., 100., 20.), "第二行".into()),
            UiTarget::recognized_text(Rect::new(200., 10., 100., 20.), "另一列".into()),
        ]);
        assert_eq!(frame.units.len(), 2);
        assert_eq!(&frame.text[frame.units[0].range.clone()], "第一行\n第二行");
    }

    #[test]
    fn disjoint_letters_join_without_adding_word_separators() {
        let mut root = Frame::from_targets(&[UiTarget::recognized_text(
            Rect::new(0., 0., 200., 20.),
            "hello".into(),
        )]);
        root.toggle(0, false);
        let mut chars = root.descend(Level::Characters);
        chars.toggle(0, false);
        chars.toggle(4, false);
        assert_eq!(chars.selected_text(), "ho");
    }
}
