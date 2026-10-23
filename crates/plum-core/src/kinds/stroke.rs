use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::element::{ElementBehavior, Style};
use crate::geom::{Rect, dist_to_segment, point_in_polygon, simplify_rdp};

/// Freehand polyline, points relative to the element origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub points: Vec<Vec2>,
}

impl Stroke {
    /// Build from absolute points (already relative to `origin`'s frame):
    /// simplifies and re-centers so local coordinates stay small.
    /// Returns the stroke and the offset its origin moved by.
    pub fn finish(points: &[Vec2], simplify_eps: f32) -> (Self, Vec2) {
        let pts = if simplify_eps > 0.0 { simplify_rdp(points, simplify_eps) } else { points.to_vec() };
        let c = Rect::from_points(pts.iter().copied()).center();
        let c = if c.is_finite() { c } else { Vec2::ZERO };
        (Self { points: pts.into_iter().map(|p| p - c).collect() }, c)
    }

    /// Does the stroke end close to where it started (so it can be filled)?
    /// The gap may be up to 3× the stroke width or 15% of the stroke's size.
    pub fn is_closed(&self, width: f32) -> bool {
        let [first, .., last] = self.points.as_slice() else { return false };
        if self.points.len() < 3 {
            return false;
        }
        let diag = Rect::from_points(self.points.iter().copied()).size().length();
        first.distance(*last) <= (width * 3.0).max(diag * 0.15)
    }
}

impl ElementBehavior for Stroke {
    fn local_bounds(&self, style: &Style) -> Rect {
        let r = Rect::from_points(self.points.iter().copied());
        if r.is_empty() { Rect::new(Vec2::ZERO, Vec2::ZERO) } else { r.expand(style.width * 0.5) }
    }

    fn hit_test_local(&self, p: Vec2, style: &Style, tol: f32) -> bool {
        let reach = style.width * 0.5 + tol;
        // A filled loop is clickable anywhere inside, like a shape.
        if style.fill.is_some() && self.is_closed(style.width) && point_in_polygon(p, &self.points) {
            return true;
        }
        match self.points.as_slice() {
            [] => false,
            [a] => p.distance(*a) <= reach,
            pts => pts.windows(2).any(|w| dist_to_segment(p, w[0], w[1]) <= reach),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_recenters() {
        let (s, off) = Stroke::finish(&[Vec2::new(10.0, 10.0), Vec2::new(20.0, 30.0)], 0.0);
        assert_eq!(off, Vec2::new(15.0, 20.0));
        assert_eq!(s.points[0], Vec2::new(-5.0, -10.0));
    }

    #[test]
    fn closed_loops_fill_and_hit_inside() {
        let ring: Vec<Vec2> = (0..=40).map(|i| Vec2::from_angle(i as f32 / 40.0 * 6.0) * 50.0).collect();
        let open = Stroke { points: (0..20).map(|i| Vec2::from_angle(i as f32 / 20.0 * 3.0) * 50.0).collect() };
        let loop_ = Stroke { points: ring };
        assert!(loop_.is_closed(2.0));
        assert!(!open.is_closed(2.0));
        let filled = Style { fill: Some(crate::Paint::Palette(1)), ..Default::default() };
        assert!(loop_.hit_test_local(Vec2::ZERO, &filled, 1.0));
        assert!(!loop_.hit_test_local(Vec2::ZERO, &Style::default(), 1.0));
    }

    #[test]
    fn hit_near_segment() {
        let s = Stroke { points: vec![Vec2::ZERO, Vec2::new(10.0, 0.0)] };
        let st = Style { width: 2.0, ..Default::default() };
        assert!(s.hit_test_local(Vec2::new(5.0, 1.5), &st, 1.0));
        assert!(!s.hit_test_local(Vec2::new(5.0, 3.0), &st, 1.0));
    }
}
