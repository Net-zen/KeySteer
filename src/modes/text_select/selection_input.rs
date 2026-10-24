//! Decimal selections share the prompt with text search, without prefix ambiguity.
use crate::api::{
    hint::{LabelMatch, match_label_input},
    text_edit::Selection,
};
use smallvec::SmallVec;
use std::ops::Range;

pub(super) struct NumberSelection {
    pub first: usize,
    pub last: usize,
    pub range: bool,
}

pub(super) fn numeric_term(term: &str) -> bool {
    !term.is_empty()
        && term
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
}

fn parse(term: &str, count: usize) -> Option<NumberSelection> {
    let number = |part: &str| {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let value = part.parse::<usize>().ok()?;
        (value > 0 && value <= count).then(|| value - 1)
    };
    let (first, last, range) = if let Some((first, last)) = term.split_once('-') {
        (number(first)?, number(last)?, true)
    } else {
        let index = number(term)?;
        (index, index, false)
    };
    Some(NumberSelection { first, last, range })
}

/// Resolve the input against the same candidates the panel displays. A caret
/// edit in the middle must not commit a number still being edited.
pub(super) fn consume_unambiguous<'a>(
    query: &mut String,
    selection: &mut Selection,
    count: usize,
    labels: impl IntoIterator<Item = (&'a str, usize)>,
) -> Option<NumberSelection> {
    if selection.cursor != query.len()
        || selection.anchor != selection.cursor
        || !query.ends_with(|ch: char| ch.is_ascii_digit())
    {
        return None;
    }
    let term = query.split_whitespace().next_back()?;
    let number = parse(term, count)?;
    let end = term.rsplit_once('-').map_or(term, |(_, end)| end);
    if !matches!(match_label_input(labels, end), LabelMatch::Complete(_)) {
        return None;
    }
    query.truncate(query.len() - term.len());
    if query.trim().is_empty() {
        query.clear();
    }
    selection.cursor = query.len();
    selection.anchor = selection.cursor;
    Some(number)
}

/// A separator commits a number/range. Copy/refine can also commit the last
/// complete term when it still has ambiguous numeric continuations.
pub(super) fn consume(
    query: &mut String,
    selection: &mut Selection,
    count: usize,
    finish: bool,
) -> SmallVec<[NumberSelection; 8]> {
    let mut numbers = SmallVec::new();
    let mut removed: SmallVec<[Range<usize>; 8]> = SmallVec::new();
    for term in query.split_whitespace() {
        let Some(span) = query.substr_range(term) else {
            continue;
        };
        let span: Range<usize> = span.into();
        if span.end == query.len() && !finish {
            continue;
        }
        let Some(number) = parse(term, count) else {
            continue;
        };
        let whitespace = query[span.end..]
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
        removed.push(span.start..span.end + whitespace);
        numbers.push(number);
    }
    // Preserve the caret/selection in remaining search text, including Unicode.
    for span in removed.into_iter().rev() {
        for position in [&mut selection.cursor, &mut selection.anchor] {
            *position -= position.saturating_sub(span.start).min(span.len());
        }
        query.replace_range(span, "");
    }
    if query.trim().is_empty() {
        query.clear();
        *selection = Selection::default();
    }
    numbers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ids_without_longer_decimal_continuations_resolve_automatically() {
        for (input, count, resolves) in [
            ("8", 20, true),
            ("9", 20, true),
            ("12", 20, true),
            ("1", 20, false),
            ("2", 20, false),
            ("8", 79, true),
            ("8", 80, false),
            ("10", 100, false),
            ("11", 100, true),
            ("1-2", 20, false),
            ("1-20", 20, true),
            ("8-5", 20, true),
            ("0", 20, false),
            ("21", 20, false),
            ("1-", 20, false),
        ] {
            let labels: Vec<_> = (1..=count).map(|value| value.to_string()).collect();
            let mut query = input.to_owned();
            let mut selection = Selection {
                cursor: query.len(),
                anchor: query.len(),
            };
            assert_eq!(
                consume_unambiguous(
                    &mut query,
                    &mut selection,
                    count,
                    labels
                        .iter()
                        .enumerate()
                        .map(|(index, label)| (label.as_str(), index))
                )
                .is_some(),
                resolves,
                "{input}/{count}"
            );
            assert_eq!(query, if resolves { "" } else { input });
            assert_eq!(selection.cursor, query.len());
        }
        let mut query = usize::MAX.to_string();
        let label = query.clone();
        let mut selection = Selection {
            cursor: query.len(),
            anchor: query.len(),
        };
        assert_eq!(
            consume_unambiguous(
                &mut query,
                &mut selection,
                usize::MAX,
                [(label.as_str(), usize::MAX - 1)]
            )
            .unwrap()
            .first,
            usize::MAX - 1
        );
    }

    #[test]
    fn numbers_wait_for_a_separator_and_ranges_keep_input_direction() {
        let mut query = "12".to_owned();
        let mut selection = Selection {
            cursor: 2,
            anchor: 2,
        };
        assert!(consume(&mut query, &mut selection, 32, false).is_empty());
        assert_eq!(query, "12");
        query.push_str(" 1-3 5-4");
        selection.cursor = query.len();
        selection.anchor = query.len();
        let numbers = consume(&mut query, &mut selection, 32, true);
        assert_eq!(
            numbers
                .iter()
                .map(|n| (n.first, n.last, n.range))
                .collect::<Vec<_>>(),
            [(11, 11, false), (0, 2, true), (4, 3, true)]
        );
        assert!(query.is_empty());
        assert_eq!(selection, Selection::default());
    }

    #[test]
    fn invalid_ranges_are_not_partially_consumed_and_search_caret_is_preserved() {
        let mut query = "0 1-33 2- 1-2-3 99999999999999999999999999999".to_owned();
        let original = query.clone();
        assert!(consume(&mut query, &mut Selection::default(), 32, true).is_empty());
        assert_eq!(query, original);
        let mut query = "2 中文 4-5 tail".to_owned();
        let mut selection = Selection {
            cursor: query.len(),
            anchor: "2 中".len(),
        };
        assert_eq!(consume(&mut query, &mut selection, 32, false).len(), 2);
        assert_eq!(query, "中文 tail");
        assert_eq!(selection.cursor, query.len());
        assert_eq!(selection.anchor, "中".len());
    }
}
