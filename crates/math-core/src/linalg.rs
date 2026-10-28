use crate::types::{ID, M3, V2, V3};

/// Leaf helpers on the geometry/photogrammetry hotpath: force inlining so call
/// overhead and missed branch hints do not dominate tiny f64 kernels.
#[inline(always)]
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline(always)]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline(always)]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline(always)]
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline(always)]
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
#[inline(always)]
pub fn unit(a: V3) -> V3 {
    scale(a, 1. / norm(a).max(1e-15))
}
#[inline(always)]
pub fn finite(p: V3) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= 1e6)
}
#[inline(always)]
pub fn add2(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}
#[inline(always)]
pub fn sub2(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
#[inline(always)]
pub fn scale2(a: V2, s: f64) -> V2 {
    [a[0] * s, a[1] * s]
}
#[inline(always)]
pub fn dot2(a: V2, b: V2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
#[inline(always)]
pub fn cross2(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
#[inline(always)]
pub fn norm2(a: V2) -> f64 {
    a[0].hypot(a[1])
}
#[inline(always)]
pub fn unit2(a: V2) -> V2 {
    scale2(a, 1. / norm2(a).max(1e-15))
}
#[inline(always)]
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[inline(always)]
pub fn mv(a: M3, b: V3) -> V3 {
    [dot(a[0], b), dot(a[1], b), dot(a[2], b)]
}
#[inline(always)]
pub fn tr(a: M3) -> M3 {
    [
        [a[0][0], a[1][0], a[2][0]],
        [a[0][1], a[1][1], a[2][1]],
        [a[0][2], a[1][2], a[2][2]],
    ]
}
#[inline(always)]
pub fn mm(a: M3, b: M3) -> M3 {
    let bt = tr(b);
    [
        [dot(a[0], bt[0]), dot(a[0], bt[1]), dot(a[0], bt[2])],
        [dot(a[1], bt[0]), dot(a[1], bt[1]), dot(a[1], bt[2])],
        [dot(a[2], bt[0]), dot(a[2], bt[1]), dot(a[2], bt[2])],
    ]
}
#[inline(always)]
pub fn det(a: M3) -> f64 {
    dot(a[0], cross(a[1], a[2]))
}
#[inline(always)]
pub fn rotation(v: V3) -> M3 {
    let angle = norm(v);
    if angle < 1e-14 {
        return ID;
    }
    let [x, y, z] = scale(v, 1. / angle);
    let c = angle.cos();
    let s = angle.sin();
    let t = 1. - c;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ]
}
/// Jacobi eigensystem of a real symmetric matrix, eigenvectors are columns.
/// Fixed stack storage avoids allocating for the bounded 9/12-dimensional systems.
pub fn eigen<const N: usize>(mut a: [[f64; N]; N]) -> ([f64; N], [[f64; N]; N]) {
    let n = N;
    let mut v = [[0.; N]; N];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.;
    }
    for _ in 0..(80 * n * n) {
        let mut p = 0;
        let mut q = 1;
        let mut largest = 0.;
        for (i, row) in a.iter().enumerate() {
            for (j, value) in row.iter().enumerate().skip(i + 1) {
                if value.abs() > largest {
                    largest = value.abs();
                    p = i;
                    q = j;
                }
            }
        }
        let diag = (0..n).map(|i| a[i][i].abs()).fold(0., f64::max);
        if largest < 1e-13 * diag.max(1e-30) {
            break;
        }
        let theta = 0.5 * (2. * a[p][q]).atan2(a[q][q] - a[p][p]);
        let (c, s) = (theta.cos(), theta.sin());
        let (app, aqq, apq) = (a[p][p], a[q][q], a[p][q]);
        // Symmetric updates read and write both rows and columns.
        #[allow(clippy::needless_range_loop, reason = "Jacobi rotation updates symmetric matrix entries")]
        for k in 0..n {
            if k != p && k != q {
                let (kp, kq) = (a[k][p], a[k][q]);
                a[k][p] = c * kp - s * kq;
                a[p][k] = a[k][p];
                a[k][q] = s * kp + c * kq;
                a[q][k] = a[k][q];
            }
        }
        a[p][p] = c * c * app - 2. * s * c * apq + s * s * aqq;
        a[q][q] = s * s * app + 2. * s * c * apq + c * c * aqq;
        a[p][q] = 0.;
        a[q][p] = 0.;
        for row in &mut v {
            let (kp, kq) = (row[p], row[q]);
            row[p] = c * kp - s * kq;
            row[q] = s * kp + c * kq;
        }
    }
    (std::array::from_fn(|i| a[i][i]), v)
}
/// Accumulate normal equations directly from fixed-width rows. No row matrix is
/// materialized; the order of products and additions matches the original solver.
pub fn smallest<const N: usize>(rows: impl IntoIterator<Item = [f64; N]>) -> [f64; N] {
    let mut a = [[0.; N]; N];
    for r in rows {
        for i in 0..N {
            for j in 0..N {
                a[i][j] += r[i] * r[j];
            }
        }
    }
    let (d, v) = eigen(a);
    let k = (0..N).min_by(|&i, &j| d[i].total_cmp(&d[j])).unwrap();
    std::array::from_fn(|i| v[i][k])
}
pub fn svd(a: M3) -> (M3, V3, M3) {
    let ata = mm(tr(a), a);
    let (d, v) = eigen(ata);
    let mut order = [0, 1, 2];
    order.sort_by(|&i, &j| d[j].total_cmp(&d[i]));
    let cols: [V3; 3] = order.map(|k| std::array::from_fn(|i| v[i][k]));
    let s = order.map(|k| d[k].max(0.).sqrt());
    let u0 = unit(mv(a, cols[0]));
    let x = mv(a, cols[1]);
    let u1 = unit(sub(x, scale(u0, dot(x, u0))));
    let u2 = cross(u0, u1);
    (tr([u0, u1, u2]), s, tr(cols))
}
/// Pivoted elimination for the bounded 3/6/8-dimensional camera systems.
/// Fixed stack storage avoids allocating every row of each tiny system.
pub fn solve<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    let n = N;
    for k in 0..n {
        let p = (k..n).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        if a[p][k].abs() < 1e-14 {
            return None;
        }
        a.swap(k, p);
        b.swap(k, p);
        let pivot = a[k][k];
        for value in a[k].iter_mut().skip(k) {
            *value /= pivot;
        }
        b[k] /= pivot;
        let pivot_row = a[k];
        for i in 0..n {
            if i == k {
                continue;
            }
            let f = a[i][k];
            for (value, pivot_value) in a[i].iter_mut().zip(pivot_row).skip(k) {
                *value -= f * pivot_value;
            }
            b[i] -= f * b[k];
        }
    }
    Some(b)
}
/// Batch `q = M*p + t` over a large point set. Plain CPU math: this formula
/// is too arithmetically cheap per point (three fused multiply-adds) for a
/// GPU/CUDA kernel to ever amortize its launch/synchronization latency —
/// measured with `examples/bench_gpu.rs`, GPU and CUDA placements are slower
/// than this reference at every size from 1K to 5M points, even with device
/// buffers reused across calls. There is intentionally no
/// `transform_points_accelerated`: see [`crate::nearest_neighbor_accelerated`] for
/// an operation from this crate whose GPU/CUDA placements do win.
pub fn transform_points(points: &[V3], m: M3, t: V3) -> Vec<V3> {
    points.iter().map(|&p| add(mv(m, p), t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eigen_reconstructs() {
        let a = [[3., 1., 0.], [1., 2., 1.], [0., 1., 4.]];
        let (d, v) = eigen(a);
        for k in 0..3 {
            for i in 0..3 {
                let av = (0..3).map(|j| a[i][j] * v[j][k]).sum::<f64>();
                assert!((av - d[k] * v[i][k]).abs() < 1e-9);
            }
        }
    }
    #[test]
    fn svd_rank_two() {
        let a = [[0., -1., 2.], [1., 0., -3.], [-2., 3., 0.]];
        let (u, s, v) = svd(a);
        let us: M3 = std::array::from_fn(|i| std::array::from_fn(|j| u[i][j] * s[j]));
        let b = mm(us, tr(v));
        for i in 0..3 {
            for j in 0..3 {
                assert!((a[i][j] - b[i][j]).abs() < 1e-6);
            }
        }
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;
    #[test]
    fn transform_points_matches_scalar_mv_add() {
        let m = rotation([0.3, -0.2, 0.7]);
        let t = [1., -2., 0.5];
        let points: Vec<V3> = (0..37)
            .map(|i| {
                let f = i as f64;
                [f * 0.37 - 5., f * -0.11 + 2., f * 0.05]
            })
            .collect();
        let got = transform_points(&points, m, t);
        for (p, q) in points.iter().zip(&got) {
            let expected = add(mv(m, *p), t);
            for k in 0..3 {
                assert!((expected[k] - q[k]).abs() < 1e-12);
            }
        }
    }
}

/// Banded Cholesky (lower triangle stored, half-bandwidth `bw`) — O(n·bw²).
/// For the banded normal equations of B-spline fitting (bw = 2p+1).
/// Row-major packed storage: `a[i*bw + j]` = A[i][i+j] for j < bw, i.e. the
/// diagonal and upper band; the lower band follows by symmetry. On success `a`
/// holds the factor (a[i*bw] = L[i][i], a[i*bw+j] = L[i+j][i]) and `b` holds
/// the solution of A·x = b.
/// Returns None on non-positive pivot or malformed input (caller maps to its
/// error type).
pub fn cholesky_banded(n: usize, bw: usize, a: &mut [f64], b: &mut [f64]) -> Option<()> {
    if bw == 0 || a.len() != n * bw || b.len() != n {
        return None;
    }
    for i in 0..n {
        let start = i.saturating_sub(bw - 1);
        for j in start..i {
            let mut s = a[j * bw + (i - j)];
            let kstart = start.max(j.saturating_sub(bw - 1));
            for k in kstart..j {
                s -= a[k * bw + (i - k)] * a[k * bw + (j - k)];
            }
            let d = a[j * bw];
            if d <= 0. || !d.is_finite() {
                return None;
            }
            a[j * bw + (i - j)] = s / d;
        }
        let mut d = a[i * bw];
        for k in start..i {
            let l = a[k * bw + (i - k)];
            d -= l * l;
        }
        if d <= 0. || !d.is_finite() {
            return None;
        }
        a[i * bw] = d.sqrt();
    }
    // Forward substitution L·y = b with L[i][k] = a[k*bw + (i-k)].
    for i in 0..n {
        let start = i.saturating_sub(bw - 1);
        let mut s = b[i];
        for k in start..i {
            s -= a[k * bw + (i - k)] * b[k];
        }
        b[i] = s / a[i * bw];
    }
    // Back substitution Lᵀ·x = y with L[j][i] = a[i*bw + (j-i)].
    for i in (0..n).rev() {
        let mut s = b[i];
        let end = (i + bw).min(n);
        for j in (i + 1)..end {
            s -= a[i * bw + (j - i)] * b[j];
        }
        b[i] = s / a[i * bw];
    }
    Some(())
}

#[cfg(test)]
mod banded_tests {
    use super::*;

    fn lcg(state: &mut u64) -> f64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*state >> 11) as f64 / (1u64 << 53) as f64
    }

    #[test]
    fn banded_matches_dense_solve() {
        const N: usize = 12;
        const BW: usize = 4;
        let mut state = 0x9E3779B97F4A7C15;
        // Random lower-banded R; A = R·Rᵀ + I is SPD with half-bandwidth BW.
        let mut r = [[0.; N]; N];
        for (i, row) in r.iter_mut().enumerate() {
            for value in row.iter_mut().take(i + 1).skip(i.saturating_sub(BW - 1)) {
                *value = lcg(&mut state) - 0.5;
            }
            row[i] += 2.;
        }
        let mut dense = [[0.; N]; N];
        for i in 0..N {
            for j in 0..N {
                dense[i][j] = (0..=i.min(j)).map(|k| r[i][k] * r[j][k]).sum();
            }
        }
        let rhs: [f64; N] = std::array::from_fn(|_| lcg(&mut state) - 0.5);
        let expected = solve(dense, rhs).expect("dense system is SPD");
        let mut packed = vec![0.; N * BW];
        for i in 0..N {
            for j in 0..BW.min(N - i) {
                packed[i * BW + j] = dense[i][i + j];
            }
        }
        let mut b = rhs.to_vec();
        assert!(cholesky_banded(N, BW, &mut packed, &mut b).is_some());
        for i in 0..N {
            assert!(
                (b[i] - expected[i]).abs() < 1e-8,
                "row {i}: banded {} vs dense {}",
                b[i],
                expected[i]
            );
        }
    }

    #[test]
    fn banded_full_bandwidth_matches_dense() {
        const N: usize = 6;
        let mut state = 42;
        let mut dense = [[0.; N]; N];
        for i in 0..N {
            for j in 0..=i {
                let v = lcg(&mut state) - 0.5;
                dense[i][j] = v;
                dense[j][i] = v;
            }
            dense[i][i] += N as f64;
        }
        let rhs: [f64; N] = std::array::from_fn(|_| lcg(&mut state));
        let expected = solve(dense, rhs).unwrap();
        let mut packed = vec![0.; N * N];
        for i in 0..N {
            for j in 0..(N - i) {
                packed[i * N + j] = dense[i][i + j];
            }
        }
        let mut b = rhs.to_vec();
        assert!(cholesky_banded(N, N, &mut packed, &mut b).is_some());
        for i in 0..N {
            assert!((b[i] - expected[i]).abs() < 1e-9);
        }
    }

    #[test]
    fn banded_rejects_non_spd_and_bad_shapes() {
        // Negative pivot: diagonal entry is negative.
        let mut a = vec![0.; 3 * 2];
        a[0] = -1.;
        a[2] = 1.;
        a[4] = 1.;
        let mut b = vec![0.; 3];
        assert!(cholesky_banded(3, 2, &mut a, &mut b).is_none());
        // Indefinite despite positive diagonal: [[1, 2], [2, 1]].
        let mut a = vec![1., 2., 1., 0.];
        let mut b = vec![1., 1.];
        assert!(cholesky_banded(2, 2, &mut a, &mut b).is_none());
        for pivot in [0., -0., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(cholesky_banded(1, 1, &mut [pivot], &mut [1.]).is_none());
        }
        // Malformed storage.
        assert!(cholesky_banded(3, 0, &mut [], &mut []).is_none());
        let mut a = vec![1.; 5];
        let mut b = vec![1.; 3];
        assert!(cholesky_banded(3, 2, &mut a, &mut b).is_none());
    }

    #[test]
    fn banded_diagonal_system() {
        let mut a = vec![4., 9., 16.];
        let mut b = vec![8., 27., 64.];
        assert!(cholesky_banded(3, 1, &mut a, &mut b).is_some());
        assert!((b[0] - 2.).abs() < 1e-14);
        assert!((b[1] - 3.).abs() < 1e-14);
        assert!((b[2] - 4.).abs() < 1e-14);
    }
}

#[cfg(test)]
mod hotpath_bench {
    use super::*;
    use std::time::Instant;

    #[test]
    fn leaf_math_throughput() {
        let mut acc = 0.0f64;
        let a = [1.1, -2.2, 3.3];
        let b = [0.4, 0.5, -0.6];
        let m = [[1.0, 0.2, 0.0], [0.1, 1.0, 0.3], [0.0, 0.1, 1.0]];
        // warmup
        for _ in 0..50_000 {
            let c = cross(a, b);
            acc += dot(unit(add(c, scale(b, 0.1))), mv(m, a)) + det(mm(m, rotation(a)));
        }
        let start = Instant::now();
        const N: u32 = 2_000_000;
        for _ in 0..N {
            let c = cross(a, b);
            acc += dot(unit(add(c, scale(b, 0.1))), mv(m, a)) + det(mm(m, rotation(a)));
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        eprintln!("math-core hotpath: {ms:.3} ms for {N} iters, acc={acc}");
        assert!(acc.is_finite());
    }
}
