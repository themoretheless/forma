use super::{CudaProgramBuilder, CudaValue};
use crate::CudaError;
use tensor_core::{BinaryOp, CompareOp, Layout, LowDtype, ReduceOp, UnaryOp};
impl CudaProgramBuilder<'_> {
    pub fn input_u32(&mut self, layout: Layout) -> Result<CudaValue, CudaError> {
        self.plan.input_u32(layout)
    }
    pub fn input_low(&mut self, dtype: LowDtype, layout: Layout) -> Result<CudaValue, CudaError> {
        self.plan.input_low(dtype, layout)
    }
    pub fn cast_to_low(
        &mut self,
        value: CudaValue,
        dtype: LowDtype,
    ) -> Result<CudaValue, CudaError> {
        self.plan.cast_to_low(value, dtype)
    }
    pub fn cast_to_f32(&mut self, value: CudaValue) -> Result<CudaValue, CudaError> {
        self.plan.cast_to_f32(value)
    }
    pub fn unary_low(&mut self, value: CudaValue, op: UnaryOp) -> Result<CudaValue, CudaError> {
        self.plan.unary_low(value, op)
    }
    pub fn binary_u32(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: BinaryOp,
    ) -> Result<CudaValue, CudaError> {
        self.plan.binary_u32(left, right, op)
    }
    pub fn binary_low(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: BinaryOp,
    ) -> Result<CudaValue, CudaError> {
        self.plan.binary_low(left, right, op)
    }
    pub fn compare(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: CompareOp,
    ) -> Result<CudaValue, CudaError> {
        self.plan.compare(left, right, op)
    }
    pub fn compare_u32(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: CompareOp,
    ) -> Result<CudaValue, CudaError> {
        self.plan.compare_u32(left, right, op)
    }
    pub fn compare_low(
        &mut self,
        left: CudaValue,
        right: CudaValue,
        op: CompareOp,
    ) -> Result<CudaValue, CudaError> {
        self.plan.compare_low(left, right, op)
    }
    pub fn select(
        &mut self,
        mask: CudaValue,
        yes: CudaValue,
        no: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        self.plan.select(mask, yes, no)
    }
    pub fn select_u32(
        &mut self,
        mask: CudaValue,
        yes: CudaValue,
        no: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        self.plan.select_u32(mask, yes, no)
    }
    pub fn select_low(
        &mut self,
        mask: CudaValue,
        yes: CudaValue,
        no: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        self.plan.select_low(mask, yes, no)
    }
    pub fn reduce_u32(
        &mut self,
        value: CudaValue,
        op: ReduceOp,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.reduce_u32(value, op, axes, keep_dims)
    }
    pub fn reduce_low_f32(
        &mut self,
        value: CudaValue,
        op: ReduceOp,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.reduce_low_f32(value, op, axes, keep_dims)
    }
    pub fn reduce_low(
        &mut self,
        value: CudaValue,
        op: ReduceOp,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.reduce_low(value, op, axes, keep_dims)
    }
    pub fn mean_low_f32(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.mean_low_f32(value, axes, keep_dims)
    }
    pub fn mean_low(
        &mut self,
        value: CudaValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaValue, CudaError> {
        self.plan.mean_low(value, axes, keep_dims)
    }
    pub fn matmul_low_f32(
        &mut self,
        left: CudaValue,
        right: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        self.plan.matmul_low_f32(left, right)
    }
    pub fn matmul_low(
        &mut self,
        left: CudaValue,
        right: CudaValue,
    ) -> Result<CudaValue, CudaError> {
        self.plan.matmul_low(left, right)
    }
}
