//! Search normalization is prepared per scan and reused across search sessions.
use crate::api::SemanticRole;
use pinyin::ToPinyin;

/// Reused query spans: no per-term String and no repeated scan for identical
/// terms. Sort only spans, then restore first-occurrence order for copying.
#[derive(Default)]
pub(in crate::modes) struct SearchTerms(smallvec::SmallVec<[std::ops::Range<usize>; 8]>);

impl SearchTerms {
    pub(in crate::modes) fn prepare(&mut self, query: &str) {
        self.0.clear();
        for term in query.split_whitespace().filter(|term| *term != "@") {
            if let Some(range) = query.substr_range(term) {
                self.0.push(range.into());
            }
        }
        if self.0.len() > 1 {
            self.0.sort_unstable_by(|a, b| {
                query[a.clone()]
                    .cmp(&query[b.clone()])
                    .then(a.start.cmp(&b.start))
            });
            self.0.dedup_by(|a, b| query[a.clone()] == query[b.clone()]);
            self.0.sort_unstable_by_key(|range| range.start);
        }
    }

    pub(in crate::modes) fn needs_dedup(&self) -> bool {
        self.0.len() > 1
    }

    pub(in crate::modes) fn iter<'a>(&'a self, query: &'a str) -> impl Iterator<Item = &'a str> {
        self.0.iter().map(|range| &query[range.clone()])
    }
}

pub(in crate::modes) const RANK_COUNT: usize = 9;

/// One successor per matching target. Nine fixed rank buckets are joined in
/// priority order while filtering, avoiding sorting and extra target copies.
#[derive(Default)]
pub(in crate::modes) struct SearchCycle {
    next: Vec<usize>,
    first: Option<usize>,
    last: Option<usize>,
}

impl SearchCycle {
    pub(in crate::modes) fn clear(&mut self) {
        self.next.clear();
        self.first = None;
        self.last = None;
    }

    pub(in crate::modes) fn first(&self) -> Option<usize> {
        self.first
    }

    pub(in crate::modes) fn next(&self, current: Option<usize>) -> Option<usize> {
        super::next_target_position(current, self.first, |index| self.next[index])
    }

    pub(in crate::modes) fn push(
        &mut self,
        rank: u8,
        buckets: &mut [Option<(usize, usize)>; RANK_COUNT],
    ) {
        let index = self.next.len();
        self.next.push(index + 1);
        if let Some((_, last)) = &mut buckets[usize::from(rank)] {
            // Consecutive members already point to the next result.
            if *last + 1 != index {
                self.next[*last] = index;
            }
            *last = index;
        } else {
            buckets[usize::from(rank)] = Some((index, index));
        }
    }

    pub(in crate::modes) fn append(&mut self, buckets: [Option<(usize, usize)>; RANK_COUNT]) {
        for (first, last) in buckets.into_iter().flatten() {
            self.first.get_or_insert(first);
            if let Some(previous) = self.last {
                self.next[previous] = first;
            }
            self.last = Some(last);
        }
        if let (Some(first), Some(last)) = (self.first, self.last) {
            self.next[last] = first;
        }
    }
}

/// Keep explicit preview identity independent of scan-vector positions.
pub(in crate::modes) struct Focus {
    pub index: usize,
    pub label: crate::api::hint::HintCode,
    pub bounds: crate::api::Rect,
}

#[derive(Default)]
pub(crate) struct SearchText {
    text: String,
    initials_start: usize,
}

/// Parse the label marker once per query term, outside the candidate loop.
pub(crate) struct Term<'a> {
    pub code: &'a str,
    pub labels_only: bool,
    initials_possible: bool,
}

