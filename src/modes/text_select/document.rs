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
pub(super) struct Frame {
    pub text: Arc<str>,
    pub units: Vec<Range<usize>>,
    /// Desktop geometry belongs to regions only; refined frames leave this empty.
    pub region_bounds: Vec<Rect>,
    pub selected: Vec<bool>,
    /// Refined units in selection order, independent of their panel positions.
    selection_order: Vec<usize>,
    pub level: Level,
}

pub(super) struct ToggleUndo {
    pub index: usize,
    position: Option<usize>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            text: Arc::from(""),
            units: Vec::new(),
            region_bounds: Vec::new(),
            selected: Vec::new(),
            selection_order: Vec::new(),
            level: Level::Regions,
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
        let mut region_bounds = Vec::with_capacity(groups.len());
        for (bounds, lines) in groups {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            let start = text.len();
            // OCR row boundaries inside this paragraph are soft wraps. Stream
            // them through UIHint's clipboard cleanup so the panel can reflow
            // to its own width without changing preview/copy whitespace rules.
            let content = lines.iter().flat_map(|line| {
                let source = if line.ocr_text().is_empty() {
                    &line.name
                } else {
                    line.ocr_text()
                };
                source.trim().chars().chain(std::iter::once(' '))
            });
            let _ = crate::api::presentation::write_ocr_text(&mut text, content);
            units.push(start..text.len());
            region_bounds.push(bounds);
        }
        let selected = vec![false; units.len()];
        Self {
            text: text.into(),
            units,
            region_bounds,
            selected,
            ..Self::default()
        }
    }

    pub fn toggle(&mut self, index: usize) {
        if index >= self.units.len() {
            return;
        }
        self.selected[index] = !self.selected[index];
        if self.level != Level::Regions {
            if self.selected[index] {
                self.selection_order.push(index);
            } else {
                self.selection_order.retain(|&selected| selected != index);
            }
        }
    }

    pub fn select(&mut self, index: usize) {
        if self.selected.get(index) == Some(&false) {
            self.toggle(index);
        }
    }

    pub fn toggle_with_undo(&mut self, index: usize) -> ToggleUndo {
        let position = self.selection_order.iter().position(|&unit| unit == index);
        self.toggle(index);
        ToggleUndo { index, position }
    }

    /// A following hyphen turns an automatically toggled number into a range.
    /// Restore just that item at its old position, without cloning the frame.
    pub fn undo_toggle(&mut self, undo: ToggleUndo) {
        self.selected[undo.index] = undo.position.is_some();
        self.selection_order.retain(|&index| index != undo.index);
        if let Some(position) = undo.position {
            self.selection_order.insert(position, undo.index);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected.fill(false);
        self.selection_order.clear();
    }

    pub fn select_all(&mut self) {
        if self.level != Level::Regions {
            self.selection_order.extend(
                self.selected
                    .iter()
                    .enumerate()
                    .filter_map(|(index, &selected)| (!selected).then_some(index)),
            );
        }
        self.selected.fill(true);
    }

    pub fn selected_text(&self) -> String {
        let mut text = String::new();
        self.write_selected_text(&mut text);
        text
    }

    pub fn write_selected_text(&self, text: &mut String) {
        text.clear();
        if self.level == Level::Regions {
            self.text_in_order(
                text,
                (0..self.units.len()).filter(|&index| self.selected[index]),
            );
        } else {
            self.text_in_order(text, self.selection_order.iter().copied());
        }
    }

    fn text_in_order(&self, text: &mut String, indices: impl Iterator<Item = usize>) {
        let mut previous: Option<&Range<usize>> = None;
        let mut previous_index = None;
        for index in indices {
            let unit = &self.units[index];
            if let Some(last) = previous {
                if previous_index == index.checked_sub(1) {
                    text.push_str(&self.text[last.end..unit.start]);
                } else if self.level == Level::Regions {
                    text.push_str("\n\n");
                } else if self.level == Level::Words
                    && text.ends_with(|c: char| c.is_ascii_alphanumeric())
                    && self.text[unit.clone()].starts_with(|c: char| c.is_ascii_alphanumeric())
                {
                    text.push(' ');
                }
            }
            text.push_str(&self.text[unit.clone()]);
            previous = Some(unit);
            previous_index = Some(index);
        }
    }

    pub fn selected_bounds(&self) -> Vec<Rect> {
        if self.level != Level::Regions {
            return Vec::new();
        }
        self.region_bounds
            .iter()
            .zip(&self.selected)
            .filter(|(bounds, selected)| **selected && !bounds.is_empty())
            .map(|(bounds, _)| *bounds)
            .collect()
    }

    pub fn descend(&self, level: Level) -> Self {
        let text: Arc<str> = self.selected_text().into();
        let units = graphemes(&text, 0);
        let selected = vec![true; units.len()];
        let selection_order = (0..units.len()).collect();
        Self {
            text,
            units,
            region_bounds: Vec::new(),
            selected,
            selection_order,
            level,
        }
    }

    pub fn apply_words(&mut self, ranges: &[Range<usize>]) {
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
            units.extend(fallback(&self.text[end..range.start], end));
            if !self.text[range.clone()].trim().is_empty() {
                units.push(range.clone());
            }
            end = range.end;
        }
        units.extend(fallback(&self.text[end..], end));
        self.selected = vec![true; units.len()];
        self.selection_order = (0..units.len()).collect();
        self.units = units;
    }
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
    fn panel_refinement_preserves_text_without_desktop_geometry() {
        let first = Rect::new(-100., 20., 380., 20.);
        let last = Rect::new(-100., 45., 380., 20.);
        let mut root = Frame::from_targets(&[
            UiTarget::recognized_text(first, "  iW   iW  ".into()),
            UiTarget::recognized_text(last, "  e\u{301}中  ".into()),
        ]);
        root.toggle(0);
        assert_eq!(root.selected_bounds(), [first.union(&last)]);
        let mut words = root.descend(Level::Words);
        words.apply_words(&[0..2, 3..5, 6..12]);
        assert_eq!(words.selected_text(), "iW iW e\u{301}中");
        assert!(words.region_bounds.is_empty());
        assert!(words.selected_bounds().is_empty());
        words.clear_selection();
        words.toggle(0);
        words.toggle(2);
        let snapshot = words.clone();
        let mut chars = words.descend(Level::Characters);
        assert_eq!(chars.selected_text(), "iW e\u{301}中");
        assert!(chars.region_bounds.is_empty());
        assert!(chars.selected_bounds().is_empty());
        chars.clear_selection();
        chars.toggle(2);
        assert_eq!(chars.selected_text(), "e\u{301}");
        assert_eq!(snapshot.selected_text(), "iW e\u{301}中");
        assert_eq!(root.selected_bounds(), [first.union(&last)]);
    }

    #[test]
    fn refinement_preserves_whitespace_and_combines_graphemes_in_selection_order() {
        let root = Frame::from_targets(&[UiTarget::recognized_text(
            Rect::new(0., 0., 300., 20.),
            "使用 键盘，选择 e\u{301} 👨‍👩‍👧‍👦".into(),
        )]);
        let mut root = root;
        root.toggle(0);
        let mut words = root.descend(Level::Words);
        words.apply_words(&[0..6, 6..12, 15..21]);
        assert_eq!(words.selected_text(), root.selected_text());
        let mut chars = words.descend(Level::Characters);
        assert!(chars.units.iter().any(|u| &chars.text[u.clone()] == "👨‍👩‍👧‍👦"));
        assert!(
            chars
                .units
                .iter()
                .any(|u| &chars.text[u.clone()] == "e\u{301}")
        );
        words.clear_selection();
        words.toggle(2);
        words.toggle(0);
        assert_eq!(words.selected_text(), "，使用");
        chars.clear_selection();
        for text in ["👨‍👩‍👧‍👦", "e\u{301}", "用"] {
            let index = chars
                .units
                .iter()
                .position(|range| &chars.text[range.clone()] == text)
                .unwrap();
            chars.toggle(index);
        }
        assert_eq!(chars.selected_text(), "👨‍👩‍👧‍👦e\u{301}用");
    }
    #[test]
    fn paragraphs_group_lines_but_keep_columns_apart() {
        let frame = Frame::from_targets(&[
            UiTarget::recognized_text(Rect::new(10., 10., 100., 20.), "第一行".into()),
            UiTarget::recognized_text(Rect::new(10., 35., 100., 20.), "第二行".into()),
            UiTarget::recognized_text(Rect::new(200., 10., 100., 20.), "另一列".into()),
        ]);
        assert_eq!(frame.units.len(), 2);
        assert_eq!(&frame.text[frame.units[0].clone()], "第一行第二行");
    }

    #[test]
    fn ocr_rows_share_uihint_cleanup_before_refinement_and_copy() {
        let rows = ["  复\t制 文  ", " 字 ，  测 试 。 ", " English   words  "];
        let mut targets: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(index, text)| {
                UiTarget::recognized_text(
                    Rect::new(10., 10. + index as f64 * 25., 300., 20.),
                    (*text).into(),
                )
            })
            .collect();
        targets.push(UiTarget::recognized_text(
            Rect::new(500., 10., 300., 20.),
            "  另 一 段  ".into(),
        ));
        let mut frame = Frame::from_targets(&targets);
        assert_eq!(frame.units.len(), 2);
        let mut hint_copy = String::new();
        crate::api::presentation::write_ocr_text(&mut hint_copy, rows.join(" ").chars()).unwrap();
        assert_eq!(hint_copy, "复制文字，测试。English words");
        assert_eq!(&frame.text[frame.units[0].clone()], hint_copy);
        frame.select_all();
        let expected = "复制文字，测试。English words\n\n另一段";
        assert_eq!(frame.selected_text(), expected);
        let mut words = frame.descend(Level::Words);
        words.apply_words(&[]);
        assert_eq!(words.selected_text(), expected);
        assert_eq!(words.descend(Level::Characters).selected_text(), expected);
    }

    #[test]
    fn disjoint_letters_join_without_adding_word_separators() {
        let mut root = Frame::from_targets(&[UiTarget::recognized_text(
            Rect::new(0., 0., 200., 20.),
            "hello".into(),
        )]);
        root.toggle(0);
        let mut chars = root.descend(Level::Characters);
        chars.clear_selection();
        chars.toggle(0);
        chars.toggle(4);
        assert_eq!(chars.selected_text(), "ho");
    }
}
