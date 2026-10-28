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
fn transformed_sum_gpu_matches_cpu_reference() {
    math_compute::install();
    let source = points(2_048, 0.);
    let target = points(2_048, 1.);
    let m = rotation([0.2, -0.1, 0.3]);
    let t = [1., -0.5, 0.25];
    let cpu = transformed_squared_distance_pair_sum(&source, &target, m, t).unwrap();
    let gpu = transformed_squared_distance_pair_sum_accelerated(
        &source,
        &target,
        m,
        t,
        Acceleration::Gpu,
    )
    .unwrap();
    assert!((gpu - cpu).abs() / cpu.max(1.) < 1e-5);
}

#[cfg(feature = "cuda")]
#[test]
fn transformed_sum_cuda_matches_cpu_reference() {
    math_compute::install();
    let source = points(2_048, 0.);
    let target = points(2_048, 1.);
    let m = rotation([0.2, -0.1, 0.3]);
    let t = [1., -0.5, 0.25];
    let cpu = transformed_squared_distance_pair_sum(&source, &target, m, t).unwrap();
    let cuda = transformed_squared_distance_pair_sum_accelerated(
        &source,
        &target,
        m,
        t,
        Acceleration::Cuda,
    )
    .unwrap();
    assert!((cuda - cpu).abs() / cpu.max(1.) < 1e-5);
}
