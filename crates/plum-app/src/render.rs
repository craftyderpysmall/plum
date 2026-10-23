//! Board rendering.
//!
//! * Only elements intersecting the viewport are touched (R-tree query).
//! * Each element is tessellated once into a local-space mesh, cached by
//!   `(version, zoom bucket)`. Per frame we only transform its vertices.
//! * Elements too small to see are drawn as a single dot (LOD).
//! * Everything is appended into one mesh → one draw call for the board.

use std::collections::HashMap;
use std::sync::Arc;

use egui::epaint::{Mesh, PathShape, PathStroke, Shape, TessellationOptions, Tessellator, TextShape, Vertex, WHITE_UV};
use egui::text::LayoutJob;
use egui::{Align, Color32, FontId, Galley, Painter, Pos2, Rect, Stroke, TextureId, pos2};
use glam::{DAffine2, DVec2, Vec2};
use plum_config::{Config, GridStyle};
use plum_core::geom::simplify_rdp;
use plum_core::{Board, Element, ElementId, ElementKind, Line, Shape as PShape, Stroke as PStroke, Text, TextAlign};

use crate::camera::Camera;
use crate::theme::{c32, paint_c32};

#[derive(Default, Clone, Copy, Debug)]
pub struct Stats {
    pub visible: usize,
    pub total: usize,
    pub lod: usize,
    pub cached: usize,
    pub rebuilt: usize,
    pub vertices: usize,
}

struct Cached {
    version: u32,
    /// Zoom bucket (shapes) or exact-ish text scale step (text).
    bucket: i32,
    /// Glyph-atlas generation the mesh's UVs point into (text only).
    atlas: u32,
    /// Local geometry was multiplied by this before tessellation.
    k: f32,
    mesh: Mesh,
    last_used: u64,
}

#[derive(Default)]
pub struct Renderer {
    cache: HashMap<ElementId, Cached>,
    frame: u64,
    pub stats: Stats,
    /// Bumped whenever egui rebuilds its glyph atlas, which invalidates cached text UVs.
    atlas: u32,
    atlas_seen: ([usize; 2], f32),
    /// Last frame's vertex/index counts, to size the output mesh up front.
    reserve: (usize, usize),
}

/// Text scale steps of 0.5%: invisible blur, but panning over text is free.
const TEXT_STEP: f64 = 1.005;

/// Everything needed to tessellate this frame.
pub struct FrameCtx<'a> {
    pub ctx: &'a egui::Context,
    pub cfg: &'a Config,
    pub cam: &'a Camera,
    pub ppp: f32,
    pub font_tex_size: [usize; 2],
    pub atlas_fill: f32,
}

impl FrameCtx<'_> {
    fn tessellator(&self) -> Tessellator {
        let mut t = Tessellator::new(self.ppp, TessellationOptions::default(), self.font_tex_size, Vec::new());
        t.set_clip_rect(Rect::EVERYTHING);
        t
    }
}

