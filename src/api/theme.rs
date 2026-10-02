//! Theme palettes and source/numeric color representations.

use serde::{Deserialize, Serialize};

use super::{Appearance, Color};

/// A color that may differ between light and dark appearance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemedColor {
    Both(String),
    PerAppearance { light: String, dark: String },
}

impl ThemedColor {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Both(value) => Color::parse(value).is_some(),
            Self::PerAppearance { light, dark } => {
                Color::parse(light).is_some() && Color::parse(dark).is_some()
            }
        }
    }

    pub fn resolve(&self, appearance: Appearance) -> Option<Color> {
        let raw = match (self, appearance) {
            (Self::Both(value), _) => value,
            (Self::PerAppearance { light, .. }, Appearance::Light) => light,
            (Self::PerAppearance { dark, .. }, Appearance::Dark) => dark,
        };
        Color::parse(raw)
    }
}

/// A style's color representation: source text while configuring, numeric
/// values after compilation. Runtime styles use CompiledColor exclusively.
pub trait ColorValue: Clone + From<ThemedColor> {
    fn resolve(&self, appearance: Appearance) -> Option<Color>;
}

impl ColorValue for ThemedColor {
    fn resolve(&self, appearance: Appearance) -> Option<Color> {
        self.resolve(appearance)
    }
}

/// Both appearances parsed once. Invalid programmatic values retain the same
/// per-appearance fallback as source colors; validated configs contain Some.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledColor {
    light: Option<Color>,
    dark: Option<Color>,
}

impl From<&ThemedColor> for CompiledColor {
    fn from(source: &ThemedColor) -> Self {
        match source {
            ThemedColor::Both(value) => {
                let color = Color::parse(value);
                Self {
                    light: color,
                    dark: color,
                }
            }
            ThemedColor::PerAppearance { light, dark } => Self {
                light: Color::parse(light),
                dark: Color::parse(dark),
            },
        }
    }
}

impl From<ThemedColor> for CompiledColor {
    fn from(source: ThemedColor) -> Self {
        Self::from(&source)
    }
}

impl CompiledColor {
    pub fn resolve(&self, appearance: Appearance) -> Option<Color> {
        match appearance {
            Appearance::Light => self.light,
            Appearance::Dark => self.dark,
        }
    }
}

impl ColorValue for CompiledColor {
    fn resolve(&self, appearance: Appearance) -> Option<Color> {
        self.resolve(appearance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_colors_preserve_alpha_theme_and_invalid_value_fallbacks() {
        for source in [
            ThemedColor::Both("#00000000".into()),
            ThemedColor::Both(" #aAbBcC80 ".into()),
            ThemedColor::PerAppearance {
                light: "#123456FF".into(),
                dark: "#ABCDEF40".into(),
            },
            ThemedColor::PerAppearance {
                light: "#11223344".into(),
                dark: "invalid".into(),
            },
            ThemedColor::Both("a€aaaa".into()),
        ] {
            let compiled = CompiledColor::from(&source);
            for appearance in [Appearance::Light, Appearance::Dark] {
                assert_eq!(compiled.resolve(appearance), source.resolve(appearance));
            }
        }
        assert!(!std::mem::needs_drop::<CompiledColor>());
        assert!(std::mem::size_of::<Option<CompiledColor>>() <= 12);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub appearance: Appearance,
    pub surface: Color,
    pub accent: Color,
    pub accent_alt: Color,
    pub on_accent_alt: Color,
    pub text: Color,
}

impl Palette {
    pub fn surface_label(&self) -> Color {
        self.surface.with_opacity(0.95)
    }

    pub fn surface_cell(&self) -> Color {
        self.surface.with_opacity(0.70)
    }

    pub fn accent_border(&self) -> Color {
        self.accent.with_opacity(0.60)
    }

    pub fn highlight(&self) -> Color {
        self.accent_alt.with_opacity(0.30)
    }

    pub fn readable_on(&self, background: Color) -> Color {
        let contrast = |foreground: Color| {
            let (left, right) = (foreground.luminance(), background.luminance());
            (left.max(right) + 0.05) / (left.min(right) + 0.05)
        };
        if contrast(self.text) >= contrast(self.on_accent_alt) {
            self.text
        } else {
            self.on_accent_alt
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            appearance: Appearance::Dark,
            surface: Color::rgb(0x0A, 0x13, 0x38),
            accent: Color::rgb(0x6E, 0x82, 0xD6),
            accent_alt: Color::rgb(0x8F, 0xA2, 0xF0),
            on_accent_alt: Color::rgb(0x08, 0x10, 0x22),
            text: Color::rgb(0xE8, 0xEE, 0xFF),
        }
    }
}
