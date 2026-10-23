//! Geometry primitives.
//!
//! Two coordinate spaces exist:
//! * **local** (`f32`, [`Rect`]): element geometry relative to the element origin.
//! * **world** (`f64`, [`Aabb`], [`Obb`]): the infinite canvas. `f64` keeps
//!   precision far from the origin.

use glam::{DAffine2, DVec2, Vec2};
use serde::{Deserialize, Serialize};

/// Axis-aligned rectangle in local (`f32`) space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    pub const EMPTY: Rect = Rect { min: Vec2::splat(f32::INFINITY), max: Vec2::splat(f32::NEG_INFINITY) };

    pub fn new(min: Vec2, max: Vec2) -> Self {
        Self { min: min.min(max), max: min.max(max) }
    }

    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        let h = size.abs() * 0.5;
        Self { min: center - h, max: center + h }
    }

    pub fn from_points(points: impl IntoIterator<Item = Vec2>) -> Self {
        points.into_iter().fold(Self::EMPTY, |r, p| Self { min: r.min.min(p), max: r.max.max(p) })
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn expand(&self, by: f32) -> Self {
        Self { min: self.min - by, max: self.max + by }
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn corners(&self) -> [Vec2; 4] {
        [self.min, Vec2::new(self.max.x, self.min.y), self.max, Vec2::new(self.min.x, self.max.y)]
    }
}

/// Axis-aligned bounding box in world (`f64`) space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: DVec2,
    pub max: DVec2,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { min: DVec2::splat(f64::INFINITY), max: DVec2::splat(f64::NEG_INFINITY) };

    pub fn new(min: DVec2, max: DVec2) -> Self {
        Self { min: min.min(max), max: min.max(max) }
    }

    pub fn from_point(p: DVec2, radius: f64) -> Self {
        Self { min: p - radius, max: p + radius }
    }

    pub fn from_points(points: impl IntoIterator<Item = DVec2>) -> Self {
        points.into_iter().fold(Self::EMPTY, |r, p| Self { min: r.min.min(p), max: r.max.max(p) })
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }

    pub fn expand(&self, by: f64) -> Self {
        Self { min: self.min - by, max: self.max + by }
    }

    pub fn center(&self) -> DVec2 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> DVec2 {
        self.max - self.min
    }

    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min.x <= o.max.x && self.max.x >= o.min.x && self.min.y <= o.max.y && self.max.y >= o.min.y
    }

    pub fn contains_aabb(&self, o: &Aabb) -> bool {
        o.min.x >= self.min.x && o.max.x <= self.max.x && o.min.y >= self.min.y && o.max.y <= self.max.y
    }

    /// World AABB of a local rect under an affine transform.
    pub fn from_transformed_rect(r: &Rect, affine: &DAffine2) -> Self {
        Self::from_points(r.corners().map(|c| affine.transform_point2(c.as_dvec2())))
    }
}

/// Oriented box in world space; used for selection gizmos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obb {
    pub center: DVec2,
    /// Half extents along the box's own axes (always positive).
    pub half: DVec2,
    pub rotation: f64,
}

impl Obb {
    pub fn from_aabb(a: &Aabb) -> Self {
        Self { center: a.center(), half: a.size() * 0.5, rotation: 0.0 }
    }

    pub fn axes(&self) -> (DVec2, DVec2) {
        let x = DVec2::from_angle(self.rotation);
        (x, x.perp())
    }

    /// Map a point given in box-frame coordinates (-half..half) to world.
    pub fn to_world(&self, frame: DVec2) -> DVec2 {
        let (ax, ay) = self.axes();
        self.center + ax * frame.x + ay * frame.y
    }

    /// Map a world point into box-frame coordinates.
    pub fn to_frame(&self, world: DVec2) -> DVec2 {
        let (ax, ay) = self.axes();
        let d = world - self.center;
        DVec2::new(d.dot(ax), d.dot(ay))
    }

    /// Corners in order: top-left, top-right, bottom-right, bottom-left.
    pub fn corners(&self) -> [DVec2; 4] {
        let h = self.half;
        [
            self.to_world(DVec2::new(-h.x, -h.y)),
            self.to_world(DVec2::new(h.x, -h.y)),
            self.to_world(DVec2::new(h.x, h.y)),
            self.to_world(DVec2::new(-h.x, h.y)),
        ]
    }

    pub fn contains(&self, world: DVec2) -> bool {
        let f = self.to_frame(world);
        f.x.abs() <= self.half.x && f.y.abs() <= self.half.y
    }
}

/// Distance from `p` to segment `a`-`b`.
pub fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= f32::EPSILON {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Even-odd point-in-polygon test.
pub fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Distance from `p` to the closed outline of `poly`.
pub fn dist_to_polygon_outline(p: Vec2, poly: &[Vec2]) -> f32 {
    (0..poly.len())
        .map(|i| dist_to_segment(p, poly[i], poly[(i + 1) % poly.len()]))
        .fold(f32::INFINITY, f32::min)
}

/// Light low-pass filter: `passes` rounds of a [1, 2, 1] / 4 kernel over the
/// interior points (endpoints stay put). Irons out input jitter while
/// barely shrinking curves sampled densely.
pub fn smooth_polyline(points: &[Vec2], passes: usize) -> Vec<Vec2> {
    let mut pts = points.to_vec();
    if pts.len() < 3 {
        return pts;
    }
    let mut prev = pts.clone();
    for _ in 0..passes {
        prev.copy_from_slice(&pts);
        for i in 1..pts.len() - 1 {
            pts[i] = (prev[i - 1] + prev[i] * 2.0 + prev[i + 1]) * 0.25;
        }
    }
    pts
}

/// Ramer–Douglas–Peucker polyline simplification.
pub fn simplify_rdp(points: &[Vec2], epsilon: f32) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((s, e)) = stack.pop() {
        let (mut idx, mut max) = (0, 0.0f32);
        for i in s + 1..e {
            let d = dist_to_segment(points[i], points[s], points[e]);
            if d > max {
                idx = i;
                max = d;
            }
        }
        if max > epsilon {
            keep[idx] = true;
            stack.push((s, idx));
            stack.push((idx, e));
        }
    }
    points.iter().zip(keep).filter_map(|(p, k)| k.then_some(*p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdp_keeps_endpoints_and_drops_collinear() {
        let pts: Vec<Vec2> = (0..10).map(|i| Vec2::new(i as f32, 0.0)).collect();
        assert_eq!(simplify_rdp(&pts, 0.1), vec![Vec2::ZERO, Vec2::new(9.0, 0.0)]);
    }

    #[test]
    fn obb_round_trip() {
        let o = Obb { center: DVec2::new(5.0, -3.0), half: DVec2::new(2.0, 1.0), rotation: 0.7 };
        let p = DVec2::new(1.25, -0.5);
        assert!((o.to_frame(o.to_world(p)) - p).length() < 1e-9);
    }

    #[test]
    fn polygon_contains() {
        let sq = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y];
        assert!(point_in_polygon(Vec2::splat(0.5), &sq));
        assert!(!point_in_polygon(Vec2::splat(1.5), &sq));
    }
}
