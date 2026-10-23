use std::collections::HashMap;

use glam::DVec2;

use crate::element::{Element, ElementId};
use crate::geom::Aabb;
use crate::spatial::SpatialIndex;

/// Board-level metadata, persisted in the file's META/VIEW chunks.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardMeta {
    pub name: String,
    /// Unix seconds.
    pub created: u64,
    pub modified: u64,
    /// Last camera, restored on open.
    pub view_center: DVec2,
    pub view_zoom: f64,
    /// File chunks this build does not understand, kept verbatim so a
    /// round-trip through an older Plum never loses data.
    pub opaque_chunks: Vec<([u8; 4], Vec<u8>)>,
}

impl Default for BoardMeta {
    fn default() -> Self {
        Self {
            name: "Untitled".into(),
            created: 0,
            modified: 0,
            view_center: DVec2::ZERO,
            view_zoom: 1.0,
            opaque_chunks: Vec::new(),
        }
    }
}

/// An infinite whiteboard: elements in z-order (back to front) plus a spatial index.
///
/// All mutation goes through methods so the id map and spatial index never drift.
#[derive(Default)]
pub struct Board {
    pub meta: BoardMeta,
    elements: Vec<Element>,
    index: HashMap<ElementId, usize>,
    spatial: SpatialIndex,
    next_id: u64,
}

impl Board {
    pub fn new() -> Self {
        Self { next_id: 1, ..Default::default() }
    }

    /// Build from loaded elements (keeps their ids; bulk-loads the index).
    pub fn from_elements(meta: BoardMeta, elements: Vec<Element>) -> Self {
        let mut b = Self { meta, elements, ..Default::default() };
        b.next_id = b.elements.iter().map(|e| e.id.0).max().unwrap_or(0) + 1;
        for e in b.elements.iter_mut().filter(|e| e.id.0 == 0) {
            e.id = ElementId(b.next_id);
            b.next_id += 1;
        }
        b.reindex();
        b.spatial = SpatialIndex::bulk(b.elements.iter().map(|e| (e.id, e.world_aabb())));
        b
    }

    pub fn alloc_id(&mut self) -> ElementId {
        let id = ElementId(self.next_id.max(1));
        self.next_id = id.0 + 1;
        id
    }

    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub fn get(&self, id: ElementId) -> Option<&Element> {
        self.index.get(&id).map(|&i| &self.elements[i])
    }

    pub fn z_index(&self, id: ElementId) -> Option<usize> {
        self.index.get(&id).copied()
    }

    pub fn contains(&self, id: ElementId) -> bool {
        self.index.contains_key(&id)
    }

    /// Append on top. Assigns a fresh id if `el.id` is 0.
    pub fn push(&mut self, el: Element) -> ElementId {
        let at = self.elements.len();
        self.insert_at(at, el)
    }

    pub fn insert_at(&mut self, at: usize, mut el: Element) -> ElementId {
        if el.id.0 == 0 || self.index.contains_key(&el.id) {
            el.id = self.alloc_id();
        } else {
            self.next_id = self.next_id.max(el.id.0 + 1);
        }
        let id = el.id;
        el.refresh_bounds();
        self.spatial.upsert(id, el.world_aabb());
        let at = at.min(self.elements.len());
        self.elements.insert(at, el);
        if at + 1 == self.elements.len() {
            self.index.insert(id, at);
        } else {
            self.reindex_from(at);
        }
        id
    }

    pub fn remove(&mut self, id: ElementId) -> Option<(usize, Element)> {
        let at = self.index.remove(&id)?;
        self.spatial.remove(id);
        let el = self.elements.remove(at);
        self.reindex_from(at);
        Some((at, el))
    }

    /// Replace an element (matched by id), keeping its z position.
    pub fn replace(&mut self, mut el: Element) -> Option<Element> {
        let &at = self.index.get(&el.id)?;
        let old = &self.elements[at];
        el.version = old.version;
        if old.kind != el.kind || old.style != el.style {
            el.touch();
        }
        self.spatial.upsert(el.id, el.world_aabb());
        Some(std::mem::replace(&mut self.elements[at], el))
    }

    /// Fast path for moves/rotations/flips: only the transform changes, so the
    /// mesh cache stays valid and nothing is cloned or rescanned.
    pub fn set_transform(&mut self, id: ElementId, t: crate::transform::Transform) -> bool {
        let Some(&at) = self.index.get(&id) else { return false };
        let el = &mut self.elements[at];
        if el.transform == t {
            return true;
        }
        el.transform = t;
        let aabb = el.world_aabb();
        self.spatial.upsert(id, aabb);
        true
    }

    /// Change only the style (color, width, opacity…).
    pub fn set_style(&mut self, id: ElementId, s: crate::element::Style) -> bool {
        let Some(&at) = self.index.get(&id) else { return false };
        let el = &mut self.elements[at];
        if el.style == s {
            return true;
        }
        el.style = s;
        el.touch();
        let aabb = el.world_aabb();
        self.spatial.upsert(id, aabb);
        true
    }

    /// Mutate in place; bounds (and version, if content changed) are refreshed afterwards.
    pub fn update(&mut self, id: ElementId, f: impl FnOnce(&mut Element)) -> bool {
        let Some(old) = self.get(id).cloned() else { return false };
        let mut el = old;
        f(&mut el);
        el.id = id;
        self.replace(el);
        true
    }

