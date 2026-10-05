//! Reusable UI style blocks.
//!
//! Every color is optional: omitted values are derived from the theme palette
//! at resolve time, `-1` means "auto" for the

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::backend::Appearance;
use crate::api::overlay::{Color, LabelStyle, Placement, SharedLabelStyle, TextAlignment};

use super::theme::{ColorValue, CompiledColor, Palette, ThemedColor};

/// Runtime-neutral mode badge and cursor decoration settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModeIndicator<C: ColorValue = ThemedColor> {
    pub cursor: CursorIndicatorUi<C>,
    pub ui: IndicatorUi<C>,
    pub modes: BTreeMap<String, ModeIndicatorEntry<C>>,
}

impl<C: ColorValue> Default for ModeIndicator<C> {
    fn default() -> Self {
        let ui = IndicatorUi {
            label: LabelUi {
                font_size: 11,
                ..Default::default()
            },
            ..Default::default()
        };
        Self {
            cursor: CursorIndicatorUi::default(),
            ui,
            modes: BTreeMap::from([
                (
                    "normal".into(),
                    ModeIndicatorEntry {
                        enabled: Some(true),
                        text: Some("Normal".into()),
                        ..Default::default()
                    },
                ),
                (
                    "text_input".into(),
                    ModeIndicatorEntry {
                        enabled: Some(false),
                        ..Default::default()
                    },
                ),
            ]),
        }
    }
}

impl<C: ColorValue> ModeIndicator<C> {
    pub fn for_mode(&self, mode_id: &str, display_name: &str) -> Option<(String, IndicatorUi<C>)> {
        self.for_mode_with(mode_id, || display_name.to_owned())
    }

    pub(crate) fn for_mode_with(
        &self,
        mode_id: &str,
        display_name: impl FnOnce() -> String,
    ) -> Option<(String, IndicatorUi<C>)> {
        let entry = self.modes.get(mode_id);
        let enabled = entry
            .and_then(|entry| entry.enabled)
            .unwrap_or(mode_id != "idle");
        if !enabled {
            return None;
        }
        let text = entry
            .and_then(|entry| entry.text.clone())
            .unwrap_or_else(display_name);
        let ui = entry.map_or_else(|| self.ui.clone(), |entry| entry.ui.apply(&self.ui));
        Some((text, ui))
    }

    pub(crate) fn cursor_for_mode_ref(
        &self,
        mode_id: &str,
    ) -> Option<ResolvedCursorIndicatorUi<'_, C>> {
        if mode_id == "idle" {
            return None;
        }
        let override_cursor = self.modes.get(mode_id).map(|entry| &entry.cursor);
        let cursor = ResolvedCursorIndicatorUi {
            enabled: override_cursor
                .and_then(|cursor| cursor.enabled)
                .unwrap_or(self.cursor.enabled),
            radius: override_cursor
                .and_then(|cursor| cursor.radius)
                .unwrap_or(self.cursor.radius),
            fill_color: override_cursor
                .and_then(|cursor| cursor.fill_color.as_ref())
                .or(self.cursor.fill_color.as_ref()),
            stroke_color: override_cursor
                .and_then(|cursor| cursor.stroke_color.as_ref())
                .or(self.cursor.stroke_color.as_ref()),
            left_pressed_color: override_cursor
                .and_then(|cursor| cursor.left_pressed_color.as_ref())
                .or(self.cursor.left_pressed_color.as_ref()),
            middle_pressed_color: override_cursor
                .and_then(|cursor| cursor.middle_pressed_color.as_ref())
                .or(self.cursor.middle_pressed_color.as_ref()),
            right_pressed_color: override_cursor
                .and_then(|cursor| cursor.right_pressed_color.as_ref())
                .or(self.cursor.right_pressed_color.as_ref()),
            stroke_width: override_cursor
                .and_then(|cursor| cursor.stroke_width)
                .unwrap_or(self.cursor.stroke_width),
        };
        cursor.enabled.then_some(cursor)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ResolvedCursorIndicatorUi<'a, C: ColorValue = ThemedColor> {
    pub enabled: bool,
    pub radius: i32,
    pub fill_color: Option<&'a C>,
    pub stroke_color: Option<&'a C>,
    pub left_pressed_color: Option<&'a C>,
    pub middle_pressed_color: Option<&'a C>,
    pub right_pressed_color: Option<&'a C>,
    pub stroke_width: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModeIndicatorEntry<C: ColorValue = ThemedColor> {
    pub enabled: Option<bool>,
    pub text: Option<String>,
    pub cursor: CursorIndicatorOverride<C>,
    pub ui: IndicatorUiOverride<C>,
}

pub const AUTO: i32 = -1;

fn auto() -> i32 {
    AUTO
}
fn one() -> i32 {
    1
}
fn font_size_default() -> i32 {
    10
}

/// Resolve an optional configured color against a derived default.
pub fn resolve<C: ColorValue>(
    configured: Option<&C>,
    appearance: Appearance,
    derived: Color,
) -> Color {
    configured
        .and_then(|c| c.resolve(appearance))
        .unwrap_or(derived)
}

/// Window identity card text and layout, in logical pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowCardUi {
    pub guide_line_enabled: bool,
    pub guide_line_width: f64,
    pub guide_line_color: Option<ThemedColor>,
    pub position_mode: WindowCardPositionMode,
    /// CSS inset order: top, right, bottom, left. Percentages only.
    pub position: [String; 4],
    /// Zero retains the automatic size derived from the number font.
    pub app_font_size: f64,
    pub title_font_size: f64,
    /// Empty inherits the number font family.
    pub app_font_family: String,
    pub title_font_family: String,
    pub app_bold: bool,
    pub title_bold: bool,
    pub app_color: Option<ThemedColor>,
    pub title_color: Option<ThemedColor>,
    pub background_color: Option<ThemedColor>,
    pub border_color: Option<ThemedColor>,
    pub number_color: Option<ThemedColor>,
    pub text_width: f64,
    pub padding_x: f64,
    pub padding_y: f64,
    pub line_height: f64,
    pub min_height: f64,
    pub number_min_width: f64,
}

