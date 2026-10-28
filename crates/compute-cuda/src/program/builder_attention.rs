use super::*;
use tensor_core::{AttentionMask, AttentionOptions, AttentionPlan};
impl CudaProgramPlanBuilder {
    pub fn attention(
        &mut self,
        query: CudaValue,
        key: CudaValue,
        value: CudaValue,
        mask: AttentionMask<'_, CudaValue, CudaValue>,
        options: AttentionOptions,
    ) -> Result<CudaValue, CudaError> {
        let query = self.require(query, CudaDtype::F32)?;
        let key = self.require(key, CudaDtype::F32)?;
        let value = self.require(value, CudaDtype::F32)?;
        self.attention_values(query, key, value, mask, options)
    }
    pub fn attention_low_f32(
        &mut self,
        query: CudaValue,
        key: CudaValue,
        value: CudaValue,
        mask: AttentionMask<'_, CudaValue, CudaValue>,
        options: AttentionOptions,
    ) -> Result<CudaValue, CudaError> {
        let (q, k) = self.low_pair(query, key)?;
        let (_, v) = self.low_pair(query, value)?;
        self.attention_values(q, k, v, mask, options)
    }
    pub fn attention_low(
        &mut self,
        query: CudaValue,
        key: CudaValue,
        value: CudaValue,
        mask: AttentionMask<'_, CudaValue, CudaValue>,
        options: AttentionOptions,
    ) -> Result<CudaValue, CudaError> {
        let dtype = self.low(query)?.dtype.low_dtype().unwrap();
        self.transaction(|this| {
            let result = this.attention_low_f32(query, key, value, mask, options)?;
            this.cast_to_low(result, dtype)
        })
    }
    fn attention_values(
        &mut self,
        query: PlannedValue,
        key: PlannedValue,
        value: PlannedValue,
        mask: AttentionMask<'_, CudaValue, CudaValue>,
        options: AttentionOptions,
    ) -> Result<CudaValue, CudaError> {
        let mask = match mask {
            AttentionMask::None => None,
            AttentionMask::Keep(mask) => Some((self.require(*mask, CudaDtype::U32)?, true)),
            AttentionMask::Additive(mask) => Some((self.require(*mask, CudaDtype::F32)?, false)),
        };
        let plan = AttentionPlan::new(
            query.layout.shape(),
            key.layout.shape(),
            value.layout.shape(),
            mask.as_ref().map(|(m, _)| m.layout.shape()),
            options,
        )?;
        // Scores are conceptual; a broadcast mask uses its existing storage.
        // Never impose a dense score allocation byte bound on this path.
        self.output(plan.output.clone(), CudaDtype::F32, |output| {
            Step::Attention {
                query,
                key,
                value,
                mask,
                output,
                plan: Box::new(plan),
            }
        })
    }
}