impl<'a> Term<'a> {
    pub(crate) fn new(word: &'a str) -> Self {
        let code = word.strip_prefix('@').or_else(|| word.strip_suffix('@'));
        let labels_only = code.is_some();
        let code = code.unwrap_or(word);
        Self {
            code,
            labels_only,
            // Every convertible character is replaced when building initials.
            // A query containing one cannot occur in that representation.
            initials_possible: if code.is_ascii() {
                // Transliteration only introduces letters. A digit/punctuation
                // query absent from the literal text cannot appear in initials.
                code.bytes().any(|byte| byte.is_ascii_alphabetic())
            } else {
                code.chars().all(|ch| ch.to_pinyin().is_none())
            },
        }
    }

    pub(in crate::modes) fn label_rank(&self, label: &str) -> Option<u8> {
        (!self.code.is_empty() && label.starts_with(self.code))
            .then_some(u8::from(label.len() != self.code.len()) * 3)
    }

    /// Match each representation once and use the compiled quality/priority table.
    pub(crate) fn rank(
        &self,
        text: &SearchText,
        label: &str,
        priority: &crate::api::hint::CompiledSearchPriority,
    ) -> Option<u8> {
        if self.labels_only {
            return self.label_rank(label);
        }
        let label_quality = if label.starts_with(self.code) {
            u8::from(label.len() != self.code.len())
        } else {
            3
        };
        if label_quality == 0 && priority.label_first() {
            return Some(0);
        }
        let (plain, initials) = text.text.split_at(text.initials_start);
        let text_quality = match_quality(plain, self.code);
        // Literal Latin text retained in initials belongs to the text group
        // and does not need a second substring search.
        let pinyin_quality = if text_quality == 3 && self.initials_possible {
            match_quality(initials, self.code)
        } else {
            3
        };
        priority.rank(
            usize::from(label_quality)
                | usize::from(text_quality) << 2
                | usize::from(pinyin_quality) << 4,
        )
    }

    #[cfg(test)]
    pub(crate) fn matches(&self, text: &SearchText, label: &str) -> bool {
        self.rank(
            text,
            label,
            &crate::api::hint::CompiledSearchPriority::new(
                crate::api::hint::DEFAULT_SEARCH_MATCH_PRIORITY,
            ),
        )
        .is_some()
    }
}

/// Find the strongest occurrence, including a later complete word. This reuses
/// normalized text and reads only the two adjacent characters at each match.
fn match_quality(text: &str, word: &str) -> u8 {
    if word.is_empty() {
        return 3;
    }
    let boundary = |ch: char| !ch.is_alphanumeric() && ch != '_';
    let (remaining, offset, best) = if let Some(remaining) = text.strip_prefix(word) {
        if remaining.chars().next().is_none_or(boundary) {
            return 0;
        }
        // The first occurrence is a prefix. Only a later whole word can beat it.
        (remaining, word.len(), 1)
    } else {
        (text, 0, 3)
    };
    // The char pattern scans an ASCII byte directly, avoiding a substring
    // searcher for the common first keystroke or a single-digit query.
    if let [byte] = word.as_bytes() {
        return quality_positions(
            text,
            offset,
            1,
            best,
            remaining
                .match_indices(char::from(*byte))
                .map(|(index, _)| index),
        );
    }
    // Boolean substring matching cheaply rejects multi-byte misses.
    if !remaining.contains(word) {
        return best;
    }
    quality_positions(
        text,
        offset,
        word.len(),
        best,
        remaining.match_indices(word).map(|(index, _)| index),
    )
}

#[inline]
fn quality_positions(
    text: &str,
    offset: usize,
    width: usize,
    mut best: u8,
    positions: impl Iterator<Item = usize>,
) -> u8 {
    let boundary = |ch: char| !ch.is_alphanumeric() && ch != '_';
    for start in positions {
        let start = start + offset;
        let left = text[..start].chars().next_back().is_none_or(boundary);
        let quality = if left {
            if text[start + width..].chars().next().is_none_or(boundary) {
                return 0;
            }
            1
        } else {
            2
        };
        best = best.min(quality);
    }
    best
}

impl SearchText {
    /// Queue at most one rebuild for a changed target. A valid split can never
    /// reach usize::MAX; keep the dirty marker in the existing index word.
    pub(in crate::modes) fn invalidate(&mut self) -> bool {
        std::mem::replace(&mut self.initials_start, usize::MAX) != usize::MAX
    }

