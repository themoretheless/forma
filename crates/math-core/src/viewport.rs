//! Row-major viewport transforms. WebGPU clip depth is 0..1.
//! Numerical display math, not certified CAD predicates.
pub type Matrix = [f64; 16];
pub type Point = [f64; 3];
/// Matrix transport has f32 GPU storage, with f64 accumulation.
pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    use crate::affine::{from_row_major, multiply, to_row_major};
    to_row_major(multiply(from_row_major(*a), from_row_major(*b))).map(|x| x as f32 as f64)
}

pub fn transpose(m: &Matrix) -> Matrix {
    std::array::from_fn(|i| m[i % 4 * 4 + i / 4])
}
#[derive(Debug)]
pub struct Ray {
    pub origin: Point,
    pub direction: Point,
}
fn homogeneous(m: &Matrix, p: Point) -> [f64; 4] {
    std::array::from_fn(|r| {
        m[r * 4] * p[0] + m[r * 4 + 1] * p[1] + m[r * 4 + 2] * p[2] + m[r * 4 + 3]
    })
}
fn point(m: &Matrix, p: Point) -> Option<Point> {
    let q = homogeneous(m, p);
    if q[3] == 0. || !q.iter().all(|v| v.is_finite()) {
        return None;
    }
    let result = [q[0] / q[3], q[1] / q[3], q[2] / q[3]];
    result.iter().all(|v| v.is_finite()).then_some(result)
}
/// Partial-pivot elimination. Singular/nonfinite inverses never become identity.
pub fn inverse(m: &Matrix) -> Option<Matrix> {
    if !m.iter().all(|v| v.is_finite()) {
        return None;
    }
    let mut rows = [[0.; 8]; 4];
    for r in 0..4 {
        rows[r][..4].copy_from_slice(&m[r * 4..r * 4 + 4]);
        rows[r][r + 4] = 1.;
    }
    for c in 0..4 {
        let pivot = (c..4).max_by(|&a, &b| rows[a][c].abs().total_cmp(&rows[b][c].abs()))?;
        if rows[pivot][c] == 0. {
            return None;
        }
        rows.swap(c, pivot);
        let d = rows[c][c];
        for value in &mut rows[c] {
            *value /= d
        }
        let pivot_row = rows[c];
        for (r, row) in rows.iter_mut().enumerate() {
            if r != c {
                let factor = row[c];
                for (value, pivot_value) in row.iter_mut().zip(pivot_row) {
                    *value -= factor * pivot_value;
                }
            }
        }
    }
    let result = std::array::from_fn(|i| rows[i / 4][i % 4 + 4]);
    result.iter().all(|v| v.is_finite()).then_some(result)
}
pub fn unproject(inverse: &Matrix, x: f64, y: f64) -> Option<Ray> {
    let origin = point(inverse, [x, y, 0.])?;
    let far = point(inverse, [x, y, 1.])?;
    let d = [far[0] - origin[0], far[1] - origin[1], far[2] - origin[2]];
    let length = d[0].hypot(d[1]).hypot(d[2]);
    if !length.is_finite() || length == 0. {
        return None;
    }
    Some(Ray {
        origin,
        direction: d.map(|v| v / length),
    })
}
/// Rect is CSS left/top/width/height; client coordinates are in the same space.
pub fn client_ray(matrix: &Matrix, rect: [f64; 4], client: [f64; 2]) -> Option<Ray> {
    if !rect.iter().chain(&client).all(|v| v.is_finite()) || rect[2] <= 0. || rect[3] <= 0. {
        return None;
    }
    let x = (client[0] - rect[0]) / rect[2] * 2. - 1.;
    let y = 1. - (client[1] - rect[1]) / rect[3] * 2.;
    unproject(&inverse(matrix)?, x, y)
}
pub fn project(matrix: &Matrix, p: Point, size: [f64; 2]) -> Option<[f64; 2]> {
    if !size.iter().all(|v| v.is_finite() && *v > 0.) {
        return None;
    }
    let q = homogeneous(matrix, p);
    if q[3] <= 1e-8 || !q.iter().all(|v| v.is_finite()) {
        return None;
    }
    let result = [
        (q[0] / q[3] + 1.) * size[0] / 2.,
        (1. - q[1] / q[3]) * size[1] / 2.,
    ];
    result.iter().all(|v| v.is_finite()).then_some(result)
}
/// Snap to the maximum-barycentric corner; exact ties keep the first corner.
pub fn selected_corner(matrix: &Matrix, vertices: [Point; 3], weights: Point) -> Option<Point> {
    if !weights.iter().all(|v| v.is_finite()) {
        return None;
    }
    let mut corner = 0;
    for i in 1..3 {
        if weights[i] > weights[corner] {
            corner = i
        }
    }
    point(matrix, vertices[corner])
}
#[cfg(test)]
mod tests {
    use super::*;
    const ID: Matrix = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    #[test]
    fn css_ray_and_projection_roundtrip() {
        let ray = client_ray(&ID, [10., 20., 200., 100.], [160., 45.]).unwrap();
        assert_eq!(ray.origin, [0.5, 0.5, 0.]);
        assert_eq!(ray.direction, [0., 0., 1.]);
        assert_eq!(project(&ID, ray.origin, [200., 100.]), Some([150., 25.]));
    }
    #[test]
    fn singular_and_invalid_viewports_refuse() {
        assert!(inverse(&[0.; 16]).is_none());
        assert!(client_ray(&ID, [0., 0., 0., 100.], [0., 0.]).is_none());
        assert!(unproject(&[0.; 16], 0., 0.).is_none());
        assert!(project(&ID, [f64::NAN, 0., 0.], [100., 100.]).is_none());
    }
    #[test]
    fn inverse_residual_and_small_invertible_scale() {
        let m = [
            2., 0.5, 0., 7., 0., 3., 0.25, -4., 0., 0., 0.5, 2., 0., 0., 0., 1.,
        ];
        let inverted = inverse(&m).unwrap();
        let product = multiply(&m, &inverted);
        for i in 0..16 {
            assert!((product[i] - ID[i]).abs() < 1e-6)
        }
        let mut small = ID;
        small[0] = 1e-12;
        small[5] = 1e-12;
        small[10] = 1e-12;
        let inverse = inverse(&small).unwrap();
        assert!((inverse[0] - 1e12).abs() < 1.);
        assert_eq!(transpose(&transpose(&m)), m);
    }
    #[test]
    fn selected_corner_ties_and_transform() {
        let mut m = ID;
        m[3] = 5.;
        m[0] = 2.;
        let vertices = [[1., 0., 0.], [2., 0., 0.], [3., 0., 0.]];
        assert_eq!(
            selected_corner(&m, vertices, [0.5, 0.5, 0.]),
            Some([7., 0., 0.])
        );
        assert_eq!(
            selected_corner(&m, vertices, [0.1, 0.2, 0.7]),
            Some([11., 0., 0.])
        );
    }
}
