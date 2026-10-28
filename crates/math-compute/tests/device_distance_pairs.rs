//! Device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]
use math_core::*;

fn points(n: usize, offset: f64) -> Vec<V3> {
    (0..n)
        .map(|i| {
            let f = i as f64 + offset;
            [f * 0.13 - 4., (f * 0.07).sin(), (f * 0.11).cos()]
        })
        .collect()
}

#[test]
fn squared_distance_pairs_accelerated_gpu_matches_reference() {
    math_compute::install();
    let a = points(256, 0.);
    let b = points(256, 1.);
    let got = squared_distance_pairs_accelerated(&a, &b, Acceleration::Gpu).unwrap();
    let want = squared_distance_pairs(&a, &b).unwrap();
    for (got, want) in got.iter().zip(&want) {
        assert!((got - want).abs() < 1e-4 * want.max(1.0), "{got} vs {want}");
    }
}

#[test]
fn squared_distance_pair_sum_accelerated_gpu_matches_reference() {
    math_compute::install();
    let a = points(1024, 0.);
    let b = points(1024, 1.);
    let got = squared_distance_pair_sum_accelerated(&a, &b, Acceleration::Gpu).unwrap();
    let want = squared_distance_pair_sum(&a, &b).unwrap();
    assert!((got - want).abs() < 1e-4 * want.max(1.0), "{got} vs {want}");
}

#[cfg(feature = "cuda")]
#[test]
fn squared_distance_pairs_accelerated_cuda_matches_reference() {
    math_compute::install();
    let a = points(256, 0.);
    let b = points(256, 1.);
    let got = squared_distance_pairs_accelerated(&a, &b, Acceleration::Cuda).unwrap();
    let want = squared_distance_pairs(&a, &b).unwrap();
    for (got, want) in got.iter().zip(&want) {
        assert!((got - want).abs() < 1e-4 * want.max(1.0), "{got} vs {want}");
    }
}

#[cfg(feature = "cuda")]
#[test]
fn squared_distance_pair_sum_accelerated_cuda_matches_reference() {
    math_compute::install();
    let a = points(1024, 0.);
    let b = points(1024, 1.);
    let got = squared_distance_pair_sum_accelerated(&a, &b, Acceleration::Cuda).unwrap();
    let want = squared_distance_pair_sum(&a, &b).unwrap();
    assert!((got - want).abs() < 1e-4 * want.max(1.0), "{got} vs {want}");
}
