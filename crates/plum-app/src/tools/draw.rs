//! Creation tools: pen, shapes, lines/arrows, text, and the hand.

use egui::{CursorIcon, Pos2};
use glam::{DVec2, Vec2};
use plum_core::{Element, ElementKind, Line, Shape, ShapeKind, Stroke, Transform};

use super::{StyleSlot, Tool, ToolCtx, ToolEvent, ToolKind, ViewCtx};

/// Insert `el` as one undoable step and select it.
fn commit(cx: &mut ToolCtx, el: Element, label: &'static str) {
    let mut cmd = plum_core::Command::new(label);
    let id = cx.board.alloc_id();
    let mut el = el;
    el.id = id;
    cmd.changes.push(plum_core::Change::Insert { at: cx.board.len(), element: el });
    cx.history.execute(cmd, cx.board);
    cx.selection.set([id]);
}

/// Snap `v` to the nearest multiple of `step_deg` degrees.
fn snap_angle(v: DVec2, step_deg: f64) -> DVec2 {
    if step_deg <= 0.0 || v.length_squared() == 0.0 {
        return v;
    }
    let step = step_deg.to_radians();
    let a = (v.y.atan2(v.x) / step).round() * step;
    DVec2::from_angle(a) * v.length()
}

// ---------------------------------------------------------------- pen

/// The in-progress stroke lives in `preview`; points are appended in place
/// (no per-move copy of the whole stroke).
#[derive(Default)]
pub struct PenTool {
    last_screen: Pos2,
    preview: Option<Element>,
}

impl Tool for PenTool {
    fn press(&mut self, cx: &mut ToolCtx, p: Pos2) {
        let origin = cx.cam.screen_to_world(p);
        let style = *cx.styles.get(StyleSlot::Pen);
        self.last_screen = p;
        self.preview = Some(Element::new(ElementKind::Stroke(Stroke { points: vec![Vec2::ZERO] }), Transform::at(origin), style));
    }

    fn drag(&mut self, cx: &mut ToolCtx, p: Pos2) {
        let Some(el) = &mut self.preview else { return };
        if p.distance(self.last_screen) < 1.0 {
            return;
        }
        self.last_screen = p;
        let local = (cx.cam.screen_to_world(p) - el.transform.pos).as_vec2();
        if let ElementKind::Stroke(s) = &mut el.kind {
            s.points.push(local);
        }
    }

    fn release(&mut self, cx: &mut ToolCtx, _p: Pos2) {
        let Some(el) = self.preview.take() else { return };
        let ElementKind::Stroke(raw) = el.kind else { return };
        let eps = (cx.cfg.tools.pen.smoothing as f64 / cx.cam.zoom) as f32;
        let (stroke, offset) = Stroke::finish(&raw.points, eps);
        let el = Element::new(ElementKind::Stroke(stroke), Transform::at(el.transform.pos + offset.as_dvec2()), el.style);
        commit(cx, el, "Draw");
        // Keep drawing: the pen doesn't leave a selection box behind.
        cx.selection.clear();
    }

    fn cancel(&mut self, _cx: &mut ToolCtx) {
        self.preview = None;
    }

    fn preview(&self) -> Option<&Element> {
        self.preview.as_ref()
    }

    fn busy(&self) -> bool {
        self.preview.is_some()
    }
}

// ---------------------------------------------------------------- shapes

pub struct ShapeTool {
    kind: ShapeKind,
    start: Option<DVec2>,
    preview: Option<Element>,
}

impl ShapeTool {
    pub fn new(kind: ShapeKind) -> Self {
        Self { kind, start: None, preview: None }
    }
}

impl Tool for ShapeTool {
    fn press(&mut self, cx: &mut ToolCtx, p: Pos2) {
        self.start = Some(cx.snap(cx.cam.screen_to_world(p)));
    }

    fn drag(&mut self, cx: &mut ToolCtx, p: Pos2) {
        let Some(a) = self.start else { return };
        let mut b = cx.snap(cx.cam.screen_to_world(p));
        if cx.mods.shift {
            let d = b - a;
            let m = d.x.abs().max(d.y.abs());
            b = a + DVec2::new(m.copysign(d.x), m.copysign(d.y));
        }
        let (center, size) = if cx.mods.alt { (a, (b - a).abs() * 2.0) } else { ((a + b) * 0.5, (b - a).abs()) };
        let style = *cx.styles.get(StyleSlot::Shape);
        self.preview = Some(Element::new(ElementKind::Shape(Shape { shape: self.kind, size: size.as_vec2() }), Transform::at(center), style));
    }

    fn release(&mut self, cx: &mut ToolCtx, _p: Pos2) {
        self.start = None;
        let Some(el) = self.preview.take() else { return };
        let ElementKind::Shape(s) = &el.kind else { return };
        if (s.size.max_element() as f64) * cx.cam.zoom < 3.0 {
            return;
        }
        commit(cx, el, "Add shape");
        cx.events.push(ToolEvent::SwitchTool(ToolKind::Select));
    }

