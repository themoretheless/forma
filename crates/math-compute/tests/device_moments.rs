//! Device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]
use math_core::*;

fn points(n: usize) -> Vec<V3> {
    (0..n)
        .map(|i| {
            let f = i as f64;
            [
                (f * 0.013).sin() * 20. - 3.,
                (f * 0.017).cos() * 12. + 2.,
                f * 0.001 - 5.,
            ]
        })
        .collect()
}

#[test]
fn point_moments_gpu_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let got = point_moments_accelerated(&points, Acceleration::Gpu).unwrap();
    let want = point_moments(&points).unwrap();
    for axis in 0..3 {
        assert!((got.centroid[axis] - want.centroid[axis]).abs() < 2e-3);
    }
    for i in 0..3 {
        for j in 0..3 {
            assert!((got.covariance[i][j] - want.covariance[i][j]).abs() < 2e-2);
        }
    }
}

#[cfg(feature = "cuda")]
#[test]
fn point_moments_cuda_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let got = point_moments_accelerated(&points, Acceleration::Cuda).unwrap();
    let want = point_moments(&points).unwrap();
    for axis in 0..3 {
        assert!((got.centroid[axis] - want.centroid[axis]).abs() < 2e-3);
    }
    for i in 0..3 {
        for j in 0..3 {
            assert!((got.covariance[i][j] - want.covariance[i][j]).abs() < 2e-2);
        }
    }
}

#[test]
fn point_fit_plane_gpu_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let got = point_fit_plane(&points, Acceleration::Gpu).unwrap();
    let want = point_fit_plane(&points, Acceleration::Cpu).unwrap();
    for axis in 0..3 {
        assert!((got.normal[axis] - want.normal[axis]).abs() < 1e-3);
    }
    assert!((got.rms_distance - want.rms_distance).abs() < 1e-3);
}

#[cfg(feature = "cuda")]
#[test]
fn point_fit_plane_cuda_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let got = point_fit_plane(&points, Acceleration::Cuda).unwrap();
    let want = point_fit_plane(&points, Acceleration::Cpu).unwrap();
    for axis in 0..3 {
        assert!((got.normal[axis] - want.normal[axis]).abs() < 1e-3);
    }
    assert!((got.rms_distance - want.rms_distance).abs() < 1e-3);
}
