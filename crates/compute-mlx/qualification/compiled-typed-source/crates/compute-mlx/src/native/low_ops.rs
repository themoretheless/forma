use super::{
    lowering::{NativeLowerer, ops},
    *,
};
use tensor_core::{ReduceOp, TensorLowOpsBackend};

impl TensorLowOpsBackend for MlxBackend {
    fn unary_low(&self, op: UnaryOp, input: &MlxLowTensor) -> Result<MlxLowTensor, MlxError> {
        self.check_low(input)?;
        let tensor = ops::unary_low(&mut NativeLowerer::new(self), input.tensor.clone(), op)?;
        Ok(MlxLowTensor {
            tensor,
            dtype: input.dtype,
        })
    }
    fn binary_low(
        &self,
        op: BinaryOp,
        left: &MlxLowTensor,
        right: &MlxLowTensor,
    ) -> Result<MlxLowTensor, MlxError> {
        self.check_low(left)?;
        self.check_low(right)?;
        let tensor = ops::binary_low(
            &mut NativeLowerer::new(self),
            left.tensor.clone(),
            right.tensor.clone(),
            op,
        )?;
        Ok(MlxLowTensor {
            tensor,
            dtype: left.dtype,
        })
    }
    fn reduce_low_f32(
        &self,
        op: ReduceOp,
        input: &MlxLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        ops::reduce_low(
            &mut NativeLowerer::new(self),
            input.tensor.clone(),
            op,
            axes,
            keep_dims,
            false,
        )
    }
    fn mean_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        ops::reduce_low(
            &mut NativeLowerer::new(self),
            input.tensor.clone(),
            ReduceOp::Sum,
            axes,
            keep_dims,
            true,
        )
    }
}
