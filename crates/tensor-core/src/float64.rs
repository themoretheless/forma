use crate::{BinaryOp, HasShape, ReduceOp, Shape, TensorBackend, UnaryOp};

/// Execution precision of the optional binary64 tensor contract. Storage alone
/// does not establish arithmetic support. SoftwareBinary64 must retain binary64
/// precision/range; a pair of f32 values with a different contract is insufficient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Float64Support {
    #[default]
    Unsupported,
    Native,
    SoftwareBinary64,
}

/// Resident binary64 arithmetic, with no implicit f32 conversion or CPU fallback.
/// Upload/read are the only host value transfers. Values, constants, intermediate
/// products and reduction accumulators remain f64. GEMM cannot opt into reduced
/// precision. Library availability, allocation limits and supported execution
/// platforms remain backend-specific and failures must be reported.
///
/// Operations follow the shared shape/view/empty-reduction contracts. Arithmetic
/// inputs and intermediate results must be finite, with each unary function's
/// domain respected. Reduction order, FMA and transcendental rounding can differ;
/// this contract does not promise bit-identical CPU/GPU results or correct rounding
/// of every transcendental. Views preserve binary64 payloads exactly.
pub trait TensorF64Backend: TensorBackend {
    type F64Tensor: HasShape;
    fn upload_f64(&self, shape: Shape, values: &[f64]) -> Result<Self::F64Tensor, Self::Error>;
    fn read_f64(&self, tensor: &Self::F64Tensor) -> Result<Vec<f64>, Self::Error>;
    fn materialize_f64(&self, tensor: &Self::F64Tensor) -> Result<Self::F64Tensor, Self::Error>;
    fn reshape_f64(
        &self,
        tensor: &Self::F64Tensor,
        shape: Shape,
    ) -> Result<Self::F64Tensor, Self::Error>;
    fn permute_f64(
        &self,
        tensor: &Self::F64Tensor,
        axes: &[usize],
    ) -> Result<Self::F64Tensor, Self::Error>;
    fn broadcast_f64(
        &self,
        tensor: &Self::F64Tensor,
        shape: Shape,
    ) -> Result<Self::F64Tensor, Self::Error>;
    fn narrow_f64(
        &self,
        tensor: &Self::F64Tensor,
        axis: usize,
        start: usize,
        len: usize,
    ) -> Result<Self::F64Tensor, Self::Error>;
    fn unary_f64(
        &self,
        op: UnaryOp,
        input: &Self::F64Tensor,
    ) -> Result<Self::F64Tensor, Self::Error>;
    fn binary_f64(
        &self,
        op: BinaryOp,
        left: &Self::F64Tensor,
        right: &Self::F64Tensor,
    ) -> Result<Self::F64Tensor, Self::Error>;
    fn reduce_f64(
        &self,
        op: ReduceOp,
        input: &Self::F64Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::F64Tensor, Self::Error>;
    fn mean_f64(
        &self,
        input: &Self::F64Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::F64Tensor, Self::Error>;
    fn matmul_f64(
        &self,
        left: &Self::F64Tensor,
        right: &Self::F64Tensor,
    ) -> Result<Self::F64Tensor, Self::Error>;
    /// Validate all owners before completing submitted producers without reading
    /// values. Empty lists still fence submitted work, as with TensorEvalBackend.
    fn evaluate_f64(&self, tensors: &[&Self::F64Tensor]) -> Result<(), Self::Error>;
}
