use super::lowering::{NativeLowerer, scatter};
use super::*;
use tensor_core::{ScatterOp, Scattered, TensorScatterBackend};

impl TensorScatterBackend for MlxBackend {
    fn scatter_f32(
        &self,
        op: ScatterOp,
        input: &MlxTensor,
        indices: &MlxTensor,
        updates: &MlxTensor,
        axis: usize,
    ) -> Result<Scattered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        scatter::scatter(
            &mut NativeLowerer::new(self),
            op,
            input.clone(),
            indices.clone(),
            updates.clone(),
            axis,
        )
    }
    fn scatter_u32(
        &self,
        op: ScatterOp,
        input: &MlxTensor,
        indices: &MlxTensor,
        updates: &MlxTensor,
        axis: usize,
    ) -> Result<Scattered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        scatter::scatter(
            &mut NativeLowerer::new(self),
            op,
            input.clone(),
            indices.clone(),
            updates.clone(),
            axis,
        )
    }
}
