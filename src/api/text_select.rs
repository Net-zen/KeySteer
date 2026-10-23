//! Portable text segmentation protocol. Ranges always refer to original UTF-8.
use std::{ops::Range, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Refine,
    Copy,
    Search,
    NextPage,
    PreviousPage,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Refine => "text_refine",
            Self::Copy => "text_copy",
            Self::Search => "text_search",
            Self::NextPage => "text_next_page",
            Self::PreviousPage => "text_previous_page",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: u64,
    pub text: Arc<str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segmented {
    pub id: u64,
    /// Empty on unavailable language rules; the mode falls back to graphemes.
    pub words: Vec<Range<usize>>,
}
