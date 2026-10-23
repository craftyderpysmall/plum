//! Snapshot-based undo/redo. A [`Command`] is a list of reversible
//! [`Change`]s; only the affected elements are stored, and moves or restyles
//! store just the transform or style rather than a copy of the whole element.

use std::collections::VecDeque;

use crate::board::Board;
use crate::element::{Element, ElementId, Style};
use crate::transform::Transform;

#[derive(Clone, Debug)]
pub enum Change {
    Insert { at: usize, element: Element },
    Remove { at: usize, element: Element },
    Modify { before: Element, after: Element },
    /// Move/rotate/scale/flip: a few dozen bytes instead of a full element.
    Transform { id: ElementId, before: Transform, after: Transform },
    Style { id: ElementId, before: Style, after: Style },
    MoveZ { id: ElementId, from: usize, to: usize },
}

impl Change {
    fn apply(&self, board: &mut Board) {
        match self {
            Change::Insert { at, element } => {
                board.insert_at(*at, element.clone());
            }
            Change::Remove { element, .. } => {
                board.remove(element.id);
            }
            Change::Modify { after, .. } => {
                board.replace(after.clone());
            }
            Change::Transform { id, after, .. } => {
                board.set_transform(*id, *after);
            }
            Change::Style { id, after, .. } => {
                board.set_style(*id, *after);
            }
            Change::MoveZ { id, to, .. } => board.move_z(*id, *to),
        }
    }

