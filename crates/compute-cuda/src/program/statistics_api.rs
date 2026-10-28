use super::{CudaProgramBuilder, CudaValue};
use crate::CudaError;
use tensor_core::Moments;
impl CudaProgramBuilder<'_> {
    pub fn softmax(&mut self, value: CudaValue, axes: &[usize]) -> Result<CudaValue, CudaError> {
        self.plan.softmax(value, axes)
    }
    pub fn softmax_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        self.plan.softmax_low_f32(value, axes)
    }
    pub fn softmax_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        self.plan.softmax_low(value, axes)
    }
    pub fn log_softmax(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        self.plan.log_softmax(value, axes)
    }
    pub fn log_softmax_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        self.plan.log_softmax_low_f32(value, axes)
    }
    pub fn log_softmax_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
    ) -> Result<CudaValue, CudaError> {
        self.plan.log_softmax_low(value, axes)
    }
    pub fn logsumexp(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.logsumexp(value, axes, keep_dims)
    }
    pub fn logsumexp_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.logsumexp_low_f32(value, axes, keep_dims)
    }
    pub fn logsumexp_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.logsumexp_low(value, axes, keep_dims)
    }
    pub fn moments(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<CudaValue>, CudaError> {
        self.plan.moments(value, axes, keep_dims)
    }
    pub fn moments_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<CudaValue>, CudaError> {
        self.plan.moments_low_f32(value, axes, keep_dims)
    }
    pub fn moments_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<CudaValue>, CudaError> {
        self.plan.moments_low(value, axes, keep_dims)
    }
    pub fn layer_norm(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<CudaValue, CudaError> {
        self.plan.layer_norm(value, axes, epsilon)
    }
    pub fn layer_norm_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<CudaValue, CudaError> {
        self.plan.layer_norm_low_f32(value, axes, epsilon)
    }
    pub fn layer_norm_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<CudaValue, CudaError> {
        self.plan.layer_norm_low(value, axes, epsilon)
    }
}
