use glam::{DAffine2, DVec2};
use serde::{Deserialize, Serialize};

/// Placement of an element on the canvas: `world = pos + R(rotation) * (scale * local)`.
///
/// A negative scale component is a flip along that local axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub pos: DVec2,
    /// Radians, clockwise on screen (y points down).
    pub rotation: f64,
    pub scale: DVec2,
}

impl Default for Transform {
    fn default() -> Self {
        Self { pos: DVec2::ZERO, rotation: 0.0, scale: DVec2::ONE }
    }
}

impl Transform {
    pub fn at(pos: DVec2) -> Self {
        Self { pos, ..Default::default() }
    }

    pub fn affine(&self) -> DAffine2 {
        DAffine2::from_scale_angle_translation(self.scale, self.rotation, self.pos)
    }

    pub fn translated(mut self, d: DVec2) -> Self {
        self.pos += d;
        self
    }

    /// Rotate around a world-space pivot.
    pub fn rotated_about(mut self, pivot: DVec2, angle: f64) -> Self {
        self.pos = pivot + DVec2::from_angle(angle).rotate(self.pos - pivot);
        self.rotation = normalize_angle(self.rotation + angle);
        self
    }

    /// Mirror across the vertical world line `x = pivot_x`.
    ///
    /// Uses `Fx * R(θ) = R(-θ) * Fx`, so the flip is absorbed into the local scale.
    pub fn flipped_h(mut self, pivot_x: f64) -> Self {
        self.pos.x = 2.0 * pivot_x - self.pos.x;
        self.rotation = normalize_angle(-self.rotation);
        self.scale.x = -self.scale.x;
        self
    }

    /// Mirror across the horizontal world line `y = pivot_y`.
    pub fn flipped_v(mut self, pivot_y: f64) -> Self {
        self.pos.y = 2.0 * pivot_y - self.pos.y;
        self.rotation = normalize_angle(-self.rotation);
        self.scale.y = -self.scale.y;
        self
    }

    /// Smallest absolute scale; converts world tolerances into local units.
    pub fn min_abs_scale(&self) -> f64 {
        self.scale.x.abs().min(self.scale.y.abs()).max(1e-9)
    }
}

pub fn normalize_angle(a: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let r = a.rem_euclid(tau);
    if r > std::f64::consts::PI { r - tau } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: DVec2, b: DVec2) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn affine_inverse_round_trip() {
        let t = Transform { pos: DVec2::new(1e7, -3.0), rotation: 0.4, scale: DVec2::new(-2.0, 0.5) };
        let p = DVec2::new(3.0, 4.0);
        let a = t.affine();
        assert!(close(a.inverse().transform_point2(a.transform_point2(p)), p));
    }

    #[test]
    fn flip_h_mirrors_every_point() {
        let t = Transform { pos: DVec2::new(3.0, 1.0), rotation: 0.6, scale: DVec2::new(1.5, 2.0) };
        let f = t.flipped_h(10.0);
        for p in [DVec2::ZERO, DVec2::new(1.0, 0.0), DVec2::new(-2.0, 5.0)] {
            let w = t.affine().transform_point2(p);
            let wf = f.affine().transform_point2(p);
            assert!(close(wf, DVec2::new(20.0 - w.x, w.y)), "{wf:?} vs {w:?}");
        }
    }

    #[test]
    fn flip_v_mirrors_every_point() {
        let t = Transform { pos: DVec2::new(3.0, 1.0), rotation: -1.1, scale: DVec2::new(1.0, -2.0) };
        let f = t.flipped_v(-4.0);
        for p in [DVec2::ZERO, DVec2::new(1.0, 2.0)] {
            let w = t.affine().transform_point2(p);
            let wf = f.affine().transform_point2(p);
            assert!(close(wf, DVec2::new(w.x, -8.0 - w.y)));
        }
    }

    #[test]
    fn rotate_about_pivot() {
        let t = Transform::at(DVec2::new(2.0, 0.0)).rotated_about(DVec2::ZERO, std::f64::consts::FRAC_PI_2);
        assert!(close(t.pos, DVec2::new(0.0, 2.0)));
        let w = t.affine().transform_point2(DVec2::new(1.0, 0.0));
        assert!(close(w, DVec2::new(0.0, 3.0)));
    }
}
