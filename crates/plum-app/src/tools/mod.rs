//! Canvas tools. Each tool turns pointer gestures into board changes.
//! Adding a tool: implement [`Tool`], add a [`ToolKind`] variant and map it in [`ToolKind::create`].

mod draw;
pub mod select;

use egui::{CursorIcon, Modifiers, Painter, Pos2};
use glam::DVec2;
use plum_config::Config;
use plum_core::{Board, Element, ElementId, History, Selection, ShapeKind, Style};

use crate::camera::Camera;
use crate::icons::Icon;

pub use select::SelectTool;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Select,
    Hand,
    Pen,
    Rect,
    Ellipse,
    Diamond,
    Line,
    Arrow,
    Text,
    Bucket,
}

impl ToolKind {
    pub const ALL: [ToolKind; 10] = [
        ToolKind::Select,
        ToolKind::Hand,
        ToolKind::Pen,
        ToolKind::Rect,
        ToolKind::Ellipse,
        ToolKind::Diamond,
        ToolKind::Line,
        ToolKind::Arrow,
        ToolKind::Text,
        ToolKind::Bucket,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Select => "Select",
            ToolKind::Hand => "Hand",
            ToolKind::Pen => "Pen",
            ToolKind::Rect => "Rectangle",
            ToolKind::Ellipse => "Ellipse",
            ToolKind::Diamond => "Diamond",
            ToolKind::Line => "Line",
            ToolKind::Arrow => "Arrow",
            ToolKind::Text => "Text",
            ToolKind::Bucket => "Fill bucket",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            ToolKind::Select => Icon::Pointer,
            ToolKind::Hand => Icon::Hand,
            ToolKind::Pen => Icon::Pen,
            ToolKind::Rect => Icon::Rect,
            ToolKind::Ellipse => Icon::Ellipse,
            ToolKind::Diamond => Icon::Diamond,
            ToolKind::Line => Icon::Line,
            ToolKind::Arrow => Icon::Arrow,
            ToolKind::Text => Icon::Text,
            ToolKind::Bucket => Icon::Bucket,
        }
    }

    /// Which style slot (pen/shape/line/arrow/text) this tool draws with.
    pub fn style_slot(self) -> Option<StyleSlot> {
        Some(match self {
            ToolKind::Pen => StyleSlot::Pen,
            ToolKind::Rect | ToolKind::Ellipse | ToolKind::Diamond => StyleSlot::Shape,
            ToolKind::Line => StyleSlot::Line,
            ToolKind::Arrow => StyleSlot::Arrow,
            ToolKind::Text => StyleSlot::Text,
            ToolKind::Bucket => StyleSlot::Bucket,
            ToolKind::Select | ToolKind::Hand => return None,
        })
    }

    pub fn create(self) -> Box<dyn Tool> {
        match self {
            ToolKind::Select => Box::new(SelectTool::default()),
            ToolKind::Hand => Box::new(draw::HandTool::default()),
            ToolKind::Pen => Box::new(draw::PenTool::default()),
            ToolKind::Rect => Box::new(draw::ShapeTool::new(ShapeKind::Rect)),
            ToolKind::Ellipse => Box::new(draw::ShapeTool::new(ShapeKind::Ellipse)),
            ToolKind::Diamond => Box::new(draw::ShapeTool::new(ShapeKind::Diamond)),
            ToolKind::Line => Box::new(draw::LineTool::new(false)),
            ToolKind::Arrow => Box::new(draw::LineTool::new(true)),
            ToolKind::Text => Box::new(draw::TextTool),
            ToolKind::Bucket => Box::new(draw::BucketTool),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StyleSlot {
    Pen,
    Shape,
    Line,
    Arrow,
    Text,
    /// Only `fill` is used: the color the bucket pours.
    Bucket,
}

/// Current drawing styles, seeded from `[tools.*]` in the config.
#[derive(Clone, Debug)]
pub struct ToolStyles {
    pub pen: Style,
    pub shape: Style,
    pub line: Style,
    pub arrow: Style,
    pub text: Style,
    pub bucket: Style,
    pub font_size: f32,
}

impl ToolStyles {
    pub fn from_config(cfg: &Config) -> Self {
        use crate::theme::color_ref as paint;
        let t = &cfg.tools;
        let o = |v: f32| v.clamp(0.0, 1.0);
        Self {
            pen: Style { stroke: paint(t.pen.color), fill: None, width: t.pen.width, opacity: o(t.pen.opacity) },
            shape: Style { stroke: paint(t.shape.color), fill: t.shape.fill.0.map(paint), width: t.shape.width, opacity: o(t.shape.opacity) },
            line: Style { stroke: paint(t.line.color), fill: None, width: t.line.width, opacity: o(t.line.opacity) },
            arrow: Style { stroke: paint(t.arrow.color), fill: None, width: t.arrow.width, opacity: o(t.arrow.opacity) },
            text: Style { stroke: paint(t.text.color), fill: None, width: 0.0, opacity: o(t.text.opacity) },
            bucket: Style { fill: Some(paint(t.bucket.color)), ..Default::default() },
            font_size: t.text.font_size,
        }
    }

    pub fn get(&self, slot: StyleSlot) -> &Style {
        match slot {
            StyleSlot::Pen => &self.pen,
            StyleSlot::Shape => &self.shape,
            StyleSlot::Line => &self.line,
            StyleSlot::Arrow => &self.arrow,
            StyleSlot::Text => &self.text,
            StyleSlot::Bucket => &self.bucket,
        }
    }

    pub fn get_mut(&mut self, slot: StyleSlot) -> &mut Style {
        match slot {
            StyleSlot::Pen => &mut self.pen,
            StyleSlot::Shape => &mut self.shape,
            StyleSlot::Line => &mut self.line,
            StyleSlot::Arrow => &mut self.arrow,
            StyleSlot::Text => &mut self.text,
            StyleSlot::Bucket => &mut self.bucket,
        }
    }
}

/// Requests a tool makes of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolEvent {
    SwitchTool(ToolKind),
    /// Open the text editor on an existing element or at a world position.
    EditText { target: Option<ElementId>, at: DVec2 },
    /// Short hint shown as a toast.
    Notice(&'static str),
}

/// Mutable state a tool may act on.
pub struct ToolCtx<'a> {
    pub board: &'a mut Board,
    pub history: &'a mut History,
    pub selection: &'a mut Selection,
    pub cam: &'a mut Camera,
    pub cfg: &'a Config,
    pub styles: &'a ToolStyles,
    pub mods: Modifiers,
    pub events: &'a mut Vec<ToolEvent>,
    /// Grid step in world units when snap-to-grid is on.
    pub snap: Option<f64>,
}

impl ToolCtx<'_> {
    /// Snap a world point to the grid (no-op when snapping is off).
    pub fn snap(&self, p: DVec2) -> DVec2 {
        snap_point(p, self.snap)
    }
}

