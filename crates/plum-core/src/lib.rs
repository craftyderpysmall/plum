//! Plum core: the pure whiteboard model.
//!
//! Nothing in here knows about windows, GPUs or files. Everything a board
//! *is* lives here: elements ("selection entries"), their transforms, the
//! spatial index that keeps an infinite canvas fast, selection math and
//! undo/redo history.

pub mod board;
pub mod element;
pub mod geom;
pub mod history;
pub mod kinds;
pub mod selection;
pub mod spatial;
pub mod transform;

pub use board::{Board, BoardMeta};
pub use element::{Element, ElementBehavior, ElementId, ElementKind, Paint, Rgba, Style};
pub use geom::{Aabb, Obb, Rect};
pub use history::{Change, Command, History};
pub use kinds::{Line, Shape, ShapeKind, Stroke, Text, TextAlign, UnknownKind};
pub use selection::Selection;
pub use transform::Transform;

pub use glam::{DVec2, Vec2};
