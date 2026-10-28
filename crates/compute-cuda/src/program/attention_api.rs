use super::{CudaProgramBuilder, CudaValue};
use crate::CudaError;
use tensor_core::{AttentionMask, AttentionOptions};
macro_rules! attention {
    ($name:ident) => {
        pub fn $name(
            &mut self,
            query: CudaValue,
            key: CudaValue,
            value: CudaValue,
            mask: AttentionMask<'_, CudaValue, CudaValue>,
            options: AttentionOptions,
        ) -> Result<CudaValue, CudaError> {
            self.plan.$name(query, key, value, mask, options)
        }
    };
}
impl CudaProgramBuilder<'_> {
    attention!(attention);
    attention!(attention_low_f32);
    attention!(attention_low);
}
