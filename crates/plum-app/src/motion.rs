//! UI motion. Every animated value eases exponentially toward its target:
//! it reacts on the very next frame when the target changes (no waiting for a
//! tween to finish), decelerates smoothly, and settles fast. Repaints are only
//! requested while something is still moving, so an idle app stays idle.

use egui::{Context, Id};

/// Rates are "per second"; ~20 reaches 90% of the way in ~115 ms.
pub const SNAPPY: f32 = 22.0;
pub const SMOOTH: f32 = 14.0;

#[derive(Clone, Copy, Debug)]
pub struct Motion {
    /// Multiplier from `[ui] animation_speed`; 0 disables animation.
    pub speed: f32,
}

impl Motion {
    /// Ease the value stored under `id` toward `target` and return it.
    /// A value seen for the first time starts at `target` (no animation).
    pub fn approach(self, ctx: &Context, id: Id, target: f32, rate: f32) -> f32 {
        if self.speed <= 0.0 {
            ctx.data_mut(|d| d.insert_temp(id, target));
            return target;
        }
        // Clamp so the first frame after a long idle doesn't jump straight to the end.
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 1.0 / 30.0);
        let k = 1.0 - (-rate * self.speed * dt).exp();
        let v = ctx.data_mut(|d| {
            let cur = d.get_temp_mut_or_insert_with(id, || target);
            *cur += (target - *cur) * k;
            if (target - *cur).abs() < 1e-3 {
                *cur = target;
            }
            *cur
        });
        if v != target {
            ctx.request_repaint();
        }
        v
    }

    /// 0..1 fade for something that toggles on and off.
    pub fn fade(self, ctx: &Context, id: Id, on: bool, rate: f32) -> f32 {
        self.approach(ctx, id, if on { 1.0 } else { 0.0 }, rate)
    }

    /// Jump straight to `v` (e.g. reset a fade so the next show animates in).
    pub fn set(self, ctx: &Context, id: Id, v: f32) {
        ctx.data_mut(|d| d.insert_temp(id, v));
    }

    /// Fraction to move this frame for an exponential ease at `rate` (for
    /// values that don't live in egui's memory, like the camera).
    pub fn step(self, dt: f32, rate: f32) -> f32 {
        if self.speed <= 0.0 { 1.0 } else { 1.0 - (-rate * self.speed * dt.clamp(0.0, 1.0 / 30.0)).exp() }
    }
}

/// Ease-out cubic for one-shot progress values (0..1).
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}
