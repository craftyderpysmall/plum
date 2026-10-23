use glam::Vec2;

use crate::element::{ElementBehavior, Style};
use crate::geom::Rect;

/// Element kind this build does not understand. The raw payload is kept so
/// saving the board writes it back untouched.
#[derive(Clone, Debug, PartialEq)]
pub struct UnknownKind {
    pub tag: [u8; 4],
    pub kind_version: u8,
    pub bytes: Vec<u8>,
    /// Placeholder bounds (local), if the writer recorded them.
    pub bounds: Rect,
}

impl ElementBehavior for UnknownKind {
    fn local_bounds(&self, _style: &Style) -> Rect {
        if self.bounds.is_empty() { Rect::new(Vec2::ZERO, Vec2::splat(32.0)) } else { self.bounds }
    }

    fn hit_test_local(&self, p: Vec2, style: &Style, tol: f32) -> bool {
        self.local_bounds(style).expand(tol).contains(p)
    }
}
