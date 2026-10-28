use crate::{HasLowDtype, Moments, TensorLowBackend, TensorStatsBackend};

/// Stable statistics and normalization from resident f16/bf16 storage.
/// Shapes, axes, empty contractions, population variance and epsilon follow
/// `TensorStatsBackend`. Inputs must be finite. Evaluation uses f32 accuracy
/// or wider intermediates, including max-shifted exponentials and centered,
/// scaled/wider moments. Layer norm remains usable when reported variance
/// exceeds f32 range. Backend f32 arithmetic and underflow limits apply.
///
/// The `_f32` methods retain the evaluated result without first rounding to
/// low storage. Other methods return the input dtype after one final nearest-
/// even conversion. Final low outputs may underflow or overflow. Compare
/// transcendental and statistical results with numerical tolerances; the final
/// storage conversion still follows the exact `TensorLowBackend` cast contract.
///
/// Empty axes treat each element as a singleton: softmax is one, log-softmax
/// and layer norm are zero, logsumexp and mean preserve finite input values
/// exactly (including subnormals and signed zero), and variance is zero.
/// The same identities apply when selected axes have contraction size one.
///
/// Input loads and all intermediates stay on device. Implementations must not
/// expand the complete input, shifted values or centered values into a separate
/// f32 tensor. Row statistics and bounded partial-reduction state are allowed,
/// as are actual f32 results and their final casts to low storage.
pub trait TensorLowStatsBackend: TensorLowBackend + TensorStatsBackend {
    fn softmax_low_f32(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
    ) -> Result<Self::Tensor, Self::Error>;

    fn log_softmax_low_f32(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
    ) -> Result<Self::Tensor, Self::Error>;

    fn logsumexp_low_f32(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::Tensor, Self::Error>;

    fn moments_low_f32(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<Self::Tensor>, Self::Error>;

    fn layer_norm_low_f32(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<Self::Tensor, Self::Error>;

    fn softmax_low(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
    ) -> Result<Self::LowTensor, Self::Error> {
        let result = self.softmax_low_f32(input, axes)?;
        self.cast_to_low(&result, input.low_dtype())
    }

    fn log_softmax_low(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
    ) -> Result<Self::LowTensor, Self::Error> {
        let result = self.log_softmax_low_f32(input, axes)?;
        self.cast_to_low(&result, input.low_dtype())
    }

    fn logsumexp_low(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::LowTensor, Self::Error> {
        let result = self.logsumexp_low_f32(input, axes, keep_dims)?;
        self.cast_to_low(&result, input.low_dtype())
    }

    fn moments_low(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<Self::LowTensor>, Self::Error> {
        let result = self.moments_low_f32(input, axes, keep_dims)?;
        Ok(Moments {
            mean: self.cast_to_low(&result.mean, input.low_dtype())?,
            variance: self.cast_to_low(&result.variance, input.low_dtype())?,
        })
    }

    fn layer_norm_low(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<Self::LowTensor, Self::Error> {
        let result = self.layer_norm_low_f32(input, axes, epsilon)?;
        self.cast_to_low(&result, input.low_dtype())
    }
}
