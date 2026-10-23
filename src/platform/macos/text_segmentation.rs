//! Foundation's system word tokenizer. No dictionary or native objects escape.
use objc2_foundation::{NSRange, NSString, NSStringEnumerationOptions};
use std::{cell::RefCell, ops::Range};

pub(super) fn words(text: &str) -> Vec<Range<usize>> {
    objc2::rc::autoreleasepool(|_| {
        let value = NSString::from_str(text);
        let ranges = std::rc::Rc::new(RefCell::new(Vec::new()));
        let result = ranges.clone();
        let block = block2::RcBlock::new(
            move |_: *mut NSString,
                  range: NSRange,
                  _: NSRange,
                  _: std::ptr::NonNull<objc2::runtime::Bool>| {
                if let Some(end) = range.location.checked_add(range.length) {
                    ranges.borrow_mut().push(range.location..end);
                }
            },
        );
        value.enumerateSubstringsInRange_options_usingBlock(
            NSRange::new(0, value.length()),
            NSStringEnumerationOptions::ByWords
                | NSStringEnumerationOptions::Localized
                | NSStringEnumerationOptions::SubstringNotRequired,
            &block,
        );
        drop(block);
        crate::platform::common::text_segmentation::utf16_ranges(
            text,
            std::mem::take(&mut *result.borrow_mut()),
        )
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn system_words_use_original_offsets_for_chinese_and_english() {
        let text = "使用键盘选择文字，hello world 👨‍👩‍👧‍👦";
        let ranges = super::words(text);
        let words: Vec<_> = ranges.iter().map(|r| &text[r.clone()]).collect();
        assert!(words.contains(&"hello"));
        assert!(words.contains(&"world"));
        assert!(
            words.iter().any(|w| w.chars().count() >= 2
                && w.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)))
        );
        assert!(ranges.windows(2).all(|r| r[0].end <= r[1].start));
    }
}
