use crate::{CudaError, CudaLowTensor, CudaRuntime, CudaTensor};
use tensor_core::{
    Moments, TensorLowBackend, TensorLowStatsBackend, statistics_shape, validate_epsilon,
};

impl TensorLowStatsBackend for CudaRuntime {
    fn softmax_low_f32(
        &self,
        input: &CudaLowTensor,
        axes: &[usize],
    ) -> Result<CudaTensor, CudaError> {
        self.normalize_loaded(&input.tensor, axes, 0, 0., Some(input.dtype as u32))
    }
    fn log_softmax_low_f32(
        &self,
        input: &CudaLowTensor,
        axes: &[usize],
    ) -> Result<CudaTensor, CudaError> {
        self.normalize_loaded(&input.tensor, axes, 1, 0., Some(input.dtype as u32))
    }
    fn layer_norm_low_f32(
        &self,
        input: &CudaLowTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<CudaTensor, CudaError> {
        validate_epsilon(epsilon)?;
        self.normalize_loaded(&input.tensor, axes, 2, epsilon, Some(input.dtype as u32))
    }
    fn logsumexp_low_f32(
        &self,
        input: &CudaLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor, CudaError> {
        self.check(&input.tensor)?;
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        if shape.is_empty() {
            return self.zeros(shape);
        }
        if input.shape().numel() == shape.numel() {
            // This is the actual result, not a widened input temporary. Direct
            // conversion preserves finite singleton subnormals and signed zero.
            let result = self.cast_to_f32(input)?;
            return self.view(&result, result.layout().reshape(shape)?);
        }
        self.stats_logsumexp_loaded(&input.tensor, axes, shape, Some(input.dtype as u32))
    }
    fn moments_low_f32(
        &self,
        input: &CudaLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<CudaTensor>, CudaError> {
        self.check(&input.tensor)?;
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        let variance = self.zeros(shape.clone())?;
        if shape.is_empty() {
            return Ok(Moments {
                mean: self.zeros(shape)?,
                variance,
            });
        }
        if input.shape().numel() == shape.numel() {
            let mean = self.cast_to_f32(input)?;
            let mean = self.view(&mean, mean.layout().reshape(shape)?)?;
            return Ok(Moments { mean, variance });
        }
        self.stats_moments_loaded(
            &input.tensor,
            axes,
            shape,
            variance,
            Some(input.dtype as u32),
        )
    }
}
