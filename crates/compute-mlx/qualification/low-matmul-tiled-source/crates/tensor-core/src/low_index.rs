use crate::low_precision::check_low_dtypes;
use crate::{
    Compacted, CompareOp, Gathered, HasLowDtype, ScanOptions, Shape, TensorError,
    TensorIndexBackend, TensorLowBackend, select_shape,
};

/// Resident low-format masks, selection, indexing and prefix sums.
///
/// Compare and Select require matching low dtypes. Comparison produces exact
/// u32 zero/one masks and follows IEEE comparisons for all low bit patterns:
/// both zeros compare equal, infinities are ordered, and any NaN makes only
/// NotEqual true. Finite subnormal comparisons are exact. Mask zero is false;
/// every other u32 value is true.
///
/// Select, Gather and Compact preserve every selected raw low bit, including
/// NaN payloads, signed zero and subnormals. They do not perform floating-point
/// conversion. Gather inserts the index shape at the selected axis, zero-fills
/// invalid reads and counts invalid logical indices once. Compact preserves
/// logical row-major order, returns capacity [input.numel()] with a zero tail,
/// and retains the selected count on the GPU.
///
/// Prefix sums use f32 accumulation in backend-dependent parallel order;
/// inputs and intermediate sums must remain finite. Each low output rounds
/// its completed f32 prefix once, with round-to-nearest, ties-to-even. Final
/// low overflow is permitted. Inclusive/exclusive and forward/reverse scans
/// retain the input shape and coordinate order. Empty tensors stay empty, but
/// the axis is still validated. Scalar tensors have no valid scan axis.
///
/// Execution and intermediates stay on the device. Implementations read low
/// inputs directly; f32 scan outputs/partials are allowed without materializing
/// a full f32 input conversion.
pub trait TensorLowIndexBackend: TensorLowBackend + TensorIndexBackend {
    fn compare_low(
        &self,
        op: CompareOp,
        left: &Self::LowTensor,
        right: &Self::LowTensor,
    ) -> Result<Self::UIntTensor, Self::Error>;

    fn select_low(
        &self,
        mask: &Self::UIntTensor,
        on_true: &Self::LowTensor,
        on_false: &Self::LowTensor,
    ) -> Result<Self::LowTensor, Self::Error>;

    fn gather_low(
        &self,
        input: &Self::LowTensor,
        indices: &Self::UIntTensor,
        axis: usize,
    ) -> Result<Gathered<Self::LowTensor, Self::UIntTensor>, Self::Error>;

    fn compact_low(
        &self,
        input: &Self::LowTensor,
        mask: &Self::UIntTensor,
    ) -> Result<Compacted<Self::LowTensor, Self::UIntTensor>, Self::Error>;

    fn scan_low_f32(
        &self,
        input: &Self::LowTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<Self::Tensor, Self::Error>;

    fn scan_low(
        &self,
        input: &Self::LowTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<Self::LowTensor, Self::Error> {
        let result = self.scan_low_f32(input, axis, options)?;
        self.cast_to_low(&result, input.low_dtype())
    }
}

/// Validate low storage types before jointly broadcasting the three operands.
/// A zero axis in the mask may make a result empty even if broadcasting the
/// two value operands alone would exceed the supported element count.
pub fn low_select_shape(
    mask: &Shape,
    on_true: &impl HasLowDtype,
    on_false: &impl HasLowDtype,
) -> Result<Shape, TensorError> {
    check_low_dtypes(on_true, on_false)?;
    select_shape(mask, on_true.shape(), on_false.shape())
}