impl Renderer {
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Draw the board. `hidden` is skipped (e.g. text being edited),
    /// `previews` are in-progress elements drawn on top without caching.
    pub fn draw_board(&mut self, painter: &Painter, fc: &FrameCtx, board: &Board, hidden: Option<ElementId>, previews: &[&Element]) {
        self.frame += 1;
        let cfg = fc.cfg;
        let cam = fc.cam;
        // The atlas only grows, unless egui cleared it (fill ratio drops) or resized it.
        let (seen_size, seen_fill) = self.atlas_seen;
        if seen_size != fc.font_tex_size || fc.atlas_fill + 1e-4 < seen_fill {
            self.atlas = self.atlas.wrapping_add(1);
        }
        self.atlas_seen = (fc.font_tex_size, fc.atlas_fill);
        let mut stats = Stats { total: board.len(), ..Default::default() };
        let mut out = Mesh::with_texture(TextureId::default());
        out.vertices.reserve(self.reserve.0);
        out.indices.reserve(self.reserve.1);
        let mut tess = fc.tessellator();
        let cam_affine = cam.affine();
        let lod_px = cfg.performance.lod_min_screen_size.max(0.0) as f64;
        let bucket_base = cfg.performance.zoom_bucket_factor.max(1.05);

        let visible = board.visible(&cam.visible_world().expand(2.0 / cam.zoom));
        stats.visible = visible.len();
        for el in visible {
            if Some(el.id) == hidden {
                continue;
            }
            // LOD: sub-pixel elements become a dot.
            if let Some(aabb) = board.element_aabb(el.id) {
                let size = aabb.size() * cam.zoom;
                if size.x.max(size.y) < lod_px {
                    stats.lod += 1;
                    let c = cam.world_to_screen(aabb.center());
                    let color = paint_c32(el.style.fill.unwrap_or(el.style.stroke), el.style.opacity, cfg);
                    quad(&mut out, Rect::from_center_size(c, egui::vec2(1.0, 1.0)), color);
                    continue;
                }
            }
            let screen_scale = cam.zoom * el.transform.scale.abs().max_element();
            let affine = cam_affine * el.transform.affine();
            let text = matches!(el.kind, ElementKind::Text(_));
            let (base, atlas) = if text { (TEXT_STEP, self.atlas) } else { (bucket_base, 0) };
            let bucket = (screen_scale.max(1e-9).ln() / base.ln()).round() as i32;
            let entry = self.cache.get_mut(&el.id).filter(|c| c.version == el.version && c.bucket == bucket && c.atlas == atlas);
            let cached = match entry {
                Some(c) => c,
                None => {
                    stats.rebuilt += 1;
                    let (k, mesh) = match &el.kind {
                        ElementKind::Text(t) => text_mesh(&mut tess, fc, t, el, base.powi(bucket)),
                        _ => {
                            let k = base.powi(bucket) as f32;
                            (k, build_local_mesh(&mut tess, el, k, cfg))
                        }
                    };
                    self.cache.insert(el.id, Cached { version: el.version, bucket, atlas, k, mesh, last_used: 0 });
                    self.cache.get_mut(&el.id).unwrap()
                }
            };
            cached.last_used = self.frame;
            append_transformed(&mut out, &cached.mesh, &(affine * DAffine2::from_scale(DVec2::splat(1.0 / cached.k as f64))));
        }

        for el in previews {
            let screen_scale = cam.zoom * el.transform.scale.abs().max_element();
            let affine = cam_affine * el.transform.affine();
            let (k, mesh) = match &el.kind {
                ElementKind::Text(t) => text_mesh(&mut tess, fc, t, el, screen_scale),
                _ => (screen_scale as f32, build_local_mesh(&mut tess, el, screen_scale as f32, cfg)),
            };
            append_transformed(&mut out, &mesh, &(affine * DAffine2::from_scale(DVec2::splat(1.0 / k as f64))));
        }

        // Evict meshes that haven't been drawn for a while.
        if self.frame.is_multiple_of(30) {
            let keep = cfg.performance.mesh_cache_frames as u64;
            let frame = self.frame;
            self.cache.retain(|_, c| frame - c.last_used <= keep);
        }
        stats.cached = self.cache.len();
        stats.vertices = out.vertices.len();
        self.reserve = (out.vertices.len(), out.indices.len());
        self.stats = stats;
        if !out.is_empty() {
            painter.add(Shape::mesh(out));
        }
    }
}

