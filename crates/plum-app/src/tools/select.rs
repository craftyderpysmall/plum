//! Selection and the transform gizmo: move, scale (8 handles), rotate, marquee.

use egui::{CursorIcon, Painter, Pos2, Rect, Stroke, StrokeKind};
use glam::DVec2;
use plum_core::selection::GroupOp;
use plum_core::{Aabb, Command, ElementId, ElementKind, Obb, Transform};

use super::{Tool, ToolCtx, ToolEvent, ViewCtx};
use crate::camera::Camera;
use crate::theme::c32;

/// Frame-space signs of the 8 scale handles (NW, N, NE, E, SE, S, SW, W).
const HANDLES: [(f64, f64); 8] = [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0)];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grip {
    Scale(usize),
    Rotate,
}

/// Screen-space geometry of the gizmo around an [`Obb`].
struct Gizmo {
    corners: [Pos2; 4],
    handles: Vec<(usize, Pos2)>,
    rotate: Pos2,
    top_mid: Pos2,
    center: Pos2,
}

impl Gizmo {
    fn new(obb: &Obb, cam: &Camera, v_handle: f32, rotate_dist: f32) -> Self {
        let corners = obb.corners().map(|c| cam.world_to_screen(c));
        let center = cam.world_to_screen(obb.center);
        let w = corners[0].distance(corners[1]);
        let h = corners[0].distance(corners[3]);
        let handles = HANDLES
            .iter()
            .enumerate()
            // Hide edge handles when the box is too small to grab them separately.
            .filter(|(_, (sx, sy))| (*sx != 0.0 && *sy != 0.0) || (if *sx == 0.0 { w } else { h }) > v_handle * 4.0)
            .map(|(i, (sx, sy))| (i, cam.world_to_screen(obb.to_world(DVec2::new(sx * obb.half.x, sy * obb.half.y)))))
            .collect();
        let top_mid = cam.world_to_screen(obb.to_world(DVec2::new(0.0, -obb.half.y)));
        let (_, ay) = obb.axes();
        let up = egui::vec2(-ay.x as f32, -ay.y as f32);
        Self { corners, handles, rotate: top_mid + up * rotate_dist, top_mid, center }
    }

    fn grip_at(&self, p: Pos2, radius: f32) -> Option<Grip> {
        if p.distance(self.rotate) <= radius + 2.0 {
            return Some(Grip::Rotate);
        }
        self.handles.iter().find(|(_, h)| p.distance(*h) <= radius + 2.0).map(|(i, _)| Grip::Scale(*i))
    }

    fn handle_pos(&self, i: usize) -> Option<Pos2> {
        self.handles.iter().find(|(j, _)| *j == i).map(|(_, p)| *p)
    }

    fn contains(&self, p: Pos2) -> bool {
        // Point-in-convex-quad via edge cross products.
        let c = &self.corners;
        let side = |a: Pos2, b: Pos2| (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        let s: Vec<f32> = (0..4).map(|i| side(c[i], c[(i + 1) % 4])).collect();
        s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
    }
}

fn resize_cursor(dir: egui::Vec2) -> CursorIcon {
    let deg = dir.y.atan2(dir.x).to_degrees().rem_euclid(180.0);
    match deg {
        d if !(22.5..157.5).contains(&d) => CursorIcon::ResizeHorizontal,
        d if d < 67.5 => CursorIcon::ResizeNwSe,
        d if d < 112.5 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeNeSw,
    }
}

/// Transforms of the selected elements when the gesture began. Gestures only
/// ever change transforms, so this is all they need to keep (and to undo).
type Snapshot = Vec<(ElementId, Transform)>;

enum Gesture {
    /// `corner` is the selection's top-left at drag start; it's what snaps to the grid.
    Move { start: DVec2, corner: DVec2, snapshot: Snapshot },
    Rotate { obb: Obb, start_angle: f64, angle: f64, snapshot: Snapshot },
    Scale { obb: Obb, handle: usize, snapshot: Snapshot },
    Marquee { start: Pos2, current: Pos2, base: Vec<plum_core::ElementId> },
}

#[derive(Default)]
pub struct SelectTool {
    gesture: Option<Gesture>,
}

fn apply(cx: &mut ToolCtx, snapshot: &[(ElementId, Transform)], op: GroupOp) {
    op.apply_to_board(cx.board, snapshot);
}

fn start_move(cx: &ToolCtx, start: DVec2) -> Gesture {
    let corner = cx.selection.elements(cx.board).iter().fold(Aabb::EMPTY, |a, e| a.union(&e.world_aabb())).min;
    Gesture::Move { start, corner, snapshot: cx.selection.transforms(cx.board) }
}

impl SelectTool {
    fn gizmo(v_obb: &Obb, cam: &Camera, cfg: &plum_config::Config) -> Gizmo {
        Gizmo::new(v_obb, cam, cfg.ui.handle_size, cfg.ui.rotate_handle_distance)
    }