    pub(in crate::modes) fn target(target: &crate::api::UiTarget) -> Self {
        let Some(details) = target.details.as_deref() else {
            return Self::new(&target.name, target.role);
        };
        let name = target.name.as_str();
        let ocr = details.ocr.as_str();
        let ocr = if ocr == name { "" } else { ocr };
        let accessibility = details.accessibility.as_str();
        let accessibility = if accessibility == name || accessibility == ocr {
            ""
        } else {
            accessibility
        };
        // Fusion commonly repeats the name in OCR or accessibility metadata.
        // Terms cannot cross these space-separated fields, so index each once.
        if ocr.is_empty() && accessibility.is_empty() {
            return Self::new(name, target.role);
        }
        Self::fields([name, ocr, accessibility], target.role)
    }

    #[inline(never)]
    fn fields(fields: [&str; 3], role: SemanticRole) -> Self {
        if !fields.iter().all(|field| field.is_ascii()) {
            // Preserve whole-string contextual Unicode case conversion.
            return Self::new(&format!("{} {} {}", fields[0], fields[1], fields[2]), role);
        }
        let role_name = role.as_str();
        let (translation, initials) = role_terms(role);
        // Construct the final index directly, without allocating a temporary
        // joined name for late OCR metadata and then copying it again.
        let capacity = fields.iter().map(|field| field.len()).sum::<usize>()
            + 4
            + role_name.len()
            + translation.len()
            + initials.len();
        let mut text = String::with_capacity(capacity);
        text.push_str(fields[0]);
        text.push(' ');
        text.push_str(fields[1]);
        text.push(' ');
        text.push_str(fields[2]);
        text.make_ascii_lowercase();
        text.push(' ');
        text.push_str(role_name);
        text.push(' ');
        text.push_str(translation);
        let initials_start = text.len();
        text.push_str(initials);
        Self {
            text,
            initials_start,
        }
    }

    pub(crate) fn new(name: &str, role: SemanticRole) -> Self {
        Self::with_role(name, Some(role))
    }

    pub(in crate::modes) fn content(name: &str) -> Self {
        Self::with_role(name, None)
    }

    fn with_role(name: &str, role: Option<SemanticRole>) -> Self {
        let role_name = role.map_or("", SemanticRole::as_str);
        let (role_translation, role_initials) = role.map_or(("", ""), role_terms);
        let suffix_len = if role.is_some() {
            2 + role_name.len() + role_translation.len()
        } else {
            0
        };
        let mut ascii = true;
        let unicode_lowercase = name.chars().any(|ch| {
            if ch.is_ascii() {
                return false;
            }
            ascii = false;
            ch.to_lowercase().ne(std::iter::once(ch))
        });
        let mut text = if unicode_lowercase {
            // Whole-string conversion preserves contextual Unicode mappings,
            // including final sigma, and mappings that expand to multiple chars.
            let mut text = name.to_lowercase();
            text.reserve_exact(text.len() + 2 * suffix_len);
            text
        } else {
            // ASCII and uncased text (including Chinese) fit with the roles in
            // one allocation. Lowercasing ASCII preserves all other UTF-8 bytes.
            let capacity = if ascii {
                name.len() + suffix_len + role_initials.len()
            } else {
                2 * (name.len() + suffix_len)
            };
            let mut text = String::with_capacity(capacity);
            text.push_str(name);
            text.make_ascii_lowercase();
            text
        };
        let name_end = text.len();
        if role.is_some() {
            text.push(' ');
            text.push_str(role_name);
            text.push(' ');
            text.push_str(role_translation);
        }
        // Both representations share one allocation; the split keeps searches
        // from matching across their boundary, including control characters.
        let initials_start = text.len();
        // ASCII names are already searched in plain text. Their identical
        // initials copy cannot add a match; retain only the role aliases below.
        if !ascii {
            let mut cursor = 0;
            while cursor < name_end {
                // ASCII cannot produce pinyin. Copy each unchanged run at once,
                // including spaces and punctuation between Unicode characters.
                let ascii_len = text.as_bytes()[cursor..name_end]
                    .iter()
                    .position(|byte| !byte.is_ascii())
                    .unwrap_or(name_end - cursor);
                if ascii_len != 0 {
                    text.extend_from_within(cursor..cursor + ascii_len);
                    cursor += ascii_len;
                    if cursor == name_end {
                        break;
                    }
                }
                let Some(ch) = text[cursor..name_end].chars().next() else {
                    break;
                };
                cursor += ch.len_utf8();
                if let Some(py) = ch.to_pinyin() {
                    text.push_str(py.first_letter());
                } else {
                    text.push(ch);
                }
            }
            if role.is_some() {
                text.push(' ');
                text.push_str(role_name);
                text.push(' ');
            }
        }
        // Role names and aliases are fixed; their initials do not need per-
        // target pinyin conversion. ASCII only needs the translated aliases;
        // the English role name already appears in plain text.
        text.push_str(role_initials);
        Self {
            text,
            initials_start,
        }
    }

