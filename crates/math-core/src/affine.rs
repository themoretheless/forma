//! Affine matrices shared by solid and planar geometry adapters.
pub type AffineMatrix = [[f64; 4]; 4];
pub const IDENTITY: AffineMatrix = [
    [1., 0., 0., 0.],
    [0., 1., 0., 0.],
    [0., 0., 1., 0.],
    [0., 0., 0., 1.],
];
/// Binary64 row-major matrix composition; applies `b` before `a`.
pub fn multiply(a: AffineMatrix, b: AffineMatrix) -> AffineMatrix {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            let mut sum = 0.;
            for k in 0..4 {
                sum += a[r][k] * b[k][c];
            }
            sum
        })
    })
}
pub fn from_row_major(values: [f64; 16]) -> AffineMatrix {
    std::array::from_fn(|r| std::array::from_fn(|c| values[r * 4 + c]))
}
pub fn to_row_major(matrix: AffineMatrix) -> [f64; 16] {
    std::array::from_fn(|i| matrix[i / 4][i % 4])
}
/// Inverse of an orthonormal rigid transform; does not invert scale or shear.
pub fn inverse_rigid(matrix: AffineMatrix) -> AffineMatrix {
    let mut out = IDENTITY;
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = matrix[c][r];
        }
        out[r][3] = -(0..3).map(|k| out[r][k] * matrix[k][3]).sum::<f64>();
    }
    out
}
/// Rigid pose with XYZ Euler degrees and an authored translation.
pub fn frame(origin: [f64; 3], rotation: [f64; 3]) -> AffineMatrix {
    let mut matrix = euler_degrees(rotation);
    for i in 0..3 {
        matrix[i][3] = origin[i];
    }
    matrix
}
/// Translation in the same coordinates as the input geometry.
pub fn translation(offset: [f64; 3]) -> AffineMatrix {
    let mut matrix = IDENTITY;
    for i in 0..3 {
        matrix[i][3] = offset[i];
    }
    matrix
}
/// Axis scaling, including reflections and collapsed axes.
pub fn scaling(factors: [f64; 3]) -> AffineMatrix {
    let mut matrix = IDENTITY;
    for i in 0..3 {
        matrix[i][i] = factors[i];
    }
    matrix
}
/// Full column-major solid matrix, including the homogeneous row.
pub fn solid_column_major(values: [f64; 16]) -> AffineMatrix {
    std::array::from_fn(|row| std::array::from_fn(|column| values[column * 4 + row]))
}
/// Column-major planar affine matrix embedded in three dimensions.
pub fn planar_column_major(values: [f64; 9]) -> AffineMatrix {
    [
        [values[0], values[3], 0., values[6]],
        [values[1], values[4], 0., values[7]],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]
}
/// Intrinsic XYZ Euler angles in degrees, applied as Rz * Ry * Rx.
pub fn euler_degrees(angles: [f64; 3]) -> AffineMatrix {
    let [x, y, z] = angles.map(f64::to_radians);
    let (b, a) = x.sin_cos();
    let (d, c) = y.sin_cos();
    let (f, e) = z.sin_cos();
    [
        [e * c, e * d * b - f * a, e * d * a + f * b, 0.],
        [f * c, f * d * b + e * a, f * d * a - e * b, 0.],
        [-d, c * b, c * a, 0.],
        [0., 0., 0., 1.],
    ]
}
/// Reflection about the plane through the origin with this normal.
/// A zero normal has no reflection plane.
pub fn reflection(normal: [f64; 3]) -> Option<AffineMatrix> {
    let length = normal[0].hypot(normal[1]).hypot(normal[2]);
    if length == 0. {
        return None;
    }
    let unit = normal.map(|x| x / length);
    let mut matrix = IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            matrix[i][j] -= 2. * unit[i] * unit[j];
        }
    }
    Some(matrix)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn apply(m: AffineMatrix, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| (0..3).map(|j| m[i][j] * p[j]).sum())
    }
    #[test]
    fn rigid_pose_inverse_and_row_major_roundtrip_preserve_coordinates() {
        let pose = frame([100., -9., 29.], [20., -30., 50.]);
        assert_eq!(from_row_major(to_row_major(pose)), pose);
        let identity = multiply(pose, inverse_rigid(pose));
        for i in 0..4 {
            for j in 0..4 {
                assert!((identity[i][j] - IDENTITY[i][j]).abs() < 1e-12);
            }
        }
        let composed = multiply(translation([2., 3., 5.]), scaling([-2., 3., 4.]));
        assert_eq!(composed[0], [-2., 0., 0., 2.]);
    }
    #[test]
    fn euler_order_matches_successive_axis_rotations() {
        let p = [2., -3., 5.];
        let expected = apply(
            euler_degrees([0., 0., 71.]),
            apply(
                euler_degrees([0., -23., 0.]),
                apply(euler_degrees([37., 0., 0.]), p),
            ),
        );
        let actual = apply(euler_degrees([37., -23., 71.]), p);
        for i in 0..3 {
            assert!((actual[i] - expected[i]).abs() < 1e-12);
        }
    }
    #[test]
    fn reflection_is_invariant_to_normal_length_and_is_its_own_inverse() {
        let p = [2., -3., 5.];
        let m = reflection([2., 3., 4.]).unwrap();
        let restored = apply(m, apply(m, p));
        for i in 0..3 {
            assert!((restored[i] - p[i]).abs() < 1e-12);
        }
        assert_eq!(reflection([0.; 3]), None);
        let large = reflection([2e200, 3e200, 4e200]).unwrap();
        for i in 0..4 {
            for j in 0..4 {
                assert!((large[i][j] - m[i][j]).abs() < 1e-12);
            }
        }
    }
}

/// Right-handed rotation about a finite nonzero axis, in degrees.
pub fn axis_angle_degrees(axis: [f64; 3], degrees: f64) -> Option<AffineMatrix> {
    if !degrees.is_finite() || !axis.iter().all(|v| v.is_finite()) {
        return None;
    }
    let largest = axis.iter().fold(0f64, |m, v| m.max(v.abs()));
    if largest == 0. {
        return None;
    }
    let scaled = axis.map(|v| v / largest);
    let length = scaled[0].hypot(scaled[1]).hypot(scaled[2]);
    let [x, y, z] = scaled.map(|v| v / length);
    let (s, c) = degrees.to_radians().sin_cos();
    let t = 1. - c;
    Some([
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x, 0.],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c, 0.],
        [0., 0., 0., 1.],
    ])
}

#[cfg(test)]
mod axis_angle_tests {
    use super::*;
    #[test]
    fn axis_rotation_is_scale_invariant_and_preserves_orientation() {
        let reference = axis_angle_degrees([1., 2., 3.], 73.).unwrap();
        for factor in [1e-250, 1e250] {
            let m = axis_angle_degrees([factor, 2. * factor, 3. * factor], 73.).unwrap();
            for i in 0..4 {
                for j in 0..4 {
                    assert!((m[i][j] - reference[i][j]).abs() < 1e-14)
                }
            }
        }
        let m = axis_angle_degrees([0., 0., 1.], 90.).unwrap();
        assert!(m[0][0].abs() < 1e-14);
        assert!((m[1][0] - 1.).abs() < 1e-14);
        assert!(axis_angle_degrees([0.; 3], 90.).is_none());
        assert!(axis_angle_degrees([1., 0., 0.], f64::NAN).is_none());
    }
}
