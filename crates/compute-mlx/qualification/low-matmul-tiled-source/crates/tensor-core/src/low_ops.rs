use crate::low_precision::check_low_dtypes;
use crate::{BinaryOp, HasLowDtype, ReduceOp, Shape, TensorError, TensorLowBackend, UnaryOp};

/// Arithmetic on resident f16/bf16 tensors, with explicit f32 evaluation.
/// Unary and binary operations return the input storage dtype after one final
/// round-to-nearest, ties-to-even conversion. Binary dtypes must match; shapes
/// use ordinary trailing-axis broadcasting. Final low output may overflow to
/// infinity even when the evaluated f32 result is finite.
///
/// Negate and Abs operate exactly on finite low-format bits, including signed
/// zero and subnormals. Min/Max choose an original finite value exactly, with
/// Min(-0,+0)=-0 and Max(-0,+0)=+0. The same extrema rule applies to reductions.
/// Other operations follow f32 arithmetic/transcendental tolerances and
/// underflow limits before final low rounding; function domains must be valid.
/// NaN/infinite inputs have no shared arithmetic policy.
///
/// Reductions accumulate in f32, keeping intermediate arithmetic in range;
/// mean uses f32 sum/count. Empty axes preserve logical values, empty outputs
/// stay empty, empty contractions use sum=0/product=1, while Min/Max/mean reject
/// nonempty outputs from an empty contraction. The f32-result methods preserve
/// information that would be lost by rounding first to low storage.
///
/// Execution and all intermediates stay on the device. Backend documentation
/// specifies direct input loads or any materialized conversion intermediates.
pub trait TensorLowOpsBackend: TensorLowBackend {
    fn unary_low(
        &self,
        op: UnaryOp,
        input: &Self::LowTensor,
    ) -> Result<Self::LowTensor, Self::Error>;

    fn binary_low(
        &self,
        op: BinaryOp,
        left: &Self::LowTensor,
        right: &Self::LowTensor,
    ) -> Result<Self::LowTensor, Self::Error>;

    fn reduce_low_f32(
        &self,
        op: ReduceOp,
        input: &Self::LowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::Tensor, Self::Error>;

    fn mean_low_f32(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::Tensor, Self::Error>;

    fn reduce_low(
        &self,
        op: ReduceOp,
        input: &Self::LowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::LowTensor, Self::Error> {
        let result = self.reduce_low_f32(op, input, axes, keep_dims)?;
        self.cast_to_low(&result, input.low_dtype())
    }

    fn mean_low(
        &self,
        input: &Self::LowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::LowTensor, Self::Error> {
        let result = self.mean_low_f32(input, axes, keep_dims)?;
        self.cast_to_low(&result, input.low_dtype())
    }
}

/// Checks matching low storage formats before the broadcast result is created.
pub fn low_binary_shape(
    left: &impl HasLowDtype,
    right: &impl HasLowDtype,
) -> Result<Shape, TensorError> {
    check_low_dtypes(left, right)?;
    left.shape().broadcast(right.shape())
}