pub fn snap_point(p: DVec2, step: Option<f64>) -> DVec2 {
    match step {
        Some(s) if s > 0.0 => (p / s).round() * s,
        _ => p,
    }
}

/// Read-only view for painting overlays and choosing cursors.
pub struct ViewCtx<'a> {
    pub board: &'a Board,
    pub selection: &'a Selection,
    pub cam: &'a Camera,
    pub cfg: &'a Config,
    pub hover: Option<Pos2>,
}

pub trait Tool {
    fn press(&mut self, _cx: &mut ToolCtx, _p: Pos2) {}
    fn drag(&mut self, _cx: &mut ToolCtx, _p: Pos2) {}
    fn release(&mut self, _cx: &mut ToolCtx, _p: Pos2) {}
    fn double_click(&mut self, _cx: &mut ToolCtx, _p: Pos2) {}
    /// Abort an in-progress gesture, restoring the board.
    fn cancel(&mut self, _cx: &mut ToolCtx) {}
    /// In-progress element to draw (not yet on the board).
    fn preview(&self) -> Option<&Element> {
        None
    }
    fn paint(&self, _v: &ViewCtx, _painter: &Painter) {}
    fn cursor(&self, _v: &ViewCtx) -> CursorIcon {
        CursorIcon::Crosshair
    }
    /// True while a gesture is running (hides the context bar).
    fn busy(&self) -> bool {
        false
    }
}
