//! Port for device (GPU/CUDA) kernel backends.
//!
//! `math-core` stays a CPU-only leaf: every `*_accelerated` entry point asks
//! the installed [`DeviceKernels`] first and falls back to the CPU reference
//! when no backend is installed or a kernel declines (`None`). Backend crates
//! such as `osv-math-compute` implement the trait and install it once.

use std::sync::OnceLock;

use crate::{
    Acceleration, DirectedChamfer, FourNearest, M3, PointBounds, PointCloudStats, PointMoments,
    TwoNearest, V3,
};

/// Device implementations of the batch kernels. `acceleration` is the
/// already-resolved placement (`Gpu` or `Cuda`, or `Auto` where a kernel does
/// not resolve it); every method defaults to `None`, i.e. "use the CPU".
#[allow(unused_variables)]
pub trait DeviceKernels: Send + Sync {
    /// Whether a CUDA device is usable; `Auto` placement heuristics prefer
    /// CUDA only when this holds.
    fn cuda_available(&self) -> bool {
        false
    }
    fn nearest_neighbor(
        &self,
        acceleration: Acceleration,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<Vec<(u32, f64)>> {
        None
    }
    fn nearest_two(
        &self,
        acceleration: Acceleration,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<Vec<TwoNearest>> {
        None
    }
    fn nearest_four(
        &self,
        acceleration: Acceleration,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<Vec<FourNearest>> {
        None
    }
    fn directed_chamfer(
        &self,
        acceleration: Acceleration,
        queries: &[V3],
        targets: &[V3],
    ) -> Option<DirectedChamfer> {
        None
    }
    fn squared_distance_pairs(
        &self,
        acceleration: Acceleration,
        a: &[V3],
        b: &[V3],
    ) -> Option<Vec<f64>> {
        None
    }
    fn squared_distance_pair_sum(
        &self,
        acceleration: Acceleration,
        a: &[V3],
        b: &[V3],
    ) -> Option<f64> {
        None
    }
    fn transformed_squared_distance_pair_sum(
        &self,
        acceleration: Acceleration,
        source: &[V3],
        target: &[V3],
        m: M3,
        t: V3,
    ) -> Option<f64> {
        None
    }
    fn point_bounds(&self, acceleration: Acceleration, points: &[V3]) -> Option<PointBounds> {
        None
    }
    fn transformed_point_bounds(
        &self,
        acceleration: Acceleration,
        points: &[V3],
        m: M3,
        t: V3,
    ) -> Option<PointBounds> {
        None
    }
    fn point_moments(&self, acceleration: Acceleration, points: &[V3]) -> Option<PointMoments> {
        None
    }
    fn point_cloud_stats(
        &self,
        acceleration: Acceleration,
        points: &[V3],
    ) -> Option<PointCloudStats> {
        None
    }
}

static KERNELS: OnceLock<&'static dyn DeviceKernels> = OnceLock::new();

/// Installs the process-wide device backend. Returns `false` when one is
/// already installed (the first installation wins).
pub fn install_device_kernels(kernels: &'static dyn DeviceKernels) -> bool {
    KERNELS.set(kernels).is_ok()
}

/// The installed device backend, if any.
pub fn device_kernels() -> Option<&'static dyn DeviceKernels> {
    KERNELS.get().copied()
}

pub(crate) fn cuda_available() -> bool {
    device_kernels().is_some_and(|kernels| kernels.cuda_available())
}

pub(crate) fn kernels() -> Option<&'static dyn DeviceKernels> {
    device_kernels()
}
