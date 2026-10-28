//! Stable arbitrary-axis statistics. Only per-group summaries and bounded
//! reduction partials are allocated; operands stay in their original layout.
use super::{GpuTensor, TensorComputeError, checked_index};
use crate::{Binding, ComputeProgram, ComputeRuntime, GpuArray, Kernel, KernelError};
use tensor_core::{Moments, Shape, normalization_shape, statistics_shape, validate_epsilon};

type Result<T> = std::result::Result<T, TensorComputeError>;

pub(crate) struct StatsKernels {
    small: Kernel,
    reduce: Kernel,
    finish: Kernel,
    output: Kernel,
    summary_words: usize,
}
impl ComputeRuntime {
    fn tensor_stats_kernels(&self) -> std::result::Result<&StatsKernels, KernelError> {
        if let Some(kernels) = self.tensor_stats.get() {
            return Ok(kernels);
        }
        let kernels = StatsKernels::new(
            self.device(),
            [
                crate::shaders::TENSOR_STATS_SMALL_WGSL,
                crate::shaders::TENSOR_STATS_REDUCE_WGSL,
                crate::shaders::TENSOR_STATS_FINISH_WGSL,
                crate::shaders::TENSOR_STATS_OUTPUT_WGSL,
            ],
            4,
        )?;
        let _ = self.tensor_stats.set(kernels);
        Ok(self
            .tensor_stats
            .get()
            .expect("statistics kernels initialized"))
    }
}

impl StatsKernels {
    pub(in crate::tensor) fn new(
        device: &crate::wgpu::Device,
        sources: [&str; 4],
        summary_words: usize,
    ) -> std::result::Result<Self, KernelError> {
        let four = [
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ];
        Ok(Self {
            summary_words,
            small: Kernel::new(
                device,
                "small tensor statistics groups",
                sources[0],
                "main",
                &[
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                    Binding::StorageReadWrite,
                ],
            )?,
            reduce: Kernel::new(
                device,
                "scaled tensor statistics partials",
                sources[1],
                "main",
                &four,
            )?,
            finish: Kernel::new(
                device,
                "scaled tensor statistics summaries",
                sources[2],
                "main",
                &four,
            )?,
            output: Kernel::new(
                device,
                "stable tensor normalization output",
                sources[3],
                "main",
                &[
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                    Binding::StorageReadWrite,
                ],
            )?,
        })
    }
}

#[derive(Clone, Copy)]
pub(in crate::tensor) enum Operation {
    Softmax = 0,
    LogSoftmax = 1,
    LogSumExp = 2,
    Moments = 3,
    LayerNorm = 4,
}

