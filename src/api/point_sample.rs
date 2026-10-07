//! Small asynchronous screen sampling protocol, shared by modes and backends.
use super::{Color, Point};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Request {
    pub id: u64,
    pub point: Point,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub request: Request,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorFormat {
    Hex,
    Rgb,
    Hsl,
}

impl ColorFormat {
    pub fn write(self, out: &mut impl std::fmt::Write, color: Color) -> std::fmt::Result {
        match self {
            Self::Hex => write!(out, "#{:02X}{:02X}{:02X}", color.r, color.g, color.b),
            Self::Rgb => write!(out, "rgb({}, {}, {})", color.r, color.g, color.b),
            Self::Hsl => {
                let [r, g, b] = [color.r, color.g, color.b].map(|c| f64::from(c) / 255.0);
                let max = r.max(g).max(b);
                let min = r.min(g).min(b);
                let delta = max - min;
                let light = (max + min) / 2.0;
                let (hue, saturation) = if delta == 0.0 {
                    (0.0, 0.0)
                } else {
                    let hue = if max == r {
                        ((g - b) / delta).rem_euclid(6.0)
                    } else if max == g {
                        (b - r) / delta + 2.0
                    } else {
                        (r - g) / delta + 4.0
                    };
                    (hue * 60.0, delta / (1.0 - (2.0 * light - 1.0).abs()))
                };
                write!(
                    out,
                    "hsl({:.0}, {:.0}%, {:.0}%)",
                    hue.round() % 360.0,
                    saturation * 100.0,
                    light * 100.0
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    PointToggle,
    PointNext,
    ColorNext,
}

pub type PromptKeys = std::sync::Arc<[(super::KeyChord, Action)]>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldMode {
    Concat,
    Switch,
}

pub const DEFAULT_FIELD_MODES: [FieldMode; 4] = [
    FieldMode::Concat,
    FieldMode::Concat,
    FieldMode::Switch,
    FieldMode::Switch,
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_formats_cover_primaries_gray_and_hue_wrap() {
        for (color, hex, rgb, hsl) in [
            (
                Color::rgb(255, 0, 0),
                "#FF0000",
                "rgb(255, 0, 0)",
                "hsl(0, 100%, 50%)",
            ),
            (
                Color::rgb(0, 255, 0),
                "#00FF00",
                "rgb(0, 255, 0)",
                "hsl(120, 100%, 50%)",
            ),
            (
                Color::rgb(0, 0, 255),
                "#0000FF",
                "rgb(0, 0, 255)",
                "hsl(240, 100%, 50%)",
            ),
            (
                Color::rgb(128, 128, 128),
                "#808080",
                "rgb(128, 128, 128)",
                "hsl(0, 0%, 50%)",
            ),
            (
                Color::rgb(0, 0, 0),
                "#000000",
                "rgb(0, 0, 0)",
                "hsl(0, 0%, 0%)",
            ),
            (
                Color::rgb(255, 255, 255),
                "#FFFFFF",
                "rgb(255, 255, 255)",
                "hsl(0, 0%, 100%)",
            ),
            (
                Color::rgb(255, 0, 1),
                "#FF0001",
                "rgb(255, 0, 1)",
                "hsl(0, 100%, 50%)",
            ),
        ] {
            for (format, expected) in [
                (ColorFormat::Hex, hex),
                (ColorFormat::Rgb, rgb),
                (ColorFormat::Hsl, hsl),
            ] {
                let mut value = String::new();
                format.write(&mut value, color).unwrap();
                assert_eq!(value, expected);
            }
        }
    }
}
