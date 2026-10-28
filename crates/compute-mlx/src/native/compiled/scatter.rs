use super::*;
use crate::native::lowering::scatter;
use tensor_core::{ScatterOp, Scattered};

impl MlxProgramBuilder {
    pub fn scatter(
        &mut self,
        value: MlxValue,
        indices: MlxValue,
        updates: MlxValue,
        op: ScatterOp,
        axis: usize,
    ) -> Result<Scattered<MlxValue, MlxValue>, MlxError> {
        self.require(value, MlxDtype::F32)?;
        self.transaction(|g| scatter::scatter(g, op, value, indices, updates, axis))
    }
    pub fn scatter_u32(
        &mut self,
        value: MlxValue,
        indices: MlxValue,
        updates: MlxValue,
        op: ScatterOp,
        axis: usize,
    ) -> Result<Scattered<MlxValue, MlxValue>, MlxError> {
        self.require(value, MlxDtype::U32)?;
        self.transaction(|g| scatter::scatter(g, op, value, indices, updates, axis))
    }
    pub fn scatter_low(
        &mut self,
        value: MlxValue,
        indices: MlxValue,
        updates: MlxValue,
        op: ScatterOp,
        axis: usize,
    ) -> Result<Scattered<MlxValue, MlxValue>, MlxError> {
        self.transaction(|g| scatter::scatter_low(g, op, value, indices, updates, axis, true))
    }
    pub fn scatter_low_f32(
        &mut self,
        value: MlxValue,
        indices: MlxValue,
        updates: MlxValue,
        op: ScatterOp,
        axis: usize,
    ) -> Result<Scattered<MlxValue, MlxValue>, MlxError> {
        self.transaction(|g| scatter::scatter_low(g, op, value, indices, updates, axis, false))
    }
}
