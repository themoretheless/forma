use super::lowering::{NativeLowerer, attention};
use super::*;
use tensor_core::{AttentionMask, AttentionOptions, TensorLowAttentionBackend};

impl TensorLowAttentionBackend for MlxBackend {
    fn attention_low_f32(
        &self,
        query: &MlxLowTensor,
        key: &MlxLowTensor,
        value: &MlxLowTensor,
        mask: AttentionMask<'_, MlxTensor, MlxTensor>,
        options: AttentionOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(query)?;
        self.check_low(key)?;
        self.check_low(value)?;
        attention::attention_low_f32(
            &mut NativeLowerer::new(self),
            query.tensor.clone(),
            key.tensor.clone(),
            value.tensor.clone(),
            mask,
            options,
        )
    }
}
