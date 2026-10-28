use super::lowering::{NativeLowerer, attention};
use super::*;
use tensor_core::{AttentionMask, AttentionOptions, TensorAttentionBackend};

impl TensorAttentionBackend for MlxBackend {
    fn attention(
        &self,
        query: &MlxTensor,
        key: &MlxTensor,
        value: &MlxTensor,
        mask: AttentionMask<'_, MlxTensor, MlxTensor>,
        options: AttentionOptions,
    ) -> Result<MlxTensor, MlxError> {
        attention::attention(
            &mut NativeLowerer::new(self),
            query.clone(),
            key.clone(),
            value.clone(),
            mask,
            options,
        )
    }
}