    /// Box to draw: during a rotation the original box turns with the pointer.
    fn display_obb(&self, board: &plum_core::Board, sel: &plum_core::Selection) -> Option<Obb> {
        match &self.gesture {
            Some(Gesture::Rotate { obb, angle, snapshot, .. }) if snapshot.len() > 1 => Some(Obb { rotation: obb.rotation + angle, ..*obb }),
            _ => sel.obb(board),
        }
    }
}

impl Tool for SelectTool {
    fn press(&mut self, cx: &mut ToolCtx, p: Pos2) {
        let world = cx.cam.screen_to_world(p);
        let cfg = cx.cfg;

        if let Some(obb) = cx.selection.obb(cx.board) {
            let g = Self::gizmo(&obb, cx.cam, cfg);
            match g.grip_at(p, cfg.ui.handle_size) {
                Some(Grip::Rotate) => {
                    let start_angle = (world - obb.center).to_angle();
                    self.gesture = Some(Gesture::Rotate { obb, start_angle, angle: 0.0, snapshot: cx.selection.transforms(cx.board) });
                    return;
                }
                Some(Grip::Scale(handle)) => {
                    self.gesture = Some(Gesture::Scale { obb, handle, snapshot: cx.selection.transforms(cx.board) });
                    return;
                }
                None => {}
            }
        }

        let tol = cfg.canvas.hit_tolerance / cx.cam.zoom;
        if let Some(id) = cx.board.hit(world, tol) {
            if cx.mods.shift {
                cx.selection.toggle(id);
            } else if !cx.selection.contains(id) {
                cx.selection.set([id]);
            }
            self.gesture = Some(start_move(cx, world));
            return;
        }

        // Dragging inside a multi-selection's box moves it, even between elements.
        if !cx.mods.shift
            && let Some(obb) = cx.selection.obb(cx.board)
            && cx.selection.len() > 1
            && Self::gizmo(&obb, cx.cam, cfg).contains(p)
        {
            self.gesture = Some(start_move(cx, world));
            return;
        }

        let base = if cx.mods.shift { cx.selection.ordered(cx.board) } else { Vec::new() };
        if !cx.mods.shift {
            cx.selection.clear();
        }
        self.gesture = Some(Gesture::Marquee { start: p, current: p, base });
    }

