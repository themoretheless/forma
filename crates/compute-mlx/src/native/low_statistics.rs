use super::{
    lowering::{NativeLowerer, statistics_low as recipes},
    *,
};
use tensor_core::{Moments, TensorLowStatsBackend};

impl TensorLowStatsBackend for MlxBackend {
    fn softmax_low_f32(&self, input: &MlxLowTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        recipes::softmax(
            &mut NativeLowerer::new(self),
            input.tensor.clone(),
            axes,
            false,
        )
    }
    fn log_softmax_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        recipes::softmax(
            &mut NativeLowerer::new(self),
            input.tensor.clone(),
            axes,
            true,
        )
    }
    fn logsumexp_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        recipes::logsumexp(
            &mut NativeLowerer::new(self),
            input.tensor.clone(),
            axes,
            keep_dims,
        )
    }
    fn moments_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<MlxTensor>, MlxError> {
        self.check_low(input)?;
        recipes::moments(
            &mut NativeLowerer::new(self),
            input.tensor.clone(),
            axes,
            keep_dims,
        )
    }
    fn layer_norm_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        recipes::layer_norm(
            &mut NativeLowerer::new(self),
            input.tensor.clone(),
            axes,
            epsilon,
        )
    }
}
