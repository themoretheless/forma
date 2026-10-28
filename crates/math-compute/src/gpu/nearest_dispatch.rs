//! Shared nearest-neighbor pipeline ownership and dispatch policy.
use super::support::{NN_BINDINGS, WG_DEFAULT, WG_METAL};
use compute_core::{Kernel, KernelError};
use gpu_compute::{GpuContext, wgpu};

/// GPU implementation selected for a nearest-neighbor problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NearestNeighborAlgorithm {
    /// One invocation per query, scanning targets in ascending index order.
    Scalar,
    /// One 64-lane workgroup per query, with a parallel target scan/reduction.
    Cooperative,
}
impl NearestNeighborAlgorithm {
    /// Selection is restricted to the backend and problem sizes measured by
    /// `bench_recorded_nearest`. Other shapes retain the original scalar path.
    pub fn for_shape(backend: wgpu::Backend, queries: usize, targets: usize) -> Self {
        // Apple M4 Max measurements cover 256x512, 4096x4096 and
        // 16384x8192. Keep the original path outside that measured envelope,
        // especially many-query/few-target problems (workgroup overhead).
        if backend == wgpu::Backend::Metal
            && (512..=8192).contains(&targets)
            && (256..=targets * 2).contains(&queries)
        {
            Self::Cooperative
        } else {
            Self::Scalar
        }
    }
    pub(super) fn index(self) -> usize {
        match self {
            Self::Scalar => 0,
            Self::Cooperative => 1,
        }
    }
}

pub(super) struct NearestKernels {
    kernels: [Kernel; 2],
    backend: wgpu::Backend,
}
impl NearestKernels {
    pub(super) fn new(context: &GpuContext) -> Result<Self, KernelError> {
        Ok(Self {
            kernels: [
                Kernel::tuned(
                    context,
                    "nearest neighbor scalar",
                    crate::NEAREST_NEIGHBOR_WGSL,
                    "main",
                    &NN_BINDINGS,
                    WG_METAL,
                    WG_DEFAULT,
                )?,
                Kernel::with_workgroup_size(
                    &context.device,
                    "nearest neighbor cooperative",
                    crate::NEAREST_NEIGHBOR_COOPERATIVE_WGSL,
                    "main",
                    &NN_BINDINGS,
                    64,
                )?,
            ],
            backend: context.backend,
        })
    }
    pub(super) fn select(&self, queries: usize, targets: usize) -> (&Kernel, usize, u32) {
        let algorithm = NearestNeighborAlgorithm::for_shape(self.backend, queries, targets);
        let index = algorithm.index();
        let kernel = &self.kernels[index];
        let groups = match algorithm {
            NearestNeighborAlgorithm::Scalar => kernel.workgroup_count(queries as u32),
            NearestNeighborAlgorithm::Cooperative => queries as u32,
        };
        (kernel, index, groups)
    }
    pub(super) fn bind_groups(
        &self,
        device: &wgpu::Device,
        buffers: &[&wgpu::Buffer],
    ) -> [wgpu::BindGroup; 2] {
        self.kernels
            .each_ref()
            .map(|kernel| kernel.create_bind_group(device, buffers))
    }
}
