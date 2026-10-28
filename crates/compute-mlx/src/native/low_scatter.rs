use super::lowering::{NativeLowerer, scatter};
use super::*;
use tensor_core::{ScatterOp, Scattered, TensorLowScatterBackend};

impl MlxBackend {
    fn scatter_low_values(
        &self,
        op: ScatterOp,
        input: &MlxLowTensor,
        indices: &MlxTensor,
        updates: &MlxLowTensor,
        axis: usize,
        low_output: bool,
    ) -> Result<Scattered<MlxTensor, MlxTensor>, MlxError> {
        self.check_low(input)?;
        self.check_low(updates)?;
        scatter::scatter_low(
            &mut NativeLowerer::new(self),
            op,
            input.tensor.clone(),
            indices.clone(),
            updates.tensor.clone(),
            axis,
            low_output,
        )
    }
}
impl TensorLowScatterBackend for MlxBackend {
    fn scatter_low(
        &self,
        op: ScatterOp,
        input: &MlxLowTensor,
        indices: &MlxTensor,
        updates: &MlxLowTensor,
        axis: usize,
    ) -> Result<Scattered<MlxLowTensor, MlxTensor>, MlxError> {
        let result = self.scatter_low_values(op, input, indices, updates, axis, true)?;
        Ok(Scattered {
            values: MlxLowTensor {
                tensor: result.values,
                dtype: input.dtype,
            },
            invalid_count: result.invalid_count,
        })
    }
    fn scatter_low_f32(
        &self,
        op: ScatterOp,
        input: &MlxLowTensor,
        indices: &MlxTensor,
        updates: &MlxLowTensor,
        axis: usize,
    ) -> Result<Scattered<MlxTensor, MlxTensor>, MlxError> {
        self.scatter_low_values(op, input, indices, updates, axis, false)
    }
}
