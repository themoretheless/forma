use super::{GpuArray, GpuMathError, MathGpuProgram, PointCloudView};
use std::ops::Range;
type Step<'a> = (&'a compute_core::Kernel, gpu_compute::wgpu::BindGroup, u32);

/// Fused bounds, raw moments and population statistics in one GPU array.
/// The count is host metadata; the 24 scalar values remain on the device.
/// Use the named ranges when another shader consumes this packed layout.
#[derive(Clone)]
pub struct GpuPointCloudStats {
    pub values: GpuArray<f32>,
    pub samples: usize,
}
impl GpuPointCloudStats {
    pub const LEN: usize = 24;
    /// Minimum xyz.
    pub const MIN: Range<usize> = 0..3;
    /// Maximum xyz.
    pub const MAX: Range<usize> = 3..6;
    /// Coordinate sums x, y, z.
    pub const SUM: Range<usize> = 6..9;
    /// Sum of products xx, xy, xz, yy, yz, zz.
    pub const OUTER_SUM: Range<usize> = 9..15;
    /// Mean x, y, z.
    pub const CENTROID: Range<usize> = 15..18;
    /// Population covariance xx, xy, xz, yy, yz, zz, divided by N.
    pub const COVARIANCE: Range<usize> = 18..24;
}

impl<'a> MathGpuProgram<'a> {
    /// Records partial bounds/moments, hierarchical folds and final centroid/
    /// covariance entirely on the GPU. Empty clouds are rejected.
    /// Arithmetic is f32, including all folds and `E[pp] - E[p]E[p]`; large
    /// coordinate offsets can lose covariance precision. This differs from the
    /// synchronous adapter, whose last fold uses f64 on the CPU. Coordinates
    /// and intermediate sums/products must remain finite within f32 range;
    /// recorded plans cannot inspect GPU input values on the host.
    pub fn point_cloud_stats(
        &mut self,
        points: PointCloudView<'_>,
    ) -> Result<GpuPointCloudStats, GpuMathError> {
        self.check(points)?;
        if points.is_empty() {
            return Err(GpuMathError::InvalidInput(
                "point statistics require a nonempty cloud",
            ));
        }
        let values = self.runtime.zeros(GpuPointCloudStats::LEN)?;
        self.point_cloud_stats_into(points, &values)?;
        Ok(GpuPointCloudStats {
            values,
            samples: points.len(),
        })
    }

    /// Records into an existing 24-element array. Bindings retain intermediates
    /// so a recorded plan can run repeatedly after input updates, without new
    /// allocations. Invalid input leaves the existing plan unchanged.
    pub fn point_cloud_stats_into(
        &mut self,
        points: PointCloudView<'_>,
        output: &GpuArray<f32>,
    ) -> Result<(), GpuMathError> {
        self.statistics_into(points, output, false)
    }

    /// Records statistics with a second pass for population covariance.
    ///
    /// The second pass accumulates `d = p - centroid`, then computes
    /// `E[dd] - E[d]E[d]`. This avoids subtracting large raw second moments;
    /// the residual-mean correction also removes error from rounding the
    /// centroid to f32. All other packed fields are identical to
    /// [`Self::point_cloud_stats`]. No host readback or submission is performed.
    ///
    /// This costs another traversal and reduction of the input. Arithmetic
    /// remains f32 and is not guaranteed positive semidefinite after rounding.
    /// Input conversion already loses unit differences above 2^24; this method
    /// cannot recover them. Inputs, raw sums/products and centered intermediate
    /// sums/products must remain finite, as for the original statistics path.
    pub fn point_cloud_stats_stable(
        &mut self,
        points: PointCloudView<'_>,
    ) -> Result<GpuPointCloudStats, GpuMathError> {
        self.check(points)?;
        if points.is_empty() {
            return Err(GpuMathError::InvalidInput(
                "point statistics require a nonempty cloud",
            ));
        }
        let values = self.runtime.zeros(GpuPointCloudStats::LEN)?;
        self.point_cloud_stats_stable_into(points, &values)?;
        Ok(GpuPointCloudStats {
            values,
            samples: points.len(),
        })
    }

