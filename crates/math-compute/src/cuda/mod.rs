//! CUDA nearest-neighbor search (feature `cuda`): runs the PTX port of
//! `NEAREST_NEIGHBOR_WGSL` (`nearest_neighbor.cu` → `nearest_neighbor.ptx`)
//! through the CUDA driver API. f32 arithmetic — see [`crate::Acceleration`].
//!
//! Device buffers are cached per query/target capacity (grow-only) so
//! repeated calls at a stable size amortize allocation; only the elements
//! actually used for the current call are copied to/from the device.
use crate::{M3, V3};
use gpu_compute::cuda::{
    CudaDevice, CudaDeviceReport, CudaFunction, CudaSlice, PushKernelArg, launch_1d,
};

/// PTX generated from `nearest_neighbor.cu` by `scripts/build-cuda-kernels.mjs`.
pub const NEAREST_NEIGHBOR_PTX: &str = include_str!("../nearest_neighbor.ptx");
/// PTX generated from `nearest_two.cu` by `scripts/build-cuda-kernels.mjs`.
pub const NEAREST_TWO_PTX: &str = include_str!("../nearest_two.ptx");
/// PTX generated from `nearest_four.cu` by `scripts/build-cuda-kernels.mjs`.
pub const NEAREST_FOUR_PTX: &str = include_str!("../nearest_four.ptx");
/// PTX generated from `distance_pairs.cu` by `scripts/build-cuda-kernels.mjs`.
pub const DISTANCE_PAIRS_PTX: &str = include_str!("../distance_pairs.ptx");
/// PTX generated from `distance_pair_sum.cu` by `scripts/build-cuda-kernels.mjs`.
pub const DISTANCE_PAIR_SUM_PTX: &str = include_str!("../distance_pair_sum.ptx");
/// PTX generated from `transformed_distance_pair_sum.cu` by `scripts/build-cuda-kernels.mjs`.
pub const TRANSFORMED_DISTANCE_PAIR_SUM_PTX: &str =
    include_str!("../transformed_distance_pair_sum.ptx");
/// PTX generated from `chamfer.cu` by `scripts/build-cuda-kernels.mjs`.
pub const CHAMFER_PTX: &str = include_str!("../chamfer.ptx");
/// PTX generated from `point_bounds.cu` by `scripts/build-cuda-kernels.mjs`.
pub const POINT_BOUNDS_PTX: &str = include_str!("../point_bounds.ptx");
/// PTX generated from `transformed_point_bounds.cu` by `scripts/build-cuda-kernels.mjs`.
pub const TRANSFORMED_POINT_BOUNDS_PTX: &str = include_str!("../transformed_point_bounds.ptx");
/// PTX generated from `point_moments.cu` by `scripts/build-cuda-kernels.mjs`.
pub const POINT_MOMENTS_PTX: &str = include_str!("../point_moments.ptx");
/// PTX generated from `point_cloud_stats.cu` by `scripts/build-cuda-kernels.mjs`.
pub const POINT_CLOUD_STATS_PTX: &str = include_str!("../point_cloud_stats.ptx");
const BLOCK: u32 = 256;

mod bounds;
mod distance;
mod moments;
mod neighbors;

pub(crate) use bounds::point_bounds_cuda;
pub(crate) use bounds::transformed_point_bounds_cuda;
pub(crate) use distance::squared_distance_pair_sum_cuda;
pub(crate) use distance::squared_distance_pairs_cuda;
pub(crate) use distance::transformed_squared_distance_pair_sum_cuda;
pub(crate) use moments::point_cloud_stats_cuda;
pub(crate) use moments::point_moments_cuda;
pub use neighbors::available;
pub use neighbors::device_name;
pub use neighbors::device_report;
pub(crate) use neighbors::directed_chamfer_cuda;
pub(crate) use neighbors::nearest_four_cuda;
pub(crate) use neighbors::nearest_neighbor_cuda;
pub(crate) use neighbors::nearest_two_cuda;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuda_nearest_neighbor_matches_cpu_reference_when_available() {
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
        let Some(got) = nearest_neighbor_cuda(&queries, &targets) else {
            eprintln!("no CUDA device available; skipping");
            return;
        };
        let want = crate::nearest_neighbor(&queries, &targets);
        assert_eq!(got.len(), want.len());
        for ((_gi, gd), (_wi, wd)) in got.iter().zip(&want) {
            assert!((gd - wd).abs() < 5e-3 * wd.max(1.0), "{gd} vs {wd}");
        }
    }

    #[test]
    fn cuda_nearest_neighbor_empty_input() {
        if let Some(got) = nearest_neighbor_cuda(&[], &[[0., 0., 0.]]) {
            assert!(got.is_empty());
        }
    }

    #[test]
    fn cuda_squared_distance_pairs_matches_cpu_reference_when_available() {
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
        let Some(got) = squared_distance_pairs_cuda(&a, &b) else {
            eprintln!("no CUDA device available; skipping");
            return;
        };
        let want = crate::squared_distance_pairs(&a, &b).unwrap();
        for (got, want) in got.iter().zip(&want) {
            assert!((got - want).abs() < 1e-4 * want.max(1.0), "{got} vs {want}");
        }
    }

    #[test]
    fn device_probe_does_not_panic() {
        if available() {
            assert!(device_name().is_some());
        }
    }

    #[test]
    fn cuda_squared_distance_pair_sum_matches_cpu_reference_when_available() {
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
        let Some(got) = squared_distance_pair_sum_cuda(&a, &b) else {
            eprintln!("no CUDA device available; skipping");
            return;
        };
        let want = crate::squared_distance_pair_sum(&a, &b).unwrap();
        assert!((got - want).abs() < 1e-4 * want.max(1.0), "{got} vs {want}");
    }
}
