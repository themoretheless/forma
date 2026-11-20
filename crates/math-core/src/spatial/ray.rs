//! 3D rays and ray-primitive intersection queries.

use super::*;

/// Oriented 3D ray with origin and unit direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray3 {
    pub origin: V3,
    pub direction: V3,
}

impl Ray3 {
    pub fn new(origin: V3, direction: V3) -> Result<Self> {
        if !origin.iter().all(|v| v.is_finite()) || !direction.iter().all(|v| v.is_finite()) {
            return Err(Error::new(
                "invalid_ray",
                "Ray3 expects finite origin and direction coordinates",
            ));
        }
        let len = norm(direction);
        if !len.is_finite() || len == 0.0 {
            return Err(Error::new("invalid_ray", "Ray3 direction must be non-zero"));
        }
        let dir = unit(direction);
        Ok(Self {
            origin,
            direction: dir,
        })
    }

    #[inline]
    pub fn at(&self, t: f64) -> V3 {
        add(self.origin, scale(self.direction, t))
    }

    /// Ray-AABB slab intersection returning `Some((t_near, t_far))` when hit with `t_far >= 0`.
    #[inline]
    pub fn intersect_aabb(&self, min: V3, max: V3) -> Option<(f64, f64)> {
        let mut t_min = f64::NEG_INFINITY;
        let mut t_max = f64::INFINITY;
        for axis in 0..3 {
            let d = self.direction[axis];
            let o = self.origin[axis];
            if d.abs() < 1e-15 {
                if o < min[axis] || o > max[axis] {
                    return None;
                }
            } else {
                let inv = 1.0 / d;
                let mut t1 = (min[axis] - o) * inv;
                let mut t2 = (max[axis] - o) * inv;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                t_min = t_min.max(t1);
                t_max = t_max.min(t2);
                if t_min > t_max {
                    return None;
                }
            }
        }
        if t_max < 0.0 {
            return None;
        }
        Some((t_min.max(0.0), t_max))
    }

    /// Ray-sphere intersection returning `Some((t_enter, t_exit))` when hit with `t_exit >= 0`.
    #[inline]
    pub fn intersect_sphere(&self, center: V3, radius: f64) -> Option<(f64, f64)> {
        if !radius.is_finite() || radius < 0.0 {
            return None;
        }
        let oc = sub(self.origin, center);
        let b = dot(oc, self.direction);
        let c = dot(oc, oc) - radius * radius;
        let disc = b * b - c;
        if disc < 0.0 {
            return None;
        }
        let s = disc.sqrt();
        let t0 = -b - s;
        let t1 = -b + s;
        if t1 < 0.0 {
            return None;
        }
        Some((t0.max(0.0), t1))
    }

    /// Möller–Trumbore ray-triangle intersection returning `(t, u, v)` when hit with `t >= 0.0`,
    /// where `P = (1 - u - v) * v0 + u * v1 + v * v2` (`u >= 0`, `v >= 0`, `u + v <= 1`).
    #[inline]
    pub fn intersect_triangle(&self, v0: V3, v1: V3, v2: V3) -> Option<(f64, f64, f64)> {
        ray_triangle_intersect_moller_trumbore(self, v0, v1, v2)
    }

    /// Batch closest-hit ray-triangle intersection over an indexed triangle mesh.
    /// Returns `Some((tri_index, t, u, v))` for the closest intersected triangle with `t >= 0.0`.
    pub fn intersect_triangles_closest(
        &self,
        vertices: &[V3],
        triangles: &[[usize; 3]],
    ) -> Option<(usize, f64, f64, f64)> {
        let mut best: Option<(usize, f64, f64, f64)> = None;
        let mut closest_t = f64::INFINITY;
        for (tri_idx, &[i0, i1, i2]) in triangles.iter().enumerate() {
            if i0 >= vertices.len() || i1 >= vertices.len() || i2 >= vertices.len() {
                continue;
            }
            if let Some((t, u, v)) = ray_triangle_intersect_moller_trumbore(
                self,
                vertices[i0],
                vertices[i1],
                vertices[i2],
            ) {
                if t < closest_t {
                    closest_t = t;
                    best = Some((tri_idx, t, u, v));
                }
            }
        }
        best
    }
}
