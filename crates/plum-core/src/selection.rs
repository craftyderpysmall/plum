//! Selection set and group transform operations.
//!
//! Gestures apply ops to a snapshot taken when the drag started, so a long
//! drag never accumulates floating-point drift.

use std::collections::HashSet;

use glam::DVec2;

use crate::board::Board;
use crate::element::{Element, ElementId};
use crate::geom::Obb;
use crate::transform::{Transform, normalize_angle};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    ids: HashSet<ElementId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn contains(&self, id: ElementId) -> bool {
        self.ids.contains(&id)
    }

    pub fn clear(&mut self) {
        self.ids.clear();
    }

    pub fn set(&mut self, ids: impl IntoIterator<Item = ElementId>) {
        self.ids = ids.into_iter().collect();
    }


    pub fn toggle(&mut self, id: ElementId) {
        if !self.ids.remove(&id) {
            self.ids.insert(id);
        }
    }

    /// Drop ids that no longer exist on the board.
    pub fn retain_existing(&mut self, board: &Board) {
        self.ids.retain(|id| board.contains(*id));
    }

    /// Selected ids in z-order (back to front).
    pub fn ordered(&self, board: &Board) -> Vec<ElementId> {
        let mut v: Vec<(usize, ElementId)> = self.ids.iter().filter_map(|&id| board.z_index(id).map(|z| (z, id))).collect();
        v.sort_unstable();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// Selected elements by reference, back to front (no cloning).
    pub fn elements<'b>(&self, board: &'b Board) -> Vec<&'b Element> {
        self.ordered(board).into_iter().filter_map(|id| board.get(id)).collect()
    }

    /// Owned copies of the selected elements, back to front (for clipboard,
    /// duplicate, and before/after edits).
    pub fn snapshot(&self, board: &Board) -> Vec<Element> {
        self.elements(board).into_iter().cloned().collect()
    }

    /// `(id, transform)` of each selected element: all a move/rotate/scale gesture needs.
    pub fn transforms(&self, board: &Board) -> Vec<(ElementId, Transform)> {
        self.elements(board).into_iter().map(|e| (e.id, e.transform)).collect()
    }

    /// Gizmo box: the element's own oriented box for a single selection,
    /// the axis-aligned union for several. Doesn't allocate.
    pub fn obb(&self, board: &Board) -> Option<Obb> {
        group_obb(self.ids.iter().filter_map(|&id| board.get(id)))
    }
}

pub fn group_obb<'a>(elements: impl IntoIterator<Item = &'a Element>) -> Option<Obb> {
    let mut it = elements.into_iter();
    let first = it.next()?;
    let Some(second) = it.next() else { return Some(first.world_obb()) };
    let a = it.fold(first.world_aabb().union(&second.world_aabb()), |a, e| a.union(&e.world_aabb()));
    Some(Obb::from_aabb(&a))
}

/// A transform applied to every element of a group.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroupOp {
    Translate(DVec2),
    Rotate { pivot: DVec2, angle: f64 },
    /// Scale by `factor` along the axes of `frame`, keeping `anchor` (world) fixed.
    Scale { frame_rotation: f64, anchor: DVec2, factor: DVec2 },
    FlipH { pivot_x: f64 },
    FlipV { pivot_y: f64 },
}

