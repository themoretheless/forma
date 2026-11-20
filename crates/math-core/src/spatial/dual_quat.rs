//! Dual quaternions for 3D rigid body transformations and DLB blending.

use super::*;

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
