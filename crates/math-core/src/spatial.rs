//! Spatial primitives: quaternions (`Quat`), dual quaternions (`DualQuat`),
//! rays (`Ray3`), planes (`Plane3`), view frustums (`Frustum`), 3D bounding
//! boxes (`Aabb3`, `Obb3`), and triangle barycentric / closest-point queries.

use crate::affine::{AffineMatrix, IDENTITY};
use crate::{Error, M3, Result, V3, add, cross, dot, norm, scale, sub, unit};

/// Unit or general quaternion `[x, y, z, w]` where `w` is the scalar part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    #[inline]
    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    #[inline]
    pub const fn identity() -> Self {
        Self::IDENTITY
    }

    /// Construct a unit quaternion from a rotation axis and angle in radians.
    pub fn from_axis_angle(axis: V3, angle_rad: f64) -> Option<Self> {
        if !angle_rad.is_finite() || !axis.iter().all(|v| v.is_finite()) {
            return None;
        }
        let len = norm(axis);
        if !len.is_finite() || len == 0.0 {
            return None;
        }
        let u = unit(axis);
        let (s, c) = (0.5 * angle_rad).sin_cos();
        Some(Self {
            x: u[0] * s,
            y: u[1] * s,
            z: u[2] * s,
            w: c,
        })
    }

    /// Construct a unit quaternion from a 3x3 orthonormal rotation matrix.
    pub fn from_mat3(m: M3) -> Self {
        let trace = m[0][0] + m[1][1] + m[2][2];
        if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            Self {
                w: 0.25 * s,
                x: (m[2][1] - m[1][2]) / s,
                y: (m[0][2] - m[2][0]) / s,
                z: (m[1][0] - m[0][1]) / s,
            }
        } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
            let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
            Self {
                w: (m[2][1] - m[1][2]) / s,
                x: 0.25 * s,
                y: (m[0][1] + m[1][0]) / s,
                z: (m[0][2] + m[2][0]) / s,
            }
        } else if m[1][1] > m[2][2] {
            let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
            Self {
                w: (m[0][2] - m[2][0]) / s,
                x: (m[0][1] + m[1][0]) / s,
                y: 0.25 * s,
                z: (m[1][2] + m[2][1]) / s,
            }
        } else {
            let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
            Self {
                w: (m[1][0] - m[0][1]) / s,
                x: (m[0][2] + m[2][0]) / s,
                y: (m[1][2] + m[2][1]) / s,
                z: 0.25 * s,
            }
        }
        .normalize()
        .unwrap_or(Self::IDENTITY)
    }

    #[inline]
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    #[inline]
    pub fn norm_sq(self) -> f64 {
        self.dot(self)
    }

    #[inline]
    pub fn norm(self) -> f64 {
        self.norm_sq().sqrt()
    }

    #[inline]
    pub fn conjugate(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }

    #[inline]
    pub fn normalize(self) -> Option<Self> {
        let len = self.norm();
        if !len.is_finite() || len == 0.0 {
            return None;
        }
        Some(Self {
            x: self.x / len,
            y: self.y / len,
            z: self.z / len,
            w: self.w / len,
        })
    }

    #[inline]
    pub fn mul(self, rhs: Self) -> Self {
        Self {
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
        }
    }

    /// Rotate a 3D vector by this unit quaternion.
    #[inline]
    pub fn rotate_vec3(self, v: V3) -> V3 {
        let qv = [self.x, self.y, self.z];
        let uv = cross(qv, v);
        let uuv = cross(qv, uv);
        add(v, scale(add(scale(uv, self.w), uuv), 2.0))
    }

    /// Spherical linear interpolation along the shortest arc between unit quaternions.
    pub fn slerp(self, mut end: Self, t: f64) -> Self {
        let mut cos_theta = self.dot(end);
        if cos_theta < 0.0 {
            end = Self::new(-end.x, -end.y, -end.z, -end.w);
            cos_theta = -cos_theta;
        }
        if cos_theta > 0.9995 {
            let lerped = Self::new(
                self.x + t * (end.x - self.x),
                self.y + t * (end.y - self.y),
                self.z + t * (end.z - self.z),
                self.w + t * (end.w - self.w),
            );
            return lerped.normalize().unwrap_or(self);
        }
        let theta = cos_theta.clamp(-1.0, 1.0).acos();
        let sin_theta = theta.sin();
        let w0 = ((1.0 - t) * theta).sin() / sin_theta;
        let w1 = (t * theta).sin() / sin_theta;
        Self::new(
            self.x * w0 + end.x * w1,
            self.y * w0 + end.y * w1,
            self.z * w0 + end.z * w1,
            self.w * w0 + end.w * w1,
        )
    }

    /// Convert unit quaternion to 3x3 row-major rotation matrix.
    #[inline]
    pub fn to_mat3(self) -> M3 {
        let q = self.normalize().unwrap_or(Self::IDENTITY);
        let xx = q.x * q.x;
        let yy = q.y * q.y;
        let zz = q.z * q.z;
        let xy = q.x * q.y;
        let xz = q.x * q.z;
        let yz = q.y * q.z;
        let wx = q.w * q.x;
        let wy = q.w * q.y;
        let wz = q.w * q.z;
        [
            [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)],
            [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)],
            [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)],
        ]
    }

    /// Convert unit quaternion to 4x4 row-major affine matrix.
    #[inline]
    pub fn to_mat4(self) -> AffineMatrix {
        let m3 = self.to_mat3();
        let mut out = IDENTITY;
        for r in 0..3 {
            for c in 0..3 {
                out[r][c] = m3[r][c];
            }
        }
        out
    }
}

/// Dual quaternion `real + eps * dual` representing a 3D rigid body transformation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DualQuat {
    pub real: Quat,
    pub dual: Quat,
}

impl Default for DualQuat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl DualQuat {
    pub const IDENTITY: Self = Self {
        real: Quat::IDENTITY,
        dual: Quat::new(0.0, 0.0, 0.0, 0.0),
    };