    /// Reuses a 24-element output with the same contract as
    /// [`Self::point_cloud_stats_stable`]. Invalid input leaves the plan unchanged.
    pub fn point_cloud_stats_stable_into(
        &mut self,
        points: PointCloudView<'_>,
        output: &GpuArray<f32>,
    ) -> Result<(), GpuMathError> {
        self.statistics_into(points, output, true)
    }

    fn statistics_into(
        &mut self,
        points: PointCloudView<'_>,
        output: &GpuArray<f32>,
        stable: bool,
    ) -> Result<(), GpuMathError> {
        self.check(points)?;
        if points.is_empty() {
            return Err(GpuMathError::InvalidInput(
                "point statistics require a nonempty cloud",
            ));
        }
        if points.count > self.kernels.stats_partial.max_dispatch_invocations() {
            return Err(GpuMathError::InvalidInput(
                "point statistics exceed one partial dispatch",
            ));
        }
        self.check_output(output, GpuPointCloudStats::LEN, &[points.view])?;
        let count = self.kernels.stats_partial.workgroup_count(points.count);
        let partial = self.runtime.zeros::<f32>(count as usize * 15)?;
        let params = self.params(points.count, 0);
        let bindings = self.kernels.stats_partial.create_bind_group_resources(
            self.runtime.device(),
            &[
                params.as_entire_binding(),
                points.view.storage_binding(self.runtime.context())?,
                partial.view().storage_binding(self.runtime.context())?,
            ],
        );
        // Accumulate locally: allocation or binding validation failure cannot
        // leave a partially appended domain operation in the caller's batch.
        let mut steps = vec![(&self.kernels.stats_partial, bindings, count)];
        let partial = self.fold_statistics(partial, count, &mut steps)?;
        let params = self.params(points.count, 0);
        let bindings = self.kernels.stats_finalize.create_bind_group_resources(
            self.runtime.device(),
            &[
                params.as_entire_binding(),
                partial.view().storage_binding(self.runtime.context())?,
                output.view().storage_binding(self.runtime.context())?,
            ],
        );
        steps.push((&self.kernels.stats_finalize, bindings, 1));
        if stable {
            let count = self.kernels.stats_centered.workgroup_count(points.count);
            let partial = self.runtime.zeros::<f32>(count as usize * 15)?;
            let bindings = self.kernels.stats_centered.create_bind_group_resources(
                self.runtime.device(),
                &[
                    params.as_entire_binding(),
                    points.view.storage_binding(self.runtime.context())?,
                    output.view().storage_binding(self.runtime.context())?,
                    partial.view().storage_binding(self.runtime.context())?,
                ],
            );
            steps.push((&self.kernels.stats_centered, bindings, count));
            let partial = self.fold_statistics(partial, count, &mut steps)?;
            let bindings = self.kernels.stats_covariance.create_bind_group_resources(
                self.runtime.device(),
                &[
                    params.as_entire_binding(),
                    partial.view().storage_binding(self.runtime.context())?,
                    output.view().storage_binding(self.runtime.context())?,
                ],
            );
            steps.push((&self.kernels.stats_covariance, bindings, 1));
        }
        for (kernel, bindings, groups) in steps {
            self.batch.push(kernel, &bindings, groups);
        }
        Ok(())
    }

    fn fold_statistics(
        &self,
        mut partial: GpuArray<f32>,
        mut count: u32,
        steps: &mut Vec<Step<'a>>,
    ) -> Result<GpuArray<f32>, GpuMathError> {
        while count > 1 {
            let groups = self.kernels.stats_fold.workgroup_count(count);
            let next = self.runtime.zeros::<f32>(groups as usize * 15)?;
            let params = self.params(count, 0);
            let bindings = self.kernels.stats_fold.create_bind_group_resources(
                self.runtime.device(),
                &[
                    params.as_entire_binding(),
                    partial.view().storage_binding(self.runtime.context())?,
                    next.view().storage_binding(self.runtime.context())?,
                ],
            );
            steps.push((&self.kernels.stats_fold, bindings, groups));
            partial = next;
            count = groups;
        }
        Ok(partial)
    }
}
