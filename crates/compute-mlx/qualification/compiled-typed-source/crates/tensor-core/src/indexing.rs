use crate::{CompareOp, HasShape, Shape, TensorBackend, TensorError};

/// Prefix sums traverse each axis forwards by default and include the current
/// value. Reverse traversal changes the accumulation direction, not output order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanOptions {
    pub inclusive: bool,
    pub reverse: bool,
}
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            inclusive: true,
            reverse: false,
        }
    }
}

/// Gathered values and a scalar u32 count of invalid logical index elements.
/// Invalid indices produce zero values. Counting indices once avoids multiplying
/// the count by the other dimensions of the source tensor.
#[derive(Clone, Debug)]
pub struct Gathered<T, U> {
    pub values: T,
    pub invalid_count: U,
}

/// Stable logical row-major selection. Values have shape `[input.numel()]`,
/// selected elements occupy the prefix, and the remaining capacity is zero.
/// The scalar u32 count stays on the device so downstream operations can compose
/// without a host synchronization for a dynamic allocation size.
#[derive(Clone, Debug)]
pub struct Compacted<T, U> {
    pub values: T,
    pub count: U,
}

/// Typed masks, indexing and scans extend the common f32 tensor backend.
/// Mask zero means false; every other u32 value means true. Comparisons return
/// exactly zero or one. Integer scans wrap modulo 2^32; f32 scans use the
/// backend's parallel arithmetic order. All intermediate data remains resident.
pub trait TensorIndexBackend: TensorBackend {
    type UIntTensor: HasShape;

    fn upload_u32(&self, shape: Shape, values: &[u32]) -> Result<Self::UIntTensor, Self::Error>;
    fn read_u32(&self, tensor: &Self::UIntTensor) -> Result<Vec<u32>, Self::Error>;
    fn materialize_u32(&self, tensor: &Self::UIntTensor) -> Result<Self::UIntTensor, Self::Error>;
    fn reshape_u32(
        &self,
        tensor: &Self::UIntTensor,
        shape: Shape,
    ) -> Result<Self::UIntTensor, Self::Error>;
    fn permute_u32(
        &self,
        tensor: &Self::UIntTensor,
        axes: &[usize],
    ) -> Result<Self::UIntTensor, Self::Error>;
    fn broadcast_u32(
        &self,
        tensor: &Self::UIntTensor,
        shape: Shape,
    ) -> Result<Self::UIntTensor, Self::Error>;
    fn compare(
        &self,
        op: CompareOp,
        left: &Self::Tensor,
        right: &Self::Tensor,
    ) -> Result<Self::UIntTensor, Self::Error>;
    fn compare_u32(
        &self,
        op: CompareOp,
        left: &Self::UIntTensor,
        right: &Self::UIntTensor,
    ) -> Result<Self::UIntTensor, Self::Error>;
    fn select_f32(
        &self,
        mask: &Self::UIntTensor,
        on_true: &Self::Tensor,
        on_false: &Self::Tensor,
    ) -> Result<Self::Tensor, Self::Error>;
    fn select_u32(
        &self,
        mask: &Self::UIntTensor,
        on_true: &Self::UIntTensor,
        on_false: &Self::UIntTensor,
    ) -> Result<Self::UIntTensor, Self::Error>;
    fn scan_f32(
        &self,
        input: &Self::Tensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<Self::Tensor, Self::Error>;
    fn scan_u32(
        &self,
        input: &Self::UIntTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<Self::UIntTensor, Self::Error>;
    fn gather_f32(
        &self,
        input: &Self::Tensor,
        indices: &Self::UIntTensor,
        axis: usize,
    ) -> Result<Gathered<Self::Tensor, Self::UIntTensor>, Self::Error>;
    fn gather_u32(
        &self,
        input: &Self::UIntTensor,
        indices: &Self::UIntTensor,
        axis: usize,
    ) -> Result<Gathered<Self::UIntTensor, Self::UIntTensor>, Self::Error>;
    fn compact_f32(
        &self,
        input: &Self::Tensor,
        mask: &Self::UIntTensor,
    ) -> Result<Compacted<Self::Tensor, Self::UIntTensor>, Self::Error>;
    fn compact_u32(
        &self,
        input: &Self::UIntTensor,
        mask: &Self::UIntTensor,
    ) -> Result<Compacted<Self::UIntTensor, Self::UIntTensor>, Self::Error>;
}

/// Gathering replaces one input axis with every axis of the index tensor.
/// Scalar indices remove that input axis. Empty index tensors preserve their
/// dimensions in the output; a zero-length input axis is allowed (all indices
/// are then invalid and the result is zero-filled).
pub fn gather_shape(input: &Shape, indices: &Shape, axis: usize) -> Result<Shape, TensorError> {
    input.validate_axes(&[axis])?;
    validate_index_count(indices)?;
    let mut dims = input.dims()[..axis].to_vec();
    dims.extend_from_slice(indices.dims());
    dims.extend_from_slice(&input.dims()[axis + 1..]);
    Shape::new(dims)
}

pub fn select_shape(mask: &Shape, on_true: &Shape, on_false: &Shape) -> Result<Shape, TensorError> {
    Shape::broadcast_all(&[mask, on_true, on_false])
}

/// Compaction's mask may broadcast to the input shape, but cannot expand it.
pub fn compact_shape(input: &Shape, mask: &Shape) -> Result<Shape, TensorError> {
    if mask.broadcast(input)? != *input {
        return Err(TensorError::IncompatibleBroadcast {
            left: mask.dims().to_vec(),
            right: input.dims().to_vec(),
        });
    }
    validate_index_count(input)?;
    Shape::new(vec![input.numel()])
}

pub fn validate_index_count(shape: &Shape) -> Result<(), TensorError> {
    if shape.numel() > u32::MAX as usize {
        return Err(TensorError::IndexCountOverflow {
            count: shape.numel(),
        });
    }
    Ok(())
}
