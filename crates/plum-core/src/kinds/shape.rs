use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::element::{ElementBehavior, Style};
use crate::geom::{Rect, dist_to_polygon_outline, point_in_polygon};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Diamond,
    Triangle,
}

/// A closed shape centered on the element origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub shape: ShapeKind,
    pub size: Vec2,
}

impl Shape {
    /// Outline polygon in local space. Ellipses are approximated with `segments` points.
    pub fn outline(&self, segments: usize) -> Vec<Vec2> {
        let h = self.size.abs() * 0.5;
        match self.shape {
            ShapeKind::Rect => vec![Vec2::new(-h.x, -h.y), Vec2::new(h.x, -h.y), Vec2::new(h.x, h.y), Vec2::new(-h.x, h.y)],
            ShapeKind::Diamond => vec![Vec2::new(0.0, -h.y), Vec2::new(h.x, 0.0), Vec2::new(0.0, h.y), Vec2::new(-h.x, 0.0)],
            ShapeKind::Triangle => vec![Vec2::new(0.0, -h.y), Vec2::new(h.x, h.y), Vec2::new(-h.x, h.y)],
            ShapeKind::Ellipse => (0..segments.max(8))
                .map(|i| {
                    let a = i as f32 / segments.max(8) as f32 * std::f32::consts::TAU;
                    Vec2::new(a.cos() * h.x, a.sin() * h.y)
                })
                .collect(),
        }
    }
}

impl ElementBehavior for Shape {
    fn local_bounds(&self, style: &Style) -> Rect {
        Rect::from_center_size(Vec2::ZERO, self.size).expand(style.width * 0.5)
    }

    /// The whole interior is clickable, filled or not.
    fn hit_test_local(&self, p: Vec2, style: &Style, tol: f32) -> bool {
        let reach = style.width * 0.5 + tol;
        let poly = self.outline(64);
        point_in_polygon(p, &poly) || dist_to_polygon_outline(p, &poly) <= reach
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfilled_interior_is_clickable() {
        let s = Shape { shape: ShapeKind::Ellipse, size: Vec2::new(100.0, 50.0) };
        let st = Style { fill: None, ..Default::default() };
        assert!(s.hit_test_local(Vec2::ZERO, &st, 0.0));
        assert!(!s.hit_test_local(Vec2::new(60.0, 0.0), &st, 1.0));
    }
}
