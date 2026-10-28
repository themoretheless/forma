//! Device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]
use math_core::*;

#[test]
fn nearest_two_gpu_matches_reference() {
    math_compute::install();
    let queries: Vec<V3> = (0..96)
        .map(|i| {
            let f = i as f64;
            [f * 0.2 - 6., f * 0.05, (f * 0.11).cos() * 3.]
        })
        .collect();
    let targets: Vec<V3> = (0..30)
        .map(|i| {
            let f = i as f64;
            [f * -0.3 + 2., (f * 0.4).sin() * 2., f * 0.15]
        })
        .collect();
    let got = nearest_two_accelerated(&queries, &targets, Acceleration::Gpu);
    let want = nearest_two(&queries, &targets);
    for ([g0, g1], [w0, w1]) in got.iter().zip(&want) {
        assert_eq!(g0.0, w0.0);
        assert_eq!(g1.0, w1.0);
        assert!((g0.1 - w0.1).abs() < 5e-3 * w0.1.max(1.0));
        assert!((g1.1 - w1.1).abs() < 5e-3 * w1.1.max(1.0));
    }
}

#[cfg(feature = "cuda")]
#[test]
fn nearest_two_cuda_matches_reference() {
    math_compute::install();
    let queries: Vec<V3> = (0..96)
        .map(|i| {
            let f = i as f64;
            [f * 0.2 - 6., f * 0.05, (f * 0.11).cos() * 3.]
        })
        .collect();
    let targets: Vec<V3> = (0..30)
        .map(|i| {
            let f = i as f64;
            [f * -0.3 + 2., (f * 0.4).sin() * 2., f * 0.15]
        })
        .collect();
    let got = nearest_two_accelerated(&queries, &targets, Acceleration::Cuda);
    let want = nearest_two(&queries, &targets);
    for ([g0, g1], [w0, w1]) in got.iter().zip(&want) {
        assert_eq!(g0.0, w0.0);
        assert_eq!(g1.0, w1.0);
        assert!((g0.1 - w0.1).abs() < 5e-3 * w0.1.max(1.0));
        assert!((g1.1 - w1.1).abs() < 5e-3 * w1.1.max(1.0));
    }
}
