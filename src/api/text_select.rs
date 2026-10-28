//! Portable text segmentation protocol. Ranges always refer to original UTF-8.
use super::theme::{ColorValue, CompiledColor, ThemedColor};
use std::{ops::Range, sync::Arc};

/// Text selection appearance. Outline padding expands each region side;
/// the pointer offset is relative to the original region's bottom-right corner.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ui<C: ColorValue = ThemedColor> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_color: Option<C>,
    pub border_width: f64,
    pub padding: [u16; 2],
    pub pointer_offset: [i16; 2],
    pub panel_width: u16,
    pub word_min_width: u16,
    pub char_min_width: u16,
}

impl<C: ColorValue> Default for Ui<C> {
    fn default() -> Self {
        Self {
            border_color: None,
            border_width: 1.5,
            padding: [2, 2],
            pointer_offset: [12, 12],
            panel_width: 800,
            word_min_width: 48,
            char_min_width: 40,
        }
    }
}

impl Ui {
    pub fn compile(&self) -> Ui<CompiledColor> {
        Ui {
            border_color: self.border_color.as_ref().map(CompiledColor::from),
            border_width: self.border_width,
            padding: self.padding,
            pointer_offset: self.pointer_offset,
            panel_width: self.panel_width,
            word_min_width: self.word_min_width,
            char_min_width: self.char_min_width,
        }
    }
}

/// Runs only after the selected text has been copied successfully.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Lifecycle {
    pub after_copy: super::LifecycleAction,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            after_copy: super::LifecycleAction::Mode(super::ModeId::normal()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Refine,
    Copy,
    Search,
    CyclePage,
    SelectAll,
    ClearSelection,
    ClearInput,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Refine => "text_refine",
            Self::Copy => "text_copy",
            Self::Search => "text_search",
            Self::CyclePage => "text_cycle_page",
            Self::SelectAll => "text_select_all",
            Self::ClearSelection => "text_clear_selection",
            Self::ClearInput => "text_clear_input",
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
