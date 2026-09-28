//! Search normalization is prepared per scan and reused across search sessions.
use crate::api::SemanticRole;
use pinyin::ToPinyin;

/// Reused query spans: no per-term String and no repeated scan for identical
/// terms. Sort only spans, then restore first-occurrence order for copying.
#[derive(Default)]
pub(super) struct SearchTerms(smallvec::SmallVec<[std::ops::Range<usize>; 8]>);

impl SearchTerms {
    pub(super) fn prepare(&mut self, query: &str) {
        self.0.clear();
        for term in query.split_whitespace().filter(|term| *term != "@") {
            let start = term.as_ptr() as usize - query.as_ptr() as usize;
            self.0.push(start..start + term.len());
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

    pub(super) fn iter<'a>(&'a self, query: &'a str) -> impl Iterator<Item = &'a str> {
        self.0.iter().map(|range| &query[range.clone()])
    }
}

#[derive(Default)]
pub(super) struct SearchText {
    text: String,
    initials: String,
}

impl SearchText {
    pub(super) fn target(target: &crate::api::UiTarget) -> Self {
        if target.details.is_none() {
            return Self::new(&target.name, target.role);
        }
        Self::new(
            &format!(
                "{} {} {}",
                target.name,
                target.ocr_text(),
                target.accessibility_text()
            ),
            target.role,
        )
    }
    pub(super) fn new(name: &str, role: SemanticRole) -> Self {
        let text = format!(
            "{} {} {}",
            name.to_lowercase(),
            role.as_str(),
            role_chinese(role)
        );
        let mut initials = String::with_capacity(text.len());
        for ch in text.chars() {
            if let Some(py) = ch.to_pinyin() {
                initials.push_str(py.first_letter());
            } else {
                initials.push(ch);
            }
        }
        Self { text, initials }
    }

    pub(super) fn matches_term(&self, word: &str, label: &str) -> bool {
        if let Some(prefix) = word.strip_prefix('@') {
            // Hint codes are prefix-free: typing narrows candidates, and
            // completing a code selects only that label. A bare marker
            // is unfinished input, not a select-all term.
            !prefix.is_empty() && label.starts_with(prefix)
        } else {
            label.starts_with(word) || self.text.contains(word) || self.initials.contains(word)
        }
    }

    #[cfg(test)]
    pub(super) fn matches(&self, query: &str, label: &str) -> bool {
        query
            .split_whitespace()
            .all(|word| self.matches_term(word, label))
    }
}

pub(super) fn role_chinese(role: SemanticRole) -> &'static str {
    match role {
        SemanticRole::Button => "按钮",
        SemanticRole::MenuButton => "菜单按钮",
        SemanticRole::Link => "链接",
        SemanticRole::Checkbox => "复选框",
        SemanticRole::Radio => "单选按钮",
        SemanticRole::ComboBox => "组合框 下拉框",
        SemanticRole::TextField => "文本框 输入框 搜索框",
        SemanticRole::StaticText => "文本",
        SemanticRole::Slider => "滑块",
        SemanticRole::Spinner => "数值框",
        SemanticRole::Stepper => "步进器",
        SemanticRole::Scrollbar => "滚动条",
        SemanticRole::Tab => "标签页 选项卡",
        SemanticRole::ListItem => "列表项",
        SemanticRole::TreeItem => "树节点",
        SemanticRole::Cell => "单元格",
        SemanticRole::Row => "行",
        SemanticRole::MenuItem => "菜单项",
        SemanticRole::MenubarItem => "菜单栏项",
        SemanticRole::Calendar => "日历",
        SemanticRole::Image => "图片 图像",
        SemanticRole::Control => "控件",
        SemanticRole::Unknown => "未知",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    }

    #[test]
    fn matches_simplified_chinese_initials_roles_labels_and_mixed_text() {
        let text = SearchText::new("复制文件 Ctrl+C", SemanticRole::Button);
        for query in ["复制", "fzwj", "an", "button", "aj", "ctrl+c", "fzwj an"] {
            assert!(text.matches(query, "aj"), "{query}");
        }
        assert!(!text.matches("粘贴", "aj"));
        assert!(!text.matches("fzwj checkbox", "aj"));
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