/// Tessellate an element in local space scaled by `k` (≈ screen pixels).
fn build_local_mesh(tess: &mut Tessellator, el: &Element, k: f32, cfg: &Config) -> Mesh {
    let mut mesh = Mesh::with_texture(TextureId::default());
    let st = &el.style;
    let stroke_color = paint_c32(st.stroke, st.opacity, cfg);
    let fill_color = st.fill.map(|f| paint_c32(f, st.opacity, cfg));
    let width = st.width * k;
    let p = |v: Vec2| pos2(v.x * k, v.y * k);
    // Round caps only matter once the stroke is visibly thick.
    let caps = width >= 1.5;
    match &el.kind {
        ElementKind::Stroke(stroke @ PStroke { points }) => {
            // Drop detail finer than ~0.4 px at this zoom.
            let pts = simplify_rdp(points, 0.4 / k);
            match pts.as_slice() {
                [] => {}
                [only] => tess.tessellate_shape(disc(p(*only), (width * 0.5).max(0.5), stroke_color), &mut mesh),
                pts => {
                    let screen: Vec<Pos2> = pts.iter().map(|v| p(*v)).collect();
                    if let Some(fill) = fill_color
                        && stroke.is_closed(st.width)
                    {
                        fill_polygon(&mut mesh, &screen, fill);
                    }
                    if caps {
                        let n = screen.len();
                        tess.tessellate_shape(half_disc(screen[0], screen[0] - screen[1], width * 0.5, stroke_color), &mut mesh);
                        tess.tessellate_shape(half_disc(screen[n - 1], screen[n - 1] - screen[n - 2], width * 0.5, stroke_color), &mut mesh);
                    }
                    tess.tessellate_shape(Shape::Path(PathShape::line(screen, Stroke::new(width, stroke_color))), &mut mesh);
                }
            }
        }
        ElementKind::Shape(s @ PShape { size, .. }) => {
            let segs = ((size.max_element() * k).sqrt() * 4.0).clamp(16.0, 160.0) as usize;
            let pts: Vec<Pos2> = s.outline(segs).into_iter().map(p).collect();
            let stroke = if st.width > 0.0 { PathStroke::new(width, stroke_color) } else { PathStroke::NONE };
            let shape = match fill_color {
                Some(f) => PathShape::convex_polygon(pts, f, stroke),
                None => PathShape::closed_line(pts, stroke),
            };
            tess.tessellate_shape(Shape::Path(shape), &mut mesh);
        }
        ElementKind::Line(l) => {
            let (a, b) = (p(l.a), p(l.b));
            let dir = (b - a).normalized();
            let head = Line::head_len(st.width) * k;
            let mut a2 = a;
            let mut b2 = b;
            if dir.length_sq() > 0.0 {
                if l.arrow_end {
                    tess.tessellate_shape(arrow_head(b, dir, head, stroke_color), &mut mesh);
                    b2 = b - dir * head * 0.6;
                }
                if l.arrow_start {
                    tess.tessellate_shape(arrow_head(a, -dir, head, stroke_color), &mut mesh);
                    a2 = a + dir * head * 0.6;
                }
            }
            // Caps only where there's no arrowhead covering the end.
            if caps && dir.length_sq() > 0.0 {
                if !l.arrow_start {
                    tess.tessellate_shape(half_disc(a2, -dir, width * 0.5, stroke_color), &mut mesh);
                }
                if !l.arrow_end {
                    tess.tessellate_shape(half_disc(b2, dir, width * 0.5, stroke_color), &mut mesh);
                }
            }
            tess.tessellate_shape(Shape::LineSegment { points: [a2, b2], stroke: Stroke::new(width, stroke_color) }, &mut mesh);
        }
        ElementKind::Unknown(_) | ElementKind::Text(_) => {
            // Placeholder box for kinds from newer files.
            let r = el.local_bounds();
            let pts = r.corners().iter().map(|c| p(*c)).collect();
            let color = c32(cfg.colors.placeholder);
            tess.tessellate_shape(Shape::Path(PathShape::convex_polygon(pts, color.gamma_multiply(0.15), Stroke::new(1.5, color))), &mut mesh);
        }
    }
    mesh
}

