use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::element::{ElementBehavior, Style};
use crate::geom::Rect;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A text box. Its top-left corner is the element origin.
///
/// The core has no font engine, so `size` is the laid-out box measured by
/// the front-end and stored here so bounds/hit testing work headless.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub content: String,
    pub font_size: f32,
    pub align: TextAlign,
    pub size: Vec2,
}

impl ElementBehavior for Text {
    fn local_bounds(&self, _style: &Style) -> Rect {
        Rect::new(Vec2::ZERO, self.size.max(Vec2::splat(self.font_size * 0.5)))
    }

    fn hit_test_local(&self, p: Vec2, style: &Style, tol: f32) -> bool {
        self.local_bounds(style).expand(tol).contains(p)
    }
}
