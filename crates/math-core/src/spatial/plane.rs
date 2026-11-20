//! Oriented 3D plane defined by unit normal and offset.

use super::*;

/// 3D plane defined by `dot(normal, x) + d == 0`, with unit `normal`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane3 {
    pub normal: V3,
    pub d: f64,
}

impl Plane3 {
    /// Normalize `[a, b, c, d]` coefficients for `a*x + b*y + c*z + d = 0`.
    pub fn from_coeffs(a: f64, b: f64, c: f64, d: f64) -> Option<Self> {
        let len = norm([a, b, c]);
        if !len.is_finite() || len == 0.0 || !d.is_finite() {
            return None;
        }
        Some(Self {
            normal: [a / len, b / len, c / len],
            d: d / len,
        })
    }

    pub fn from_point_normal(point: V3, normal: V3) -> Option<Self> {
        let len = norm(normal);
        if !len.is_finite() || len == 0.0 {
            return None;
        }
        let n = unit(normal);
        let d = -dot(n, point);
        if !d.is_finite() {
            return None;
        }
        Some(Self { normal: n, d })
    }

    #[inline]
    pub fn signed_distance(&self, point: V3) -> f64 {
        dot(self.normal, point) + self.d
    }

    /// Intersect a ray with this plane, returning parameter `t >= 0.0`.
    #[inline]
    pub fn intersect_ray(&self, ray: &Ray3) -> Option<f64> {
        let denom = dot(self.normal, ray.direction);
        if denom.abs() < 1e-15 {
            return None;
        }
        let t = -self.signed_distance(ray.origin) / denom;
        if t >= 0.0 && t.is_finite() {
            Some(t)
        } else {
            None
        }
    }
}
