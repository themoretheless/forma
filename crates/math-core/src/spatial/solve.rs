//! Small dense solvers and fits.

use super::*;

/// Solve a 3x3 real symmetric eigenvalue problem `A * V = V * diag(lambda)` using exact
/// Jacobi rotations, returning `(eigenvalues, eigenvectors_columns)` sorted in descending
/// order of eigenvalue (`lambda[0] >= lambda[1] >= lambda[2]`).
///
/// The returned `eigenvectors` matrix `V` has orthonormal columns (`V[row][col]` is the `row`-th
/// component of the `col`-th unit eigenvector) with right-handed orientation (`det(V) == +1`).
pub fn symmetric_eigen3(a: M3) -> (V3, M3) {
    // Symmetrize input defensively in case of tiny numerical asymmetry.
    let sym = [
        [
            a[0][0],
            0.5 * (a[0][1] + a[1][0]),
            0.5 * (a[0][2] + a[2][0]),
        ],
        [
            0.5 * (a[1][0] + a[0][1]),
            a[1][1],
            0.5 * (a[1][2] + a[2][1]),
        ],
        [
            0.5 * (a[2][0] + a[0][2]),
            0.5 * (a[2][1] + a[1][2]),
            a[2][2],
        ],
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
    let max_entry = a.iter().flatten().map(|v| v.abs()).fold(0.0_f64, f64::max);
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