fn disc(center: Pos2, r: f32, color: Color32) -> Shape {
    if r <= 0.0 {
        return Shape::Noop;
    }
    let n = ((r.sqrt() * 6.0) as usize).clamp(8, 48);
    let pts = (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            center + egui::vec2(a.cos(), a.sin()) * r
        })
        .collect();
    Shape::Path(PathShape::convex_polygon(pts, color, PathStroke::NONE))
}

/// Round cap that only covers the area *beyond* the line end, so translucent
/// strokes don't get a darker blob where a full disc would overlap the line.
fn half_disc(center: Pos2, outward: egui::Vec2, r: f32, color: Color32) -> Shape {
    let d = outward.normalized();
    if r <= 0.0 || !d.x.is_finite() {
        return Shape::Noop;
    }
    let perp = egui::vec2(-d.y, d.x);
    let n = ((r.sqrt() * 3.0) as usize).clamp(4, 24);
    let pts = (0..=n)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / n as f32;
            center + (d * a.cos() + perp * a.sin()) * r
        })
        .collect();
    Shape::Path(PathShape::convex_polygon(pts, color, PathStroke::NONE))
}

/// Fill an arbitrary (possibly concave) closed outline via ear clipping.
fn fill_polygon(out: &mut Mesh, pts: &[Pos2], color: Color32) {
    let flat: Vec<f64> = pts.iter().flat_map(|p| [p.x as f64, p.y as f64]).collect();
    let Ok(tris) = earcutr::earcut(&flat, &[], 2) else { return };
    let base = out.vertices.len() as u32;
    out.vertices.extend(pts.iter().map(|p| Vertex { pos: *p, uv: WHITE_UV, color }));
    out.indices.extend(tris.into_iter().map(|i| base + i as u32));
}

fn arrow_head(tip: Pos2, dir: egui::Vec2, len: f32, color: Color32) -> Shape {
    let perp = egui::vec2(-dir.y, dir.x);
    let base = tip - dir * len;
    Shape::Path(PathShape::convex_polygon(vec![tip, base + perp * len * 0.5, base - perp * len * 0.5], color, PathStroke::NONE))
}

/// Lay out text at roughly screen resolution (local units × k). Returns `(k, mesh)`.
fn text_mesh(tess: &mut Tessellator, fc: &FrameCtx, t: &Text, el: &Element, screen_scale: f64) -> (f32, Mesh) {
    // Glyphs are rasterized at the laid-out size; cap it to keep the atlas small.
    let k = (screen_scale as f32).min(256.0 / t.font_size.max(1.0)).max(0.01);
    let color = paint_c32(el.style.stroke, el.style.opacity, fc.cfg);
    let galley = layout_text(fc.ctx, t, k, color);
    let mut mesh = Mesh::with_texture(TextureId::default());
    tess.tessellate_text(&TextShape::new(Pos2::ZERO - galley.rect.min.to_vec2(), galley, color), &mut mesh);
    (k, mesh)
}

pub fn layout_text(ctx: &egui::Context, t: &Text, k: f32, color: Color32) -> Arc<Galley> {
    let mut job = LayoutJob::simple(t.content.clone(), FontId::proportional(t.font_size * k), color, f32::INFINITY);
    job.halign = match t.align {
        TextAlign::Left => Align::LEFT,
        TextAlign::Center => Align::Center,
        TextAlign::Right => Align::RIGHT,
    };
    ctx.fonts_mut(|f| f.layout_job(job))
}

/// Size of a text element's box in local units.
pub fn measure_text(ctx: &egui::Context, t: &Text) -> Vec2 {
    let content = if t.content.is_empty() { " " } else { &t.content };
    let probe = Text { content: content.to_string(), ..t.clone() };
    let g = layout_text(ctx, &probe, 1.0, Color32::WHITE);
    Vec2::new(g.rect.width(), g.rect.height())
}