    fn drag(&mut self, cx: &mut ToolCtx, p: Pos2) {
        let world = cx.cam.screen_to_world(p);
        let Some(g) = &mut self.gesture else { return };
        match g {
            Gesture::Move { start, corner, snapshot } => {
                let raw = world - *start;
                // Shift locks movement to the dominant axis.
                let horizontal = cx.mods.shift.then_some(raw.x.abs() > raw.y.abs());
                let mut d = cx.snap(*corner + raw) - *corner;
                match horizontal {
                    Some(true) => d.y = 0.0,
                    Some(false) => d.x = 0.0,
                    None => {}
                }
                apply(cx, snapshot, GroupOp::Translate(d));
            }
            Gesture::Rotate { obb, start_angle, angle, snapshot } => {
                let mut delta = plum_core::transform::normalize_angle((world - obb.center).to_angle() - *start_angle);
                if cx.mods.shift {
                    let step = cx.cfg.canvas.rotate_snap_degrees.to_radians().max(1e-6);
                    // Snap the resulting orientation, not just the delta.
                    let base = obb.rotation;
                    delta = ((base + delta) / step).round() * step - base;
                }
                *angle = delta;
                apply(cx, snapshot, GroupOp::Rotate { pivot: obb.center, angle: delta });
            }
            Gesture::Scale { obb, handle, snapshot } => {
                let (hx, hy) = HANDLES[*handle];
                let anchor_f = DVec2::new(-hx * obb.half.x, -hy * obb.half.y);
                let pf = obb.to_frame(cx.snap(world));
                let min_world = 4.0 / cx.cam.zoom;
                let axis = |h: f64, half: f64, pf: f64, anchor: f64| {
                    if h == 0.0 || half < 1e-9 {
                        1.0
                    } else {
                        ((pf - anchor) * h / (2.0 * half)).max(min_world / (2.0 * half))
                    }
                };
                let mut fx = axis(hx, obb.half.x, pf.x, anchor_f.x);
                let mut fy = axis(hy, obb.half.y, pf.y, anchor_f.y);
                // Multi-selections always scale proportionally (no shear on rotated members).
                if cx.mods.shift || snapshot.len() > 1 {
                    let f = match (hx != 0.0, hy != 0.0) {
                        (true, true) => fx.max(fy),
                        (true, false) => fx,
                        _ => fy,
                    };
                    fx = f;
                    fy = f;
                }
                let op = GroupOp::Scale { frame_rotation: obb.rotation, anchor: obb.to_world(anchor_f), factor: DVec2::new(fx, fy) };
                apply(cx, snapshot, op);
            }
            Gesture::Marquee { start, current, base } => {
                *current = p;
                let area = Aabb::new(cx.cam.screen_to_world(*start), world);
                let mut ids = base.clone();
                ids.extend(cx.board.query_contained(&area));
                cx.selection.set(ids);
            }
        }
    }

    fn release(&mut self, cx: &mut ToolCtx, _p: Pos2) {
        let label = match &self.gesture {
            Some(Gesture::Move { .. }) => "Move",
            Some(Gesture::Rotate { .. }) => "Rotate",
            Some(Gesture::Scale { .. }) => "Resize",
            _ => "",
        };
        match self.gesture.take() {
            Some(Gesture::Move { snapshot, .. } | Gesture::Rotate { snapshot, .. } | Gesture::Scale { snapshot, .. }) => {
                let board = &*cx.board;
                let items = snapshot.into_iter().filter_map(|(id, before)| Some((id, before, board.get(id)?.transform)));
                let cmd = Command::transforms(label, items);
                cx.history.push_applied(cmd);
            }
            Some(Gesture::Marquee { .. }) | None => {}
        }
    }

    fn double_click(&mut self, cx: &mut ToolCtx, p: Pos2) {
        let world = cx.cam.screen_to_world(p);
        let hit = cx.board.hit(world, cx.cfg.canvas.hit_tolerance / cx.cam.zoom);
        match hit.and_then(|id| cx.board.get(id)).map(|e| (e.id, &e.kind)) {
            Some((id, ElementKind::Text(_))) => cx.events.push(ToolEvent::EditText { target: Some(id), at: world }),
            None => cx.events.push(ToolEvent::EditText { target: None, at: world }),
            _ => {}
        }
    }

    fn cancel(&mut self, cx: &mut ToolCtx) {
        if let Some(Gesture::Move { snapshot, .. } | Gesture::Rotate { snapshot, .. } | Gesture::Scale { snapshot, .. }) = self.gesture.take() {
            for (id, t) in snapshot {
                cx.board.set_transform(id, t);
            }
        }
    }

