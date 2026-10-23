use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::element::{ElementBehavior, Style};
use crate::geom::{Rect, dist_to_segment};

/// Straight line or arrow between two local points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub a: Vec2,
    pub b: Vec2,
    pub arrow_start: bool,
    pub arrow_end: bool,
}

impl Line {
    /// Arrowhead length for a given stroke width.
    pub fn head_len(width: f32) -> f32 {
        (width * 4.0).max(10.0)
    }
}

impl ElementBehavior for Line {
    fn local_bounds(&self, style: &Style) -> Rect {
        let pad = if self.arrow_start || self.arrow_end { Self::head_len(style.width) * 0.6 } else { 0.0 };
        Rect::new(self.a, self.b).expand(style.width * 0.5 + pad)
    }

    fn hit_test_local(&self, p: Vec2, style: &Style, tol: f32) -> bool {
        dist_to_segment(p, self.a, self.b) <= style.width * 0.5 + tol
    }
}
