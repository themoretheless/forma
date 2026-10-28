//! Device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]
use math_core::*;

fn points(n: usize) -> Vec<V3> {
    (0..n)
        .map(|i| {
            let f = i as f64;
            [
                (f * 0.013).sin() * 200. - 7.,
                (f * 0.017).cos() * 120. + 3.,
                f * 0.00031 - 50.,
            ]
        })
        .collect()
}

fn assert_close(got: PointCloudStats, want: PointCloudStats) {
    assert_eq!(got.samples, want.samples);
    for axis in 0..3 {
        assert!((got.bounds.min[axis] - want.bounds.min[axis]).abs() < 1e-4);
        assert!((got.bounds.max[axis] - want.bounds.max[axis]).abs() < 1e-4);
        assert!((got.moments.centroid[axis] - want.moments.centroid[axis]).abs() < 1e-1);
    }
    for i in 0..3 {
        for j in 0..3 {
            assert!((got.moments.covariance[i][j] - want.moments.covariance[i][j]).abs() < 1e-1);
        }
    }
}

#[test]
fn point_cloud_stats_gpu_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    assert_close(
        point_cloud_stats_accelerated(&points, Acceleration::Gpu).unwrap(),
        point_cloud_stats(&points).unwrap(),
    );
}

#[cfg(feature = "cuda")]
#[test]
fn point_cloud_stats_cuda_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    assert_close(
        point_cloud_stats_accelerated(&points, Acceleration::Cuda).unwrap(),
        point_cloud_stats(&points).unwrap(),
    );
}

#[test]
fn transformed_point_cloud_stats_gpu_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let m = rotation([0.2, -0.1, 0.3]);
    let t = [1., -0.5, 0.25];
    assert_close(
        transformed_point_cloud_stats_accelerated(&points, m, t, Acceleration::Gpu).unwrap(),
        transformed_point_cloud_stats(&points, m, t).unwrap(),
    );
}

#[cfg(feature = "cuda")]
#[test]
fn transformed_point_cloud_stats_cuda_matches_cpu_reference() {
    math_compute::install();
    let points = points(4097);
    let m = rotation([0.2, -0.1, 0.3]);
    let t = [1., -0.5, 0.25];
    assert_close(
        transformed_point_cloud_stats_accelerated(&points, m, t, Acceleration::Cuda).unwrap(),
        transformed_point_cloud_stats(&points, m, t).unwrap(),
    );
}