fn append_transformed(out: &mut Mesh, mesh: &Mesh, m: &DAffine2) {
    let ma = m.matrix2;
    let (a, b, c, d) = (ma.x_axis.x as f32, ma.x_axis.y as f32, ma.y_axis.x as f32, ma.y_axis.y as f32);
    let (tx, ty) = (m.translation.x as f32, m.translation.y as f32);
    let base = out.vertices.len() as u32;
    out.vertices.extend(mesh.vertices.iter().map(|v| Vertex {
        pos: pos2(a * v.pos.x + c * v.pos.y + tx, b * v.pos.x + d * v.pos.y + ty),
        uv: v.uv,
        color: v.color,
    }));
    out.indices.extend(mesh.indices.iter().map(|i| i + base));
}

fn quad(out: &mut Mesh, r: Rect, color: Color32) {
    let base = out.vertices.len() as u32;
    for p in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()] {
        out.vertices.push(Vertex { pos: p, uv: WHITE_UV, color });
    }
    out.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// World spacing of the finest grid level visible at this zoom (also the snap step).
pub fn grid_step(cam: &Camera, cfg: &Config) -> f64 {
    let g = &cfg.grid;
    let major = g.major_every.max(2) as f64;
    let min_px = g.min_screen_spacing.max(2.0) as f64;
    let mut step = (g.spacing as f64).max(1e-6);
    while step * cam.zoom < min_px {
        step *= major;
    }
    while step * cam.zoom / major >= min_px {
        step /= major;
    }
    step
}

/// Infinite grid: only visible cells are generated, and the spacing steps by
/// `major_every` as you zoom so density stays constant on screen.
pub fn draw_grid(painter: &Painter, cam: &Camera, cfg: &Config) {
    let g = &cfg.grid;
    if g.style == GridStyle::None || g.spacing <= 0.0 {
        return;
    }
    let major = g.major_every.max(2) as f64;
    let min_px = g.min_screen_spacing.max(2.0) as f64;
    let step = grid_step(cam, cfg);
    // Fade the minor level in as it opens up.
    let px = step * cam.zoom;
    let fade = (((px - min_px) / (min_px * (major - 1.0))).clamp(0.0, 1.0)) as f32;
    let minor = c32(cfg.colors.grid).gamma_multiply(fade);
    let major_c = c32(cfg.colors.grid_major);

    let view = cam.visible_world();
    let (x0, x1) = ((view.min.x / step).floor() as i64, (view.max.x / step).ceil() as i64);
    let (y0, y1) = ((view.min.y / step).floor() as i64, (view.max.y / step).ceil() as i64);
    if (x1 - x0) * (y1 - y0) > 60_000 {
        return;
    }
    let is_major = |i: i64| (i as f64).rem_euclid(major) == 0.0;
    match g.style {
        GridStyle::Dots => {
            let mut mesh = Mesh::default();
            for ix in x0..=x1 {
                for iy in y0..=y1 {
                    let mj = is_major(ix) && is_major(iy);
                    let color = if mj { major_c } else { minor };
                    if color.a() == 0 {
                        continue;
                    }
                    let p = cam.world_to_screen(DVec2::new(ix as f64 * step, iy as f64 * step));
                    let s = if mj { 2.0 } else { 1.5 };
                    quad(&mut mesh, Rect::from_center_size(p, egui::vec2(s, s)), color);
                }
            }
            painter.add(Shape::mesh(mesh));
        }
        GridStyle::Lines => {
            let r = cam.viewport;
            for ix in x0..=x1 {
                let x = cam.world_to_screen(DVec2::new(ix as f64 * step, 0.0)).x;
                let c = if is_major(ix) { major_c } else { minor };
                painter.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], Stroke::new(1.0, c));
            }
            for iy in y0..=y1 {
                let y = cam.world_to_screen(DVec2::new(0.0, iy as f64 * step)).y;
                let c = if is_major(iy) { major_c } else { minor };
                painter.line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, c));
            }
        }
        GridStyle::None => {}
    }
}