impl Default for WindowCardUi {
    fn default() -> Self {
        Self {
            guide_line_enabled: true,
            guide_line_width: 3.0,
            guide_line_color: None,
            position_mode: WindowCardPositionMode::Window,
            position: std::array::from_fn(|_| "50%".into()),
            app_font_size: 0.0,
            title_font_size: 0.0,
            app_font_family: String::new(),
            title_font_family: String::new(),
            app_bold: true,
            title_bold: false,
            app_color: None,
            title_color: None,
            background_color: None,
            border_color: None,
            number_color: None,
            text_width: 260.0,
            padding_x: 9.0,
            padding_y: 4.0,
            line_height: 1.4,
            min_height: 44.0,
            number_min_width: 38.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowCardPositionMode {
    #[default]
    Window,
    Screen,
}

impl WindowCardUi {
    pub fn position_ratios(&self) -> Result<[f64; 4], &'static str> {
        percentage_position(&self.position)
    }
}

/// Shared by window cards and configurable input/detail panels.
pub fn percentage_position(position: &[String; 4]) -> Result<[f64; 4], &'static str> {
    let mut ratios = [0.0; 4];
    for (result, source) in ratios.iter_mut().zip(position) {
        let value = source
            .trim()
            .strip_suffix('%')
            .ok_or("position requires four percentages")?;
        *result = value
            .parse::<f64>()
            .map_err(|_| "invalid position percentage")?
            / 100.0;
        if !result.is_finite() || !(0.0..=1.0).contains(result) {
            return Err("position percentages must be 0%..=100%");
        }
    }
    if ratios[0] + ratios[2] > 1.0 + 1e-12 || ratios[1] + ratios[3] > 1.0 + 1e-12 {
        return Err("opposite position percentages must sum to at most 100%");
    }
    Ok(ratios)
}

/// Visual style shared by hint labels, grid cells and badges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LabelUi<C: ColorValue = ThemedColor> {
    pub font_size: i32,
    /// Empty means the platform default UI font.
    pub font_family: String,
    /// `-1` = auto.
    pub border_radius: i32,
    /// `-1` = auto.
    pub padding_x: i32,
    /// `-1` = auto.
    pub padding_y: i32,
    pub border_width: i32,
    pub background_color: Option<C>,
    pub text_color: Option<C>,
    pub matched_text_color: Option<C>,
    pub border_color: Option<C>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickSwitchPosition {
    Screen,
    Window,
    #[default]
    Mouse,
}

/// Precompiled panel, keycap and left-aligned caption styles.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickSwitchStyles {
    pub panel: crate::api::overlay::SharedLabelStyle,
    pub key: crate::api::overlay::SharedLabelStyle,
    pub caption: crate::api::overlay::SharedLabelStyle,
}
impl QuickSwitchStyles {
    pub fn new(panel: LabelStyle) -> Self {
        let mut caption = panel.clone();
        caption.background = Color::TRANSPARENT;
        caption.border_width = 0.0;
        caption.padding_x = 0.0;
        caption.padding_y = 0.0;
        caption.text_alignment = crate::api::overlay::TextAlignment::Left;
        let mut key = caption.clone();
        key.text_alignment = crate::api::overlay::TextAlignment::Center;
        key.background = Color::rgb(235, 237, 242);
        key.text_color = Color::rgb(30, 34, 43);
        key.border_color = Color::rgb(196, 201, 211);
        key.border_width = 1.0;
        key.border_radius = 3.0;
        key.padding_x = key.font_size * 0.12;
        key.padding_y = 1.0;
        caption.bold = false;
        Self {
            panel: panel.into(),
            key: key.into(),
            caption: caption.into(),
        }
    }
}

impl<C: ColorValue> Default for LabelUi<C> {
    fn default() -> Self {
        Self {
            font_size: font_size_default(),
            font_family: String::new(),
            border_radius: auto(),
            padding_x: auto(),
            padding_y: auto(),
            border_width: one(),
            background_color: None,
            text_color: None,
            matched_text_color: None,
            border_color: None,
        }
    }
}

impl<C: ColorValue> LabelUi<C> {
    /// Turn configuration into a concrete [`LabelStyle`].
    ///
    /// `background`, `text` and `border` are the theme-derived defaults used
    /// when the config leaves the corresponding field unset.
    pub fn resolve(
        &self,
        palette: &Palette,
        background: Color,
        text: Color,
        border: Color,
    ) -> LabelStyle {
        let appearance = palette.appearance;
        let font_size = self.font_size.max(1) as f64;
        let background = resolve(self.background_color.as_ref(), appearance, background);
        LabelStyle {
            text_alignment: Default::default(),
            background,
            text_color: resolve(self.text_color.as_ref(), appearance, text),
            matched_text_color: resolve(
                self.matched_text_color.as_ref(),
                appearance,
                palette.accent_alt,
            ),
            border_color: resolve(self.border_color.as_ref(), appearance, border),
            border_width: self.border_width.max(0) as f64,
            // Auto radius/padding scale with the font so labels stay legible.
            border_radius: if self.border_radius == AUTO {
                (font_size * 0.35).round()
            } else {
                self.border_radius.max(0) as f64
            },
            padding_x: if self.padding_x == AUTO {
                (font_size * 0.4).round()
            } else {
                self.padding_x.max(0) as f64
            },
            padding_y: if self.padding_y == AUTO {
                (font_size * 0.2).round()
            } else {
                self.padding_y.max(0) as f64
            },
            font_size,
            font_family: self.font_family.clone(),
            bold: true,
        }
    }
}

