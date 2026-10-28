use super::{
    lowering::{NativeLowerer, statistics as recipes},
    *,
};
use tensor_core::{Moments, TensorStatsBackend};

impl TensorStatsBackend for MlxBackend {
    fn softmax(&self, input: &MlxTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        recipes::softmax(&mut NativeLowerer::new(self), input.clone(), axes)
    }
    fn log_softmax(&self, input: &MlxTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        recipes::log_softmax(&mut NativeLowerer::new(self), input.clone(), axes)
    }
    fn logsumexp(
        &self,
        input: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        recipes::logsumexp(
            &mut NativeLowerer::new(self),
            input.clone(),
            axes,
            keep_dims,
        )
    }
    fn moments(
        &self,
        input: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        recipes::moments(
            &mut NativeLowerer::new(self),
            input.clone(),
            axes,
            keep_dims,
        )
    }
    fn layer_norm(
        &self,
        input: &MlxTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        recipes::layer_norm(&mut NativeLowerer::new(self), input.clone(), axes, epsilon)
    }
}