    /// Move an element to a new z position.
    pub fn move_z(&mut self, id: ElementId, to: usize) {
        let Some(&from) = self.index.get(&id) else { return };
        let to = to.min(self.elements.len() - 1);
        if from == to {
            return;
        }
        let el = self.elements.remove(from);
        self.elements.insert(to, el);
        self.reindex_from(from.min(to));
    }

    /// Ids intersecting `area`, back-to-front.
    pub fn query(&self, area: &Aabb) -> Vec<ElementId> {
        let mut idx: Vec<usize> = self.spatial.query(area).filter_map(|id| self.index.get(&id).copied()).collect();
        idx.sort_unstable();
        idx.into_iter().map(|i| self.elements[i].id).collect()
    }

    /// Ids fully inside `area`, back-to-front.
    pub fn query_contained(&self, area: &Aabb) -> Vec<ElementId> {
        let mut idx: Vec<usize> = self.spatial.query_contained(area).filter_map(|id| self.index.get(&id).copied()).collect();
        idx.sort_unstable();
        idx.into_iter().map(|i| self.elements[i].id).collect()
    }

    /// Visible elements (back-to-front) for rendering. Falls back to a linear
    /// scan when the area covers most of the board.
    pub fn visible(&self, area: &Aabb) -> Vec<&Element> {
        let hits: Vec<usize> = self.spatial.query(area).filter_map(|id| self.index.get(&id).copied()).collect();
        if hits.len() * 2 > self.elements.len() {
            return self.elements.iter().filter(|e| self.spatial.get(e.id).is_some_and(|a| a.intersects(area))).collect();
        }
        let mut hits = hits;
        hits.sort_unstable();
        hits.into_iter().map(|i| &self.elements[i]).collect()
    }

    /// Topmost element under a world point.
    pub fn hit(&self, p: DVec2, tol: f64) -> Option<ElementId> {
        let mut idx: Vec<usize> =
            self.spatial.query(&Aabb::from_point(p, tol)).filter_map(|id| self.index.get(&id).copied()).collect();
        idx.sort_unstable_by(|a, b| b.cmp(a));
        idx.into_iter().map(|i| &self.elements[i]).find(|e| e.hit_test(p, tol)).map(|e| e.id)
    }

    /// Union of all element bounds.
    pub fn bounds(&self) -> Option<Aabb> {
        let a = self.elements.iter().filter_map(|e| self.spatial.get(e.id)).fold(Aabb::EMPTY, |a, b| a.union(&b));
        (!a.is_empty()).then_some(a)
    }

    pub fn element_aabb(&self, id: ElementId) -> Option<Aabb> {
        self.spatial.get(id)
    }

    fn reindex(&mut self) {
        self.reindex_from(0);
    }

    fn reindex_from(&mut self, from: usize) {
        for (i, e) in self.elements.iter().enumerate().skip(from) {
            self.index.insert(e.id, i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{ElementKind, Style};
    use crate::kinds::{Shape, ShapeKind};
    use crate::transform::Transform;
    use glam::Vec2;

    fn square(x: f64, y: f64) -> Element {
        Element::new(
            ElementKind::Shape(Shape { shape: ShapeKind::Rect, size: Vec2::splat(10.0) }),
            Transform::at(DVec2::new(x, y)),
            Style { fill: Some(crate::Paint::BLACK), width: 0.0, ..Default::default() },
        )
    }

    #[test]
    fn query_matches_brute_force() {
        let mut b = Board::new();
        let mut seed = 7u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) * 2000.0 - 1000.0
        };
        for _ in 0..500 {
            b.push(square(rnd(), rnd()));
        }
        let area = Aabb::new(DVec2::new(-200.0, -300.0), DVec2::new(400.0, 100.0));
        let brute: Vec<ElementId> = b.elements().iter().filter(|e| e.world_aabb().intersects(&area)).map(|e| e.id).collect();
        assert_eq!(b.query(&area), brute);
        assert_eq!(b.visible(&area).iter().map(|e| e.id).collect::<Vec<_>>(), brute);
    }

    #[test]
    fn hit_picks_topmost_and_tracks_moves() {
        let mut b = Board::new();
        let a = b.push(square(0.0, 0.0));
        let c = b.push(square(2.0, 2.0));
        assert_eq!(b.hit(DVec2::new(1.0, 1.0), 0.5), Some(c));
        b.update(c, |e| e.transform.pos = DVec2::new(1000.0, 0.0));
        assert_eq!(b.hit(DVec2::new(1.0, 1.0), 0.5), Some(a));
        assert_eq!(b.hit(DVec2::new(1000.0, 0.0), 0.5), Some(c));
        b.remove(a);
        assert_eq!(b.hit(DVec2::new(1.0, 1.0), 0.5), None);
        assert_eq!(b.z_index(c), Some(0));
    }

    #[test]
    fn move_z_reorders() {
        let mut b = Board::new();
        let a = b.push(square(0.0, 0.0));
        let c = b.push(square(0.0, 0.0));
        b.move_z(a, 1);
        assert_eq!(b.elements()[1].id, a);
        assert_eq!(b.z_index(c), Some(0));
        assert_eq!(b.hit(DVec2::ZERO, 0.1), Some(a));
    }
}
