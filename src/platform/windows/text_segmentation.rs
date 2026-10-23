//! Windows 10 system word breaking, with explicit Chinese language selection.
use std::ops::Range;
use windows::{Data::Text::WordsSegmenter, core::HSTRING};

pub(super) fn words(text: &str) -> Vec<Range<usize>> {
    let Ok(_apartment) = super::native::ComApartment::initialise() else {
        return Vec::new();
    };
    let chinese = text
        .chars()
        .any(|c| matches!(c as u32, 0x3400..=0x9fff | 0x20000..=0x323af));
    let Ok(segmenter) =
        WordsSegmenter::CreateWithLanguage(&HSTRING::from(if chinese { "zh-Hans" } else { "und" }))
    else {
        return Vec::new();
    };
    if chinese
        && segmenter
            .ResolvedLanguage()
            .is_ok_and(|language| language == "und")
    {
        return Vec::new();
    }
    let Ok(tokens) = segmenter.GetTokens(&HSTRING::from(text)) else {
        return Vec::new();
    };
    let ranges = tokens.into_iter().filter_map(|token| {
        let range = token.SourceTextSegment().ok()?;
        let start = range.StartPosition as usize;
        Some(start..start.checked_add(range.Length as usize)?)
    });
    crate::platform::common::text_segmentation::utf16_ranges(text, ranges)
}
