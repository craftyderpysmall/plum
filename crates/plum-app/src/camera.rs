use egui::{Pos2, Rect, pos2};
use glam::{DAffine2, DVec2};
use plum_core::Aabb;

/// View onto the infinite canvas. World coordinates are `f64`; only the
/// final screen positions are `f32`, so precision holds anywhere on the board.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub center: DVec2,
    pub zoom: f64,
    /// Screen rectangle the canvas occupies (points).
    pub viewport: Rect,
}

impl Default for Camera {
    fn default() -> Self {
        Self { center: DVec2::ZERO, zoom: 1.0, viewport: Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)) }
    }
}

impl Camera {
    fn screen_center(&self) -> DVec2 {
        let c = self.viewport.center();
        DVec2::new(c.x as f64, c.y as f64)
    }

    pub fn world_to_screen(&self, w: DVec2) -> Pos2 {
        let s = (w - self.center) * self.zoom + self.screen_center();
        pos2(s.x as f32, s.y as f32)
    }

    pub fn screen_to_world(&self, p: Pos2) -> DVec2 {
        (DVec2::new(p.x as f64, p.y as f64) - self.screen_center()) / self.zoom + self.center
    }

    /// World → screen as an affine (`screen = A * world`).
    pub fn affine(&self) -> DAffine2 {
        DAffine2::from_translation(self.screen_center()) * DAffine2::from_scale(DVec2::splat(self.zoom)) * DAffine2::from_translation(-self.center)
    }

    pub fn visible_world(&self) -> Aabb {
        Aabb::new(self.screen_to_world(self.viewport.min), self.screen_to_world(self.viewport.max))
    }

    /// Pan by a screen-space delta (content follows the pointer).
    pub fn pan(&mut self, delta: egui::Vec2) {
        self.center -= DVec2::new(delta.x as f64, delta.y as f64) / self.zoom;
    }

    /// Zoom keeping the world point under `anchor` fixed on screen.
    pub fn zoom_at(&mut self, anchor: Pos2, factor: f64, min: f64, max: f64) {
        let before = self.screen_to_world(anchor);
        self.zoom = (self.zoom * factor).clamp(min, max);
        let after = self.screen_to_world(anchor);
        self.center += before - after;
    }

    /// Zoom with a rubber band: past `min`/`max` the zoom keeps moving but with
    /// growing resistance, up to `overshoot`× beyond the limit. [`Self::settle`]
    /// springs it back.
    pub fn zoom_soft(&mut self, anchor: Pos2, factor: f64, min: f64, max: f64, overshoot: f64) {
        let give = overshoot.max(1.0).ln().max(1e-6);
        let (lmin, lmax) = (min.ln(), max.ln());
        let l = self.zoom.ln();
        let mut target = l + factor.ln();
        // Distance already past a limit damps further movement in that direction.
        let damp = |past: f64| 0.35 * (1.0 - past / give).clamp(0.0, 1.0);
        if target > lmax && factor > 1.0 {
            let start = l.max(lmax);
            let inside = (lmax - l).max(0.0);
            target = start + (factor.ln() - inside) * damp(start - lmax);
        } else if target < lmin && factor < 1.0 {
            let start = l.min(lmin);
            let inside = (l - lmin).max(0.0);
            target = start - (-factor.ln() - inside) * damp(lmin - start);
        }
        let target = target.clamp(lmin - give, lmax + give);
        self.zoom_at(anchor, (target - l).exp(), f64::MIN_POSITIVE, f64::MAX);
    }

    /// Ease an overshot zoom back inside `[min, max]`. Returns true while moving.
    pub fn settle(&mut self, anchor: Pos2, min: f64, max: f64, dt: f64) -> bool {
        let goal = self.zoom.clamp(min, max);
        if goal == self.zoom {
            return false;
        }
        let (l, lg) = (self.zoom.ln(), goal.ln());
        let t = 1.0 - (-dt.clamp(0.0, 0.1) * 14.0).exp();
        let next = if (lg - l).abs() < 1e-3 { lg } else { l + (lg - l) * t };
        self.zoom_at(anchor, (next - l).exp(), f64::MIN_POSITIVE, f64::MAX);
        if next == lg {
            self.zoom = goal;
        }
        true
    }

    /// Move fraction `t` of the way toward `goal` (zoom eases in log space so
    /// zooming feels even). Returns true once it has arrived.
    pub fn blend_toward(&mut self, goal: &Camera, t: f64) -> bool {
        let t = t.clamp(0.0, 1.0);
        self.zoom = (self.zoom.ln() + (goal.zoom.ln() - self.zoom.ln()) * t).exp();
        self.center += (goal.center - self.center) * t;
        let arrived = (goal.zoom / self.zoom - 1.0).abs() < 1e-3 && (goal.center - self.center).length() * self.zoom < 0.25;
        if arrived {
            self.center = goal.center;
            self.zoom = goal.zoom;
        }
        arrived
    }

    pub fn fit(&mut self, area: &Aabb, margin: f32, min: f64, max: f64) {
        if area.is_empty() {
            return;
        }
        let vs = self.viewport.shrink(margin).size();
        let size = area.size().max(DVec2::splat(1.0));
        self.zoom = (vs.x as f64 / size.x).min(vs.y as f64 / size.y).clamp(min, max);
        self.center = area.center();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_far_from_origin() {
        let cam = Camera { center: DVec2::new(1e8, -1e8), zoom: 3.7, ..Default::default() };
        let w = DVec2::new(1e8 + 12.25, -1e8 - 3.5);
        let back = cam.screen_to_world(cam.world_to_screen(w));
        assert!((back - w).length() < 1e-3);
        let a = cam.affine().transform_point2(w);
        let s = cam.world_to_screen(w);
        assert!((a.x as f32 - s.x).abs() < 1e-3 && (a.y as f32 - s.y).abs() < 1e-3);
    }

    #[test]
    fn soft_zoom_overshoots_then_settles() {
        let mut cam = Camera { zoom: 60.0, ..Default::default() };
        let a = pos2(10.0, 10.0);
        for _ in 0..50 {
            cam.zoom_soft(a, 1.2, 0.02, 64.0, 1.6);
        }
        assert!(cam.zoom > 64.0 && cam.zoom <= 64.0 * 1.6 + 1e-9, "{}", cam.zoom);
        let mut frames = 0;
        while cam.settle(a, 0.02, 64.0, 1.0 / 60.0) {
            frames += 1;
            assert!(frames < 600);
        }
        assert_eq!(cam.zoom, 64.0);
        // Inside the limits it behaves like a normal zoom.
        cam.zoom_soft(a, 0.5, 0.02, 64.0, 1.6);
        assert!((cam.zoom - 32.0).abs() < 1e-9);
    }

    #[test]
    fn blend_arrives_exactly() {
        let mut cam = Camera::default();
        let goal = Camera { center: DVec2::new(300.0, -40.0), zoom: 4.0, ..Default::default() };
        let mut frames = 0;
        while !cam.blend_toward(&goal, 0.25) {
            frames += 1;
            assert!(frames < 200);
        }
        assert_eq!((cam.center, cam.zoom), (goal.center, goal.zoom));
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut cam = Camera::default();
        let anchor = pos2(100.0, 50.0);
        let w = cam.screen_to_world(anchor);
        cam.zoom_at(anchor, 2.5, 0.01, 100.0);
        assert!((cam.screen_to_world(anchor) - w).length() < 1e-9);
    }
}
