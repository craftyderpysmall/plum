//! R-tree over element world bounds: viewport culling and hit-test
//! candidate lookup in `O(log n)` instead of scanning the whole board.

use std::collections::HashMap;

use glam::DVec2;
use rstar::{AABB, RTree, RTreeObject};

use crate::element::ElementId;
use crate::geom::Aabb;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Entry {
    id: ElementId,
    min: [f64; 2],
    max: [f64; 2],
}

impl RTreeObject for Entry {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(self.min, self.max)
    }
}

impl Entry {
    fn new(id: ElementId, a: &Aabb) -> Self {
        Self { id, min: a.min.to_array(), max: a.max.to_array() }
    }
}

fn envelope(a: &Aabb) -> AABB<[f64; 2]> {
    AABB::from_corners(a.min.to_array(), a.max.to_array())
}

#[derive(Default)]
pub struct SpatialIndex {
    tree: RTree<Entry>,
    current: HashMap<ElementId, Entry>,
}

impl SpatialIndex {
    pub fn bulk(items: impl IntoIterator<Item = (ElementId, Aabb)>) -> Self {
        let entries: Vec<Entry> = items.into_iter().map(|(id, a)| Entry::new(id, &a)).collect();
        let current = entries.iter().map(|e| (e.id, *e)).collect();
        Self { tree: RTree::bulk_load(entries), current }
    }


    pub fn upsert(&mut self, id: ElementId, aabb: Aabb) {
        let e = Entry::new(id, &aabb);
        if let Some(old) = self.current.insert(id, e) {
            if old == e {
                return;
            }
            self.tree.remove(&old);
        }
        self.tree.insert(e);
    }

    pub fn remove(&mut self, id: ElementId) {
        if let Some(old) = self.current.remove(&id) {
            self.tree.remove(&old);
        }
    }

    pub fn get(&self, id: ElementId) -> Option<Aabb> {
        self.current.get(&id).map(|e| Aabb { min: DVec2::from(e.min), max: DVec2::from(e.max) })
    }

    /// Ids whose bounds intersect `area` (unordered).
    pub fn query(&self, area: &Aabb) -> impl Iterator<Item = ElementId> + '_ {
        self.tree.locate_in_envelope_intersecting(envelope(area)).map(|e| e.id)
    }

    /// Ids whose bounds lie fully inside `area` (unordered).
    pub fn query_contained(&self, area: &Aabb) -> impl Iterator<Item = ElementId> + '_ {
        self.tree.locate_in_envelope(envelope(area)).map(|e| e.id)
    }
}