impl GroupOp {
    /// The op applied to one transform; elements' geometry never changes.
    pub fn transform(&self, t: &Transform) -> Transform {
        let t = *t;
        match *self {
            GroupOp::Translate(d) => t.translated(d),
            GroupOp::Rotate { pivot, angle } => t.rotated_about(pivot, angle),
            GroupOp::FlipH { pivot_x } => t.flipped_h(pivot_x),
            GroupOp::FlipV { pivot_y } => t.flipped_v(pivot_y),
            GroupOp::Scale { frame_rotation, anchor, factor } => {
                let r = DVec2::from_angle(frame_rotation);
                let rel = r.rotate(DVec2::from_angle(-frame_rotation).rotate(t.pos - anchor) * factor);
                let mut nt = t;
                nt.pos = anchor + rel;
                // Element axes relative to the frame decide how the factor maps
                // onto its local scale; off-axis elements scale uniformly to avoid shear.
                let rel_angle = normalize_angle(t.rotation - frame_rotation).rem_euclid(std::f64::consts::PI);
                let local = if rel_angle < 1e-6 || (std::f64::consts::PI - rel_angle) < 1e-6 {
                    factor
                } else if (rel_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-6 {
                    DVec2::new(factor.y, factor.x)
                } else {
                    DVec2::splat((factor.x * factor.y).abs().sqrt())
                };
                nt.scale = t.scale * local;
                nt
            }
        }
    }

    /// Apply to `(id, before)` pairs on the board via the transform-only fast path.
    pub fn apply_to_board(&self, board: &mut Board, items: &[(ElementId, Transform)]) {
        for (id, t) in items {
            board.set_transform(*id, self.transform(t));
        }
    }
}

#[cfg(test)]
impl GroupOp {
    fn apply(&self, el: &Element) -> Element {
        let mut out = el.clone();
        out.transform = self.transform(&el.transform);
        out
    }

    fn apply_all(&self, els: &[Element]) -> Vec<Element> {
        els.iter().map(|e| self.apply(e)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{ElementKind, Style};
    use crate::kinds::{Shape, ShapeKind};
    use crate::transform::Transform;
    use glam::Vec2;

    fn rect_at(x: f64, y: f64, rot: f64) -> Element {
        Element::new(
            ElementKind::Shape(Shape { shape: ShapeKind::Rect, size: Vec2::new(20.0, 10.0) }),
            Transform { pos: DVec2::new(x, y), rotation: rot, scale: DVec2::ONE },
            Style { width: 0.0, ..Default::default() },
        )
    }

    fn world_corners(e: &Element) -> Vec<DVec2> {
        e.local_bounds().corners().iter().map(|c| e.transform.affine().transform_point2(c.as_dvec2())).collect()
    }

    #[test]
    fn group_flip_mirrors_geometry() {
        let els = vec![rect_at(0.0, 0.0, 0.3), rect_at(50.0, 10.0, -0.2)];
        let obb = group_obb(&els).unwrap();
        let flipped = GroupOp::FlipH { pivot_x: obb.center.x }.apply_all(&els);
        for (a, b) in els.iter().zip(&flipped) {
            for (p, q) in world_corners(a).iter().zip(world_corners(b)) {
                assert!((q - DVec2::new(2.0 * obb.center.x - p.x, p.y)).length() < 1e-6);
            }
        }
        // Flipping twice is identity.
        let back = GroupOp::FlipH { pivot_x: obb.center.x }.apply_all(&flipped);
        for (a, b) in els.iter().zip(&back) {
            for (p, q) in world_corners(a).iter().zip(world_corners(b)) {
                assert!((*p - q).length() < 1e-6);
            }
        }
    }

    #[test]
    fn group_rotate_keeps_relative_layout() {
        let els = vec![rect_at(0.0, 0.0, 0.0), rect_at(100.0, 0.0, 0.0)];
        let r = GroupOp::Rotate { pivot: DVec2::new(50.0, 0.0), angle: std::f64::consts::FRAC_PI_2 }.apply_all(&els);
        assert!((r[0].transform.pos - DVec2::new(50.0, -50.0)).length() < 1e-9);
        assert!((r[1].transform.pos - DVec2::new(50.0, 50.0)).length() < 1e-9);
    }

    #[test]
    fn scale_single_in_own_frame() {
        let e = rect_at(0.0, 0.0, 0.5);
        let obb = e.world_obb();
        let anchor = obb.to_world(DVec2::new(-obb.half.x, -obb.half.y));
        let s = GroupOp::Scale { frame_rotation: obb.rotation, anchor, factor: DVec2::new(2.0, 3.0) }.apply(&e);
        let o2 = s.world_obb();
        assert!((o2.half - DVec2::new(20.0, 15.0)).length() < 1e-6);
        // Anchor corner stays put.
        assert!((o2.to_world(DVec2::new(-o2.half.x, -o2.half.y)) - anchor).length() < 1e-6);
    }
}
