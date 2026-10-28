use crate::{CudaError, CudaLowTensor, CudaRuntime, CudaTensor};
use tensor_core::{AttentionMask, AttentionOptions, TensorLowAttentionBackend, low_attention_plan};

impl TensorLowAttentionBackend for CudaRuntime {
    fn attention_low_f32(
        &self,
        query: &CudaLowTensor,
        key: &CudaLowTensor,
        value: &CudaLowTensor,
        mask: AttentionMask<'_, CudaTensor, CudaTensor<u32>>,
        options: AttentionOptions,
    ) -> Result<CudaTensor, CudaError> {
        self.check(&query.tensor)?;
        self.check(&key.tensor)?;
        self.check(&value.tensor)?;
        // Shared dtype/geometry validation precedes allocation and every empty
        // shortcut. Masks retain f32/u32 storage and are checked by the loader.
        let plan = low_attention_plan(query, key, value, mask.shape(), options)?;
        self.attention_loaded(
            &query.tensor,
            &key.tensor,
            &value.tensor,
            mask,
            plan,
            Some(query.dtype as u32),
        )
    }
}
