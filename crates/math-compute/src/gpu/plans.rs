//! Domain plans over borrowed, device-validated scalar arrays. No uploads,
//! submits or readbacks occur while recording; the caller owns submission.
use super::{GpuMathError, MathGpuSession};
use crate::{M3, V3};
mod aggregates;
mod neighbors;
pub use aggregates::GpuPointCloudStats;
pub use neighbors::GpuNearestNeighbors;

use compute_core::{
    Binding, ComputeBatch, ComputeRuntime, GpuArray, Kernel, KernelError, Reduction, uniform_f32,
};
use gpu_compute::{GpuBufferView, GpuContext, wgpu};

/// Packed xyz triples of f32 values. Other layouts must be converted explicitly.
#[derive(Clone, Copy)]
pub struct PointCloudView<'a> {
    view: GpuBufferView<'a>,
    count: u32,
}
impl<'a> PointCloudView<'a> {
    pub fn new(array: &'a GpuArray<f32>) -> Result<Self, GpuMathError> {
        if !array.len().is_multiple_of(3) {
            return Err(GpuMathError::InvalidInput(
                "point arrays require packed xyz triples",
            ));
        }
        Ok(Self {
            view: array.view(),
            count: (array.len() / 3) as u32,
        })
    }
    pub fn len(&self) -> usize {
        self.count as usize
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

pub(super) struct PointKernels {
    transform: Kernel,
    distance: Kernel,
    sum: Kernel,
    nearest: super::nearest_dispatch::NearestKernels,
    stats_partial: Kernel,
    stats_fold: Kernel,
    stats_finalize: Kernel,
    stats_centered: Kernel,
    stats_covariance: Kernel,
}
impl PointKernels {
    pub(super) fn new(context: &GpuContext) -> Result<Self, KernelError> {
        let unary = [
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ];
        // The fused partial shader stores 17 scalar equivalents per lane.
        let mut stats_size = 256;
        while stats_size * 68 > context.device.limits().max_compute_workgroup_storage_size {
            stats_size /= 2;
        }
        Ok(Self {
            nearest: super::nearest_dispatch::NearestKernels::new(context)?,
            stats_partial: Kernel::with_workgroup_size(
                &context.device,
                "recorded statistics partials",
                crate::POINT_CLOUD_STATS_WGSL,
                "main",
                &unary,
                stats_size,
            )?,
            stats_fold: Kernel::new(
                &context.device,
                "recorded statistics fold",
                include_str!("../stats_fold.wgsl"),
                "main",
                &unary,
            )?,
            stats_finalize: Kernel::new(
                &context.device,
                "recorded statistics finalize",
                include_str!("../stats_finalize.wgsl"),
                "main",
                &unary,
            )?,
            stats_centered: Kernel::new(
                &context.device,
                "recorded centered statistics partials",
                include_str!("../stats_centered.wgsl"),
                "main",
                &[
                    Binding::Uniform,
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                ],
            )?,
            stats_covariance: Kernel::new(
                &context.device,
                "recorded centered covariance finalize",
                include_str!("../stats_covariance.wgsl"),
                "main",
                &unary,
            )?,
            transform: Kernel::new(
                &context.device,
                "transform points",
                include_str!("../transform_points.wgsl"),
                "main",
                &unary,
            )?,
            distance: Kernel::new(
                &context.device,
                "point distances",
                crate::DISTANCE_PAIRS_WGSL,
                "main",
                &[
                    Binding::Uniform,
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                ],
            )?,
            sum: Kernel::new(
                &context.device,
                "point reduction",
                compute_core::shaders::BLOCK_SUM_WGSL,
                "main",
                &unary,
            )?,
        })
    }
}

pub struct MathGpuProgram<'a> {
    runtime: &'a ComputeRuntime,
    kernels: &'a PointKernels,
    batch: ComputeBatch<'a>,
}
impl MathGpuSession {
    pub fn program<'a>(
        &'a self,
        runtime: &'a ComputeRuntime,
    ) -> Result<MathGpuProgram<'a>, GpuMathError> {
        if !self.context().same_device(runtime.context()) {
            return Err(gpu_compute::BufferError::ForeignDevice.into());
        }
        let kernels = self
            .point_kernels
            .get_or_init(|| PointKernels::new(self.context()))
            .as_ref()
            .map_err(|e| GpuMathError::Kernel(e.clone()))?;
        Ok(MathGpuProgram {
            runtime,
            kernels,
            batch: ComputeBatch::new(),
        })
    }
}
impl<'a> MathGpuProgram<'a> {
    fn check(&self, points: PointCloudView<'_>) -> Result<(), GpuMathError> {
        points
            .view
            .validate(self.runtime.context(), wgpu::BufferUsages::STORAGE)?;
        if points.count > self.kernels.transform.max_dispatch_invocations() {
            return Err(GpuMathError::InvalidInput(
                "point count exceeds one dispatch",
            ));
        }
        Ok(())
    }
    fn check_output<T: compute_core::GpuElement>(
        &self,
        output: &GpuArray<T>,
        len: usize,
        inputs: &[GpuBufferView<'_>],
    ) -> Result<(), GpuMathError> {
        let view = output.view();
        view.validate(self.runtime.context(), wgpu::BufferUsages::STORAGE)?;
        if output.len() != len {
            return Err(compute_core::ComputeError::LengthMismatch {
                expected: len,
                actual: output.len(),
            }
            .into());
        }
        if inputs.iter().any(|input| input.raw() == view.raw()) {
            return Err(compute_core::ComputeError::AliasedOutput.into());
        }
        Ok(())
    }
    fn params(&self, count: u32, extra: u32) -> wgpu::Buffer {
        uniform_f32(
            self.runtime.device(),
            self.runtime.queue(),
            &[f32::from_bits(count), f32::from_bits(extra), 0.0, 0.0],
        )
    }
    pub fn transform(
        &mut self,
        points: PointCloudView<'_>,
        matrix: M3,
        translation: V3,
    ) -> Result<GpuArray<f32>, GpuMathError> {
        self.check(points)?;
        if matrix
            .iter()
            .flatten()
            .chain(translation.iter())
            .any(|v| !(*v as f32).is_finite())
        {
            return Err(GpuMathError::InvalidInput("nonfinite f32 transform"));
        }
        let output = self.runtime.zeros::<f32>(points.len() * 3)?;
        if points.is_empty() {
            return Ok(output);
        }
        let mut params = vec![f32::from_bits(points.count), 0.0, 0.0, 0.0];
        for (row, translation) in matrix.iter().zip(translation) {
            params.extend(row.iter().map(|v| *v as f32));
            params.push(translation as f32);
        }
        let params = uniform_f32(self.runtime.device(), self.runtime.queue(), &params);
        let bindings = self.kernels.transform.create_bind_group_resources(
            self.runtime.device(),
            &[
                params.as_entire_binding(),
                points.view.storage_binding(self.runtime.context())?,
                output.view().storage_binding(self.runtime.context())?,
            ],
        );
        self.batch.push(
            &self.kernels.transform,
            &bindings,
            self.kernels.transform.workgroup_count(points.count),
        );
        Ok(output)
    }
    pub fn squared_distances(
        &mut self,
        a: PointCloudView<'_>,
        b: PointCloudView<'_>,
    ) -> Result<GpuArray<f32>, GpuMathError> {
        self.check(a)?;
        self.check(b)?;
        if a.count != b.count {
            return Err(GpuMathError::InvalidInput("point counts differ"));
        }
        let output = self.runtime.zeros::<f32>(a.len())?;
        if a.is_empty() {
            return Ok(output);
        }
        let params = uniform_f32(
            self.runtime.device(),
            self.runtime.queue(),
            &[f32::from_bits(a.count), 0.0, 0.0, 0.0],
        );
        let bindings = self.kernels.distance.create_bind_group_resources(
            self.runtime.device(),
            &[
                params.as_entire_binding(),
                a.view.storage_binding(self.runtime.context())?,
                b.view.storage_binding(self.runtime.context())?,
                output.view().storage_binding(self.runtime.context())?,
            ],
        );
        self.batch.push(
            &self.kernels.distance,
            &bindings,
            self.kernels.distance.workgroup_count(a.count),
        );
        Ok(output)
    }
    pub fn sum(&mut self, values: &GpuArray<f32>) -> Result<GpuArray<f32>, GpuMathError> {
        values
            .view()
            .validate(self.runtime.context(), wgpu::BufferUsages::STORAGE)?;
        let output = self.runtime.zeros::<f32>(1)?;
        let plan = Reduction::with_output(
            self.runtime.device(),
            self.runtime.queue(),
            &self.kernels.sum,
            values.view().raw(),
            values.len() as u32,
            output.view().raw(),
        );
        self.batch.push_reduction(&plan);
        Ok(output)
    }
    pub fn record(&self, encoder: &mut wgpu::CommandEncoder) {
        self.batch.record(encoder);
    }
    /// Records the ordered domain operations into a caller-owned compute pass,
    /// including a pass with GPU timestamps.
    pub fn record_in_pass(&self, pass: &mut wgpu::ComputePass<'_>) {
        self.batch.record_in_pass(pass);
    }
}
