use crate::{
    HasLowDtype, ScatterOp, Scattered, Shape, TensorError, TensorLowIndexBackend,
    low_precision::check_low_dtypes, scatter_updates_shape,
};

/// Resident scatter from matching f16/bf16 base and update tensors.
/// Shapes, index expansion, update broadcasting and invalid-count semantics
/// match `TensorScatterBackend`. Inputs remain unchanged; results retain the
/// base shape and invalid indices are ignored and counted once on the GPU,
/// including when other dimensions make output slices empty.
///
/// Replace selects the last row-major logical index on duplicates. Its low
/// result preserves all selected raw bits, including NaN payloads and zero
/// signs. Its f32 result uses the storage cast contract: finite values, zeros
/// and infinities widen exactly, while NaNs retain their classification.
///
/// Min/Max operate on finite inputs and preserve selected subnormals exactly;
/// Min(-0,+0)=-0 and Max(-0,+0)=+0. Add/Multiply include the base and every valid
/// update in f32 arithmetic. Intermediate values must remain finite; parallel
/// order and underflow follow backend limits. A low result rounds the completed
/// destination once with round-to-nearest, ties-to-even. Final low overflow is
/// permitted. An f32 result must not first round to low storage.
///
/// All work stays resident. Kernels load low updates directly without making
/// an expanded f32 update tensor. A result-shaped f32 accumulator and integer
/// indexing metadata are allowed. Raw Replace/Min/Max need no f32 arithmetic.
pub trait TensorLowScatterBackend: TensorLowIndexBackend {
    fn scatter_low(
        &self,
        op: ScatterOp,
        input: &Self::LowTensor,
        indices: &Self::UIntTensor,
        updates: &Self::LowTensor,
        axis: usize,
    ) -> Result<Scattered<Self::LowTensor, Self::UIntTensor>, Self::Error>;

    fn scatter_low_f32(
        &self,
        op: ScatterOp,
        input: &Self::LowTensor,
        indices: &Self::UIntTensor,
        updates: &Self::LowTensor,
        axis: usize,
    ) -> Result<Scattered<Self::Tensor, Self::UIntTensor>, Self::Error>;
}

/// Check storage dtypes before the shared scatter axis/count/broadcast rules.
/// Returns the logical expanded update shape, not the base-shaped result.
pub fn low_scatter_updates_shape(
    input: &impl HasLowDtype,
    indices: &Shape,
    updates: &impl HasLowDtype,
    axis: usize,
) -> Result<Shape, TensorError> {
    check_low_dtypes(input, updates)?;
    scatter_updates_shape(input.shape(), indices, updates.shape(), axis)
}
