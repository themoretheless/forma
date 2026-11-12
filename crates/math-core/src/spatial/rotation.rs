//! Rotations: quaternions and dual quaternions.

use super::*;

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

    /// Hamilton product `self * rhs`.
    #[inline]
    pub fn hamilton_product(self, rhs: Self) -> Self {
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

impl std::ops::Mul for Quat {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        self.hamilton_product(rhs)
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
        let dual = t_quat * r;
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

    /// Compose rigid transforms `self * rhs`.
    #[inline]
    pub fn compose(self, rhs: Self) -> Self {
        let r = self.real * rhs.real;
        let d1 = self.real * rhs.dual;
        let d2 = self.dual * rhs.real;
        Self {
            real: r,
            dual: Quat::new(d1.x + d2.x, d1.y + d2.y, d1.z + d2.z, d1.w + d2.w),
        }
    }

    /// Extract the rotation quaternion and translation vector.
    #[inline]
    pub fn to_rotation_translation(self) -> (Quat, V3) {
        let dq = self.normalize().unwrap_or(Self::IDENTITY);
        let t_quat = dq.dual * dq.real.conjugate();
        (dq.real, [2.0 * t_quat.x, 2.0 * t_quat.y, 2.0 * t_quat.z])
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

impl std::ops::Mul for DualQuat {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        self.compose(rhs)
    }
}
