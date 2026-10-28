use super::*;
use crate::native::lowering::{attention, casts};
use tensor_core::{AttentionMask, AttentionOptions};

impl MlxProgramBuilder {
    /// Record f32 attention with resident masks and the shared eager contract.
    pub fn attention(
        &mut self,
        query: MlxValue,
        key: MlxValue,
        value: MlxValue,
        mask: AttentionMask<'_, MlxValue, MlxValue>,
        options: AttentionOptions,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|g| attention::attention(g, query, key, value, mask, options))
    }
    /// Direct native low inputs, f32 accumulation/output, and no complete Q/K/V
    /// expansion or score matrix. Uses the same custom kernel as eager calls.
    pub fn attention_low_f32(
        &mut self,
        query: MlxValue,
        key: MlxValue,
        value: MlxValue,
        mask: AttentionMask<'_, MlxValue, MlxValue>,
        options: AttentionOptions,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|g| attention::attention_low_f32(g, query, key, value, mask, options))
    }
    /// Round the complete f32 attention result into Q's low dtype once.
    pub fn attention_low(
        &mut self,
        query: MlxValue,
        key: MlxValue,
        value: MlxValue,
        mask: AttentionMask<'_, MlxValue, MlxValue>,
        options: AttentionOptions,
    ) -> Result<MlxValue, MlxError> {
        let (_, dtype) = self.low(query)?;
        self.transaction(|g| {
            let out = attention::attention_low_f32(g, query, key, value, mask, options)?;
            casts::cast_to_low(g, out, dtype)
        })
    }
}
