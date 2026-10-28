//! Device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]
use math_core::*;

fn points(n: usize, offset: f64) -> Vec<V3> {
    (0..n)
        .map(|i| {
            let f = i as f64 + offset;
            [f * 0.17 - 4., (f * 0.07).sin(), (f * 0.11).cos()]
        })
        .collect()
}

#[test]
fn chamfer_gpu_matches_cpu_reference() {
    math_compute::install();
    let a = points(96, 0.);
    let b = points(64, 100.);
    let got = chamfer_distance(&a, &b, Acceleration::Gpu).unwrap();
    let want = chamfer_distance(&a, &b, Acceleration::Cpu).unwrap();
    assert!(
        (got.symmetric_mean_squared_distance - want.symmetric_mean_squared_distance).abs()
            < 5e-3 * want.symmetric_mean_squared_distance.max(1.0),
        "{got:?} vs {want:?}"
    );
}

#[cfg(feature = "cuda")]
#[test]
fn chamfer_cuda_matches_cpu_reference() {
    math_compute::install();
    let a = points(96, 0.);
    let b = points(64, 100.);
    let got = chamfer_distance(&a, &b, Acceleration::Cuda).unwrap();
    let want = chamfer_distance(&a, &b, Acceleration::Cpu).unwrap();
    assert!(
        (got.symmetric_mean_squared_distance - want.symmetric_mean_squared_distance).abs()
            < 5e-3 * want.symmetric_mean_squared_distance.max(1.0),
        "{got:?} vs {want:?}"
    );
}