    /// Construct a unit dual quaternion from a rotation quaternion and translation vector.
    #[inline]
    pub fn from_rotation_translation(rotation: Quat, translation: V3) -> Self {
        let r = rotation.normalize().unwrap_or(Quat::IDENTITY);
        let t_quat = Quat::new(
            0.5 * translation[0],
            0.5 * translation[1],
            0.5 * translation[2],
            0.0,
        );
        let dual = t_quat.mul(r);
        Self { real: r, dual }
    }

    /// Normalize a dual quaternion so `|real| == 1` and `dot(real, dual) == 0`.
    #[inline]
    pub fn normalize(self) -> Option<Self> {
        let mag = self.real.norm();
        if !mag.is_finite() || mag == 0.0 {
            return None;
        }
        let inv = 1.0 / mag;
        let real = Quat::new(
            self.real.x * inv,
            self.real.y * inv,
            self.real.z * inv,
            self.real.w * inv,
        );
        let dual_scaled = Quat::new(
            self.dual.x * inv,
            self.dual.y * inv,
            self.dual.z * inv,
            self.dual.w * inv,
        );
        let rd = real.dot(dual_scaled);
        let dual = Quat::new(
            dual_scaled.x - real.x * rd,
            dual_scaled.y - real.y * rd,
            dual_scaled.z - real.z * rd,
            dual_scaled.w - real.w * rd,
        );
        Some(Self { real, dual })
    }

    #[inline]
    pub fn mul(self, rhs: Self) -> Self {
        let r = self.real.mul(rhs.real);
        let d1 = self.real.mul(rhs.dual);
        let d2 = self.dual.mul(rhs.real);
        Self {
            real: r,
            dual: Quat::new(d1.x + d2.x, d1.y + d2.y, d1.z + d2.z, d1.w + d2.w),
        }
    }

    /// Extract the rotation quaternion and translation vector.
    #[inline]
    pub fn to_rotation_translation(self) -> (Quat, V3) {
        let dq = self.normalize().unwrap_or(Self::IDENTITY);
        let t_quat = dq.dual.mul(dq.real.conjugate());
        (
            dq.real,
            [2.0 * t_quat.x, 2.0 * t_quat.y, 2.0 * t_quat.z],
        )
    }

    /// Transform a 3D point by this dual quaternion.
    #[inline]
    pub fn transform_point(self, point: V3) -> V3 {
        let (rot, trans) = self.to_rotation_translation();
        add(rot.rotate_vec3(point), trans)
    }

    /// Dual quaternion linear blending (DLB) with antipodal sign correction and normalization.
    pub fn blend(self, mut other: Self, t: f64) -> Self {
        if self.real.dot(other.real) < 0.0 {
            other.real = Quat::new(-other.real.x, -other.real.y, -other.real.z, -other.real.w);
            other.dual = Quat::new(-other.dual.x, -other.dual.y, -other.dual.z, -other.dual.w);
        }
        let w0 = 1.0 - t;
        let w1 = t;
        let raw = Self {
            real: Quat::new(
                self.real.x * w0 + other.real.x * w1,
                self.real.y * w0 + other.real.y * w1,
                self.real.z * w0 + other.real.z * w1,
                self.real.w * w0 + other.real.w * w1,
            ),
            dual: Quat::new(
                self.dual.x * w0 + other.dual.x * w1,
                self.dual.y * w0 + other.dual.y * w1,
                self.dual.z * w0 + other.dual.z * w1,
                self.dual.w * w0 + other.dual.w * w1,
            ),
        };
        raw.normalize().unwrap_or(self)
    }
}

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
            return Err(Error::new(
                "invalid_ray",
                "Ray3 direction must be non-zero",
            ));
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
            if let Some((t, u, v)) =
                ray_triangle_intersect_moller_trumbore(self, vertices[i0], vertices[i1], vertices[i2])
            {
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
        let near = Plane3::from_coeffs(r3[0] + r2[0], r3[1] + r2[1], r3[2] + r2[2], r3[3] + r2[3])?;
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

/// 3D Axis-Aligned Bounding Box defined by inclusive `min` and `max` corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb3 {
    pub min: V3,
    pub max: V3,
}

impl Default for Aabb3 {
    fn default() -> Self {
        Self::empty()
    }
}

impl Aabb3 {
    /// Return an inverted (empty) bounding box ready for incremental expansion.
    #[inline]
    pub const fn empty() -> Self {
        Self {
            min: [f64::INFINITY, f64::INFINITY, f64::INFINITY],
            max: [f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
        }
    }

    #[inline]
    pub const fn new(min: V3, max: V3) -> Self {
        Self { min, max }
    }

    /// True when `min[i] > max[i]` on any axis.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1] || self.min[2] > self.max[2]
    }

    /// Build the smallest AABB enclosing `points`, or [`Self::empty()`] if `points` is empty.
    pub fn from_points(points: &[V3]) -> Self {
        let mut bbox = Self::empty();
        for &p in points {
            bbox.expand_point(p);
        }
        bbox
    }

    /// Expand this AABB in-place to include `point` (ignores non-finite coordinates).
    #[inline]
    pub fn expand_point(&mut self, point: V3) {
        if !point.iter().all(|v| v.is_finite()) {
            return;
        }
        for i in 0..3 {
            self.min[i] = self.min[i].min(point[i]);
            self.max[i] = self.max[i].max(point[i]);
        }
    }

    /// Smallest AABB enclosing both `self` and `other`.
    #[inline]
    pub fn union(&self, other: &Self) -> Self {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        Self {
            min: [
                self.min[0].min(other.min[0]),
                self.min[1].min(other.min[1]),
                self.min[2].min(other.min[2]),
            ],
            max: [
                self.max[0].max(other.max[0]),
                self.max[1].max(other.max[1]),
                self.max[2].max(other.max[2]),
            ],
        }
    }

