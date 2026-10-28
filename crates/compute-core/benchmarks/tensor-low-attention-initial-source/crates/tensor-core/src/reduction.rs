use crate::{Shape, TensorError, TensorIndexBackend};

/// Associative reductions for finite f32 values and exact u32 storage.
/// Integer sums and products wrap modulo 2^32. Floating-point order depends on
/// the backend; callers must keep intermediate arithmetic within its domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ReduceOp {
    Sum,
    Product,
    Min,
    Max,
}

/// Axis reductions keep intermediate data on the device. Empty axes are an
/// identity operation. Sum and product use zero and one for empty contractions;
/// min, max and mean reject empty contractions that would produce values.
/// An empty output is valid for every operation. Mean returns f32 values and
/// uses the full contracted element count, including broadcast dimensions.
pub trait TensorReduceBackend: TensorIndexBackend {
    fn reduce_f32(
        &self,
        op: ReduceOp,
        input: &Self::Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::Tensor, Self::Error>;
    fn reduce_u32(
        &self,
        op: ReduceOp,
        input: &Self::UIntTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::UIntTensor, Self::Error>;
    fn mean_axes(
        &self,
        input: &Self::Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::Tensor, Self::Error>;
}

/// Validates axes, output size and the operation's empty-contraction contract.
pub fn reduction_shape(
    op: ReduceOp,
    input: &Shape,
    axes: &[usize],
    keep_dims: bool,
) -> Result<Shape, TensorError> {
    let output = input.reduce(axes, keep_dims)?;
    if matches!(op, ReduceOp::Min | ReduceOp::Max) {
        validate_nonempty_contraction(input, &output, axes)?;
    }
    Ok(output)
}

pub fn mean_shape(input: &Shape, axes: &[usize], keep_dims: bool) -> Result<Shape, TensorError> {
    let output = input.reduce(axes, keep_dims)?;
    validate_nonempty_contraction(input, &output, axes)?;
    Ok(output)
}

fn validate_nonempty_contraction(
    input: &Shape,
    output: &Shape,
    axes: &[usize],
) -> Result<(), TensorError> {
    if !output.is_empty() && axes.iter().any(|&axis| input.dims()[axis] == 0) {
        return Err(TensorError::EmptyReduction);
    }
    Ok(())
}
