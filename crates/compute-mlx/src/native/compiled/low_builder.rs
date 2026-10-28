use super::*;
use crate::native::lowering::{casts, ops};
use tensor_core::{LowDtype, ReduceOp};

impl MlxProgramBuilder {
    pub fn cast_to_low(&mut self, value: MlxValue, dtype: LowDtype) -> Result<MlxValue, MlxError> {
        self.require(value, MlxDtype::F32)?;
        self.backend.low_accessor(dtype)?;
        self.transaction(|graph| casts::cast_to_low(graph, value, dtype))
    }
    pub fn cast_to_f32(&mut self, value: MlxValue) -> Result<MlxValue, MlxError> {
        self.low(value)?;
        self.transaction(|graph| casts::cast_to_f32(graph, value))
    }
    pub fn unary_low(&mut self, value: MlxValue, op: UnaryOp) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| ops::unary_low(graph, value, op))
    }
    pub fn binary_low(
        &mut self,
        left: MlxValue,
        right: MlxValue,
        op: BinaryOp,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| ops::binary_low(graph, left, right, op))
    }
    pub fn reduce_low_f32(
        &mut self,
        value: MlxValue,
        op: ReduceOp,
        axes: &[usize],
        keep: bool,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| ops::reduce_low(graph, value, op, axes, keep, false))
    }
    pub fn mean_low_f32(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep: bool,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| ops::reduce_low(graph, value, ReduceOp::Sum, axes, keep, true))
    }
    pub fn reduce_low(
        &mut self,
        value: MlxValue,
        op: ReduceOp,
        axes: &[usize],
        keep: bool,
    ) -> Result<MlxValue, MlxError> {
        let (_, dtype) = self.low(value)?;
        self.transaction(|graph| {
            let out = ops::reduce_low(graph, value, op, axes, keep, false)?;
            casts::cast_to_low(graph, out, dtype)
        })
    }
    pub fn mean_low(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep: bool,
    ) -> Result<MlxValue, MlxError> {
        let (_, dtype) = self.low(value)?;
        self.transaction(|graph| {
            let out = ops::reduce_low(graph, value, ReduceOp::Sum, axes, keep, true)?;
            casts::cast_to_low(graph, out, dtype)
        })
    }
    pub fn matmul_low_f32(
        &mut self,
        left: MlxValue,
        right: MlxValue,
    ) -> Result<MlxValue, MlxError> {
        let (a, dtype) = self.low(left)?;
        let (b, other) = self.low(right)?;
        if dtype != other {
            return Err(TensorError::LowDtypeMismatch {
                left: dtype,
                right: other,
            }
            .into());
        }
        let plan = tensor_core::MatmulPlan::new(&a.shape, &b.shape)?;
        if !tensor_core::TensorLowBackend::low_precision_support(&self.backend, dtype).matmul_f32 {
            return Err(MlxError::UnsupportedLowPrecision {
                dtype,
                operation: "direct low-input matmul with f32 output",
            });
        }
        self.transaction(|graph| ops::matmul_low_f32(graph, left, right, plan))
    }
    /// Compiled low output rounds the shared direct f32 result once. The eager
    /// native same-low matmul method retains its existing separate route.
    pub fn matmul_low(&mut self, left: MlxValue, right: MlxValue) -> Result<MlxValue, MlxError> {
        let (_, dtype) = self.low(left)?;
        self.transaction(|graph| {
            let out = graph.matmul_low_f32(left, right)?;
            casts::cast_to_low(graph, out, dtype)
        })
    }
}