    /// Intersection of `self` and `other` (returns an empty box if disjoint).
    #[inline]
    pub fn intersection(&self, other: &Self) -> Self {
        if self.is_empty() || other.is_empty() {
            return Self::empty();
        }
        let min = [
            self.min[0].max(other.min[0]),
            self.min[1].max(other.min[1]),
            self.min[2].max(other.min[2]),
        ];
        let max = [
            self.max[0].min(other.max[0]),
            self.max[1].min(other.max[1]),
            self.max[2].min(other.max[2]),
        ];
        if min[0] > max[0] || min[1] > max[1] || min[2] > max[2] {
            Self::empty()
        } else {
            Self { min, max }
        }
    }

    /// Side lengths `max - min` clamped to `>= 0.0` (returns `[0.0; 3]` if empty).
    #[inline]
    pub fn extents(&self) -> V3 {
        if self.is_empty() {
            return [0.0, 0.0, 0.0];
        }
        [
            (self.max[0] - self.min[0]).max(0.0),
            (self.max[1] - self.min[1]).max(0.0),
            (self.max[2] - self.min[2]).max(0.0),
        ]
    }

    /// Center `(min + max) * 0.5`.
    #[inline]
    pub fn center(&self) -> V3 {
        if self.is_empty() {
            return [0.0, 0.0, 0.0];
        }
        [
            0.5 * (self.min[0] + self.max[0]),
            0.5 * (self.min[1] + self.max[1]),
            0.5 * (self.min[2] + self.max[2]),
        ]
    }

    /// Surface area `2 * (dx*dy + dy*dz + dz*dx)`.
    #[inline]
    pub fn surface_area(&self) -> f64 {
        let e = self.extents();
        2.0 * (e[0] * e[1] + e[1] * e[2] + e[2] * e[0])
    }

    /// Volume `dx * dy * dz`.
    #[inline]
    pub fn volume(&self) -> f64 {
        let e = self.extents();
        e[0] * e[1] * e[2]
    }

    /// Index (`0`, `1`, or `2`) of the longest extent axis.
    #[inline]
    pub fn longest_axis(&self) -> usize {
        let e = self.extents();
        if e[0] >= e[1] && e[0] >= e[2] {
            0
        } else if e[1] >= e[2] {
            1
        } else {
            2
        }
    }

    /// True when `point` lies inside or on the boundary of this AABB.
    #[inline]
    pub fn contains_point(&self, point: V3) -> bool {
        !self.is_empty()
            && point[0] >= self.min[0]
            && point[0] <= self.max[0]
            && point[1] >= self.min[1]
            && point[1] <= self.max[1]
            && point[2] >= self.min[2]
            && point[2] <= self.max[2]
    }

    /// True when `self` and `other` overlap (including touching boundaries).
    #[inline]
    pub fn intersects(&self, other: &Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.min[0] <= other.max[0]
            && self.max[0] >= other.min[0]
            && self.min[1] <= other.max[1]
            && self.max[1] >= other.min[1]
            && self.min[2] <= other.max[2]
            && self.max[2] >= other.min[2]
    }
}

/// 3D Oriented Bounding Box defined by `center`, orthonormal local `axes` (rows), and positive `half_extents`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obb3 {
    pub center: V3,
    /// Orthonormal local axes `[u0, u1, u2]`.
    pub axes: M3,
    pub half_extents: V3,
}

impl Obb3 {
    #[inline]
    pub fn new(center: V3, axes: M3, half_extents: V3) -> Self {
        Self {
            center,
            axes,
            half_extents: [
                half_extents[0].abs(),
                half_extents[1].abs(),
                half_extents[2].abs(),
            ],
        }
    }

    /// Construct an axis-aligned OBB from an [`Aabb3`].
    #[inline]
    pub fn from_aabb(aabb: &Aabb3) -> Self {
        let e = aabb.extents();
        Self {
            center: aabb.center(),
            axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            half_extents: [0.5 * e[0], 0.5 * e[1], 0.5 * e[2]],
        }
    }

    /// Construct an OBB from center, unit rotation quaternion, and half-extents.
    pub fn from_rotation(center: V3, rotation: Quat, half_extents: V3) -> Self {
        let m = rotation.to_mat3();
        // Columns of rotation matrix are the rotated local X, Y, Z axes.
        let axes = [
            [m[0][0], m[1][0], m[2][0]],
            [m[0][1], m[1][1], m[2][1]],
            [m[0][2], m[1][2], m[2][2]],
        ];
        Self::new(center, axes, half_extents)
    }

    /// True if `point` lies within (or on the boundary of) this OBB.
    #[inline]
    pub fn contains_point(&self, point: V3) -> bool {
        let d = sub(point, self.center);
        for i in 0..3 {
            let proj = dot(d, self.axes[i]).abs();
            if proj > self.half_extents[i] + 1e-12 {
                return false;
            }
        }
        true
    }

    /// 15-axis Separating Axis Theorem (SAT) intersection test between two 3D OBBs (Gottschalk / Ericson).
    pub fn intersects_obb(&self, other: &Self) -> bool {
        const EPS: f64 = 1e-12;
        let a = &self.half_extents;
        let b = &other.half_extents;

        // Rotation matrix R[i][j] = dot(self.axes[i], other.axes[j])
        let mut r = [[0.0_f64; 3]; 3];
        let mut abs_r = [[0.0_f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = dot(self.axes[i], other.axes[j]);
                abs_r[i][j] = r[i][j].abs() + EPS;
            }
        }

        // Translation vector t in `self`'s local frame
        let t_world = sub(other.center, self.center);
        let t = [
            dot(t_world, self.axes[0]),
            dot(t_world, self.axes[1]),
            dot(t_world, self.axes[2]),
        ];

        // Test axes L = A0, A1, A2
        for i in 0..3 {
            let ra = a[i];
            let rb = b[0] * abs_r[i][0] + b[1] * abs_r[i][1] + b[2] * abs_r[i][2];
            if t[i].abs() > ra + rb {
                return false;
            }
        }

        // Test axes L = B0, B1, B2
        for j in 0..3 {
            let ra = a[0] * abs_r[0][j] + a[1] * abs_r[1][j] + a[2] * abs_r[2][j];
            let rb = b[j];
            let proj = (t[0] * r[0][j] + t[1] * r[1][j] + t[2] * r[2][j]).abs();
            if proj > ra + rb {
                return false;
            }
        }

        // Test 9 cross-product axes L = Ai x Bj
        for i in 0..3 {
            let i1 = (i + 1) % 3;
            let i2 = (i + 2) % 3;
            for j in 0..3 {
                let j1 = (j + 1) % 3;
                let j2 = (j + 2) % 3;
                let ra = a[i1] * abs_r[i2][j] + a[i2] * abs_r[i1][j];
                let rb = b[j1] * abs_r[i][j2] + b[j2] * abs_r[i][j1];
                let proj = (t[i2] * r[i1][j] - t[i1] * r[i2][j]).abs();
                if proj > ra + rb {
                    return false;
                }
            }
        }

        true
    }
}