    pub(in crate::modes) fn matches_text(&self, word: &str) -> bool {
        let (text, initials) = self.text.split_at(self.initials_start);
        text.contains(word) || initials.contains(word)
    }

    #[cfg(test)]
    pub(in crate::modes) fn matches(&self, query: &str, label: &str) -> bool {
        query
            .split_whitespace()
            .all(|word| Term::new(word).matches(self, label))
    }
}

pub(in crate::modes) fn role_chinese(role: SemanticRole) -> &'static str {
    role_terms(role).0
}

fn role_terms(role: SemanticRole) -> (&'static str, &'static str) {
    match role {
        SemanticRole::Button => ("按钮", "an"),
        SemanticRole::MenuButton => ("菜单按钮", "cdan"),
        SemanticRole::Link => ("链接", "lj"),
        SemanticRole::Checkbox => ("复选框", "fxk"),
        SemanticRole::Radio => ("单选按钮", "dxan"),
        SemanticRole::ComboBox => ("组合框 下拉框", "zhk xlk"),
        SemanticRole::TextField => ("文本框 输入框 搜索框", "wbk srk ssk"),
        SemanticRole::StaticText => ("文本", "wb"),
        SemanticRole::Slider => ("滑块", "hk"),
        SemanticRole::Spinner => ("数值框", "szk"),
        SemanticRole::Stepper => ("步进器", "bjq"),
        SemanticRole::Scrollbar => ("滚动条", "gdt"),
        SemanticRole::Tab => ("标签页 选项卡", "bqy xxk"),
        SemanticRole::ListItem => ("列表项", "lbx"),
        SemanticRole::TreeItem => ("树节点", "sjd"),
        SemanticRole::Cell => ("单元格", "dyg"),
        SemanticRole::Row => ("行", "x"),
        SemanticRole::MenuItem => ("菜单项", "cdx"),
        SemanticRole::MenubarItem => ("菜单栏项", "cdlx"),
        SemanticRole::Calendar => ("日历", "rl"),
        SemanticRole::Image => ("图片 图像", "tp tx"),
        SemanticRole::Control => ("控件", "kj"),
        SemanticRole::Unknown => ("未知", "wz"),
    }
}

/// Keep query normalization, ranking and candidate storage off the first-label path.
#[inline(never)]
pub(in crate::modes) fn filter(
    session: &mut super::session::ScanSession,
    query: &str,
    priority: &crate::api::hint::CompiledSearchPriority,
) -> bool {
    // Small lists complete more cheaply than showing a preparation scene and
    // returning through the scheduler. Keep the large-list input budget small.
    let budget = if session.scanned.len() <= 2_048 {
        2_048
    } else {
        super::SEARCH_PREWARM_TARGETS
    };
    filter_with_budget(session, query, priority, budget)
}

