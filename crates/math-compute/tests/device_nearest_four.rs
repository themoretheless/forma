//! Device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]
use math_core::*;

#[test]
fn nearest_four_gpu_matches_reference() {
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
    let got = nearest_four_accelerated(&queries, &targets, Acceleration::Gpu);
    let want = nearest_four(&queries, &targets);
    for (got, want) in got.iter().zip(&want) {
        for k in 0..4 {
            assert_eq!(got[k].0, want[k].0);
            assert!((got[k].1 - want[k].1).abs() < 5e-3 * want[k].1.max(1.0));
        }
    }
}

#[cfg(feature = "cuda")]
#[test]
fn nearest_four_cuda_matches_reference() {
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
    let got = nearest_four_accelerated(&queries, &targets, Acceleration::Cuda);
    let want = nearest_four(&queries, &targets);
    for (got, want) in got.iter().zip(&want) {
        for k in 0..4 {
            assert_eq!(got[k].0, want[k].0);
            assert!((got[k].1 - want[k].1).abs() < 5e-3 * want[k].1.max(1.0));
        }
    }
}