/// Compute the 3D barycentric coordinates `[u, v, w]` of `p` projected onto triangle `(a, b, c)`
/// such that the orthogonal projection is `u*a + v*b + w*c` with `u + v + w == 1.0`.
/// Returns `None` if the triangle is degenerate (zero area) or inputs are non-finite.
pub fn barycentric_coords_3d(p: V3, a: V3, b: V3, c: V3) -> Option<V3> {
    let v0 = sub(b, a);
    let v1 = sub(c, a);
    let v2 = sub(p, a);
    let d00 = dot(v0, v0);
    let d01 = dot(v0, v1);
    let d11 = dot(v1, v1);
    let d20 = dot(v2, v0);
    let d21 = dot(v2, v1);
    let denom = d00 * d11 - d01 * d01;
    if !denom.is_finite() || denom.abs() <= 1e-24 {
        return None;
    }
    let inv = 1.0 / denom;
    let v = (d11 * d20 - d01 * d21) * inv;
    let w = (d00 * d21 - d01 * d20) * inv;
    let u = 1.0 - v - w;
    if u.is_finite() && v.is_finite() && w.is_finite() {
        Some([u, v, w])
    } else {
        None
    }
}

/// Find the closest point on 3D triangle `(a, b, c)` to query point `p` (Christer Ericson's Voronoi region algorithm).
pub fn closest_point_on_triangle_3d(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }

    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let denom = d1 - d3;
        let v = if denom.abs() > 1e-24 { d1 / denom } else { 0.0 };
        return add(a, scale(ab, v));
    }

    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let denom = d2 - d6;
        let w = if denom.abs() > 1e-24 { d2 / denom } else { 0.0 };
        return add(a, scale(ac, w));
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let bc = sub(c, b);
        let denom = (d4 - d3) + (d5 - d6);
        let w = if denom.abs() > 1e-24 {
            (d4 - d3) / denom
        } else {
            0.0
        };
        return add(b, scale(bc, w));
    }

    let sum = va + vb + vc;
    if sum.abs() <= 1e-24 {
        return a;
    }
    let denom = 1.0 / sum;
    let v = vb * denom;
    let w = vc * denom;
    add(a, add(scale(ab, v), scale(ac, w)))
}

/// Möller–Trumbore ray-triangle intersection returning `Some((t, u, v))` when `ray` hits
/// triangle `(v0, v1, v2)` at distance `t >= 0.0` with barycentric weights `u >= 0.0`, `v >= 0.0`,
/// `u + v <= 1.0` such that the intersection point is `(1 - u - v) * v0 + u * v1 + v * v2`.
#[inline]
pub fn ray_triangle_intersect_moller_trumbore(
    ray: &Ray3,
    v0: V3,
    v1: V3,
    v2: V3,
) -> Option<(f64, f64, f64)> {
    const EPS: f64 = 1e-12;
    let edge1 = sub(v1, v0);
    let edge2 = sub(v2, v0);
    let h = cross(ray.direction, edge2);
    let a = dot(edge1, h);
    if !a.is_finite() || a.abs() < EPS {
        return None;
    }
    let f = 1.0 / a;
    let s = sub(ray.origin, v0);
    let u = f * dot(s, h);
    if !(-EPS..=1.0 + EPS).contains(&u) {
        return None;
    }
    let q = cross(s, edge1);
    let v = f * dot(ray.direction, q);
    if v < -EPS || u + v > 1.0 + EPS {
        return None;
    }
    let t = f * dot(edge2, q);
    if t >= 0.0 && t.is_finite() {
        Some((t, u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
    } else {
        None
    }
}

/// Solve a 3x3 real symmetric eigenvalue problem `A * V = V * diag(lambda)` using exact
/// Jacobi rotations, returning `(eigenvalues, eigenvectors_columns)` sorted in descending
/// order of eigenvalue (`lambda[0] >= lambda[1] >= lambda[2]`).
///
/// The returned `eigenvectors` matrix `V` has orthonormal columns (`V[row][col]` is the `row`-th
/// component of the `col`-th unit eigenvector) with right-handed orientation (`det(V) == +1`).
pub fn symmetric_eigen3(a: M3) -> (V3, M3) {
    // Symmetrize input defensively in case of tiny numerical asymmetry.
    let sym = [
        [a[0][0], 0.5 * (a[0][1] + a[1][0]), 0.5 * (a[0][2] + a[2][0])],
        [0.5 * (a[1][0] + a[0][1]), a[1][1], 0.5 * (a[1][2] + a[2][1])],
        [0.5 * (a[2][0] + a[0][2]), 0.5 * (a[2][1] + a[1][2]), a[2][2]],
    ];
    let (raw_d, raw_v) = crate::linalg::eigen(sym);
    let mut order = [0usize, 1, 2];
    order.sort_by(|&i, &j| raw_d[j].total_cmp(&raw_d[i]));
    let eigenvalues = [raw_d[order[0]], raw_d[order[1]], raw_d[order[2]]];
    let c0 = unit([raw_v[0][order[0]], raw_v[1][order[0]], raw_v[2][order[0]]]);
    let c1 = unit([raw_v[0][order[1]], raw_v[1][order[1]], raw_v[2][order[1]]]);
    let mut c2 = unit([raw_v[0][order[2]], raw_v[1][order[2]], raw_v[2][order[2]]]);
    if dot(cross(c0, c1), c2) < 0.0 {
        c2 = scale(c2, -1.0);
    }
    let eigenvectors = [
        [c0[0], c1[0], c2[0]],
        [c0[1], c1[1], c2[1]],
        [c0[2], c1[2], c2[2]],
    ];
    (eigenvalues, eigenvectors)
}

/// Solve the 3x3 linear system `A * x = b` using Gaussian elimination with partial pivoting
/// and condition/finiteness checks. Returns `None` if `A` is singular, nearly singular, or contains
/// non-finite entries.
pub fn solve_linear_3x3(a: M3, b: V3) -> Option<V3> {
    if !a.iter().flatten().all(|v| v.is_finite()) || !b.iter().all(|v| v.is_finite()) {
        return None;
    }
    let max_entry = a
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0.0_f64, f64::max);
    if max_entry <= 1e-30 {
        return None;
    }
    let det_a = crate::linalg::det(a);
    let scale3 = max_entry * max_entry * max_entry;
    if !det_a.is_finite() || det_a.abs() <= 1e-14 * scale3 {
        return None;
    }
    crate::linalg::solve(a, b)
}

/// 3D Catmull-Rom spline segment defined by four control points `(p0, p1, p2, p3)` and
/// chordal exponent `alpha` (`0.0` = uniform, `0.5` = centripetal, `1.0` = chordal).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CatmullRomSpline3 {
    pub p0: V3,
    pub p1: V3,
    pub p2: V3,
    pub p3: V3,
    pub alpha: f64,
}

