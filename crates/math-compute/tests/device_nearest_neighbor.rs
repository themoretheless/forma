//! Device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]
use math_core::*;

#[test]
fn nearest_neighbor_accelerated_gpu_dispatch_matches_cpu() {
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
    let got = nearest_neighbor_accelerated(&queries, &targets, Acceleration::Gpu);
    let want = nearest_neighbor(&queries, &targets);
    for ((_gi, gd), (_wi, wd)) in got.iter().zip(&want) {
        assert!((gd - wd).abs() < 5e-3 * wd.max(1.0), "{gd} vs {wd}");
    }
}

#[cfg(feature = "cuda")]
#[test]
fn nearest_neighbor_accelerated_cuda_dispatch_matches_cpu() {
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
    let got = nearest_neighbor_accelerated(&queries, &targets, Acceleration::Cuda);
    let want = nearest_neighbor(&queries, &targets);
    for ((_gi, gd), (_wi, wd)) in got.iter().zip(&want) {
        assert!((gd - wd).abs() < 5e-3 * wd.max(1.0), "{gd} vs {wd}");
    }
}