/// `[ui_hint.boundary_highlight]`: optional outlines around hinted elements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoundaryHighlight<C: ColorValue = ThemedColor> {
    pub enabled: bool,
    pub border_width: i32,
    pub border_radius: i32,
    pub background_color: Option<C>,
    pub border_color: Option<C>,
}

impl<C: ColorValue> Default for BoundaryHighlight<C> {
    fn default() -> Self {
        Self {
            enabled: false,
            border_width: one(),
            border_radius: auto(),
            background_color: None,
            border_color: None,
        }
    }
}

impl<C: ColorValue> BoundaryHighlight<C> {
    pub fn fill(&self, palette: &Palette) -> Color {
        resolve(
            self.background_color.as_ref(),
            palette.appearance,
            Color::TRANSPARENT,
        )
    }

    pub fn stroke(&self, palette: &Palette) -> Color {
        resolve(
            self.border_color.as_ref(),
            palette.appearance,
            palette.accent_border(),
        )
    }

    pub fn radius(&self) -> f64 {
        if self.border_radius == AUTO {
            2.0
        } else {
            self.border_radius.max(0) as f64
        }
    }
}

/// Anchor for a floating panel such as the hint search box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    Center,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
}

impl Anchor {
    /// Place a `width` x `height` panel inside `area` with the given offsets.
    pub fn place(
        &self,
        area: crate::api::geometry::Rect,
        width: f64,
        height: f64,
        x_offset: f64,
        y_offset: f64,
    ) -> crate::api::geometry::Rect {
        use crate::api::geometry::Rect;
        let (x, y) = match self {
            Anchor::TopLeft => (area.left(), area.top() + y_offset),
            Anchor::TopCenter => (area.center().x - width / 2.0, area.top() + y_offset),
            Anchor::TopRight => (area.right() - width, area.top() + y_offset),
            Anchor::Center => (
                area.center().x - width / 2.0,
                area.center().y - height / 2.0,
            ),
            Anchor::BottomLeft => (area.left(), area.bottom() - height - y_offset),
            Anchor::BottomCenter => (
                area.center().x - width / 2.0,
                area.bottom() - height - y_offset,
            ),
            Anchor::BottomRight => (area.right() - width, area.bottom() - height - y_offset),
        };
        Rect::new(x + x_offset, y, width, height)
    }
}

/// `[ui_hint.search_input_ui]`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchInputUi {
    pub position: PanelPosition,
    pub position_mode: PanelPositionMode,
    pub x_offset: i32,
    pub y_offset: i32,
    pub width: i32,
    #[serde(flatten)]
    pub label: LabelUi,
}