impl CatmullRomSpline3 {
    #[inline]
    pub const fn new(p0: V3, p1: V3, p2: V3, p3: V3, alpha: f64) -> Self {
        Self {
            p0,
            p1,
            p2,
            p3,
            alpha,
        }
    }

    /// Construct a centripetal (`alpha = 0.5`) 3D Catmull-Rom segment, which guarantees no cusps or self-intersections.
    #[inline]
    pub const fn centripetal(p0: V3, p1: V3, p2: V3, p3: V3) -> Self {
        Self::new(p0, p1, p2, p3, 0.5)
    }

    #[inline]
    pub fn evaluate(&self, t: f64) -> V3 {
        evaluate_catmull_rom_3d(self.p0, self.p1, self.p2, self.p3, t, self.alpha)
    }

    #[inline]
    pub fn tangent(&self, t: f64) -> V3 {
        evaluate_catmull_rom_tangent_3d(self.p0, self.p1, self.p2, self.p3, t, self.alpha)
    }
}

/// Evaluate a 3D Barry–Goldman parameterized Catmull-Rom spline between `p1` (`t = 0`) and `p2` (`t = 1`)
/// with neighbor controls `p0`, `p3` and parameterization exponent `alpha` (`0.0` = uniform, `0.5` = centripetal).
pub fn evaluate_catmull_rom_3d(p0: V3, p1: V3, p2: V3, p3: V3, t: f64, alpha: f64) -> V3 {
    let u = t.clamp(0.0, 1.0);
    let a = alpha.clamp(0.0, 1.0);
    let knot_delta = |pa: V3, pb: V3| -> f64 {
        let d = norm(sub(pb, pa));
        let dt = d.powf(a);
        if dt.is_finite() && dt > 1e-9 {
            dt
        } else {
            1.0
        }
    };
    let t0 = 0.0;
    let t1 = t0 + knot_delta(p0, p1);
    let t2 = t1 + knot_delta(p1, p2);
    let t3 = t2 + knot_delta(p2, p3);
    let tau = t1 + u * (t2 - t1);

    let lerp_pt = |pa: V3, pb: V3, ta: f64, tb: f64| -> V3 {
        let denom = tb - ta;
        if denom.abs() <= 1e-15 {
            pa
        } else {
            let w = (tau - ta) / denom;
            add(scale(pa, 1.0 - w), scale(pb, w))
        }
    };

    let a1 = lerp_pt(p0, p1, t0, t1);
    let a2 = lerp_pt(p1, p2, t1, t2);
    let a3 = lerp_pt(p2, p3, t2, t3);

    let b1 = lerp_pt(a1, a2, t0, t2);
    let b2 = lerp_pt(a2, a3, t1, t3);

    lerp_pt(b1, b2, t1, t2)
}

/// Evaluate the first derivative $dC/dt$ (with respect to normalized segment parameter $t \in [0, 1]$)
/// of a 3D Barry–Goldman Catmull-Rom spline between `p1` and `p2`.
pub fn evaluate_catmull_rom_tangent_3d(
    p0: V3,
    p1: V3,
    p2: V3,
    p3: V3,
    t: f64,
    alpha: f64,
) -> V3 {
    let u = t.clamp(0.0, 1.0);
    let a = alpha.clamp(0.0, 1.0);
    let knot_delta = |pa: V3, pb: V3| -> f64 {
        let d = norm(sub(pb, pa));
        let dt = d.powf(a);
        if dt.is_finite() && dt > 1e-9 {
            dt
        } else {
            1.0
        }
    };
    let t0 = 0.0;
    let t1 = t0 + knot_delta(p0, p1);
    let t2 = t1 + knot_delta(p1, p2);
    let t3 = t2 + knot_delta(p2, p3);
    let dt12 = t2 - t1;
    let tau = t1 + u * dt12;

    let lerp_pt = |pa: V3, pb: V3, ta: f64, tb: f64| -> (V3, V3) {
        let denom = tb - ta;
        if denom.abs() <= 1e-15 {
            (pa, [0.0, 0.0, 0.0])
        } else {
            let inv = 1.0 / denom;
            let w = (tau - ta) * inv;
            let val = add(scale(pa, 1.0 - w), scale(pb, w));
            let dval = scale(sub(pb, pa), inv);
            (val, dval)
        }
    };

    let (a1, da1) = lerp_pt(p0, p1, t0, t1);
    let (a2, da2) = lerp_pt(p1, p2, t1, t2);
    let (a3, da3) = lerp_pt(p2, p3, t2, t3);

    let combine = |va: V3, dva: V3, vb: V3, dvb: V3, ta: f64, tb: f64| -> (V3, V3) {
        let denom = tb - ta;
        if denom.abs() <= 1e-15 {
            (va, dva)
        } else {
            let inv = 1.0 / denom;
            let w = (tau - ta) * inv;
            let val = add(scale(va, 1.0 - w), scale(vb, w));
            let term1 = scale(sub(vb, va), inv);
            let term2 = add(scale(dva, 1.0 - w), scale(dvb, w));
            (val, add(term1, term2))
        }
    };

    let (b1, db1) = combine(a1, da1, a2, da2, t0, t2);
    let (b2, db2) = combine(a2, da2, a3, da3, t1, t3);
    let (_c, dc_dtau) = combine(b1, db1, b2, db2, t1, t2);
    scale(dc_dtau, dt12)
}

