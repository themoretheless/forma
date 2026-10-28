use crate::{
    Compacted, CompareOp, Gathered, ScanOptions, ScatterOp, Scattered, TensorF64Backend,
    TensorIndexBackend,
};

/// Binary64 comparisons, selection, prefix sums and device indexing. Uses the
/// existing u32 index/mask contract: zero is false, nonzero is true; comparison
/// results are zero or one. Views and value selection retain all binary64 bits.
/// Scans accumulate in f64 with backend-dependent parallel ordering; all inputs
/// and intermediate sums must be finite. No f32 or CPU fallback is permitted.
pub trait TensorF64IndexBackend: TensorF64Backend + TensorIndexBackend {
    fn compare_f64(
        &self,
        op: CompareOp,
        left: &Self::F64Tensor,
        right: &Self::F64Tensor,
    ) -> Result<Self::UIntTensor, Self::Error>;
    fn select_f64(
        &self,
        mask: &Self::UIntTensor,
        on_true: &Self::F64Tensor,
        on_false: &Self::F64Tensor,
    ) -> Result<Self::F64Tensor, Self::Error>;
    fn scan_f64(
        &self,
        input: &Self::F64Tensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<Self::F64Tensor, Self::Error>;
    /// Invalid indices produce zero and are counted once per logical index,
    /// including when another axis makes the value output empty.
    fn gather_f64(
        &self,
        input: &Self::F64Tensor,
        indices: &Self::UIntTensor,
        axis: usize,
    ) -> Result<Gathered<Self::F64Tensor, Self::UIntTensor>, Self::Error>;
    /// Stable row-major selection. Output capacity is input.numel(), the selected
    /// prefix length is a resident u32 scalar, and unused capacity is zero.
    fn compact_f64(
        &self,
        input: &Self::F64Tensor,
        mask: &Self::UIntTensor,
    ) -> Result<Compacted<Self::F64Tensor, Self::UIntTensor>, Self::Error>;
}

/// Binary64 scatter with the shared gather-shaped update expansion. The input
/// remains unchanged. Replace chooses the last logical index on duplicates;
/// arithmetic folds include the original value and all valid updates in a
/// backend-dependent order. Min/max order signed zeros as -0 < +0. Arithmetic
/// inputs/intermediates must be finite. Replace preserves selected payload bits.
/// Out-of-range indices are ignored and counted once each on the device.
pub trait TensorF64ScatterBackend: TensorF64IndexBackend {
    fn scatter_f64(
        &self,
        op: ScatterOp,
        input: &Self::F64Tensor,
        indices: &Self::UIntTensor,
        updates: &Self::F64Tensor,
        axis: usize,
    ) -> Result<Scattered<Self::F64Tensor, Self::UIntTensor>, Self::Error>;
}
