//! Direct packed statistics with shared arbitrary-axis traversal and one final
//! low conversion. Tiny groups use exact exponent scaling before arithmetic.
use super::{GpuLowTensor, GpuTensor, Result};
use crate::tensor::normalization::{Operation, StatsKernels};
use crate::{ComputeProgram, ComputeRuntime, KernelError};
use tensor_core::{Moments, Shape, normalization_shape, statistics_shape, validate_epsilon};

impl ComputeRuntime {
    fn low_stats_kernels(&self) -> std::result::Result<&StatsKernels, KernelError> {
        if let Some(kernels) = self.tensor_low_stats.get() {
            return Ok(kernels);
        }
        let sources = [
            super::statistics_sources::small(),
            super::statistics_sources::reduce(),
            super::statistics_sources::finish(),
            super::statistics_sources::output(),
        ];
        let kernels = StatsKernels::new(
            self.device(),
            [&sources[0], &sources[1], &sources[2], &sources[3]],
            8,
        )?;
        let _ = self.tensor_low_stats.set(kernels);
        Ok(self
            .tensor_low_stats
            .get()
            .expect("low statistics kernels initialized"))
    }
}
impl ComputeProgram<'_> {
    /// Stable statistics from packed input; retains the f32 result without low rounding.
    pub fn tensor_softmax_low_f32(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
    ) -> Result<GpuTensor> {
        self.low_check(input)?;
        let output = self.index_new(normalization_shape(input.shape(), axes)?)?;
        self.tensor_softmax_low_f32_into(input, axes, &output)?;
        Ok(output)
    }
    pub fn tensor_softmax_low_f32_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = normalization_shape(input.shape(), axes)?;
        self.low_stats_f32(Operation::Softmax, input, axes, shape, 1.0, output, None)
    }
    /// Stable statistics from packed input; rounds only the completed f32 result to the input dtype.
    pub fn tensor_softmax_low(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
    ) -> Result<GpuLowTensor> {
        self.low_check(input)?;
        let output = self
            .runtime
            .zeros_low(input.dtype(), normalization_shape(input.shape(), axes)?)?;
        self.tensor_softmax_low_into(input, axes, &output)?;
        Ok(output)
    }
    pub fn tensor_softmax_low_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        output: &GpuLowTensor,
    ) -> Result<()> {
        let shape = normalization_shape(input.shape(), axes)?;
        self.low_stats_rounded(Operation::Softmax, input, axes, shape, 1.0, output, None)
    }
    /// Stable statistics from packed input; retains the f32 result without low rounding.
    pub fn tensor_log_softmax_low_f32(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
    ) -> Result<GpuTensor> {
        self.low_check(input)?;
        let output = self.index_new(normalization_shape(input.shape(), axes)?)?;
        self.tensor_log_softmax_low_f32_into(input, axes, &output)?;
        Ok(output)
    }
    pub fn tensor_log_softmax_low_f32_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = normalization_shape(input.shape(), axes)?;
        self.low_stats_f32(Operation::LogSoftmax, input, axes, shape, 1.0, output, None)
    }
    /// Stable statistics from packed input; rounds only the completed f32 result to the input dtype.
    pub fn tensor_log_softmax_low(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
    ) -> Result<GpuLowTensor> {
        self.low_check(input)?;
        let output = self
            .runtime
            .zeros_low(input.dtype(), normalization_shape(input.shape(), axes)?)?;
        self.tensor_log_softmax_low_into(input, axes, &output)?;
        Ok(output)
    }
    pub fn tensor_log_softmax_low_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        output: &GpuLowTensor,
    ) -> Result<()> {
        let shape = normalization_shape(input.shape(), axes)?;
        self.low_stats_rounded(Operation::LogSoftmax, input, axes, shape, 1.0, output, None)
    }
    /// Stable statistics from packed input; retains the f32 result without low rounding.
    pub fn tensor_logsumexp_low_f32(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor> {
        self.low_check(input)?;
        let output = self.index_new(statistics_shape(input.shape(), axes, keep_dims)?)?;
        self.tensor_logsumexp_low_f32_into(input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_logsumexp_low_f32_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        self.low_stats_f32(Operation::LogSumExp, input, axes, shape, 1.0, output, None)
    }
    /// Stable statistics from packed input; rounds only the completed f32 result to the input dtype.
    pub fn tensor_logsumexp_low(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuLowTensor> {
        self.low_check(input)?;
        let output = self.runtime.zeros_low(
            input.dtype(),
            statistics_shape(input.shape(), axes, keep_dims)?,
        )?;
        self.tensor_logsumexp_low_into(input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_logsumexp_low_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuLowTensor,
    ) -> Result<()> {
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        self.low_stats_rounded(Operation::LogSumExp, input, axes, shape, 1.0, output, None)
    }
    /// Stable statistics from packed input; retains the f32 result without low rounding.
    pub fn tensor_layer_norm_low_f32(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<GpuTensor> {
        validate_epsilon(epsilon)?;
        self.low_check(input)?;
        let output = self.index_new(normalization_shape(input.shape(), axes)?)?;
        self.tensor_layer_norm_low_f32_into(input, axes, epsilon, &output)?;
        Ok(output)
    }
    pub fn tensor_layer_norm_low_f32_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        epsilon: f32,
        output: &GpuTensor,
    ) -> Result<()> {
        validate_epsilon(epsilon)?;
        let shape = normalization_shape(input.shape(), axes)?;
        self.low_stats_f32(
            Operation::LayerNorm,
            input,
            axes,
            shape,
            epsilon,
            output,
            None,
        )
    }
    /// Stable statistics from packed input; rounds only the completed f32 result to the input dtype.
    pub fn tensor_layer_norm_low(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<GpuLowTensor> {
        validate_epsilon(epsilon)?;
        self.low_check(input)?;
        let output = self
            .runtime
            .zeros_low(input.dtype(), normalization_shape(input.shape(), axes)?)?;
        self.tensor_layer_norm_low_into(input, axes, epsilon, &output)?;
        Ok(output)
    }
    pub fn tensor_layer_norm_low_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        epsilon: f32,
        output: &GpuLowTensor,
    ) -> Result<()> {
        validate_epsilon(epsilon)?;
        let shape = normalization_shape(input.shape(), axes)?;
        self.low_stats_rounded(
            Operation::LayerNorm,
            input,
            axes,
            shape,
            epsilon,
            output,
            None,
        )
    }
    /// Centered population moments from packed input, with f32 evaluation.
    pub fn tensor_moments_low_f32(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<GpuTensor>> {
        self.low_check(input)?;
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        let output = Moments {
            mean: self.index_new(shape.clone())?,
            variance: self.index_new(shape.clone())?,
        };
        self.tensor_moments_low_f32_into(input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_moments_low_f32_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &Moments<GpuTensor>,
    ) -> Result<()> {
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        self.low_stats_f32(
            Operation::Moments,
            input,
            axes,
            shape,
            1.0,
            &output.mean,
            Some(&output.variance),
        )
    }
    /// Centered population moments from packed input, with f32 evaluation.
    pub fn tensor_moments_low(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<GpuLowTensor>> {
        self.low_check(input)?;
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        let output = Moments {
            mean: self.runtime.zeros_low(input.dtype(), shape.clone())?,
            variance: self.runtime.zeros_low(input.dtype(), shape.clone())?,
        };
        self.tensor_moments_low_into(input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_moments_low_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &Moments<GpuLowTensor>,
    ) -> Result<()> {
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        self.low_stats_rounded(
            Operation::Moments,
            input,
            axes,
            shape,
            1.0,
            &output.mean,
            Some(&output.variance),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn low_stats_f32(
        &mut self,
        op: Operation,
        input: &GpuLowTensor,
        axes: &[usize],
        shape: Shape,
        epsilon: f32,
        output: &GpuTensor,
        variance: Option<&GpuTensor>,
    ) -> Result<()> {
        self.low_check(input)?;
        self.stats_plan_loaded(
            op,
            input.packed_words().buffer(),
            input.layout(),
            axes,
            shape,
            epsilon,
            output,
            variance,
            self.runtime.low_stats_kernels()?,
            &[input.dtype() as u32, op as u32],
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn low_stats_rounded(
        &mut self,
        op: Operation,
        input: &GpuLowTensor,
        axes: &[usize],
        shape: Shape,
        epsilon: f32,
        output: &GpuLowTensor,
        variance: Option<&GpuLowTensor>,
    ) -> Result<()> {
        self.low_check(input)?;
        self.low_output(
            output,
            &shape,
            input.dtype(),
            &[input.packed_words().buffer()],
        )?;
        if let Some(variance) = variance {
            self.low_output(
                variance,
                &shape,
                input.dtype(),
                &[
                    input.packed_words().buffer(),
                    output.packed_words().buffer(),
                ],
            )?;
        }
        if shape.numel() == 0 {
            return Ok(());
        }
        let mut prepared = self.runtime.program();
        let result = self.index_new(shape.clone())?;
        let result_variance = variance
            .map(|_| self.index_new(shape.clone()))
            .transpose()?;
        prepared.low_stats_f32(
            op,
            input,
            axes,
            shape,
            epsilon,
            &result,
            result_variance.as_ref(),
        )?;
        prepared.tensor_cast_to_low_into(&result, output)?;
        if let (Some(source), Some(output)) = (&result_variance, variance) {
            prepared.tensor_cast_to_low_into(source, output)?;
        }
        self.batch.append(prepared.batch);
        Ok(())
    }
}