    fn paint(&self, v: &ViewCtx, painter: &Painter) {
        let cfg = v.cfg;
        let col = &cfg.colors;

        // Hover outline for the element under the pointer.
        if self.gesture.is_none()
            && let Some(hp) = v.hover
            && let Some(id) = v.board.hit(v.cam.screen_to_world(hp), cfg.canvas.hit_tolerance / v.cam.zoom)
            && !v.selection.contains(id)
            && let Some(el) = v.board.get(id)
        {
            let c = el.world_obb().corners().map(|c| v.cam.world_to_screen(c));
            painter.add(egui::Shape::closed_line(c.to_vec(), Stroke::new(1.5, c32(col.hover))));
        }

        if let Some(Gesture::Marquee { start, current, .. }) = &self.gesture {
            let r = Rect::from_two_pos(*start, *current);
            painter.rect(r, 3.0, c32(col.marquee_fill), Stroke::new(1.0, c32(col.marquee)), StrokeKind::Inside);
        }

        // Thin outlines around each member of a multi-selection.
        if v.selection.len() > 1 {
            for el in v.selection.elements(v.board) {
                let c = el.world_obb().corners().map(|c| v.cam.world_to_screen(c));
                painter.add(egui::Shape::closed_line(c.to_vec(), Stroke::new(1.0, c32(col.selection).gamma_multiply(0.5))));
            }
        }

        let Some(obb) = self.display_obb(v.board, v.selection) else { return };
        let g = Self::gizmo(&obb, v.cam, cfg);
        painter.add(egui::Shape::convex_polygon(g.corners.to_vec(), c32(col.selection_fill), Stroke::new(1.25, c32(col.selection))));
        if matches!(self.gesture, Some(Gesture::Move { .. } | Gesture::Marquee { .. })) {
            return;
        }
        let hs = cfg.ui.handle_size;
        painter.line_segment([g.top_mid, g.rotate], Stroke::new(1.0, c32(col.selection)));
        painter.circle(g.rotate, hs * 0.62, c32(col.handle_fill), Stroke::new(1.5, c32(col.rotate_handle)));
        for (_, h) in &g.handles {
            let r = Rect::from_center_size(*h, egui::vec2(hs, hs));
            painter.rect(r, 2.5, c32(col.handle_fill), Stroke::new(1.5, c32(col.handle_stroke)), StrokeKind::Middle);
        }
        if let Some(Gesture::Rotate { angle, .. }) = &self.gesture {
            // Angle readout in a small pill above the rotate handle.
            let txt = format!("{:.0}°", angle.to_degrees());
            let galley = painter.layout_no_wrap(txt, egui::FontId::monospace(11.0), c32(col.accent_text));
            let pill = Rect::from_center_size(g.rotate + egui::vec2(0.0, -20.0), galley.size() + egui::vec2(12.0, 6.0));
            painter.rect_filled(pill, pill.height() / 2.0, c32(col.accent));
            painter.galley(pill.center() - galley.size() / 2.0, galley, c32(col.accent_text));
        }
    }

    fn cursor(&self, v: &ViewCtx) -> CursorIcon {
        match &self.gesture {
            Some(Gesture::Move { .. }) => return CursorIcon::Grabbing,
            Some(Gesture::Rotate { .. }) => return CursorIcon::Grabbing,
            Some(Gesture::Scale { obb, handle, .. }) => {
                let g = Self::gizmo(obb, v.cam, v.cfg);
                return g.handle_pos(*handle).map(|h| resize_cursor(h - g.center)).unwrap_or(CursorIcon::Default);
            }
            _ => {}
        }
        let Some(hp) = v.hover else { return CursorIcon::Default };
        if let Some(obb) = v.selection.obb(v.board) {
            let g = Self::gizmo(&obb, v.cam, v.cfg);
            match g.grip_at(hp, v.cfg.ui.handle_size) {
                Some(Grip::Rotate) => return CursorIcon::Grab,
                Some(Grip::Scale(i)) => return g.handle_pos(i).map(|h| resize_cursor(h - g.center)).unwrap_or(CursorIcon::Default),
                None => {}
            }
        }
        if v.board.hit(v.cam.screen_to_world(hp), v.cfg.canvas.hit_tolerance / v.cam.zoom).is_some() {
            CursorIcon::Move
        } else {
            CursorIcon::Default
        }
    }

    fn busy(&self) -> bool {
        self.gesture.is_some()
    }
}

/// Screen-space bounding rect of the current selection (for placing the context bar).
pub fn selection_screen_rect(board: &plum_core::Board, sel: &plum_core::Selection, cam: &Camera) -> Option<Rect> {
    let obb = sel.obb(board)?;
    let pts = obb.corners().map(|c| cam.world_to_screen(c));
    Some(Rect::from_points(&pts))
}
