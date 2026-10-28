use crate::{Shape, TensorError, TensorIndexBackend, gather_shape};

/// How device-indexed updates combine with the original tensor.
/// Replace selects the last logical index when indices repeat. Other operations
/// include the base value and every valid update; f32 order is backend-dependent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ScatterOp {
    Replace,
    Add,
    Multiply,
    Min,
    Max,
}

/// Updated values retain the base shape. Invalid indices leave values unchanged
/// and are counted once per logical index in a device-resident u32 scalar,
/// including when another dimension makes the output empty.
#[derive(Clone, Debug)]
pub struct Scattered<T, U> {
    pub values: T,
    pub invalid_count: U,
}

/// Resident scatter along one axis, using the same index expansion as gather.
/// Updates broadcast to `base[..axis] + indices.shape + base[axis+1..]` without
/// enlarging it. Inputs are unchanged. Out-of-range indices are ignored.
///
/// Replace is deterministic: the greatest row-major logical position in the
/// index tensor wins, independently of its storage strides. Add, multiply, min
/// and max fold the original value with all valid updates. u32 addition and
/// multiplication wrap modulo 2^32; f32 inputs and intermediates must remain
/// finite, and accumulation order may vary across executions and backends.
pub trait TensorScatterBackend: TensorIndexBackend {
    fn scatter_f32(
        &self,
        op: ScatterOp,
        input: &Self::Tensor,
        indices: &Self::UIntTensor,
        updates: &Self::Tensor,
        axis: usize,
    ) -> Result<Scattered<Self::Tensor, Self::UIntTensor>, Self::Error>;
    fn scatter_u32(
        &self,
        op: ScatterOp,
        input: &Self::UIntTensor,
        indices: &Self::UIntTensor,
        updates: &Self::UIntTensor,
        axis: usize,
    ) -> Result<Scattered<Self::UIntTensor, Self::UIntTensor>, Self::Error>;
}

/// Validates the axis, u32 invalid-count limit and update broadcasting. Returns
/// the expanded update shape; the scatter result itself retains `input` shape.
pub fn scatter_updates_shape(
    input: &Shape,
    indices: &Shape,
    updates: &Shape,
    axis: usize,
) -> Result<Shape, TensorError> {
    let expected = gather_shape(input, indices, axis)?;
    if updates.broadcast(&expected)? != expected {
        return Err(TensorError::IncompatibleBroadcast {
            left: updates.dims().to_vec(),
            right: expected.dims().to_vec(),
        });
    }
    Ok(expected)
}