    fn cancel(&mut self, _cx: &mut ToolCtx) {
        self.start = None;
        self.preview = None;
    }

    fn preview(&self) -> Option<&Element> {
        self.preview.as_ref()
    }

    fn busy(&self) -> bool {
        self.start.is_some()
    }
}

// ---------------------------------------------------------------- lines

pub struct LineTool {
    arrow: bool,
    start: Option<DVec2>,
    preview: Option<Element>,
}

impl LineTool {
    pub fn new(arrow: bool) -> Self {
        Self { arrow, start: None, preview: None }
    }
}

impl Tool for LineTool {
    fn press(&mut self, cx: &mut ToolCtx, p: Pos2) {
        self.start = Some(cx.snap(cx.cam.screen_to_world(p)));
    }

    fn drag(&mut self, cx: &mut ToolCtx, p: Pos2) {
        let Some(a) = self.start else { return };
        let mut d = cx.cam.screen_to_world(p) - a;
        // Shift locks the angle; otherwise the end point follows the grid.
        if cx.mods.shift {
            d = snap_angle(d, cx.cfg.canvas.rotate_snap_degrees);
        } else {
            d = cx.snap(a + d) - a;
        }
        let mid = a + d * 0.5;
        let half = (d * 0.5).as_vec2();
        let slot = if self.arrow { StyleSlot::Arrow } else { StyleSlot::Line };
        let line = Line { a: -half, b: half, arrow_start: false, arrow_end: self.arrow };
        self.preview = Some(Element::new(ElementKind::Line(line), Transform::at(mid), *cx.styles.get(slot)));
    }

    fn release(&mut self, cx: &mut ToolCtx, _p: Pos2) {
        self.start = None;
        let Some(el) = self.preview.take() else { return };
        let ElementKind::Line(l) = &el.kind else { return };
        if ((l.b - l.a).length() as f64) * cx.cam.zoom < 3.0 {
            return;
        }
        commit(cx, el, if self.arrow { "Add arrow" } else { "Add line" });
        cx.events.push(ToolEvent::SwitchTool(ToolKind::Select));
    }

    fn cancel(&mut self, _cx: &mut ToolCtx) {
        self.start = None;
        self.preview = None;
    }

    fn preview(&self) -> Option<&Element> {
        self.preview.as_ref()
    }

    fn busy(&self) -> bool {
        self.start.is_some()
    }
}

// ---------------------------------------------------------------- text

pub struct TextTool;

impl Tool for TextTool {
    fn release(&mut self, cx: &mut ToolCtx, p: Pos2) {
        let at = cx.cam.screen_to_world(p);
        let target = cx.board.hit(at, cx.cfg.canvas.hit_tolerance / cx.cam.zoom).filter(|id| matches!(cx.board.get(*id).map(|e| &e.kind), Some(ElementKind::Text(_))));
        cx.events.push(ToolEvent::EditText { target, at });
    }

    fn cursor(&self, _v: &ViewCtx) -> CursorIcon {
        CursorIcon::Text
    }
}

// ---------------------------------------------------------------- bucket

/// Fills the topmost shape or closed pencil loop under the click.
pub struct BucketTool;

impl Tool for BucketTool {
    fn release(&mut self, cx: &mut ToolCtx, p: Pos2) {
        let at = cx.cam.screen_to_world(p);
        let mut hits = cx.board.query(&plum_core::Aabb::from_point(at, 0.0));
        hits.reverse();
        let target = hits.into_iter().filter_map(|id| cx.board.get(id)).find(|e| e.fill_contains(at)).map(|e| (e.id, e.style));
        let Some((id, before_style)) = target else {
            cx.events.push(ToolEvent::Notice("Click inside a shape or a closed pencil loop to fill it"));
            return;
        };
        let mut style = before_style;
        style.fill = cx.styles.get(StyleSlot::Bucket).fill;
        if style != before_style {
            let cmd = plum_core::Command { label: "Fill", changes: vec![plum_core::Change::Style { id, before: before_style, after: style }] };
            cx.history.execute(cmd, cx.board);
        }
    }

    fn cursor(&self, _v: &ViewCtx) -> CursorIcon {
        CursorIcon::Crosshair
    }
}

// ---------------------------------------------------------------- hand

#[derive(Default)]
pub struct HandTool {
    last: Option<Pos2>,
}

impl Tool for HandTool {
    fn press(&mut self, _cx: &mut ToolCtx, p: Pos2) {
        self.last = Some(p);
    }

    fn drag(&mut self, cx: &mut ToolCtx, p: Pos2) {
        if let Some(l) = self.last.replace(p) {
            cx.cam.pan(p - l);
        }
    }

    fn release(&mut self, _cx: &mut ToolCtx, _p: Pos2) {
        self.last = None;
    }

    fn cursor(&self, _v: &ViewCtx) -> CursorIcon {
        if self.last.is_some() { CursorIcon::Grabbing } else { CursorIcon::Grab }
    }

    fn busy(&self) -> bool {
        self.last.is_some()
    }
}
