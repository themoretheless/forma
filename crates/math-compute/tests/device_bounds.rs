//! Device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]
use math_core::*;

fn points(n: usize) -> Vec<V3> {
    (0..n)
        .map(|i| {
            let f = i as f64;
            [
                (f * 0.17).sin() * 20. - 3.,
                (f * 0.07).cos() * 11. + 2.,
                f * 0.013 - 5.,
            ]
        })
        .collect()
}

#[test]
fn point_bounds_gpu_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let got = point_bounds_accelerated(&points, Acceleration::Gpu).unwrap();
    let want = point_bounds(&points).unwrap();
    for axis in 0..3 {
        assert!((got.min[axis] - want.min[axis]).abs() < 1e-4);
        assert!((got.max[axis] - want.max[axis]).abs() < 1e-4);
    }
}

#[cfg(feature = "cuda")]
#[test]
fn point_bounds_cuda_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let got = point_bounds_accelerated(&points, Acceleration::Cuda).unwrap();
    let want = point_bounds(&points).unwrap();
    for axis in 0..3 {
        assert!((got.min[axis] - want.min[axis]).abs() < 1e-4);
        assert!((got.max[axis] - want.max[axis]).abs() < 1e-4);
    }
}

#[test]
fn transformed_point_bounds_gpu_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let m = rotation([0.2, -0.1, 0.3]);
    let t = [1., -0.5, 0.25];
    let got = transformed_point_bounds_accelerated(&points, m, t, Acceleration::Gpu).unwrap();
    let want = transformed_point_bounds(&points, m, t).unwrap();
    for axis in 0..3 {
        assert!((got.min[axis] - want.min[axis]).abs() < 1e-4);
        assert!((got.max[axis] - want.max[axis]).abs() < 1e-4);
    }
}

#[cfg(feature = "cuda")]
#[test]
fn transformed_point_bounds_cuda_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let m = rotation([0.2, -0.1, 0.3]);
    let t = [1., -0.5, 0.25];
    let got = transformed_point_bounds_accelerated(&points, m, t, Acceleration::Cuda).unwrap();
    let want = transformed_point_bounds(&points, m, t).unwrap();
    for axis in 0..3 {
        assert!((got.min[axis] - want.min[axis]).abs() < 1e-4);
        assert!((got.max[axis] - want.max[axis]).abs() < 1e-4);
    }
}