/// The same query, ranking and multi-selection semantics with a caller-owned
/// preparation budget. Zero defers text indexing until after the input paint.
pub(in crate::modes) fn filter_with_budget(
    session: &mut super::session::ScanSession,
    query: &str,
    priority: &crate::api::hint::CompiledSearchPriority,
    budget: usize,
) -> bool {
    let needs_names = query
        .split_whitespace()
        .any(|word| !word.starts_with('@') && !word.ends_with('@'));
    session.search_pending = needs_names && !session.prepare_search_names(budget);
    if session.search_pending {
        // Never expose old candidates as matches for new text. Keep the label
        // storage for reuse; the mode displays preparation status until ready.
        session.search_matches.clear();
        session.search_cycle.clear();
        session.search_focus = None;
        session.search_selected.clear();
        return false;
    }
    let mut display_changed = false;
    let query = if !query.chars().any(char::is_uppercase) {
        std::borrow::Cow::Borrowed(query)
    } else {
        std::borrow::Cow::Owned(query.to_lowercase())
    };
    let multiple = query.chars().any(char::is_whitespace);
    let show_all = query.is_empty()
        || multiple && query.ends_with(char::is_whitespace)
        || query.split_whitespace().next_back() == Some("@");
    let mut matched = 0;
    let mut selected = 0;
    session.search_cycle.clear();
    session.search_seen.clear();
    // Each space-separated item runs the same search against original labels.
    // Union in item order; the first occurrence owns each target's position.
    session.search_terms.prepare(&query);
    let needs_dedup = session.search_terms.needs_dedup();
    if needs_dedup {
        session.search_seen.resize(session.scanned.len(), false);
    }
    for term in session.search_terms.iter(&query) {
        let term = Term::new(term);
        let mut buckets = [None; RANK_COUNT];
        let (mut exact, mut last_match, mut term_matches) = (None, None, 0);
        for (hint_index, hint) in session.search_hints.iter().enumerate() {
            let rank = if term.labels_only {
                term.label_rank(hint.label.as_str())
            } else {
                session
                    .search_text
                    .get(hint.value)
                    .and_then(|text| term.rank(text, hint.label.as_str(), priority))
            };
            if let Some(rank) = rank {
                if hint.label.as_str() == term.code {
                    exact = Some(hint);
                }
                last_match = Some(hint);
                term_matches += 1;
                if needs_dedup {
                    if session.search_seen[hint.value] {
                        continue;
                    }
                    session.search_seen[hint.value] = true;
                }
                if let Some(previous) = session.search_matches.get_mut(matched) {
                    *previous = hint_index;
                } else {
                    session.search_matches.push(hint_index);
                }
                // Candidate order keeps indices only. Update the displayed
                // labels in place, without a second full clone pass.
                if !show_all {
                    if let Some(previous) = session.hints.get_mut(matched) {
                        if previous.value != hint.value
                            || previous.label != hint.label
                            || previous.bounds != hint.bounds
                        {
                            display_changed = true;
                            previous.clone_from(hint);
                        }
                    } else {
                        display_changed = true;
                        session.hints.push(hint.clone());
                    }
                }
                session.search_cycle.push(rank, &mut buckets);
                matched += 1;
            }
        }
        session.search_cycle.append(buckets);
        // A completed label takes priority over semantic collisions.
        // Other search terms select a point only after disambiguation.
        let resolved = exact.or_else(|| {
            last_match.filter(|hint| {
                term_matches == 1
                    && !term.labels_only
                    && session.search_text[hint.value].matches_text(term.code)
            })
        });
        if let Some(hint) = resolved
            && !session.search_selected[..selected]
                .iter()
                .any(|previous| previous.value == hint.value)
        {
            if let Some(previous) = session.search_selected.get_mut(selected) {
                previous.clone_from(hint);
            } else {
                session.search_selected.push(hint.clone());
            }
            selected += 1;
        }
    }
    session.search_matches.truncate(matched);
    session.search_focus = session.search_focus.take().and_then(|mut focused| {
        focused.index = session.search_matches.iter().position(|&index| {
            let hint = &session.search_hints[index];
            hint.label == focused.label && hint.bounds == focused.bounds
        })?;
        Some(focused)
    });
    session.search_selected.truncate(selected);
    if show_all {
        for (index, hint) in session.search_hints.iter().enumerate() {
            if let Some(previous) = session.hints.get_mut(index) {
                if previous.value != hint.value
                    || previous.label != hint.label
                    || previous.bounds != hint.bounds
                {
                    display_changed = true;
                    previous.clone_from(hint);
                }
            } else {
                session.hints.push(hint.clone());
            }
        }
        session.hints.truncate(session.search_hints.len());
    } else {
        session.hints.truncate(matched);
    }
    display_changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_shortcut_preserves_initials_matching_and_ranking() {
        let priority = crate::api::hint::CompiledSearchPriority::new(
            crate::api::hint::DEFAULT_SEARCH_MATCH_PRIORITY,
        );
        for name in [
            "设置 123",
            "第1项",
            "12设置34",
            "1_2",
            "１２ 12",
            "!设置!",
            "Control 123",
        ] {
            let text = SearchText::new(name, SemanticRole::Button);
            for query in ["1", "12", "34", "!", "_", "1_2", "23", "1x", "sz"] {
                let term = Term::new(query);
                let full = Term {
                    initials_possible: true,
                    ..Term::new(query)
                };
                assert_eq!(
                    term.rank(&text, "12", &priority),
                    full.rank(&text, "12", &priority),
                    "{name}/{query}"
                );
            }
        }
    }
    #[test]
    fn fixed_role_initials_match_transliteration_for_unicode_names() {
        for role in SemanticRole::ALL {
            for name in [
                "Control 设置",
                "设置 Control 123",
                "a复制\0b设置 c!日本語 XYZ",
                "🙂ASCII设置𠀀tail",
                "ΟΔΟΣ İẞ",
                "复制\0面板",
            ] {
                let plain = format!(
                    "{} {} {}",
                    name.to_lowercase(),
                    role.as_str(),
                    role_chinese(role)
                );
                let initials: String = plain
                    .chars()
                    .map(|ch| {
                        ch.to_pinyin()
                            .map_or(ch, |py| py.first_letter().chars().next().unwrap())
                    })
                    .collect();
                let actual = SearchText::new(name, role);
                assert_eq!(
                    actual.text.split_at(actual.initials_start),
                    (plain.as_str(), initials.as_str()),
                    "{role:?} / {name:?}"
                );
            }
        }
    }

    #[test]
    fn match_quality_finds_later_words_and_respects_unicode_boundaries() {
        for (text, word, expected) in [
            ("asz sz", "sz", 0),
            ("szextra sz", "sz", 0),
            ("aaaa aa", "aa", 0),
            ("a_a a", "a", 0),
            ("设置菜单 设置", "设置", 0),
            ("szextra", "sz", 1),
            ("extrasz", "sz", 2),
            ("设置，按钮", "设置", 0),
            ("设置菜单", "设置", 1),
            ("关于设置", "设置", 2),
            ("σ δ", "σ", 0),
            ("foo_bar", "bar", 2),
            ("11 1", "1", 0),
            ("1111", "1", 1),
            ("a1b", "1", 2),
            ("界1", "1", 2),
            ("1界", "1", 1),
            ("_1", "1", 2),
            ("foo", "missing", 3),
            ("foo", "", 3),
        ] {
            assert_eq!(match_quality(text, word), expected, "{text:?} / {word:?}");
        }
    }

    #[test]
    fn result_cycle_joins_interleaved_ranks_and_terms_then_reuses_storage() {
        let mut cycle = SearchCycle::default();
        for ranks in [&[4, 0, 4, 2, 0, 8][..], &[1, 1, 0][..]] {
            let mut buckets = [None; RANK_COUNT];
            for &rank in ranks {
                cycle.push(rank, &mut buckets);
            }
            cycle.append(buckets);
        }
        let mut current = None;
        for expected in [1, 4, 3, 0, 2, 5, 8, 6, 7, 1] {
            current = cycle.next(current);
            assert_eq!(current, Some(expected));
        }
        cycle.clear();
        assert_eq!(cycle.next(None), None);
        let mut buckets = [None; RANK_COUNT];
        for _ in 0..3 {
            cycle.push(0, &mut buckets);
        }
        cycle.append(buckets);
        current = None;
        for expected in [0, 1, 2, 0] {
            current = cycle.next(current);
            assert_eq!(current, Some(expected));
        }
    }

    #[test]
    fn repeated_terms_scan_once_in_original_order_and_reuse_capacity() {
        let query = "@ka 复制 @ka missing 复制 @";
        let mut terms = SearchTerms::default();
        terms.prepare(query);
        assert_eq!(
            terms.iter(query).collect::<Vec<_>>(),
            ["@ka", "复制", "missing"]
        );
        let repeated = "missing ".repeat(2048);
        terms.prepare(&repeated);
        assert_eq!(terms.iter(&repeated).collect::<Vec<_>>(), ["missing"]);
        let storage = terms.0.as_ptr();
        let capacity = terms.0.capacity();
        terms.prepare(&repeated);
        assert_eq!(terms.0.as_ptr(), storage);
        assert_eq!(terms.0.capacity(), capacity);
        terms.prepare("@ @");
        assert_eq!(terms.iter("@ @").count(), 0);
        assert_eq!(terms.0.capacity(), capacity);
    }

    #[test]
    fn label_query_also_matches_other_targets_text() {
        let label = SearchText::new("Save", SemanticRole::Button);
        let semantic = SearchText::new("Language", SemanticRole::Button);
        assert!(label.matches("la", "la"));
        assert!(semantic.matches("la", "ka"));
        assert!(!label.matches("la", "ka"));
        assert!(label.matches("@la", "la"));
        assert!(!semantic.matches("@la", "ka"));
        assert!(semantic.matches("@l", "la"));
        assert!(!semantic.matches("@l", "ka"));
        assert!(!semantic.matches("@", "ka"));
        assert!(label.matches("la@", "la"));
        assert!(!semantic.matches("la@", "ka"));
        assert!(label.matches("l@", "la"));
        assert!(!semantic.matches("l@", "ka"));
    }

    #[test]
    fn matches_simplified_chinese_initials_roles_labels_and_mixed_text() {
        let text = SearchText::new("复制文件 Ctrl+C", SemanticRole::Button);
        for query in ["复制", "fzwj", "an", "button", "aj", "ctrl+c", "fzwj an"] {
            assert!(text.matches(query, "aj"), "{query}");
        }
        assert!(!text.matches("粘贴", "aj"));
        assert!(!text.matches("fzwj checkbox", "aj"));
        let separate_fields = SearchText::new("X", SemanticRole::Button);
        assert!(separate_fields.matches_text("an"));
        assert!(!separate_fields.matches_text("钮x"));
        for name in [
            "",
            "SAVE",
            "复制 Ctrl+C",
            "ΟΣ",
            "ΟΣΑ",
            "İ",
            "ǅ",
            "Σ 复制",
            "Σ\u{301}",
        ] {
            let indexed = SearchText::new(name, SemanticRole::Button);
            assert_eq!(
                &indexed.text[..indexed.initials_start],
                format!("{} button 按钮", name.to_lowercase()),
                "{name:?}"
            );
        }
    }

    #[test]
    fn ascii_names_preserve_literal_initials_and_label_match_ranks() {
        let priority = crate::api::hint::CompiledSearchPriority::new(
            crate::api::hint::DEFAULT_SEARCH_MATCH_PRIORITY,
        );
        for role in SemanticRole::ALL {
            for name in ["", "SAVE Ctrl+S", "an", "under_score 123", "buttonan"] {
                let current = SearchText::new(name, role);
                let lower = name.to_ascii_lowercase();
                let (translation, initials) = role_terms(role);
                let plain = format!("{lower} {} {translation}", role.as_str());
                let previous = SearchText {
                    initials_start: plain.len(),
                    text: format!("{plain}{lower} {} {initials}", role.as_str()),
                };
                for query in [
                    "save", "sav", "ave", "ctrl+s", "an", "button", "buttonan", "score", "123",
                    "按钮", "wbk", "srk", "ssk", "zhk", "xlk", "wz", "missing", "@sa", "sa@",
                ]
                .into_iter()
                .chain(std::iter::once(role.as_str()))
                .chain(initials.split_whitespace())
                {
                    let term = Term::new(query);
                    assert_eq!(
                        term.rank(&current, "sa", &priority),
                        term.rank(&previous, "sa", &priority),
                        "name={name:?} role={role:?} query={query:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn fused_fields_are_indexed_once_and_keep_unicode_and_search_semantics() {
        for (name, ocr, accessibility, distinct) in [
            ("Action 1", "Text 1", "", "Action 1 Text 1 "),
            (
                "SAVE Ctrl+S",
                "",
                "button\0help",
                "SAVE Ctrl+S  button\0help",
            ),
            ("", "TEXT", "TEXT", " TEXT "),
            ("SAVE 复制", "SAVE 复制", "", "SAVE 复制"),
            ("SAVE 复制", "", "SAVE 复制", "SAVE 复制"),
            ("SAVE 复制", "SAVE 复制", "SAVE 复制", "SAVE 复制"),
            ("SAVE 复制", "设置", "设置", "SAVE 复制 设置 "),
            ("SAVE 复制", "设置", "Cancel", "SAVE 复制 设置 Cancel"),
            ("", "设置", "设置", " 设置 "),
            ("", "", "", ""),
            ("ΟΣ İ ǅ", "ΟΣ İ ǅ", "设置", "ος i\u{307} ǆ  设置"),
        ] {
            let target = crate::api::UiTarget {
                name: name.into(),
                rect: crate::api::Rect::default(),
                role: SemanticRole::Button,
                details: Some(Box::new(crate::api::geometry::UiTargetDetails {
                    ocr: ocr.into(),
                    accessibility: accessibility.into(),
                    ..Default::default()
                })),
            };
            let indexed = SearchText::target(&target);
            let expected = SearchText::new(distinct, target.role);
            assert_eq!(indexed.text, expected.text);
            assert_eq!(indexed.initials_start, expected.initials_start);
            let previous = SearchText::new(&format!("{name} {ocr} {accessibility}"), target.role);
            for query in [
                "save", "复制", "fz", "设置", "sz", "cancel", "ο", "ος", "i\u{307}", "ǆ", "button",
                "按钮", "an", "save sz", "missing", "@ka", "ka@",
            ] {
                assert_eq!(
                    indexed.matches(query, "ka"),
                    previous.matches(query, "ka"),
                    "name={name:?} ocr={ocr:?} accessibility={accessibility:?} query={query:?}"
                );
            }
        }
    }

    #[test]
    #[ignore = "search throughput probe; run in release"]
    fn prepared_search_throughput() {
        for count in [100, 1000, 10000] {
            let start = std::time::Instant::now();
            let index: Vec<_> = (0..count)
                .map(|i| SearchText::new(&format!("复制文件 设置 {i}"), SemanticRole::Button))
                .collect();
            let build = start.elapsed();
            let mut samples = Vec::new();
            for _ in 0..200 {
                let start = std::time::Instant::now();
                std::hint::black_box(
                    index
                        .iter()
                        .filter(|entry| entry.matches("fzwj an", "aj"))
                        .count(),
                );
                samples.push(start.elapsed());
            }
            samples.sort();
            println!(
                "search count={count} build={build:?} filter_p50={:?} filter_p99={:?}",
                samples[100], samples[198]
            );
        }
    }
}
