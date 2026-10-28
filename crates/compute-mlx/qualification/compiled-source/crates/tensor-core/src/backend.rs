use crate::{BinaryOp, Shape, UnaryOp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Wgsl,
    Cuda,
    Mlx,
}

/// Multiplication precision is an explicit policy, distinct from f32 output
/// storage. Reduced-precision modes permit, but do not prove, Tensor Core use.
/// Unsupported modes must return an error instead of silently changing precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MatmulPrecision {
    #[default]
    F32,
    AllowTf32,
    AllowF16,
    AllowBf16,
}

pub trait HasShape {
    fn shape(&self) -> &Shape;
}

/// Common native f32 tensor operations. Implementations keep intermediate data
/// on their device; only upload/read cross the host boundary. Operations may
/// queue work or build a lazy graph. `read_f32` completes outstanding producers.
/// Backend-specific recorded programs remain available for explicit batching.
///
/// Shapes and arithmetic domains are shared. f32 reduction order/FMA behavior
/// can differ, so equality across backends is tolerance-based. No backend may
/// silently execute an unsupported operation on the CPU.
pub trait TensorBackend {
    type Tensor: HasShape;
    type Error: std::error::Error;

    fn kind(&self) -> BackendKind;
    fn upload_f32(&self, shape: Shape, values: &[f32]) -> Result<Self::Tensor, Self::Error>;
    fn read_f32(&self, tensor: &Self::Tensor) -> Result<Vec<f32>, Self::Error>;
    fn materialize(&self, tensor: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;
    /// Reshapes logical row-major values, materializing a view when required.
    fn reshape(&self, tensor: &Self::Tensor, shape: Shape) -> Result<Self::Tensor, Self::Error>;
    fn permute(&self, tensor: &Self::Tensor, axes: &[usize]) -> Result<Self::Tensor, Self::Error>;
    fn broadcast_to(
        &self,
        tensor: &Self::Tensor,
        shape: Shape,
    ) -> Result<Self::Tensor, Self::Error>;
    fn unary(&self, op: UnaryOp, input: &Self::Tensor) -> Result<Self::Tensor, Self::Error>;
    fn binary(
        &self,
        op: BinaryOp,
        left: &Self::Tensor,
        right: &Self::Tensor,
    ) -> Result<Self::Tensor, Self::Error>;
    fn sum_axes(
        &self,
        input: &Self::Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::Tensor, Self::Error>;
    /// Vector, matrix or batched matrix product with broadcast batch axes.
    /// Rank-one operands are promoted and their inserted dimensions removed;
    /// two vectors yield a scalar. Rank-zero operands are invalid. A zero
    /// contraction length yields zeros, including the empty-vector dot product.
    fn matmul(
        &self,
        left: &Self::Tensor,
        right: &Self::Tensor,
        precision: MatmulPrecision,
    ) -> Result<Self::Tensor, Self::Error>;
}
