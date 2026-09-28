//! Search normalization is prepared per scan and reused across search sessions.
use crate::api::SemanticRole;
use pinyin::ToPinyin;

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

    pub(super) fn matches(&self, query: &str, label: &str) -> bool {
        query.split_whitespace().all(|word| {
            label.starts_with(word) || self.text.contains(word) || self.initials.contains(word)
        })
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
