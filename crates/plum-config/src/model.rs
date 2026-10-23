//! Typed configuration. Field names mirror the keys in `default.toml`.

use serde::{Deserialize, Deserializer};

/// `"#rrggbb"` / `"#rrggbbaa"` color, straight alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub [u8; 4]);

impl Color {
    pub fn parse(s: &str) -> Result<Self, String> {
        let hex = s.trim().strip_prefix('#').ok_or_else(|| format!("color {s:?} must start with '#'"))?;
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| format!("invalid color {s:?}"));
        match hex.len() {
            6 => Ok(Self([byte(0)?, byte(2)?, byte(4)?, 255])),
            8 => Ok(Self([byte(0)?, byte(2)?, byte(4)?, byte(6)?])),
            3 => {
                let n = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).map(|v| v * 17).map_err(|_| format!("invalid color {s:?}"));
                Ok(Self([n(0)?, n(1)?, n(2)?, 255]))
            }
            _ => Err(format!("color {s:?} must be #rgb, #rrggbb or #rrggbbaa")),
        }
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Color::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// A color or `"none"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptColor(pub Option<Color>);

impl<'de> Deserialize<'de> for OptColor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s.trim().eq_ignore_ascii_case("none") || s.trim().is_empty() {
            return Ok(Self(None));
        }
        Color::parse(&s).map(|c| Self(Some(c))).map_err(serde::de::Error::custom)
    }
}

/// A palette slot (`0` = first swatch, follows the theme) or a fixed `"#hex"` color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorRef {
    Palette(u8),
    Fixed(Color),
}

impl<'de> Deserialize<'de> for ColorRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Index(u8),
            Hex(String),
        }
        match Raw::deserialize(d)? {
            Raw::Index(i) => Ok(ColorRef::Palette(i)),
            Raw::Hex(s) => Color::parse(&s).map(ColorRef::Fixed).map_err(serde::de::Error::custom),
        }
    }
}

/// A [`ColorRef`] or `"none"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptColorRef(pub Option<ColorRef>);

impl<'de> Deserialize<'de> for OptColorRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Index(u8),
            Text(String),
        }
        Ok(OptColorRef(match Raw::deserialize(d)? {
            Raw::Index(i) => Some(ColorRef::Palette(i)),
            Raw::Text(s) if s.trim().is_empty() || s.trim().eq_ignore_ascii_case("none") => None,
            Raw::Text(s) => Some(ColorRef::Fixed(Color::parse(&s).map_err(serde::de::Error::custom)?)),
        }))
    }
}

/// One chord or a list of chords, e.g. `"Ctrl+Z"` or `["Delete", "Backspace"]`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Chords(pub Vec<String>);

impl<'de> Deserialize<'de> for Chords {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOrMany {
            One(String),
            Many(Vec<String>),
        }
        Ok(match OneOrMany::deserialize(d)? {
            OneOrMany::One(s) => Chords(vec![s]),
            OneOrMany::Many(v) => Chords(v),
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Config {
    pub theme: String,
    pub meta: Meta,
    pub colors: Colors,
    pub ui: Ui,
    pub grid: Grid,
    pub canvas: Canvas,
    pub palette: Palette,
    pub tools: Tools,
    pub keybinds: Keybinds,
    pub performance: Performance,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Meta {
    pub name: String,
    pub author: String,
    pub extends: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Colors {
    pub canvas: Color,
    pub grid: Color,
    pub grid_major: Color,
    pub selection: Color,
    pub selection_fill: Color,
    pub marquee: Color,
    pub marquee_fill: Color,
    pub hover: Color,
    pub handle_fill: Color,
    pub handle_stroke: Color,
    pub rotate_handle: Color,
    pub panel: Color,
    pub panel_border: Color,
    pub panel_shadow: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub button_hover: Color,
    pub placeholder: Color,
    pub error: Color,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolbarPosition {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Ui {
    pub toolbar_position: ToolbarPosition,
    pub button_size: f32,
    pub icon_size: f32,
    pub corner_radius: f32,
    pub spacing: f32,
    pub margin: f32,
    pub shadow: bool,
    pub font_size: f32,
    pub font_file: String,
    pub handle_size: f32,
    pub rotate_handle_distance: f32,
    pub show_context_bar: bool,
    pub show_style_panel: bool,
    pub animation_speed: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GridStyle {
    Dots,
    Lines,
    None,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Grid {
    pub style: GridStyle,
    pub spacing: f32,
    pub major_every: u32,
    pub min_screen_spacing: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Canvas {
    pub min_zoom: f64,
    pub max_zoom: f64,
    pub zoom_speed: f64,
    pub pan_speed: f64,
    pub rotate_snap_degrees: f64,
    pub nudge: f64,
    pub nudge_large: f64,
    pub hit_tolerance: f64,
    pub snap_to_grid: bool,
    pub zoom_overshoot: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Palette {
    pub colors: Vec<Color>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Tools {
    pub pen: PenTool,
    pub shape: ShapeTool,
    pub line: LineTool,
    pub arrow: LineTool,
    pub text: TextTool,
    pub bucket: BucketTool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PenTool {
    pub color: ColorRef,
    pub width: f32,
    pub smoothing: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ShapeTool {
    pub color: ColorRef,
    pub fill: OptColorRef,
    pub width: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct LineTool {
    pub color: ColorRef,
    pub width: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct TextTool {
    pub color: ColorRef,
    pub font_size: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct BucketTool {
    pub color: ColorRef,
}

/// Action name (as in `[keybinds]`) → chords. Names are defined by default.toml;
/// the app maps them to actions, so unknown names are caught as unknown keys.
pub type Keybinds = std::collections::BTreeMap<String, Chords>;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Performance {
    pub zoom_bucket_factor: f64,
    pub mesh_cache_frames: u32,
    pub lod_min_screen_size: f32,
    pub undo_limit: usize,
}
