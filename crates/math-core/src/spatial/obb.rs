//! 3D Oriented Bounding Box (`Obb3`) with Separating Axis Theorem overlap test.

use super::*;

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