impl<'a> ComputeProgram<'a> {
    /// Stable probabilities over arbitrary axes, preserving input shape.
    /// Finite inputs are required. Empty axes treat each value as a singleton.
    pub fn tensor_softmax(&mut self, input: &GpuTensor, axes: &[usize]) -> Result<GpuTensor> {
        self.index_check(input)?;
        let output = self.index_new(normalization_shape(input.shape(), axes)?)?;
        self.tensor_softmax_into(input, axes, &output)?;
        Ok(output)
    }
    pub fn tensor_softmax_into(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = normalization_shape(input.shape(), axes)?;
        self.stats_plan(Operation::Softmax, input, axes, shape, 1.0, output, None)
    }
    /// Shifted log probabilities. A mathematically out-of-range negative
    /// result is written as IEEE negative infinity.
    pub fn tensor_log_softmax(&mut self, input: &GpuTensor, axes: &[usize]) -> Result<GpuTensor> {
        self.index_check(input)?;
        let output = self.index_new(normalization_shape(input.shape(), axes)?)?;
        self.tensor_log_softmax_into(input, axes, &output)?;
        Ok(output)
    }
    pub fn tensor_log_softmax_into(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = normalization_shape(input.shape(), axes)?;
        self.stats_plan(Operation::LogSoftmax, input, axes, shape, 1.0, output, None)
    }
    pub fn tensor_logsumexp(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor> {
        self.index_check(input)?;
        let output = self.index_new(statistics_shape(input.shape(), axes, keep_dims)?)?;
        self.tensor_logsumexp_into(input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_logsumexp_into(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        self.stats_plan(Operation::LogSumExp, input, axes, shape, 1.0, output, None)
    }
    /// Mean and population variance from scaled, centered deviations.
    /// An out-of-range variance is written as IEEE positive infinity.
    pub fn tensor_moments(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<GpuTensor>> {
        self.index_check(input)?;
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        let output = Moments {
            mean: self.index_new(shape.clone())?,
            variance: self.index_new(shape)?,
        };
        self.tensor_moments_into(input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_moments_into(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &Moments<GpuTensor>,
    ) -> Result<()> {
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        self.stats_plan(
            Operation::Moments,
            input,
            axes,
            shape,
            1.0,
            &output.mean,
            Some(&output.variance),
        )
    }
    /// Normalizes in scaled coordinates without forming an overflowing
    /// original variance. Epsilon must be finite and positive; positive
    /// subnormals are accepted. Constant groups produce zero.
    pub fn tensor_layer_norm(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<GpuTensor> {
        validate_epsilon(epsilon)?;
        self.index_check(input)?;
        let output = self.index_new(normalization_shape(input.shape(), axes)?)?;
        self.tensor_layer_norm_into(input, axes, epsilon, &output)?;
        Ok(output)
    }
    pub fn tensor_layer_norm_into(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        epsilon: f32,
        output: &GpuTensor,
    ) -> Result<()> {
        validate_epsilon(epsilon)?;
        let shape = normalization_shape(input.shape(), axes)?;
        self.stats_plan(
            Operation::LayerNorm,
            input,
            axes,
            shape,
            epsilon,
            output,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn stats_plan(
        &mut self,
        op: Operation,
        input: &GpuTensor,
        axes: &[usize],
        shape: Shape,
        epsilon: f32,
        output: &GpuTensor,
        variance: Option<&GpuTensor>,
    ) -> Result<()> {
        self.index_check(input)?;
        self.stats_plan_loaded(
            op,
            input.values().buffer(),
            input.layout(),
            axes,
            shape,
            epsilon,
            output,
            variance,
            self.runtime.tensor_stats_kernels()?,
            &[],
        )
    }

    /// Shared checked traversal. Callers validate input runtime ownership.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::tensor) fn stats_plan_loaded(
        &mut self,
        op: Operation,
        input: &crate::wgpu::Buffer,
        layout: &tensor_core::Layout,
        axes: &[usize],
        shape: Shape,
        epsilon: f32,
        output: &GpuTensor,
        variance: Option<&GpuTensor>,
        kernels: &'a StatsKernels,
        metadata_tail: &[u32],
    ) -> Result<()> {
        self.index_output(output, &shape, &[input])?;
        if let Some(variance) = variance {
            self.index_output(variance, &shape, &[input, output.values().buffer()])?;
        }
        if shape.numel() == 0 {
            return Ok(());
        }
        let mut reduced = vec![false; layout.shape().rank()];
        for &axis in axes {
            reduced[axis] = true;
        }
        let dense = tensor_core::Layout::contiguous(layout.shape().clone())?;
        let mut row_dims = Vec::new();
        let mut red_dims = Vec::new();
        let mut row_descriptors = Vec::new();
        let mut red_descriptors = Vec::new();
        for (axis, &is_reduced) in reduced.iter().enumerate() {
            let dim = layout.shape().dims()[axis];
            let descriptor = [
                checked_index(dim)?,
                checked_index(layout.strides()[axis])?,
                checked_index(dense.strides()[axis])?,
            ];
            if is_reduced {
                red_dims.push(dim);
                red_descriptors.extend(descriptor);
            } else {
                row_dims.push(dim);
                row_descriptors.extend(descriptor);
            }
        }
        let rows = checked_index(Shape::new(row_dims)?.numel())?;
        let red_count = checked_index(Shape::new(red_dims)?.numel())?;
        let parts = red_count
            .div_ceil(4096)
            .clamp(1, 256)
            .min((4096 / rows).max(1));
        let groups = self.index_groups(rows * parts);
        let mut metadata = vec![
            rows,
            checked_index(layout.shape().rank() - axes.len())?,
            checked_index(layout.offset())?,
            red_count,
            checked_index(axes.len())?,
            groups,
            parts,
            0,
            epsilon.sqrt().to_bits(),
            checked_index(output.layout().offset())?,
            checked_index(variance.map_or(0, |v| v.layout().offset()))?,
            checked_index(shape.numel())?,
            self.index_groups(rows.div_ceil(256)),
        ];
        metadata.extend(row_descriptors);
        metadata.extend(red_descriptors);

        metadata.extend(metadata_tail);
        let mut prepared = self.runtime.program();
        let unused_variance = self.runtime.zeros::<f32>(1)?;
        if (2..=256).contains(&red_count) {
            metadata[7] = op as u32;
            prepared.index_dispatch(
                &kernels.small,
                &metadata,
                &[
                    input,
                    output.values().buffer(),
                    variance.map_or(unused_variance.buffer(), |v| v.values().buffer()),
                ],
                groups,
            )?;
            self.batch.append(prepared.batch);
            return Ok(());
        }
        let dummy = self.runtime.zeros::<f32>(kernels.summary_words)?;
        let mut summary = dummy.clone();
        if red_count > 1 {
            let partials = self
                .runtime
                .zeros::<f32>(rows as usize * parts as usize * 2)?;
            let stages: &[u32] = match op {
                Operation::Softmax | Operation::LogSoftmax | Operation::LogSumExp => &[0, 1],
                Operation::Moments | Operation::LayerNorm => &[0, 2, 3],
            };
            for &stage in stages {
                metadata[7] = stage;
                let next: GpuArray<f32> =
                    self.runtime.zeros(rows as usize * kernels.summary_words)?;
                prepared.index_dispatch(
                    &kernels.reduce,
                    &metadata,
                    &[input, summary.buffer(), partials.buffer()],
                    groups,
                )?;
                prepared.index_dispatch(
                    &kernels.finish,
                    &metadata,
                    &[partials.buffer(), summary.buffer(), next.buffer()],
                    metadata[12],
                )?;
                summary = next;
            }
        }
        metadata[7] = op as u32;
        metadata[12] = self.index_groups(metadata[11].div_ceil(256));
        prepared.index_dispatch(
            &kernels.output,
            &metadata,
            &[
                input,
                summary.buffer(),
                output.values().buffer(),
                variance.map_or(unused_variance.buffer(), |v| v.values().buffer()),
            ],
            metadata[12],
        )?;
        self.batch.append(prepared.batch);
        Ok(())
    }
}
