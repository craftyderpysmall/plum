use glam::{DVec2, Vec2};
use serde::{Deserialize, Serialize};

use crate::geom::{Aabb, Obb, Rect};
use crate::kinds::{Line, Shape, Stroke, Text, UnknownKind};
use crate::transform::Transform;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ElementId(pub u64);

/// Straight (non-premultiplied) sRGBA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);
}

/// A color as stored on an element.
///
/// `Palette` refers to a slot in the active theme's palette, so "ink" drawn in
/// a light theme stays readable after switching to a dark one. `Rgba` is a
/// fixed color that never changes with the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Paint {
    Rgba(Rgba),
    Palette(u8),
}

impl Paint {
    pub const BLACK: Paint = Paint::Rgba(Rgba::BLACK);
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub stroke: Paint,
    pub fill: Option<Paint>,
    pub width: f32,
    /// 0..=1, multiplies the alpha of stroke and fill.
    pub opacity: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self { stroke: Paint::BLACK, fill: None, width: 2.0, opacity: 1.0 }
    }
}

/// Behaviour every element kind implements. Geometry is in local space.
pub trait ElementBehavior {
    /// Tight local bounds, including stroke width.
    fn local_bounds(&self, style: &Style) -> Rect;
    /// `tol` is in local units.
    fn hit_test_local(&self, p: Vec2, style: &Style, tol: f32) -> bool;
}

#[derive(Clone, Debug, PartialEq)]
pub enum ElementKind {
    Stroke(Stroke),
    Shape(Shape),
    Line(Line),
    Text(Text),
    /// A kind from a newer file version; preserved verbatim, drawn as a placeholder.
    Unknown(UnknownKind),
}

impl ElementKind {
    fn behavior(&self) -> &dyn ElementBehavior {
        match self {
            ElementKind::Stroke(k) => k,
            ElementKind::Shape(k) => k,
            ElementKind::Line(k) => k,
            ElementKind::Text(k) => k,
            ElementKind::Unknown(k) => k,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ElementKind::Stroke(_) => "stroke",
            ElementKind::Shape(_) => "shape",
            ElementKind::Line(_) => "line",
            ElementKind::Text(_) => "text",
            ElementKind::Unknown(_) => "unknown",
        }
    }
}

/// A "selection entry": anything on the board that can be selected and transformed.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub id: ElementId,
    pub transform: Transform,
    pub style: Style,
    pub kind: ElementKind,
    /// Bumped when geometry or style changes (not on pure transform changes);
    /// renderers use it as a mesh cache key.
    pub version: u32,
    /// Cached `local_bounds` (a long stroke would otherwise rescan every
    /// point each time its bounds are needed). Refreshed by [`Self::touch`].
    bounds: Rect,
}

impl Element {
    pub fn new(kind: ElementKind, transform: Transform, style: Style) -> Self {
        let bounds = kind.behavior().local_bounds(&style);
        Self { id: ElementId(0), transform, style, kind, version: 0, bounds }
    }

    /// Call after changing `kind` or `style` directly: bumps the version and
    /// refreshes cached bounds. (Board methods do this for you.)
    pub fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
        self.refresh_bounds();
    }

    pub(crate) fn refresh_bounds(&mut self) {
        self.bounds = self.kind.behavior().local_bounds(&self.style);
    }

    pub fn local_bounds(&self) -> Rect {
        self.bounds
    }

    pub fn world_aabb(&self) -> Aabb {
        Aabb::from_transformed_rect(&self.local_bounds(), &self.transform.affine())
    }

    /// The element's bounds as an oriented box in world space.
    pub fn world_obb(&self) -> Obb {
        let r = self.local_bounds();
        let t = &self.transform;
        Obb {
            center: t.affine().transform_point2(r.center().as_dvec2()),
            half: r.size().as_dvec2() * 0.5 * t.scale.abs(),
            rotation: t.rotation,
        }
    }

    /// Is `world` inside a region the fill bucket can fill (a shape's interior
    /// or a closed freehand loop)?
    pub fn fill_contains(&self, world: DVec2) -> bool {
        let local = self.transform.affine().inverse().transform_point2(world).as_vec2();
        match &self.kind {
            ElementKind::Shape(s) => crate::geom::point_in_polygon(local, &s.outline(64)),
            ElementKind::Stroke(s) => s.is_closed(self.style.width) && crate::geom::point_in_polygon(local, &s.points),
            _ => false,
        }
    }

    /// Precise hit test; `tol` is in world units.
    pub fn hit_test(&self, world: DVec2, tol: f64) -> bool {
        let local = self.transform.affine().inverse().transform_point2(world).as_vec2();
        let tol_local = (tol / self.transform.min_abs_scale()) as f32;
        self.kind.behavior().hit_test_local(local, &self.style, tol_local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::ShapeKind;

    #[test]
    fn rotated_flipped_shape_hit_test() {
        let shape = ElementKind::Shape(Shape { shape: ShapeKind::Rect, size: Vec2::new(100.0, 20.0) });
        let style = Style { fill: Some(Paint::BLACK), ..Default::default() };
        let t = Transform { pos: DVec2::new(500.0, 500.0), rotation: std::f64::consts::FRAC_PI_2, scale: DVec2::new(-1.0, 1.0) };
        let el = Element::new(shape, t, style);
        // Rotated 90°: long axis is now vertical.
        assert!(el.hit_test(DVec2::new(500.0, 545.0), 0.5));
        assert!(!el.hit_test(DVec2::new(545.0, 500.0), 0.5));
    }

    #[test]
    fn world_aabb_rotated() {
        let shape = ElementKind::Shape(Shape { shape: ShapeKind::Rect, size: Vec2::new(10.0, 10.0) });
        let style = Style { width: 0.0, ..Default::default() };
        let t = Transform { rotation: std::f64::consts::FRAC_PI_4, ..Default::default() };
        let a = Element::new(shape, t, style).world_aabb();
        let d = 50f64.sqrt();
        assert!((a.max.x - d).abs() < 1e-4 && (a.min.y + d).abs() < 1e-4);
    }
}
