//! [`DeviceKernels`] adapter: CUDA first when requested (feature `cuda`),
//! then the portable wgpu shader; `None` lets `osv-math` use the CPU.

use math_core::{
    Acceleration, DeviceKernels, DirectedChamfer, FourNearest, M3, PointBounds, PointCloudStats,
    PointMoments, TwoNearest, V3,
};

use crate::gpu;

pub(crate) struct Device;

/// Tries the CUDA port when `Cuda` was requested, then the wgpu shader.
macro_rules! cuda_then_gpu {
    ($acceleration:expr, $cuda:ident, $gpu:ident($($arg:expr),*)) => {{
        #[cfg(feature = "cuda")]
        if $acceleration == Acceleration::Cuda {
            if let Some(value) = crate::cuda::$cuda($($arg),*) {
                return Some(value);
            }
        }
        let _ = $acceleration;
        gpu::$gpu($($arg),*)
    }};
}

impl DeviceKernels for Device {
    fn cuda_available(&self) -> bool {
        cfg!(feature = "cuda")
    }
    fn nearest_neighbor(
        &self,
        a: Acceleration,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<Vec<(u32, f64)>> {
        cuda_then_gpu!(
            a,
            nearest_neighbor_cuda,
            nearest_neighbor_gpu(queries, targets)
        )
    }
    fn nearest_two(
        &self,
        a: Acceleration,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<Vec<TwoNearest>> {
        cuda_then_gpu!(a, nearest_two_cuda, nearest_two_gpu(queries, targets))
    }
    fn nearest_four(
        &self,
        a: Acceleration,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<Vec<FourNearest>> {
        cuda_then_gpu!(a, nearest_four_cuda, nearest_four_gpu(queries, targets))
    }
    fn directed_chamfer(
        &self,
        a: Acceleration,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<DirectedChamfer> {
        cuda_then_gpu!(
            a,
            directed_chamfer_cuda,
            directed_chamfer_gpu(queries, targets)
        )
    }
    fn squared_distance_pairs(&self, a: Acceleration, p: &[V3], q: &[V3]) -> Option<Vec<f64>> {
        cuda_then_gpu!(
            a,
            squared_distance_pairs_cuda,
            squared_distance_pairs_gpu(p, q)
        )
    }
    fn squared_distance_pair_sum(&self, a: Acceleration, p: &[V3], q: &[V3]) -> Option<f64> {
        cuda_then_gpu!(
            a,
            squared_distance_pair_sum_cuda,
            squared_distance_pair_sum_gpu(p, q)
        )
    }
    fn transformed_squared_distance_pair_sum(
        &self,
        a: Acceleration,
        source: &[V3],
        target: &[V3],
        m: M3,
        t: V3,
    ) -> Option<f64> {
        cuda_then_gpu!(
            a,
            transformed_squared_distance_pair_sum_cuda,
            transformed_squared_distance_pair_sum_gpu(source, target, m, t)
        )
    }
    fn point_bounds(&self, a: Acceleration, points: &[V3]) -> Option<PointBounds> {
        cuda_then_gpu!(a, point_bounds_cuda, point_bounds_gpu(points))
    }
    fn transformed_point_bounds(
        &self,
        a: Acceleration,
        points: &[V3],
        m: M3,
        t: V3,
    ) -> Option<PointBounds> {
        cuda_then_gpu!(
            a,
            transformed_point_bounds_cuda,
            transformed_point_bounds_gpu(points, m, t)
        )
    }
    fn point_moments(&self, a: Acceleration, points: &[V3]) -> Option<PointMoments> {
        cuda_then_gpu!(a, point_moments_cuda, point_moments_gpu(points))
    }
    fn point_cloud_stats(&self, a: Acceleration, points: &[V3]) -> Option<PointCloudStats> {
        cuda_then_gpu!(a, point_cloud_stats_cuda, point_cloud_stats_gpu(points))
    }
}