/// Fit a best-fit 3D plane `(centroid, unit_normal)` to a point cloud via Principal Component
/// Analysis (PCA) using [`symmetric_eigen3`] on the 3x3 covariance matrix.
/// Returns `None` if fewer than 3 finite points are provided or if the point cloud is collinear/degenerate.
pub fn plane_from_points_pca(points: &[V3]) -> Option<(V3, V3)> {
    if points.len() < 3 || !points.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let inv_n = 1.0 / (points.len() as f64);
    let mut centroid = [0.0_f64; 3];
    for &p in points {
        centroid[0] += p[0];
        centroid[1] += p[1];
        centroid[2] += p[2];
    }
    centroid[0] *= inv_n;
    centroid[1] *= inv_n;
    centroid[2] *= inv_n;

    let mut cov = [[0.0_f64; 3]; 3];
    for &p in points {
        let dx = p[0] - centroid[0];
        let dy = p[1] - centroid[1];
        let dz = p[2] - centroid[2];
        cov[0][0] += dx * dx;
        cov[0][1] += dx * dy;
        cov[0][2] += dx * dz;
        cov[1][1] += dy * dy;
        cov[1][2] += dy * dz;
        cov[2][2] += dz * dz;
    }
    cov[1][0] = cov[0][1];
    cov[2][0] = cov[0][2];
    cov[2][1] = cov[1][2];

    let (evals, evecs) = symmetric_eigen3(cov);
    // Require at least two non-degenerate principal variance directions (non-collinear points)
    if evals[0] <= 1e-24 || evals[1] <= 1e-20 * evals[0] {
        return None;
    }
    let normal = unit([evecs[0][2], evecs[1][2], evecs[2][2]]);
    let len = norm(normal);
    if !len.is_finite() || len <= 0.5 {
        return None;
    }
    Some((centroid, normal))
}

/// Spreads the lowest 10 bits of `v` so there are 2 zero bits between each bit, producing a 30-bit
/// mask ready for 3D Morton (Z-order curve) interleaving.
#[inline]
pub const fn expand_bits_10_to_30(v: u32) -> u32 {
    let mut x = v & 0x0000_03FF;
    x = (x.wrapping_mul(0x0001_0001)) & 0xFF00_00FF;
    x = (x.wrapping_mul(0x0000_0101)) & 0x0F00_F00F;
    x = (x.wrapping_mul(0x0000_0011)) & 0xC30C_30C3;
    x = (x.wrapping_mul(0x0000_0005)) & 0x4924_9249;
    x
}

/// Compacts every third bit of a 30-bit Morton word back into a 10-bit integer in `0..=1023`.
#[inline]
const fn compact_bits_30_to_10(v: u32) -> u32 {
    let mut x = v & 0x4924_9249;
    x = (x ^ (x >> 2)) & 0xC30C_30C3;
    x = (x ^ (x >> 4)) & 0x0F00_F00F;
    x = (x ^ (x >> 8)) & 0xFF00_00FF;
    x = (x ^ (x >> 16)) & 0x0000_03FF;
    x
}

/// Encodes 10-bit integer coordinates `(x, y, z)` (clamped/masked to `0..=1023`) into a 30-bit
/// 3D Z-order Morton code (`x` at bit 0, `y` at bit 1, `z` at bit 2).
#[inline]
pub const fn morton3d_encode(x: u32, y: u32, z: u32) -> u32 {
    expand_bits_10_to_30(x) | (expand_bits_10_to_30(y) << 1) | (expand_bits_10_to_30(z) << 2)
}

/// Decodes a 30-bit 3D Z-order Morton code back into its 10-bit `(x, y, z)` integer coordinates.
#[inline]
pub const fn morton3d_decode(code: u32) -> (u32, u32, u32) {
    (
        compact_bits_30_to_10(code),
        compact_bits_30_to_10(code >> 1),
        compact_bits_30_to_10(code >> 2),
    )
}

