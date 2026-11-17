//! Rays, planes and frusta.

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

/// Six-plane view frustum (inward-pointing normals: `signed_distance(p) >= 0` is inside).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    /// Order: left, right, bottom, top, near, far.
    pub planes: [Plane3; 6],
}

impl Frustum {
    /// Extract a 6-plane frustum (Gribb-Hartmann) from a row-major 4x4 view-projection matrix.
    /// Near plane uses standard OpenGL/clip `[-w, w]` (`r3 + r2`); see [`Self::from_view_proj_zero_to_one`] for WebGPU `[0, w]`.
    pub fn from_view_proj_row_major(m: AffineMatrix) -> Option<Self> {
        let near = Plane3::from_coeffs(
            m[3][0] + m[2][0],
            m[3][1] + m[2][1],
            m[3][2] + m[2][2],
            m[3][3] + m[2][3],
        )?;
        Self::from_view_proj_with_near(m, near)
    }

    /// Extract a 6-plane frustum from a row-major 4x4 view-projection matrix using WebGPU/Vulkan/D3D
    /// zero-to-one `[0, w]` clip-space depth (`z >= 0` from `r2`).
    pub fn from_view_proj_zero_to_one(m: AffineMatrix) -> Option<Self> {
        let r2 = m[2];
        let near = Plane3::from_coeffs(r2[0], r2[1], r2[2], r2[3])?;
        Self::from_view_proj_with_near(m, near)
    }

    fn from_view_proj_with_near(m: AffineMatrix, near: Plane3) -> Option<Self> {
        let r0 = m[0];
        let r1 = m[1];
        let r2 = m[2];
        let r3 = m[3];
        let left = Plane3::from_coeffs(r3[0] + r0[0], r3[1] + r0[1], r3[2] + r0[2], r3[3] + r0[3])?;
        let right =
            Plane3::from_coeffs(r3[0] - r0[0], r3[1] - r0[1], r3[2] - r0[2], r3[3] - r0[3])?;
        let bottom =
            Plane3::from_coeffs(r3[0] + r1[0], r3[1] + r1[1], r3[2] + r1[2], r3[3] + r1[3])?;
        let top = Plane3::from_coeffs(r3[0] - r1[0], r3[1] - r1[1], r3[2] - r1[2], r3[3] - r1[3])?;
        let far = Plane3::from_coeffs(r3[0] - r2[0], r3[1] - r2[1], r3[2] - r2[2], r3[3] - r2[3])?;
        Some(Self {
            planes: [left, right, bottom, top, near, far],
        })
    }

    /// Extract a 6-plane frustum from a column-major 16-element view-projection matrix (WebGPU/wgpu layout).
    pub fn from_view_proj_col_major(col_major: &[f64; 16]) -> Option<Self> {
        let m = crate::affine::solid_column_major(*col_major);
        Self::from_view_proj_row_major(m)
    }

    #[inline]
    pub fn contains_point(&self, point: V3) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.signed_distance(point) >= -1e-9)
    }

    #[inline]
    pub fn intersects_sphere(&self, center: V3, radius: f64) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.signed_distance(center) >= -radius)
    }

    /// Conservative frustum-AABB intersection test using the positive-vertex (p-vertex) test.
    #[inline]
    pub fn intersects_aabb(&self, min: V3, max: V3) -> bool {
        for plane in &self.planes {
            let p_vertex = [
                if plane.normal[0] >= 0.0 {
                    max[0]
                } else {
                    min[0]
                },
                if plane.normal[1] >= 0.0 {
                    max[1]
                } else {
                    min[1]
                },
                if plane.normal[2] >= 0.0 {
                    max[2]
                } else {
                    min[2]
                },
            ];
            if plane.signed_distance(p_vertex) < 0.0 {
                return false;
            }
        }
        true
    }

    /// Batch SIMD-friendly frustum culling mask over a slice of [`Aabb3`] bounding boxes.
    /// Precomputes plane normal sign selectors so each AABB evaluation avoids per-plane branchy sign checks.
    /// Returns a `Vec<bool>` where `true` indicates the AABB is visible (intersects the frustum).
    pub fn cull_aabbs_mask(&self, aabbs: &[Aabb3]) -> Vec<bool> {
        let sign_pos: [[bool; 3]; 6] = std::array::from_fn(|p| {
            let n = self.planes[p].normal;
            [n[0] >= 0.0, n[1] >= 0.0, n[2] >= 0.0]
        });
        aabbs
            .iter()
            .map(|aabb| {
                if aabb.is_empty() {
                    return false;
                }
                for (plane, signs) in self.planes.iter().zip(sign_pos.iter()) {
                    let px = if signs[0] { aabb.max[0] } else { aabb.min[0] };
                    let py = if signs[1] { aabb.max[1] } else { aabb.min[1] };
                    let pz = if signs[2] { aabb.max[2] } else { aabb.min[2] };
                    if plane.normal[0].mul_add(
                        px,
                        plane.normal[1].mul_add(py, plane.normal[2].mul_add(pz, plane.d)),
                    ) < 0.0
                    {
                        return false;
                    }
                }
                true
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frustum_zero_to_one_enforces_non_negative_clip_z() {
        let frustum = Frustum::from_view_proj_zero_to_one(IDENTITY).unwrap();
        assert!(frustum.contains_point([0.0, 0.0, 0.5]));
        assert!(frustum.contains_point([0.0, 0.0, 0.0]));
        assert!(!frustum.contains_point([0.0, 0.0, -0.1]));
        assert!(!frustum.contains_point([0.0, 0.0, 1.1]));
    }
}
