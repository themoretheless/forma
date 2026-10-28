use crate::{Acceleration, V3, dot, sub};

/// Brute-force nearest-neighbor search: for every query point, the index into
/// `targets` of its closest point and the squared Euclidean distance to it
/// (`(u32::MAX, f64::INFINITY)` for a query when `targets` is empty). This is
/// the CPU reference for [`crate::nearest_neighbor_accelerated`]; O(queries *
/// targets) work, exact in f64.
pub fn nearest_neighbor(queries: &[V3], targets: &[V3]) -> Vec<(u32, f64)> {
    queries
        .iter()
        .map(|&q| {
            let mut best_index = u32::MAX;
            let mut best_dist = f64::INFINITY;
            for (i, &t) in targets.iter().enumerate() {
                let d = sub(q, t);
                let dist = dot(d, d);
                if dist < best_dist {
                    best_dist = dist;
                    best_index = i as u32;
                }
            }
            (best_index, best_dist)
        })
        .collect()
}

/// `nearest_neighbor` with an optional GPU/CUDA batch kernel.
/// `Acceleration::Cuda` runs the PTX port through the CUDA driver (feature
/// `cuda`), then the wgpu shader (feature `gpu`), then the CPU reference;
/// anything unavailable or that fails falls through to the next stage, so
/// the CPU result is always returned. Unlike [`crate::transform_points`], this
/// operation has enough work per query (a full scan of `targets`) that the
/// GPU/CUDA placements measurably win at moderate-to-large sizes — see
/// `examples/bench_gpu.rs`.
pub fn nearest_neighbor_accelerated(
    queries: &[V3],
    targets: &[V3],
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Vec<(u32, f64)> {
    if queries.is_empty() || targets.is_empty() {
        return nearest_neighbor(queries, targets);
    }
    #[allow(unused_variables)]
    let acceleration = acceleration.resolve_for_nearest_neighbor(queries.len(), targets.len());
    if acceleration.is_gpu() {
        if let Some(values) = crate::device::kernels()
            .and_then(|kernels| kernels.nearest_neighbor(acceleration, queries, targets))
        {
            return values;
        }
    }
    nearest_neighbor(queries, targets)
}

#[cfg(test)]
mod nearest_neighbor_tests {
    use super::*;
    #[test]
    fn nearest_neighbor_finds_closest_by_brute_force() {
        let queries = vec![[0., 0., 0.], [10., 0., 0.], [-5., -5., -5.]];
        let targets = vec![[1., 0., 0.], [0., 0., 0.9], [9., 0.5, 0.], [-4., -4., -4.]];
        let got = nearest_neighbor(&queries, &targets);
        assert_eq!(got[0], (1, 0.81));
        assert_eq!(got[1].0, 2);
        assert_eq!(got[2].0, 3);
    }
    #[test]
    fn nearest_neighbor_empty_targets_is_infinite() {
        let got = nearest_neighbor(&[[0., 0., 0.]], &[]);
        assert_eq!(got, vec![(u32::MAX, f64::INFINITY)]);
    }
    #[test]
    fn nearest_neighbor_empty_queries_is_empty() {
        assert!(nearest_neighbor(&[], &[[0., 0., 0.]]).is_empty());
    }
    #[test]
    fn nearest_neighbor_accelerated_cpu_matches_reference() {
        let queries = vec![[0., 0., 0.], [1., 1., 1.], [-1., 2., -3.]];
        let targets = vec![[0.1, 0., 0.], [5., 5., 5.]];
        let got = nearest_neighbor_accelerated(&queries, &targets, Acceleration::Cpu);
        let want = nearest_neighbor(&queries, &targets);
        assert_eq!(got, want);
    }
    #[test]
    fn nearest_neighbor_accelerated_auto_matches_reference() {
        let queries: Vec<V3> = (0..512)
            .map(|i| {
                let f = i as f64;
                [f * 0.07, (f * 0.13).sin(), (f * 0.11).cos()]
            })
            .collect();
        let targets: Vec<V3> = (0..512)
            .map(|i| {
                let f = i as f64;
                [f * -0.03, (f * 0.17).cos(), (f * 0.19).sin()]
            })
            .collect();
        let got = nearest_neighbor_accelerated(&queries, &targets, Acceleration::Auto);
        let want = nearest_neighbor(&queries, &targets);
        for ((_gi, gd), (_wi, wd)) in got.iter().zip(&want) {
            assert!((gd - wd).abs() < 1e-3);
        }
    }
    #[test]
    fn nearest_neighbor_accelerated_empty_input() {
        let got = nearest_neighbor_accelerated(&[], &[[0., 0., 0.]], Acceleration::Gpu);
        assert!(got.is_empty());
        let got = nearest_neighbor_accelerated(&[[0., 0., 0.]], &[], Acceleration::Gpu);
        assert_eq!(got, vec![(u32::MAX, f64::INFINITY)]);
    }
}
