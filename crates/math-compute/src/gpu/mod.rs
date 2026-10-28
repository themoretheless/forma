//! Optional WGSL adapters for math-core. Algorithms own their wire layouts and
//! CPU folds; compute-core owns pipeline/binding/dispatch mechanics.
//! Use MathGpuSession for explicit device ownership. Free functions preserve
//! the legacy per-thread, process-lifetime convenience API.
#[cfg(target_arch = "wasm32")]
mod bounds;
#[cfg(target_arch = "wasm32")]
mod distance;
mod error;
#[cfg(not(target_arch = "wasm32"))]
mod float64;
#[cfg(target_arch = "wasm32")]
mod moments;
mod nearest_dispatch;
mod neighbors;
mod plans;
mod session;
mod support;

use crate::{M3, V3};
pub use error::{GpuArithmetic, GpuMathError, MathExecution};
use gpu_compute::{BackendReport, GpuContext};
pub use nearest_dispatch::NearestNeighborAlgorithm;
pub use plans::{GpuNearestNeighbors, GpuPointCloudStats, MathGpuProgram, PointCloudView};
pub use session::MathGpuSession;

thread_local! {
    // Preserve the existing process-lifetime policy: wgpu teardown from TLS
    // destructors is avoided here, as in photogrammetry gpu::matching.
    // All lazy math kernels now share one device per thread.
    static DEFAULT: std::cell::LazyCell<Option<&'static MathGpuSession>> =
        std::cell::LazyCell::new(|| GpuContext::new().map(|context| {
            Box::leak(Box::new(MathGpuSession::new(&context))) as &'static MathGpuSession
        }));
}

fn with_default<T>(run: impl FnOnce(&MathGpuSession) -> Option<T>) -> Option<T> {
    DEFAULT.with(|session| session.as_ref().and_then(|session| run(session)))
}

pub fn backend_label() -> Option<&'static str> {
    backend_report().map(|report| report.label)
}
pub fn backend_report() -> Option<BackendReport> {
    with_default(|session| Some(session.backend_report()))
}

pub fn nearest_neighbor_gpu(queries: &[V3], targets: &[V3]) -> Option<Vec<(u32, f64)>> {
    with_default(|session| session.nearest_neighbor(queries, targets))
}

pub fn squared_distance_pairs_gpu(a: &[V3], b: &[V3]) -> Option<Vec<f64>> {
    if a.len() != b.len() {
        return None;
    }
    with_default(|session| session.squared_distance_pairs(a, b))
}

pub fn squared_distance_pair_sum_gpu(a: &[V3], b: &[V3]) -> Option<f64> {
    if a.len() != b.len() {
        return None;
    }
    with_default(|session| session.squared_distance_pair_sum(a, b))
}

pub fn transformed_squared_distance_pair_sum_gpu(
    source: &[V3],
    target: &[V3],
    m: M3,
    t: V3,
) -> Option<f64> {
    if source.len() != target.len() {
        return None;
    }
    with_default(|session| session.transformed_squared_distance_pair_sum(source, target, m, t))
}

pub fn point_bounds_gpu(points: &[V3]) -> Option<crate::PointBounds> {
    if points.is_empty() {
        return None;
    }
    with_default(|session| session.point_bounds(points))
}

pub fn transformed_point_bounds_gpu(points: &[V3], m: M3, t: V3) -> Option<crate::PointBounds> {
    if points.is_empty() {
        return None;
    }
    with_default(|session| session.transformed_point_bounds(points, m, t))
}

pub fn point_moments_gpu(points: &[V3]) -> Option<crate::PointMoments> {
    if points.is_empty() {
        return None;
    }
    with_default(|session| session.point_moments(points))
}

pub fn point_cloud_stats_gpu(points: &[V3]) -> Option<crate::PointCloudStats> {
    if points.is_empty() {
        return None;
    }
    with_default(|session| session.point_cloud_stats(points))
}

pub fn directed_chamfer_gpu(queries: &[V3], targets: &[V3]) -> Option<crate::DirectedChamfer> {
    if queries.is_empty() || targets.is_empty() {
        return None;
    }
    with_default(|session| session.directed_chamfer(queries, targets))
}

pub fn nearest_two_gpu(queries: &[V3], targets: &[V3]) -> Option<Vec<crate::TwoNearest>> {
    with_default(|session| session.nearest_two(queries, targets))
}

pub fn nearest_four_gpu(queries: &[V3], targets: &[V3]) -> Option<Vec<crate::FourNearest>> {
    with_default(|session| session.nearest_four(queries, targets))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_nearest_neighbor_matches_cpu_reference_when_available() {
        let queries: Vec<V3> = (0..64)
            .map(|i| {
                let f = i as f64;
                [f * 0.13 - 10., f * -0.07 + 4., (f * 0.031).sin() * 5.]
            })
            .collect();
        let targets: Vec<V3> = (0..20)
            .map(|i| {
                let f = i as f64;
                [f * 0.9 - 6., (f * 0.21).cos() * 4., f * -0.4]
            })
            .collect();
        let Some(got) = nearest_neighbor_gpu(&queries, &targets) else {
            eprintln!("no wgpu adapter available; skipping");
            return;
        };
        let want = crate::nearest_neighbor(&queries, &targets);
        assert_eq!(got.len(), want.len());
        for ((_gi, gd), (_wi, wd)) in got.iter().zip(&want) {
            assert!((gd - wd).abs() < 5e-3 * wd.max(1.0), "{gd} vs {wd}");
        }
    }

    #[test]
    fn gpu_nearest_neighbor_empty_input() {
        if let Some(got) = nearest_neighbor_gpu(&[], &[[0., 0., 0.]]) {
            assert!(got.is_empty());
        }
    }

    #[test]
    fn gpu_squared_distance_pairs_matches_cpu_reference_when_available() {
        let a: Vec<V3> = (0..128)
            .map(|i| {
                let f = i as f64;
                [f * 0.07, (f * 0.03).sin(), (f * 0.11).cos()]
            })
            .collect();
        let b: Vec<V3> = (0..128)
            .map(|i| {
                let f = i as f64;
                [f * -0.02, (f * 0.13).cos(), (f * 0.17).sin()]
            })
            .collect();
        let Some(got) = squared_distance_pairs_gpu(&a, &b) else {
            eprintln!("no wgpu adapter available; skipping");
            return;
        };
        let want = crate::squared_distance_pairs(&a, &b).unwrap();
        for (got, want) in got.iter().zip(&want) {
            assert!((got - want).abs() < 1e-4 * want.max(1.0), "{got} vs {want}");
        }
    }

    #[test]
    fn gpu_squared_distance_pair_sum_matches_cpu_reference_when_available() {
        let a: Vec<V3> = (0..1024)
            .map(|i| {
                let f = i as f64;
                [f * 0.07, (f * 0.03).sin(), (f * 0.11).cos()]
            })
            .collect();
        let b: Vec<V3> = (0..1024)
            .map(|i| {
                let f = i as f64;
                [f * -0.02, (f * 0.13).cos(), (f * 0.17).sin()]
            })
            .collect();
        let Some(got) = squared_distance_pair_sum_gpu(&a, &b) else {
            eprintln!("no wgpu adapter available; skipping");
            return;
        };
        let want = crate::squared_distance_pair_sum(&a, &b).unwrap();
        assert!((got - want).abs() < 1e-4 * want.max(1.0), "{got} vs {want}");
    }
}