    fn revert(&self, board: &mut Board) {
        match self {
            Change::Insert { element, .. } => {
                board.remove(element.id);
            }
            Change::Remove { at, element } => {
                board.insert_at(*at, element.clone());
            }
            Change::Modify { before, .. } => {
                board.replace(before.clone());
            }
            Change::Transform { id, before, .. } => {
                board.set_transform(*id, *before);
            }
            Change::Style { id, before, .. } => {
                board.set_style(*id, *before);
            }
            Change::MoveZ { id, from, .. } => board.move_z(*id, *from),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Command {
    pub label: &'static str,
    pub changes: Vec<Change>,
}

impl Command {
    pub fn new(label: &'static str) -> Self {
        Self { label, changes: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Build a "modify" command from before/after snapshots (matched by position).
    /// Each change is stored in its most compact form.
    pub fn modify(label: &'static str, before: &[Element], after: &[Element]) -> Self {
        let changes = before
            .iter()
            .zip(after)
            .filter_map(|(b, a)| {
                let same_kind = b.kind == a.kind;
                let same_style = b.style == a.style;
                let same_t = b.transform == a.transform;
                match (same_kind, same_style, same_t) {
                    (true, true, true) => None,
                    (true, true, false) => Some(Change::Transform { id: a.id, before: b.transform, after: a.transform }),
                    (true, false, true) => Some(Change::Style { id: a.id, before: b.style, after: a.style }),
                    _ => Some(Change::Modify { before: b.clone(), after: a.clone() }),
                }
            })
            .collect();
        Self { label, changes }
    }

    /// Transform-only command from `(id, before, after)` triples.
    pub fn transforms(label: &'static str, items: impl IntoIterator<Item = (ElementId, Transform, Transform)>) -> Self {
        let changes = items.into_iter().filter(|(_, b, a)| b != a).map(|(id, before, after)| Change::Transform { id, before, after }).collect();
        Self { label, changes }
    }

    /// Command removing `ids` (records z positions so undo restores order).
    pub fn remove(label: &'static str, board: &Board, ids: &[ElementId]) -> Self {
        let mut items: Vec<(usize, Element)> =
            ids.iter().filter_map(|&id| Some((board.z_index(id)?, board.get(id)?.clone()))).collect();
        // Remove top-most first so recorded indices stay valid on revert (reverse order).
        items.sort_unstable_by_key(|(at, _)| std::cmp::Reverse(*at));
        Self { label, changes: items.into_iter().map(|(at, element)| Change::Remove { at, element }).collect() }
    }

    pub fn apply(&self, board: &mut Board) {
        for c in &self.changes {
            c.apply(board);
        }
    }

    pub fn revert(&self, board: &mut Board) {
        for c in self.changes.iter().rev() {
            c.revert(board);
        }
    }
}

pub struct History {
    undo: VecDeque<Command>,
    redo: Vec<Command>,
    limit: usize,
    /// Undo depth at last save; `None` if the saved state is unreachable.
    saved_at: Option<usize>,
}

impl Default for History {
    fn default() -> Self {
        Self::new(500)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self { undo: VecDeque::new(), redo: Vec::new(), limit: limit.max(1), saved_at: Some(0) }
    }

    /// Apply a command and record it.
    pub fn execute(&mut self, cmd: Command, board: &mut Board) {
        cmd.apply(board);
        self.push_applied(cmd);
    }

    /// Record a command whose effects are already on the board (e.g. a finished drag).
    pub fn push_applied(&mut self, cmd: Command) {
        if cmd.is_empty() {
            return;
        }
        if self.saved_at.is_some_and(|s| s > self.undo.len()) {
            self.saved_at = None;
        }
        self.redo.clear();
        self.undo.push_back(cmd);
        if self.undo.len() > self.limit {
            self.undo.pop_front();
            self.saved_at = self.saved_at.and_then(|s| s.checked_sub(1));
        }
    }

    pub fn undo(&mut self, board: &mut Board) -> bool {
        let Some(cmd) = self.undo.pop_back() else { return false };
        cmd.revert(board);
        self.redo.push(cmd);
        true
    }

    pub fn redo(&mut self, board: &mut Board) -> bool {
        let Some(cmd) = self.redo.pop() else { return false };
        cmd.apply(board);
        self.undo.push_back(cmd);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn mark_saved(&mut self) {
        self.saved_at = Some(self.undo.len());
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_at != Some(self.undo.len())
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{ElementKind, Paint};
    use crate::kinds::Stroke;
    use glam::{DVec2, Vec2};

    fn dot(x: f64) -> Element {
        Element::new(ElementKind::Stroke(Stroke { points: vec![Vec2::ZERO] }), Transform::at(DVec2::new(x, 0.0)), Style::default())
    }

    #[test]
    fn undo_redo_remove_restores_order() {
        let mut b = Board::new();
        let ids: Vec<_> = (0..4).map(|i| b.push(dot(i as f64))).collect();
        let mut h = History::default();
        h.execute(Command::remove("del", &b, &[ids[0], ids[2]]), &mut b);
        assert_eq!(b.len(), 2);
        assert!(h.is_dirty());
        h.undo(&mut b);
        assert_eq!(b.elements().iter().map(|e| e.id).collect::<Vec<_>>(), ids);
        assert!(!h.is_dirty());
        h.redo(&mut b);
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn modify_stores_compact_changes() {
        let mut b = Board::new();
        let id = b.push(dot(0.0));
        let before = vec![b.get(id).unwrap().clone()];
        let mut moved = before[0].clone();
        moved.transform.pos.x = 99.0;
        let cmd = Command::modify("move", &before, std::slice::from_ref(&moved));
        assert!(matches!(cmd.changes[..], [Change::Transform { .. }]));
        let mut h = History::default();
        h.execute(cmd, &mut b);
        assert_eq!(b.get(id).unwrap().transform.pos.x, 99.0);
        h.undo(&mut b);
        assert_eq!(b.get(id).unwrap().transform.pos.x, 0.0);

        let mut recolored = before[0].clone();
        recolored.style.stroke = Paint::Palette(3);
        let cmd = Command::modify("style", &before, &[recolored]);
        assert!(matches!(cmd.changes[..], [Change::Style { .. }]));
        let v0 = b.get(id).unwrap().version;
        h.execute(cmd, &mut b);
        assert_eq!(b.get(id).unwrap().style.stroke, Paint::Palette(3));
        assert_ne!(b.get(id).unwrap().version, v0, "style change must invalidate meshes");
        h.undo(&mut b);
        assert_eq!(b.get(id).unwrap().style, Style::default());
    }

    #[test]
    fn limit_drops_oldest() {
        let mut b = Board::new();
        let id = b.push(dot(0.0));
        let mut h = History::new(3);
        for i in 1..=5 {
            let t = b.get(id).unwrap().transform;
            h.execute(Command::transforms("m", [(id, t, Transform::at(DVec2::new(i as f64, 0.0)))]), &mut b);
        }
        let mut undone = 0;
        while h.undo(&mut b) {
            undone += 1;
        }
        assert_eq!(undone, 3);
        assert_eq!(b.get(id).unwrap().transform.pos.x, 2.0);
    }
}