/// Quantizes a normalized 3D point `p` in $[0, 1]^3$ onto a $1024^3$ lattice and returns its
/// 30-bit 3D Z-order Morton code for GPU/CPU Linear BVH (`LBVH`) construction.
#[inline]
pub fn morton3d_from_unit_point(p: V3) -> u32 {
    let quantize = |c: f64| -> u32 {
        if !c.is_finite() {
            0
        } else {
            (c.clamp(0.0, 1.0) * 1023.0).round().clamp(0.0, 1023.0) as u32
        }
    };
    morton3d_encode(quantize(p[0]), quantize(p[1]), quantize(p[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quat_axis_angle_slerp_and_mat3_roundtrip() {
        let q = Quat::from_axis_angle([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2).unwrap();
        let v = q.rotate_vec3([1.0, 0.0, 0.0]);
        assert!((v[0] - 0.0).abs() < 1e-12);
        assert!((v[1] - 1.0).abs() < 1e-12);
        assert!((v[2] - 0.0).abs() < 1e-12);

        let m = q.to_mat3();
        let recovered = Quat::from_mat3(m);
        assert!((q.dot(recovered).abs() - 1.0).abs() < 1e-12);

        let mid = Quat::IDENTITY.slerp(q, 0.5);
        let v_mid = mid.rotate_vec3([1.0, 0.0, 0.0]);
        let s45 = std::f64::consts::FRAC_1_SQRT_2;
        assert!((v_mid[0] - s45).abs() < 1e-12);
        assert!((v_mid[1] - s45).abs() < 1e-12);
    }

    #[test]
    fn dual_quat_transforms_and_blends_rigid_motions() {
        let q = Quat::from_axis_angle([0.0, 1.0, 0.0], std::f64::consts::FRAC_PI_2).unwrap();
        let dq = DualQuat::from_rotation_translation(q, [10.0, -5.0, 2.0]);
        let p = dq.transform_point([1.0, 2.0, 3.0]);
        // Rotating [1, 2, 3] by +90 deg around Y gives [3, 2, -1], plus [10, -5, 2] = [13, -3, 1].
        assert!((p[0] - 13.0).abs() < 1e-12);
        assert!((p[1] - (-3.0)).abs() < 1e-12);
        assert!((p[2] - 1.0).abs() < 1e-12);

        let mid = DualQuat::IDENTITY.blend(dq, 0.5);
        let (mid_r, mid_t) = mid.to_rotation_translation();
        assert!((mid_r.norm() - 1.0).abs() < 1e-12);
        assert!(mid_t.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn ray_aabb_sphere_and_plane_intersections() {
        let ray = Ray3::new([-5.0, 0.5, 0.5], [1.0, 0.0, 0.0]).unwrap();
        let hit = ray.intersect_aabb([0.0, 0.0, 0.0], [2.0, 1.0, 1.0]).unwrap();
        assert!((hit.0 - 5.0).abs() < 1e-12);
        assert!((hit.1 - 7.0).abs() < 1e-12);

        let sphere_hit = ray.intersect_sphere([0.0, 0.5, 0.5], 1.5).unwrap();
        assert!((sphere_hit.0 - 3.5).abs() < 1e-12);
        assert!((sphere_hit.1 - 6.5).abs() < 1e-12);

        let plane = Plane3::from_point_normal([3.0, 0.0, 0.0], [-1.0, 0.0, 0.0]).unwrap();
        let t_plane = plane.intersect_ray(&ray).unwrap();
        assert!((t_plane - 8.0).abs() < 1e-12);
    }

    #[test]
    fn frustum_culling_identifies_inside_and_outside_aabbs() {
        // Identity VP matrix defines the canonical [-1, 1]^3 clip cube.
        let frustum = Frustum::from_view_proj_row_major(IDENTITY).unwrap();
        assert!(frustum.contains_point([0.0, 0.0, 0.0]));
        assert!(!frustum.contains_point([2.0, 0.0, 0.0]));
        assert!(frustum.intersects_aabb([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]));
        assert!(frustum.intersects_aabb([0.8, -0.2, -0.2], [2.0, 0.2, 0.2]));
        assert!(!frustum.intersects_aabb([1.5, 1.5, 1.5], [2.5, 2.5, 2.5]));
        assert!(frustum.intersects_sphere([1.25, 0.0, 0.0], 0.5));
        assert!(!frustum.intersects_sphere([2.0, 0.0, 0.0], 0.5));

        let batch = [
            Aabb3::new([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]),
            Aabb3::new([1.5, 1.5, 1.5], [2.5, 2.5, 2.5]),
            Aabb3::empty(),
        ];
        assert_eq!(frustum.cull_aabbs_mask(&batch), vec![true, false, false]);
    }

    #[test]
    fn aabb3_and_obb3_operations_and_sat_intersection() {
        let a = Aabb3::from_points(&[[0.0, 0.0, 0.0], [2.0, 4.0, 6.0]]);
        assert_eq!(a.center(), [1.0, 2.0, 3.0]);
        assert_eq!(a.extents(), [2.0, 4.0, 6.0]);
        assert_eq!(a.volume(), 48.0);
        assert_eq!(a.surface_area(), 88.0);
        assert_eq!(a.longest_axis(), 2);
        assert!(a.contains_point([1.0, 2.0, 3.0]));
        assert!(!a.contains_point([3.0, 2.0, 3.0]));

        let b = Aabb3::new([1.0, 1.0, 1.0], [3.0, 3.0, 3.0]);
        assert!(a.intersects(&b));
        let inter = a.intersection(&b);
        assert_eq!(inter.min, [1.0, 1.0, 1.0]);
        assert_eq!(inter.max, [2.0, 3.0, 3.0]);
        let uni = a.union(&b);
        assert_eq!(uni.min, [0.0, 0.0, 0.0]);
        assert_eq!(uni.max, [3.0, 4.0, 6.0]);

        let rot = Quat::from_axis_angle([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_4).unwrap();
        let obb1 = Obb3::from_rotation([0.0, 0.0, 0.0], rot, [2.0, 0.5, 0.5]);
        assert!(obb1.contains_point([1.0, 1.0, 0.0]));
        assert!(!obb1.contains_point([2.0, -2.0, 0.0]));

        let obb2 = Obb3::from_aabb(&Aabb3::new([0.8, 0.8, -0.5], [1.5, 1.5, 0.5]));
        assert!(obb1.intersects_obb(&obb2));

        let obb3 = Obb3::from_aabb(&Aabb3::new([2.5, 2.5, -0.5], [3.5, 3.5, 0.5]));
        assert!(!obb1.intersects_obb(&obb3));
    }

    #[test]
    fn barycentric_and_closest_point_on_triangle_3d_match_analytic_regions() {
        let a = [0.0, 0.0, 0.0];
        let b = [4.0, 0.0, 0.0];
        let c = [0.0, 4.0, 0.0];
        let bary = barycentric_coords_3d([1.0, 1.0, 5.0], a, b, c).unwrap();
        assert!((bary[0] - 0.5).abs() < 1e-12);
        assert!((bary[1] - 0.25).abs() < 1e-12);
        assert!((bary[2] - 0.25).abs() < 1e-12);

        // Inside face projection
        let cp_face = closest_point_on_triangle_3d([1.0, 1.0, 5.0], a, b, c);
        assert_eq!(cp_face, [1.0, 1.0, 0.0]);

        // Vertex Voronoi region A
        let cp_a = closest_point_on_triangle_3d([-2.0, -3.0, 1.0], a, b, c);
        assert_eq!(cp_a, a);

        // Hypotenuse edge Voronoi region BC
        let cp_bc = closest_point_on_triangle_3d([4.0, 4.0, 2.0], a, b, c);
        assert!((cp_bc[0] - 2.0).abs() < 1e-12);
        assert!((cp_bc[1] - 2.0).abs() < 1e-12);
        assert!((cp_bc[2] - 0.0).abs() < 1e-12);
    }

    #[test]
    fn moller_trumbore_ray_triangle_and_symmetric_eigen3_and_solve_linear_3x3() {
        let v0 = [0.0, 0.0, 0.0];
        let v1 = [2.0, 0.0, 0.0];
        let v2 = [0.0, 2.0, 0.0];
        let ray = Ray3::new([0.5, 0.5, 3.0], [0.0, 0.0, -1.0]).unwrap();
        let (t, u, v) = ray_triangle_intersect_moller_trumbore(&ray, v0, v1, v2).unwrap();
        assert!((t - 3.0).abs() < 1e-12);
        assert!((u - 0.25).abs() < 1e-12);
        assert!((v - 0.25).abs() < 1e-12);

        // Miss outside triangle
        let miss_ray = Ray3::new([2.5, 2.5, 3.0], [0.0, 0.0, -1.0]).unwrap();
        assert!(ray_triangle_intersect_moller_trumbore(&miss_ray, v0, v1, v2).is_none());

        // Batch closest-triangle hit
        let verts = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 1.5],
            [2.0, 0.0, 1.5],
            [0.0, 2.0, 1.5],
        ];
        let tris = [[0, 1, 2], [3, 4, 5]];
        let closest = ray.intersect_triangles_closest(&verts, &tris).unwrap();
        assert_eq!(closest.0, 1);
        assert!((closest.1 - 1.5).abs() < 1e-12);
        assert!(miss_ray.intersect_triangles_closest(&verts, &tris).is_none());

        // Symmetric 3x3 eigendecomposition sorted descending
        let cov = [
            [5.0, 2.0, 0.0],
            [2.0, 5.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let (evals, evecs) = symmetric_eigen3(cov);
        assert!((evals[0] - 7.0).abs() < 1e-12);
        assert!((evals[1] - 3.0).abs() < 1e-12);
        assert!((evals[2] - 1.0).abs() < 1e-12);
        for k in 0..3 {
            let col = [evecs[0][k], evecs[1][k], evecs[2][k]];
            let av = crate::linalg::mv(cov, col);
            for r in 0..3 {
                assert!((av[r] - evals[k] * col[r]).abs() < 1e-11);
            }
        }

        // 3x3 linear solver with partial pivoting
        let a = [
            [0.0, 2.0, 1.0],
            [1.0, -2.0, -3.0],
            [-1.0, 1.0, 2.0],
        ];
        let b = [-8.0, 0.0, 3.0];
        let x = solve_linear_3x3(a, b).unwrap();
        let ax = crate::linalg::mv(a, x);
        for i in 0..3 {
            assert!((ax[i] - b[i]).abs() < 1e-12);
        }
        // Singular matrix returns None
        let singular = [
            [1.0, 2.0, 3.0],
            [2.0, 4.0, 6.0],
            [0.0, 1.0, 1.0],
        ];
        assert!(solve_linear_3x3(singular, [1.0, 2.0, 3.0]).is_none());
    }

    #[test]
    fn catmull_rom_spline_and_plane_from_points_pca_work() {
        let p0 = [0.0, 0.0, 0.0];
        let p1 = [1.0, 0.0, 0.0];
        let p2 = [2.0, 1.0, 0.0];
        let p3 = [3.0, 1.0, 0.0];
        let spline = CatmullRomSpline3::centripetal(p0, p1, p2, p3);
        let start = spline.evaluate(0.0);
        let end = spline.evaluate(1.0);
        assert!(( norm(sub(start, p1)) ) < 1e-12);
        assert!(( norm(sub(end, p2)) ) < 1e-12);

        // Verify analytical tangent against central finite difference at t = 0.5
        let eps = 1e-6;
        let c_plus = evaluate_catmull_rom_3d(p0, p1, p2, p3, 0.5 + eps, 0.5);
        let c_minus = evaluate_catmull_rom_3d(p0, p1, p2, p3, 0.5 - eps, 0.5);
        let fd = scale(sub(c_plus, c_minus), 0.5 / eps);
        let tan = spline.tangent(0.5);
        assert!(norm(sub(tan, fd)) < 1e-7);

        // PCA plane fitting on tilted plane z = 2x - y + 3
        let pts = [
            [0.0, 0.0, 3.0],
            [1.0, 0.0, 5.0],
            [0.0, 1.0, 2.0],
            [-1.0, -1.0, 2.0],
            [2.0, 1.0, 6.0],
        ];
        let (centroid, normal) = plane_from_points_pca(&pts).unwrap();
        // Check that all points lie on the fitted plane dot(p - centroid, normal) == 0
        for &p in &pts {
            assert!(dot(sub(p, centroid), normal).abs() < 1e-11);
        }
        // Collinear points return None
        assert!(plane_from_points_pca(&[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]).is_none());

        // 3D Morton code (Z-order curve) encode / decode roundtrip and unit-point quantization
        for &(x, y, z) in &[(0, 0, 0), (1, 2, 3), (511, 256, 1023), (1023, 1023, 1023), (341, 682, 170)] {
            let code = morton3d_encode(x, y, z);
            assert_eq!(morton3d_decode(code), (x, y, z));
        }
        assert_eq!(morton3d_from_unit_point([0.0, 0.0, 0.0]), 0);
        assert_eq!(morton3d_from_unit_point([1.0, 1.0, 1.0]), 0x3FFF_FFFF);
        assert!(morton3d_from_unit_point([0.2, 0.2, 0.2]) < morton3d_from_unit_point([0.8, 0.8, 0.8]));
    }
}
