use crate::{Shape, TensorBackend, TensorError, mean_shape};

/// Mean and population variance (division by N, with no degrees-of-freedom
/// correction). Both tensors have the same reduced shape.
#[derive(Clone, Debug)]
pub struct Moments<T> {
    pub mean: T,
    pub variance: T,
}

/// Stable f32 distribution and normalization operations over arbitrary axes.
/// All intermediate values remain on the device. Inputs must be finite; NaN
/// and infinity input policies are not shared across backends. Results follow
/// f32 accuracy and underflow limits; mathematically out-of-range variances or
/// log probabilities may overflow to infinity. Compare using tolerances.
///
/// Implementations subtract a row maximum before exponentiation, and preserve
/// that shift when computing log probabilities. Moments/normalization use
/// scaled or wider centered deviations, avoiding both raw squared moments and a
/// potentially overflowing sum of uncentered inputs. Layer normalization must
/// remain usable when the unnormalized variance exceeds the f32 range.
///
/// An empty axis list treats each element as a singleton group: softmax is one,
/// log-softmax and layer norm are zero, logsumexp and mean preserve the input,
/// and variance is zero. Elementwise results preserve empty input shapes.
/// Reduced results reject empty contractions only when their output is nonempty.
/// Gamma/beta affine transforms compose through ordinary resident binary ops.
pub trait TensorStatsBackend: TensorBackend {
    fn softmax(&self, input: &Self::Tensor, axes: &[usize]) -> Result<Self::Tensor, Self::Error>;
    fn log_softmax(
        &self,
        input: &Self::Tensor,
        axes: &[usize],
    ) -> Result<Self::Tensor, Self::Error>;
    fn logsumexp(
        &self,
        input: &Self::Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::Tensor, Self::Error>;
    fn moments(
        &self,
        input: &Self::Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<Self::Tensor>, Self::Error>;
    /// (x - mean) / sqrt(population_variance + epsilon), preserving input shape.
    /// Epsilon is finite and strictly positive, including representable positive
    /// subnormals. Constant groups normalize to zero. Implementations may use
    /// scaled units throughout, without storing overflowing original variance.
    fn layer_norm(
        &self,
        input: &Self::Tensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<Self::Tensor, Self::Error>;
}

/// Validates axes for distribution/normalization output, which keeps input shape.
pub fn normalization_shape(input: &Shape, axes: &[usize]) -> Result<Shape, TensorError> {
    input.validate_axes(axes)?;
    Ok(input.clone())
}

/// Moments and logsumexp share the nonempty-contraction reduction rule.
pub fn statistics_shape(
    input: &Shape,
    axes: &[usize],
    keep_dims: bool,
) -> Result<Shape, TensorError> {
    mean_shape(input, axes, keep_dims)
}

pub fn validate_epsilon(epsilon: f32) -> Result<(), TensorError> {
    if !epsilon.is_finite() || epsilon <= 0.0 {
        return Err(TensorError::InvalidEpsilon);
    }
    Ok(())
}