impl Default for SearchInputUi {
    fn default() -> Self {
        Self {
            position: PanelPosition::Percentages(["100%", "50%", "0%", "50%"].map(String::from)),
            position_mode: PanelPositionMode::Screen,
            x_offset: 0,
            y_offset: 24,
            width: 280,
            label: LabelUi::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PanelPosition {
    Percentages([String; 4]),
    Anchor(Anchor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelPositionMode {
    Screen,
    Window,
    SearchInput,
}

impl PanelPosition {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Percentages(value) => percentage_position(value).map(|_| ()),
            Self::Anchor(_) => Ok(()),
        }
    }
}

/// Numeric layout and both theme variants, compiled with the runtime plan.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSearchPanel {
    pub position: CompiledPanelPosition,
    pub position_mode: PanelPositionMode,
    pub width: f64,
    pub x_offset: f64,
    pub y_offset: f64,
    light: QuickSwitchStyles,
    dark: QuickSwitchStyles,
}

impl CompiledSearchPanel {
    pub fn new(ui: &SearchInputUi, light: &Palette, dark: &Palette) -> Self {
        let compile = |palette: &Palette| {
            QuickSwitchStyles::new(ui.label.resolve(
                palette,
                palette.surface_label(),
                palette.text,
                palette.accent,
            ))
        };
        Self {
            position: match &ui.position {
                PanelPosition::Anchor(anchor) => CompiledPanelPosition::Anchor(*anchor),
                PanelPosition::Percentages(value) => {
                    CompiledPanelPosition::Percentages(percentage_position(value).unwrap_or_else(
                        |error| panic!("search style requires validated position: {error}"),
                    ))
                }
            },
            position_mode: ui.position_mode,
            width: ui.width.max(1) as f64,
            x_offset: ui.x_offset as f64,
            y_offset: ui.y_offset as f64,
            light: compile(light),
            dark: compile(dark),
        }
    }
    pub fn for_appearance(&self, appearance: Appearance) -> &QuickSwitchStyles {
        match appearance {
            Appearance::Light => &self.light,
            Appearance::Dark => &self.dark,
        }
    }
}

#[cfg(test)]
mod search_panel_tests {
    use super::*;

    #[test]
    fn missing_and_partial_search_styles_use_the_specific_panel_defaults() {
        let defaults = crate::config::Config::default();
        for text in [
            "",
            "[ui_hint]\nmax_depth = 20",
            "[ui_hint.search_input_ui]\n[ui_hint.search_info_ui]",
        ] {
            let parsed = crate::config::Config::parse(text).unwrap();
            assert_eq!(
                parsed.ui_hint.search_input_ui,
                defaults.ui_hint.search_input_ui
            );
            assert_eq!(
                parsed.ui_hint.search_info_ui,
                defaults.ui_hint.search_info_ui
            );
            parsed.validate().unwrap();
        }
        let parsed = crate::config::Config::parse(
            "[ui_hint.search_input_ui]\nwidth = 320\n[ui_hint.search_info_ui]\nfont_size = 18",
        )
        .unwrap();
        let mut input = defaults.ui_hint.search_input_ui.clone();
        input.width = 320;
        let mut info = defaults.ui_hint.search_info_ui;
        info.label.font_size = 18;
        assert_eq!(parsed.ui_hint.search_input_ui, input);
        assert_eq!(parsed.ui_hint.search_info_ui, info);
        assert_eq!(
            parsed.ui_hint.search_info_ui.position_mode,
            PanelPositionMode::SearchInput
        );
        let compiled = crate::app::mode_catalog::hint_settings(&parsed);
        assert_eq!(compiled.search_input_ui.width, 320.0);
        assert_eq!(compiled.search_info_ui.width, 520.0);
        let restored = crate::config::Config::parse(&parsed.to_toml().unwrap()).unwrap();
        assert_eq!(restored.ui_hint.search_input_ui, input);
        assert_eq!(restored.ui_hint.search_info_ui, info);
    }

    #[test]
    fn search_panels_compile_positions_themes_and_reuse_styles_without_allocation() {
        let config = crate::config::Config::parse(
            r##"
[ui_hint.search_input_ui]
position_mode = "window"
position = ["50%", "50%", "50%", "50%"]
font_size = 23
background_color = { light = "#FFFFFFFF", dark = "#000000FF" }
"##,
        )
        .unwrap();
        config.validate().unwrap();
        let panel = CompiledSearchPanel::new(
            &config.ui_hint.search_input_ui,
            &config.palette(Appearance::Light),
            &config.palette(Appearance::Dark),
        );
        assert_eq!(panel.position, CompiledPanelPosition::Percentages([0.5; 4]));
        assert_eq!(
            panel.for_appearance(Appearance::Light).panel.background,
            Color::rgb(255, 255, 255)
        );
        assert_eq!(panel.for_appearance(Appearance::Dark).panel.font_size, 23.0);
        let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
        for _ in 0..1000 {
            std::hint::black_box(panel.for_appearance(Appearance::Dark).panel.clone());
            std::hint::black_box(panel.position.place(
                crate::api::Rect::new(100.0, 100.0, 800.0, 600.0),
                320.0,
                40.0,
                0.0,
                0.0,
            ));
        }
        assert_eq!(region.change().allocations, 0);
        assert_eq!(region.change().reallocations, 0);
    }

    #[test]
    fn search_panel_config_rejects_invalid_geometry_and_copy_slots() {
        for value in [
            "[ui_hint.search_input_ui]\nposition = ['101%', '0%', '0%', '0%']",
            "[ui_hint.search_input_ui]\nposition_mode = 'search_input'",
            "[ui_hint.search_info_ui]\nwidth = 0",
            "[ui_hint]\nsearch_copy_keys = ['ctrl+1']",
            "[ui_hint]\nsearch_copy_keys = ['ctrl+1','ctrl+1','ctrl+3','ctrl+4','ctrl+5']",
        ] {
            assert!(
                crate::config::Config::parse(value)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{value}"
            );
        }
        let legacy =
            crate::config::Config::parse("[ui_hint.search_input_ui]\nposition = 'bottom_center'")
                .unwrap();
        legacy.validate().unwrap();
        let restored = crate::config::Config::parse(&legacy.to_toml().unwrap()).unwrap();
        assert_eq!(
            legacy.ui_hint.search_input_ui,
            restored.ui_hint.search_input_ui
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompiledPanelPosition {
    Percentages([f64; 4]),
    Anchor(Anchor),
}

impl CompiledPanelPosition {
    pub fn place(
        &self,
        area: crate::api::Rect,
        width: f64,
        height: f64,
        x: f64,
        y: f64,
    ) -> crate::api::Rect {
        match self {
            Self::Anchor(anchor) => anchor.place(area, width, height, x, y),
            Self::Percentages(value) => {
                let region = percentage_region(area, *value);
                crate::api::Rect::new(
                    (region.center().x - width / 2.0 + x)
                        .clamp(area.x, (area.right() - width).max(area.x)),
                    (region.center().y - height / 2.0 - y)
                        .clamp(area.y, (area.bottom() - height).max(area.y)),
                    width.min(area.width),
                    height.min(area.height),
                )
            }
        }
    }
}

pub fn percentage_region(
    area: crate::api::Rect,
    [top, right, bottom, left]: [f64; 4],
) -> crate::api::Rect {
    crate::api::Rect::new(
        area.x + area.width * left,
        area.y + area.height * top,
        area.width * (1.0 - left - right).max(0.0),
        area.height * (1.0 - top - bottom).max(0.0),
    )
}

/// `[mode_indicator.ui]`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IndicatorUi<C: ColorValue = ThemedColor> {
    #[serde(flatten)]
    pub label: LabelUi<C>,
    /// Shared right/top anchor relative to the cursor hotspot, in scene units.
    pub indicator_offset: [i16; 2],
}

impl<C: ColorValue> Default for IndicatorUi<C> {
    fn default() -> Self {
        Self {
            label: LabelUi::default(),
            indicator_offset: [-12, 18],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CursorIndicatorUi<C: ColorValue = ThemedColor> {
    pub enabled: bool,
    pub radius: i32,
    pub fill_color: Option<C>,
    pub stroke_color: Option<C>,
    pub left_pressed_color: Option<C>,
    pub middle_pressed_color: Option<C>,
    pub right_pressed_color: Option<C>,
    pub stroke_width: i32,
}

impl<C: ColorValue> Default for CursorIndicatorUi<C> {
    fn default() -> Self {
        Self {
            enabled: true,
            radius: 13,
            fill_color: None,
            stroke_color: None,
            left_pressed_color: Some(ThemedColor::Both("#00FF00FF".into()).into()),
            middle_pressed_color: Some(ThemedColor::Both("#FF00FFFF".into()).into()),
            right_pressed_color: Some(ThemedColor::Both("#00FFFFFF".into()).into()),
            stroke_width: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CursorIndicatorOverride<C: ColorValue = ThemedColor> {
    pub enabled: Option<bool>,
    pub radius: Option<i32>,
    pub fill_color: Option<C>,
    pub stroke_color: Option<C>,
    pub left_pressed_color: Option<C>,
    pub middle_pressed_color: Option<C>,
    pub right_pressed_color: Option<C>,
    pub stroke_width: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IndicatorUiOverride<C: ColorValue = ThemedColor> {
    pub indicator_offset: Option<[i16; 2]>,
    pub font_size: Option<i32>,
    pub font_family: Option<String>,
    pub border_radius: Option<i32>,
    pub padding_x: Option<i32>,
    pub padding_y: Option<i32>,
    pub border_width: Option<i32>,
    pub background_color: Option<C>,
    pub text_color: Option<C>,
    pub matched_text_color: Option<C>,
    pub border_color: Option<C>,
}

impl<C: ColorValue> IndicatorUiOverride<C> {
    pub fn apply(&self, base: &IndicatorUi<C>) -> IndicatorUi<C> {
        let mut resolved = base.clone();
        if let Some(value) = self.indicator_offset {
            resolved.indicator_offset = value;
        }
        if let Some(value) = self.font_size {
            resolved.label.font_size = value;
        }
        if let Some(value) = &self.font_family {
            resolved.label.font_family.clone_from(value);
        }
        if let Some(value) = self.border_radius {
            resolved.label.border_radius = value;
        }
        if let Some(value) = self.padding_x {
            resolved.label.padding_x = value;
        }
        if let Some(value) = self.padding_y {
            resolved.label.padding_y = value;
        }
        if let Some(value) = self.border_width {
            resolved.label.border_width = value;
        }
        for (target, value) in [
            (&mut resolved.label.background_color, &self.background_color),
            (&mut resolved.label.text_color, &self.text_color),
            (
                &mut resolved.label.matched_text_color,
                &self.matched_text_color,
            ),
            (&mut resolved.label.border_color, &self.border_color),
        ] {
            if let Some(value) = value {
                *target = Some(value.clone());
            }
        }
        resolved
    }
}

/// Placement of hint labels; re-exported for config ergonomics.
pub type HintPlacement = Placement;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::geometry::Rect;

    #[test]
    fn compiled_indicator_preserves_overrides_and_live_palette_defaults() {
        let source: ModeIndicator = toml::from_str(
            r##"
            [ui]
            text_color = { light = "#aAbBcCFF", dark = "#12345680" }
            [cursor]
            stroke_color = "#00000000"
            [modes.normal.ui]
            background_color = "#11223344"
            [modes.normal.cursor]
            left_pressed_color = { light = "#FF000080", dark = "#00FF0040" }
        "##,
        )
        .unwrap();
        let original = toml::to_string(&source).unwrap();
        let compiled = source.compile();
        assert_eq!(toml::to_string(&source).unwrap(), original);
        assert!(original.contains("#aAbBcCFF"));
        for mode in ["normal", "ui_hint", "idle"] {
            let before = source.for_mode(mode, mode);
            let after = compiled.for_mode(mode, mode);
            assert_eq!(before.is_some(), after.is_some());
            for appearance in [Appearance::Light, Appearance::Dark] {
                let palette = Palette {
                    appearance,
                    surface: Color::rgb(7, 8, 9),
                    ..Default::default()
                };
                if let (Some((name, ui)), Some((compiled_name, compiled_ui))) = (&before, &after) {
                    assert_eq!(name, compiled_name);
                    assert_eq!(ui.indicator_offset, compiled_ui.indicator_offset);
                    assert_eq!(
                        ui.label
                            .resolve(&palette, palette.surface, palette.text, palette.accent),
                        compiled_ui.label.resolve(
                            &palette,
                            palette.surface,
                            palette.text,
                            palette.accent
                        ),
                    );
                }
                let cursor = source.cursor_for_mode_ref(mode);
                let compiled_cursor = compiled.cursor_for_mode_ref(mode);
                assert_eq!(cursor.is_some(), compiled_cursor.is_some());
                if let (Some(a), Some(b)) = (cursor, compiled_cursor) {
                    assert_eq!(
                        (a.enabled, a.radius, a.stroke_width),
                        (b.enabled, b.radius, b.stroke_width)
                    );
                    for (a, b) in [
                        (a.fill_color, b.fill_color),
                        (a.stroke_color, b.stroke_color),
                        (a.left_pressed_color, b.left_pressed_color),
                        (a.middle_pressed_color, b.middle_pressed_color),
                        (a.right_pressed_color, b.right_pressed_color),
                    ] {
                        assert_eq!(
                            resolve(a, appearance, palette.accent),
                            resolve(b, appearance, palette.accent)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn auto_padding_scales_with_font_size() {
        let palette = Palette::default();
        let ui: LabelUi = LabelUi {
            font_size: 20,
            ..Default::default()
        };
        let style = ui.resolve(&palette, palette.surface, palette.text, palette.accent);
        assert_eq!(style.padding_x, 8.0);
        assert_eq!(style.padding_y, 4.0);
        assert_eq!(style.border_radius, 7.0);
    }

    #[test]
    fn explicit_values_override_auto() {
        let palette = Palette::default();
        let ui: LabelUi = LabelUi {
            padding_x: 3,
            border_radius: 0,
            ..Default::default()
        };
        let style = ui.resolve(&palette, palette.surface, palette.text, palette.accent);
        assert_eq!(style.padding_x, 3.0);
        assert_eq!(style.border_radius, 0.0);
    }

    #[test]
    fn configured_color_wins_over_derived_default() {
        let palette = Palette::default();
        let ui: LabelUi = LabelUi {
            background_color: Some(ThemedColor::Both("#FF0000FF".into())),
            ..Default::default()
        };
        let style = ui.resolve(&palette, palette.surface, palette.text, palette.accent);
        assert_eq!(style.background, Color::rgb(255, 0, 0));
    }

    #[test]
    fn anchor_places_panel_within_area() {
        let area = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let r = Anchor::BottomCenter.place(area, 320.0, 30.0, 0.0, 24.0);
        assert_eq!(r, Rect::new(340.0, 746.0, 320.0, 30.0));
    }
}

/// Available-key panel style; typography and layout adapt to this compact block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KeyHelp<C: ColorValue = ThemedColor> {
    pub mouse_key_help: bool,
    pub window_key_help: bool,
    pub font_family: String,
    pub font_size: f64,
    pub background_color: Option<C>,
    pub text_color: Option<C>,
    pub border_color: Option<C>,
    pub border_width: f64,
    pub border_radius: f64,
    pub padding_x: f64,
    pub padding_y: f64,
}

impl<C: ColorValue> Default for KeyHelp<C> {
    fn default() -> Self {
        Self {
            mouse_key_help: false,
            window_key_help: true,
            font_family: String::new(),
            font_size: 12.0,
            background_color: None,
            text_color: None,
            border_color: None,
            border_width: 0.0,
            border_radius: 10.0,
            padding_x: 24.0,
            padding_y: 8.0,
        }
    }
}

pub type WindowSceneRenderer = for<'a, 'b, 'c> fn(
    &crate::api::presentation::WindowView<'a>,
    &'b crate::api::HostContext<'c>,
) -> crate::api::OverlayScene;

/// Supplied by the host composer; the configuration compiler selects once.
#[derive(Clone, Copy)]
pub struct WindowSceneRenderers {
    pub plain: WindowSceneRenderer,
    pub with_guides: WindowSceneRenderer,
}

// Compiled once per configuration generation; scene construction only borrows these styles.
#[derive(Debug)]
pub struct WindowStyles {
    pub render_scene: WindowSceneRenderer,
    pub card: WindowCardMetrics,
    pub position: [f64; 4],
    pub anchor: crate::api::Point,
    light: ResolvedWindowStyle,
    dark: ResolvedWindowStyle,
}

/// Numeric runtime data only; source strings remain in the configuration DTO.
#[derive(Debug, Clone, Copy)]
pub struct WindowCardMetrics {
    pub position_mode: WindowCardPositionMode,
    pub text_width: f64,
    pub padding_x: f64,
    pub padding_y: f64,
    pub number_min_width: f64,
}

#[derive(Debug)]
pub struct ResolvedWindowStyle {
    pub guide_line: Option<crate::api::overlay::LabelConnectorStyle>,
    pub base: SharedLabelStyle,
    pub number: SharedLabelStyle,
    pub background: SharedLabelStyle,
    pub app: SharedLabelStyle,
    pub title: SharedLabelStyle,
    pub row_height: f64,
    pub min_height: f64,
}

impl WindowStyles {
    pub fn new(
        ui: &LabelUi,
        card: &WindowCardUi,
        light: &Palette,
        dark: &Palette,
        renderers: WindowSceneRenderers,
    ) -> Self {
        let position = card
            .position_ratios()
            .unwrap_or_else(|error| panic!("window style requires validated position: {error}"));
        Self {
            render_scene: if card.guide_line_enabled && card.guide_line_width > 0.0 {
                renderers.with_guides
            } else {
                renderers.plain
            },
            card: WindowCardMetrics {
                position_mode: card.position_mode,
                text_width: card.text_width,
                padding_x: card.padding_x,
                padding_y: card.padding_y,
                number_min_width: card.number_min_width,
            },
            position,
            anchor: crate::api::Point::new(
                position[3] + (1.0 - position[3] - position[1]).max(0.0) / 2.0,
                position[0] + (1.0 - position[0] - position[2]).max(0.0) / 2.0,
            ),
            light: ResolvedWindowStyle::new(ui, card, light),
            dark: ResolvedWindowStyle::new(ui, card, dark),
        }
    }

    pub fn for_appearance(&self, appearance: Appearance) -> &ResolvedWindowStyle {
        match appearance {
            Appearance::Light => &self.light,
            Appearance::Dark => &self.dark,
        }
    }
}

#[cfg(test)]
mod window_styles_tests {
    use super::*;

    #[test]
    fn compiled_card_style_selection_and_cloning_do_not_allocate() {
        let config = crate::config::Config::default();
        let light = config.palette(Appearance::Light);
        let dark = config.palette(Appearance::Dark);
        let styles = WindowStyles::new(
            &config.window.ui,
            &config.window.card,
            &light,
            &dark,
            crate::presentation::window::RENDERERS,
        );
        let region = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
        for i in 0..1000 {
            let resolved = styles.for_appearance(if i % 2 == 0 {
                Appearance::Light
            } else {
                Appearance::Dark
            });
            std::hint::black_box((
                resolved.app.clone(),
                resolved.title.clone(),
                resolved.number.clone(),
                resolved.background.clone(),
            ));
            std::hint::black_box((styles.anchor, styles.card));
        }
        let stats = region.change();
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.reallocations, 0);
    }

    #[test]
    fn card_position_requires_percentages_and_survives_export() {
        for position in [
            r#"["50%", "50%", "50%", "50%"]"#,
            r#"["0%", "0%", "100%", "0%"]"#,
        ] {
            let config = crate::config::Config::parse(&format!(
                "[window.card]\nposition_mode = 'screen'\nposition = {position}"
            ))
            .unwrap();
            config.validate().unwrap();
            let restored = crate::config::Config::parse(&config.to_toml().unwrap()).unwrap();
            assert_eq!(config.window.card, restored.window.card);
        }
        for position in [
            r#"["50", "50%", "50%", "50%"]"#,
            r#"["101%", "0%", "0%", "0%"]"#,
            r#"["60%", "0%", "50%", "0%"]"#,
            r#"["NaN%", "0%", "0%", "0%"]"#,
        ] {
            let config =
                crate::config::Config::parse(&format!("[window.card]\nposition = {position}"))
                    .unwrap();
            assert!(config.validate().is_err());
        }
        assert!(crate::config::Config::parse("[window.card]\nposition = [50,50,50,50]").is_err());
    }

    #[test]
    fn theme_selection_reuses_styles_and_new_configuration_recompiles() {
        let config = crate::config::Config::default();
        let light = config.palette(Appearance::Light);
        let dark = config.palette(Appearance::Dark);
        let styles = WindowStyles::new(
            &config.window.ui,
            &config.window.card,
            &light,
            &dark,
            crate::presentation::window::RENDERERS,
        );
        for (appearance, palette) in [(Appearance::Light, &light), (Appearance::Dark, &dark)] {
            let first = styles.for_appearance(appearance);
            let again = styles.for_appearance(appearance);
            assert!(first.app.ptr_eq(&again.app));
            assert_eq!(first.app.text_color, palette.text);
        }
        let mut changed = config.window.card.clone();
        changed.app_font_size = 35.0;
        let replacement = WindowStyles::new(
            &config.window.ui,
            &changed,
            &light,
            &dark,
            crate::presentation::window::RENDERERS,
        );
        assert_eq!(
            replacement.for_appearance(Appearance::Light).app.font_size,
            35.0
        );
        assert_ne!(styles.for_appearance(Appearance::Light).app.font_size, 35.0);
    }
}

impl ResolvedWindowStyle {
    fn new(ui: &LabelUi, card: &WindowCardUi, palette: &Palette) -> Self {
        let style: SharedLabelStyle = ui
            .resolve(
                palette,
                palette.surface_label(),
                palette.text,
                palette.accent,
            )
            .into();
        let base = style.clone();
        let inherited_text = style.text_color;
        let mut card_style = (*style).clone();
        card_style.background = crate::api::style::resolve(
            card.background_color.as_ref(),
            palette.appearance,
            style.background,
        );
        card_style.border_color = crate::api::style::resolve(
            card.border_color.as_ref(),
            palette.appearance,
            style.border_color,
        );
        card_style.text_color = crate::api::style::resolve(
            card.number_color.as_ref(),
            palette.appearance,
            inherited_text,
        );
        let style: SharedLabelStyle = card_style.into();
        let mut text_style = (*style).clone();
        text_style.font_size = if card.app_font_size > 0.0 {
            card.app_font_size
        } else {
            (style.font_size * 0.6).max(14.0)
        };
        text_style.bold = card.app_bold;
        text_style.text_color =
            crate::api::style::resolve(card.app_color.as_ref(), palette.appearance, inherited_text);
        if !card.app_font_family.is_empty() {
            text_style.font_family.clone_from(&card.app_font_family);
        }
        text_style.text_alignment = TextAlignment::Left;
        text_style.background = Color::TRANSPARENT;
        text_style.border_color = Color::TRANSPARENT;
        text_style.border_width = 0.0;
        text_style.padding_x = 0.0;
        text_style.padding_y = 0.0;
        let small: SharedLabelStyle = text_style.clone().into();
        text_style.bold = card.title_bold;
        text_style.text_color = crate::api::style::resolve(
            card.title_color.as_ref(),
            palette.appearance,
            inherited_text,
        );
        text_style
            .font_family
            .clone_from(if card.title_font_family.is_empty() {
                &style.font_family
            } else {
                &card.title_font_family
            });
        text_style.font_size = if card.title_font_size > 0.0 {
            card.title_font_size
        } else {
            (style.font_size * 0.45).max(12.0)
        };
        let title_style: SharedLabelStyle = text_style.into();

        let row_height = small.font_size.max(title_style.font_size) * card.line_height;
        let min_height = (style.font_size * 1.4 + style.padding_y * 2.0)
            .max(row_height * 2.0 + card.padding_y * 2.0)
            .max(card.min_height);
        let background = LabelStyle {
            font_size: 1.0,
            ..(*style).clone()
        }
        .into();
        Self {
            guide_line: (card.guide_line_enabled && card.guide_line_width > 0.0).then(|| {
                crate::api::overlay::LabelConnectorStyle {
                    width: card.guide_line_width,
                    color: resolve(
                        card.guide_line_color.as_ref(),
                        palette.appearance,
                        style.border_color,
                    ),
                }
            }),
            base,
            number: style,
            background,
            app: small,
            title: title_style,
            row_height,
            min_height,
        }
    }
}

impl<C: ColorValue> Default for ModeIndicatorEntry<C> {
    fn default() -> Self {
        Self {
            enabled: None,
            text: None,
            cursor: Default::default(),
            ui: Default::default(),
        }
    }
}

impl<C: ColorValue> Default for CursorIndicatorOverride<C> {
    fn default() -> Self {
        Self {
            enabled: None,
            radius: None,
            fill_color: None,
            stroke_color: None,
            left_pressed_color: None,
            middle_pressed_color: None,
            right_pressed_color: None,
            stroke_width: None,
        }
    }
}

impl<C: ColorValue> Default for IndicatorUiOverride<C> {
    fn default() -> Self {
        Self {
            indicator_offset: None,
            font_size: None,
            font_family: None,
            border_radius: None,
            padding_x: None,
            padding_y: None,
            border_width: None,
            background_color: None,
            text_color: None,
            matched_text_color: None,
            border_color: None,
        }
    }
}

impl ModeIndicator {
    pub fn compile(&self) -> ModeIndicator<CompiledColor> {
        ModeIndicator {
            cursor: self.cursor.compile(),
            ui: self.ui.compile(),
            modes: self
                .modes
                .iter()
                .map(|(id, entry)| (id.clone(), entry.compile()))
                .collect(),
        }
    }
}

impl ModeIndicatorEntry {
    pub fn compile(&self) -> ModeIndicatorEntry<CompiledColor> {
        ModeIndicatorEntry {
            enabled: self.enabled,
            text: self.text.clone(),
            cursor: self.cursor.compile(),
            ui: self.ui.compile(),
        }
    }
}

impl LabelUi {
    pub fn compile(&self) -> LabelUi<CompiledColor> {
        LabelUi {
            font_size: self.font_size,
            font_family: self.font_family.clone(),
            border_radius: self.border_radius,
            padding_x: self.padding_x,
            padding_y: self.padding_y,
            border_width: self.border_width,
            background_color: self.background_color.as_ref().map(CompiledColor::from),
            text_color: self.text_color.as_ref().map(CompiledColor::from),
            matched_text_color: self.matched_text_color.as_ref().map(CompiledColor::from),
            border_color: self.border_color.as_ref().map(CompiledColor::from),
        }
    }
}

impl BoundaryHighlight {
    pub fn compile(&self) -> BoundaryHighlight<CompiledColor> {
        BoundaryHighlight {
            enabled: self.enabled,
            border_width: self.border_width,
            border_radius: self.border_radius,
            background_color: self.background_color.as_ref().map(CompiledColor::from),
            border_color: self.border_color.as_ref().map(CompiledColor::from),
        }
    }
}

impl IndicatorUi {
    pub fn compile(&self) -> IndicatorUi<CompiledColor> {
        IndicatorUi {
            label: self.label.compile(),
            indicator_offset: self.indicator_offset,
        }
    }
}

impl CursorIndicatorUi {
    pub fn compile(&self) -> CursorIndicatorUi<CompiledColor> {
        CursorIndicatorUi {
            enabled: self.enabled,
            radius: self.radius,
            fill_color: self.fill_color.as_ref().map(CompiledColor::from),
            stroke_color: self.stroke_color.as_ref().map(CompiledColor::from),
            left_pressed_color: self.left_pressed_color.as_ref().map(CompiledColor::from),
            middle_pressed_color: self.middle_pressed_color.as_ref().map(CompiledColor::from),
            right_pressed_color: self.right_pressed_color.as_ref().map(CompiledColor::from),
            stroke_width: self.stroke_width,
        }
    }
}

impl CursorIndicatorOverride {
    pub fn compile(&self) -> CursorIndicatorOverride<CompiledColor> {
        CursorIndicatorOverride {
            enabled: self.enabled,
            radius: self.radius,
            fill_color: self.fill_color.as_ref().map(CompiledColor::from),
            stroke_color: self.stroke_color.as_ref().map(CompiledColor::from),
            left_pressed_color: self.left_pressed_color.as_ref().map(CompiledColor::from),
            middle_pressed_color: self.middle_pressed_color.as_ref().map(CompiledColor::from),
            right_pressed_color: self.right_pressed_color.as_ref().map(CompiledColor::from),
            stroke_width: self.stroke_width,
        }
    }
}

impl IndicatorUiOverride {
    pub fn compile(&self) -> IndicatorUiOverride<CompiledColor> {
        IndicatorUiOverride {
            indicator_offset: self.indicator_offset,
            font_size: self.font_size,
            font_family: self.font_family.clone(),
            border_radius: self.border_radius,
            padding_x: self.padding_x,
            padding_y: self.padding_y,
            border_width: self.border_width,
            background_color: self.background_color.as_ref().map(CompiledColor::from),
            text_color: self.text_color.as_ref().map(CompiledColor::from),
            matched_text_color: self.matched_text_color.as_ref().map(CompiledColor::from),
            border_color: self.border_color.as_ref().map(CompiledColor::from),
        }
    }
}

impl KeyHelp {
    pub fn compile(&self) -> KeyHelp<CompiledColor> {
        KeyHelp {
            mouse_key_help: self.mouse_key_help,
            window_key_help: self.window_key_help,
            font_family: self.font_family.clone(),
            font_size: self.font_size,
            background_color: self.background_color.as_ref().map(CompiledColor::from),
            text_color: self.text_color.as_ref().map(CompiledColor::from),
            border_color: self.border_color.as_ref().map(CompiledColor::from),
            border_width: self.border_width,
            border_radius: self.border_radius,
            padding_x: self.padding_x,
            padding_y: self.padding_y,
        }
    }
}

/// Concrete runtime styles; source-color forms stay at the configuration boundary.
pub mod compiled {
    use super::CompiledColor;
    pub type ModeIndicator = super::ModeIndicator<CompiledColor>;
    pub type LabelUi = super::LabelUi<CompiledColor>;
    pub type BoundaryHighlight = super::BoundaryHighlight<CompiledColor>;
    pub type IndicatorUi = super::IndicatorUi<CompiledColor>;
    pub type KeyHelp = super::KeyHelp<CompiledColor>;
    pub(crate) type ResolvedCursorIndicatorUi<'a> =
        super::ResolvedCursorIndicatorUi<'a, CompiledColor>;
}
