//! Tiny vector icons painted directly (no icon font needed). Each icon is
//! drawn in a unit box and scaled into the target rect, so it themes with
//! the current colors and stays crisp at any size.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Pointer,
    Hand,
    Pen,
    Rect,
    Ellipse,
    Diamond,
    Line,
    Arrow,
    Text,
    Bucket,
    Undo,
    Redo,
    Menu,
    FlipH,
    FlipV,
    RotateCw,
    RotateCcw,
    Forward,
    Backward,
    Duplicate,
    Delete,
    Minus,
    Plus,
    Check,
}

impl Icon {
    pub fn paint(self, painter: &Painter, rect: Rect, color: Color32) {
        let s = rect.width().min(rect.height());
        let o = rect.center() - egui::vec2(s, s) * 0.5;
        let p = |x: f32, y: f32| pos2(o.x + x * s, o.y + y * s);
        let st = Stroke::new((s * 0.08).max(1.25), color);
        let line = |pts: Vec<Pos2>| painter.add(Shape::line(pts, st));
        let closed = |pts: Vec<Pos2>| painter.add(Shape::closed_line(pts, st));
        let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<Pos2> {
            (0..=16).map(|i| a0 + (a1 - a0) * i as f32 / 16.0).map(|a| p(cx + r * a.cos(), cy + r * a.sin())).collect()
        };
        use std::f32::consts::PI;
        match self {
            Icon::Pointer => {
                painter.add(Shape::convex_polygon(
                    vec![p(0.25, 0.12), p(0.78, 0.55), p(0.52, 0.58), p(0.38, 0.86)],
                    Color32::TRANSPARENT,
                    st,
                ));
            }
            Icon::Hand => {
                for (x, top) in [(0.3, 0.32), (0.43, 0.2), (0.56, 0.22), (0.69, 0.32)] {
                    line(vec![p(x, 0.58), p(x, top)]);
                }
                line(vec![p(0.3, 0.55), p(0.3, 0.7), p(0.42, 0.86), p(0.64, 0.86), p(0.75, 0.66), p(0.69, 0.4)]);
                line(vec![p(0.3, 0.6), p(0.18, 0.5)]);
            }
            Icon::Pen => {
                closed(vec![p(0.68, 0.14), p(0.86, 0.32), p(0.36, 0.82), p(0.16, 0.84), p(0.18, 0.64)]);
                line(vec![p(0.58, 0.24), p(0.76, 0.42)]);
            }
            Icon::Rect => {
                closed(vec![p(0.18, 0.24), p(0.82, 0.24), p(0.82, 0.76), p(0.18, 0.76)]);
            }
            Icon::Ellipse => {
                closed(arc(0.5, 0.5, 0.32, 0.0, 2.0 * PI));
            }
            Icon::Diamond => {
                closed(vec![p(0.5, 0.14), p(0.86, 0.5), p(0.5, 0.86), p(0.14, 0.5)]);
            }
            Icon::Line => {
                line(vec![p(0.2, 0.8), p(0.8, 0.2)]);
            }
            Icon::Arrow => {
                line(vec![p(0.2, 0.8), p(0.8, 0.2)]);
                line(vec![p(0.48, 0.2), p(0.8, 0.2), p(0.8, 0.52)]);
            }
            Icon::Text => {
                line(vec![p(0.22, 0.28), p(0.22, 0.2), p(0.78, 0.2), p(0.78, 0.28)]);
                line(vec![p(0.5, 0.2), p(0.5, 0.82)]);
                line(vec![p(0.38, 0.82), p(0.62, 0.82)]);
            }
            Icon::Bucket => {
                // Tilted paint can with a drip.
                closed(vec![p(0.18, 0.42), p(0.48, 0.12), p(0.80, 0.44), p(0.50, 0.74)]);
                line(vec![p(0.18, 0.42), p(0.80, 0.44)]);
                line(vec![p(0.48, 0.12), p(0.40, 0.06), p(0.30, 0.10)]);
                painter.add(Shape::convex_polygon(vec![p(0.80, 0.50), p(0.86, 0.66), p(0.80, 0.74), p(0.74, 0.66)], color, Stroke::NONE));
            }
            Icon::Undo | Icon::Redo => {
                let flip = self == Icon::Redo;
                let fx = |x: f32| if flip { 1.0 - x } else { x };
                let pts: Vec<Pos2> = arc(0.5, 0.56, 0.26, -PI * 0.95, PI * 0.35).into_iter().map(|q| pos2(if flip { o.x + s - (q.x - o.x) } else { q.x }, q.y)).collect();
                line(pts);
                line(vec![p(fx(0.12), 0.34), p(fx(0.25), 0.55), p(fx(0.40), 0.40)]);
            }
            Icon::Menu => {
                for y in [0.3, 0.5, 0.7] {
                    line(vec![p(0.22, y), p(0.78, y)]);
                }
            }
            Icon::FlipH => {
                line(vec![p(0.5, 0.12), p(0.5, 0.88)]);
                closed(vec![p(0.4, 0.25), p(0.4, 0.75), p(0.12, 0.75)]);
                painter.add(Shape::convex_polygon(vec![p(0.6, 0.25), p(0.88, 0.75), p(0.6, 0.75)], color, Stroke::NONE));
            }
            Icon::FlipV => {
                line(vec![p(0.12, 0.5), p(0.88, 0.5)]);
                closed(vec![p(0.25, 0.4), p(0.75, 0.4), p(0.75, 0.12)]);
                painter.add(Shape::convex_polygon(vec![p(0.25, 0.6), p(0.75, 0.6), p(0.75, 0.88)], color, Stroke::NONE));
            }
            Icon::RotateCw | Icon::RotateCcw => {
                let cw = self == Icon::RotateCw;
                let pts: Vec<Pos2> = arc(0.5, 0.54, 0.28, -PI * 0.5, PI * 1.1).into_iter().map(|q| if cw { pos2(o.x + s - (q.x - o.x), q.y) } else { q }).collect();
                line(pts);
                let fx = |x: f32| if cw { 1.0 - x } else { x };
                line(vec![p(fx(0.36), 0.14), p(fx(0.52), 0.26), p(fx(0.38), 0.4)]);
            }
            Icon::Forward | Icon::Backward => {
                let front = self == Icon::Forward;
                let back = vec![p(0.14, 0.14), p(0.58, 0.14), p(0.58, 0.58), p(0.14, 0.58)];
                let fore = vec![p(0.42, 0.42), p(0.86, 0.42), p(0.86, 0.86), p(0.42, 0.86)];
                if front {
                    closed(back);
                    painter.add(Shape::convex_polygon(fore, color, Stroke::NONE));
                } else {
                    painter.add(Shape::convex_polygon(back, color, Stroke::NONE));
                    painter.add(Shape::convex_polygon(fore.clone(), painter.ctx().global_style().visuals.window_fill, Stroke::NONE));
                    closed(fore);
                }
            }
            Icon::Duplicate => {
                closed(vec![p(0.14, 0.14), p(0.6, 0.14), p(0.6, 0.6), p(0.14, 0.6)]);
                closed(vec![p(0.4, 0.4), p(0.86, 0.4), p(0.86, 0.86), p(0.4, 0.86)]);
            }
            Icon::Delete => {
                line(vec![p(0.16, 0.26), p(0.84, 0.26)]);
                line(vec![p(0.4, 0.26), p(0.4, 0.14), p(0.6, 0.14), p(0.6, 0.26)]);
                closed(vec![p(0.24, 0.26), p(0.76, 0.26), p(0.7, 0.86), p(0.3, 0.86)]);
            }
            Icon::Minus => {
                line(vec![p(0.24, 0.5), p(0.76, 0.5)]);
            }
            Icon::Plus => {
                line(vec![p(0.24, 0.5), p(0.76, 0.5)]);
                line(vec![p(0.5, 0.24), p(0.5, 0.76)]);
            }
            Icon::Check => {
                line(vec![p(0.2, 0.52), p(0.42, 0.74), p(0.8, 0.28)]);
            }
        }
    }
}
