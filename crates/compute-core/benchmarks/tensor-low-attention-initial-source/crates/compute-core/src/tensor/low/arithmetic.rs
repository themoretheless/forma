use super::{GpuLowTensor, Result, checked_index};
use crate::{
    ComputeProgram,
    tensor_core::{BinaryOp, Layout, UnaryOp, low_binary_shape},
};

impl ComputeProgram<'_> {
    /// Packed unary arithmetic with f32 evaluation and one final low rounding.
    /// Negate/Abs operate on sign bits and preserve finite subnormals exactly.
    pub fn tensor_unary_low(&mut self, op: UnaryOp, input: &GpuLowTensor) -> Result<GpuLowTensor> {
        self.low_check(input)?;
        let output = self
            .runtime
            .zeros_low(input.dtype(), input.shape().clone())?;
        self.tensor_unary_low_into(op, input, &output)?;
        Ok(output)
    }
    pub fn tensor_unary_low_into(
        &mut self,
        op: UnaryOp,
        input: &GpuLowTensor,
        output: &GpuLowTensor,
    ) -> Result<()> {
        self.low_check(input)?;
        self.low_output(
            output,
            input.shape(),
            input.dtype(),
            &[input.packed_words().buffer()],
        )?;
        self.low_arithmetic(
            op as u32,
            input,
            input.layout(),
            input,
            input.layout(),
            output,
        )
    }
    /// Broadcast arithmetic requires matching low dtypes. Min/Max select exact
    /// finite input bits, including BF16 subnormals and -0/+0 ordering.
    pub fn tensor_binary_low(
        &mut self,
        op: BinaryOp,
        left: &GpuLowTensor,
        right: &GpuLowTensor,
    ) -> Result<GpuLowTensor> {
        self.low_check(left)?;
        self.low_check(right)?;
        let shape = low_binary_shape(left, right)?;
        let output = self.runtime.zeros_low(left.dtype(), shape)?;
        self.tensor_binary_low_into(op, left, right, &output)?;
        Ok(output)
    }
    pub fn tensor_binary_low_into(
        &mut self,
        op: BinaryOp,
        left: &GpuLowTensor,
        right: &GpuLowTensor,
        output: &GpuLowTensor,
    ) -> Result<()> {
        self.low_check(left)?;
        self.low_check(right)?;
        let shape = low_binary_shape(left, right)?;
        self.low_output(
            output,
            &shape,
            left.dtype(),
            &[left.packed_words().buffer(), right.packed_words().buffer()],
        )?;
        let a = left.layout().broadcast_to(shape.clone())?;
        let b = right.layout().broadcast_to(shape)?;
        self.low_arithmetic(32 + op as u32, left, &a, right, &b, output)
    }
    fn low_arithmetic(
        &mut self,
        op: u32,
        a: &GpuLowTensor,
        av: &Layout,
        b: &GpuLowTensor,
        bv: &Layout,
        output: &GpuLowTensor,
    ) -> Result<()> {
        let count = checked_index(output.shape().numel())?;
        if count == 0 {
            return Ok(());
        }
        let words =
            checked_index((output.shape().numel() + (output.layout().offset() & 1)).div_ceil(2))?;
        let groups = self.index_groups(words.div_ceil(256));
        let mut meta = vec![
            count,
            checked_index(output.shape().rank())?,
            groups,
            checked_index(av.offset())?,
            checked_index(bv.offset())?,
            checked_index(output.layout().offset())?,
            output.dtype() as u32,
            op,
            words,
        ];
        for (axis, &dim) in output.shape().dims().iter().enumerate() {
            meta.extend([
                checked_index(dim)?,
                checked_index(av.strides()[axis])?,
                checked_index(bv.strides()[axis])?,
            ]);
        }
        let kernel = &self.runtime.low_kernels()?.arithmetic;
        self.index_dispatch(
            kernel,
            &meta,
            &[
                a.packed_words().buffer(),
                b.packed_words().buffer(),
                output.packed_words().buffer(),
            ],
            groups,
        )
    }
}
