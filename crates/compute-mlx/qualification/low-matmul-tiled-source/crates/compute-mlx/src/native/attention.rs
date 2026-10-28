use super::*;
use tensor_core::{
    AttentionMask, AttentionOptions, AttentionPlan, ReduceOp, TensorAttentionBackend,
    TensorStatsBackend,
};

impl MlxBackend {
    pub(super) fn check_attention_mask(
        &self,
        mask: &AttentionMask<'_, MlxTensor, MlxTensor>,
    ) -> Result<(), MlxError> {
        match mask {
            AttentionMask::None => Ok(()),
            AttentionMask::Keep(mask) => self.check(mask, Some(MlxDtype::U32)),
            AttentionMask::Additive(mask) => self.check(mask, Some(MlxDtype::F32)),
        }
    }

    fn attention_scalar(&self, value: f32) -> Result<MlxTensor, MlxError> {
        self.upload_f32(Shape::new(vec![])?, &[value])
    }

    fn attention_indices(&self, count: usize) -> Result<MlxTensor, MlxError> {
        self.output(
            "attention indices",
            Shape::new(vec![count])?,
            MlxDtype::U32,
            |a, out| unsafe {
                (a.arange)(out, 0., count as f64, 1., ffi::U32, self.context.stream)
            },
        )
    }

    fn attention_causal_mask(
        &self,
        plan: &AttentionPlan,
        offset: i32,
    ) -> Result<MlxTensor, MlxError> {
        let q = self.attention_indices(plan.queries)?;
        let k = self.attention_indices(plan.keys)?;
        let q = self.reshape(&q, Shape::new(vec![plan.queries, 1])?)?;
        let k = self.reshape(&k, Shape::new(vec![1, plan.keys])?)?;
        // Dimensions are bounded by i32::MAX; adding an i32 offset's unsigned
        // magnitude fits u32, including i32::MIN, without signed arithmetic.
        let magnitude = self.upload_u32(Shape::new(vec![])?, &[offset.unsigned_abs()])?;
        if offset >= 0 {
            let end = self.binary_u32(self.context.api.add, &q, &magnitude)?;
            self.compare_values(CompareOp::LessEqual, &k, &end)
        } else {
            let begin = self.binary_u32(self.context.api.add, &k, &magnitude)?;
            self.compare_values(CompareOp::LessEqual, &begin, &q)
        }
    }

    fn attention_operand(
        &self,
        input: &MlxTensor,
        promoted: &Shape,
        batch: &Shape,
    ) -> Result<MlxTensor, MlxError> {
        let promoted_input = self.reshape(input, promoted.clone())?;
        let tail = &promoted.dims()[promoted.rank() - 3..];
        let mut expanded = batch.dims().to_vec();
        expanded.extend_from_slice(tail);
        let expanded = self.broadcast_to(&promoted_input, Shape::new(expanded)?)?;
        let mut flat = vec![batch.numel()];
        flat.extend_from_slice(tail);
        self.reshape(&expanded, Shape::new(flat)?)
    }

    // Explicit GPU graph for scales whose pre-multiplication into Q could
    // overflow, and shapes beyond fused kernels' signed indexing range.
    // Grouped heads broadcast along a separate group axis without tiling K/V.
    fn attention_graph(
        &self,
        query: &MlxTensor,
        key: &MlxTensor,
        value: &MlxTensor,
        mask: Option<&MlxTensor>,
        valid: Option<&MlxTensor>,
        plan: &AttentionPlan,
    ) -> Result<MlxTensor, MlxError> {
        let b = plan.batch.numel();
        let q = self.reshape(
            query,
            Shape::new(vec![
                b,
                plan.key_heads,
                plan.group_size,
                plan.queries,
                plan.depth,
            ])?,
        )?;
        let k = self.reshape(
            key,
            Shape::new(vec![b, plan.key_heads, 1, plan.keys, plan.depth])?,
        )?;
        let k = self.permute(&k, &[0, 1, 2, 4, 3])?;
        let scores = self.matmul(&q, &k, MatmulPrecision::F32)?;
        let scores = self.reshape(
            &scores,
            Shape::new(vec![b, plan.query_heads, plan.queries, plan.keys])?,
        )?;
        let mut scores = self.binary(
            BinaryOp::Multiply,
            &scores,
            &self.attention_scalar(plan.scale)?,
        )?;
        if let Some(mask) = mask {
            scores = self.binary(BinaryOp::Add, &scores, mask)?;
        }
        // A closed row must not feed all -Inf to softmax. Its final result is
        // still replaced by zero; open rows retain their exact additive bias.
        if let Some(valid) = valid {
            scores = self.select_values(valid, &scores, &self.attention_scalar(0.)?)?;
        }
        let weights = self.softmax(&scores, &[3])?;
        let weights = self.reshape(
            &weights,
            Shape::new(vec![
                b,
                plan.key_heads,
                plan.group_size,
                plan.queries,
                plan.keys,
            ])?,
        )?;
        let value = self.reshape(
            value,
            Shape::new(vec![b, plan.key_heads, 1, plan.keys, plan.value_depth])?,
        )?;
        let output = self.matmul(&weights, &value, MatmulPrecision::F32)?;
        self.reshape(
            &output,
            Shape::new(vec![b, plan.query_heads, plan.queries, plan.value_depth])?,
        )
    }
}

impl TensorAttentionBackend for MlxBackend {
    fn attention(
        &self,
        query: &MlxTensor,
        key: &MlxTensor,
        value: &MlxTensor,
        mask: AttentionMask<'_, MlxTensor, MlxTensor>,
        options: AttentionOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check(query, Some(MlxDtype::F32))?;
        self.check(key, Some(MlxDtype::F32))?;
        self.check(value, Some(MlxDtype::F32))?;
        self.check_attention_mask(&mask)?;
        let plan = AttentionPlan::new(
            &query.shape,
            &key.shape,
            &value.shape,
            mask.shape(),
            options,
        )?;
        Self::dimensions(&plan.output)?;
        if plan.output.is_empty() || plan.keys == 0 {
            return self.zeros(plan.output, MlxDtype::F32);
        }
        let query = self.attention_operand(query, &plan.query, &plan.batch)?;
        let key = self.attention_operand(key, &plan.key, &plan.batch)?;
        let value = self.attention_operand(value, &plan.value, &plan.batch)?;
        // Native fused kernels sum unnormalized exp weights times V before
        // dividing by their sum. Bound that numerator by scaling every V by
        // 1/2^ceil(log2(Lk)); the factor depends only on shape, not masked data.
        // MLX dimensions fit i32, so the shift is at most 31. Tiny intermediates
        // retain the shared f32 underflow limits; no global-maximum-dependent
        // scaling can suppress a small live value because another key is masked.
        let value_shift = u32::BITS - (plan.keys as u32 - 1).leading_zeros();
        let value_factor = f32::from_bits((127 + value_shift) << 23);
        let inverse_factor = f32::from_bits((127 - value_shift) << 23);
        let value = if value_shift == 0 {
            value
        } else {
            self.binary(
                BinaryOp::Multiply,
                &value,
                &self.attention_scalar(inverse_factor)?,
            )?
        };
        let fused_safe = plan.scale.abs() <= 1.
            && [&query, &key, &value]
                .iter()
                .all(|t| t.shape.numel() <= i32::MAX as usize);
        let native_causal = fused_safe
            && matches!(mask, AttentionMask::None)
            && plan
                .causal
                .is_some_and(|offset| i64::from(offset) == plan.keys as i64 - plan.queries as i64);
        let zero = self.attention_scalar(0.)?;
        let excluded = self.attention_scalar(f32::NEG_INFINITY)?;
        let mut additive = match mask {
            AttentionMask::None => None,
            // Native bool-masked fully closed rows differ across MLX kernels.
            // Additive -Inf has consistent exclusion for partially open rows.
            AttentionMask::Keep(mask) => Some(self.select_values(mask, &zero, &excluded)?),
            AttentionMask::Additive(mask) => Some(mask.clone()),
        };
        if let Some(offset) = plan.causal.filter(|_| !native_causal) {
            let keep = self.attention_causal_mask(&plan, offset)?;
            additive =
                Some(self.select_values(&keep, additive.as_ref().unwrap_or(&zero), &excluded)?);
        }
        let mut valid = None;
        let additive = if let Some(additive) = additive {
            let additive = self.broadcast_to(&additive, plan.scores.clone())?;
            let additive = self.reshape(
                &additive,
                Shape::new(vec![
                    plan.batch.numel(),
                    plan.query_heads,
                    plan.queries,
                    plan.keys,
                ])?,
            )?;
            let allowed = self.compare(CompareOp::Greater, &additive, &excluded)?;
            valid = Some(self.reduce_values(ReduceOp::Max, &allowed, &[3], true)?);
            Some(additive)
        } else {
            if native_causal && plan.queries > plan.keys {
                // Native lower-right causal may have closed leading rows.
                // Only O(Lq) metadata is needed; no score-sized mask is built.
                let rows = self.attention_indices(plan.queries)?;
                let threshold =
                    self.upload_u32(Shape::new(vec![])?, &[(plan.queries - plan.keys) as u32])?;
                let allowed = self.compare_values(CompareOp::GreaterEqual, &rows, &threshold)?;
                valid = Some(self.reshape(&allowed, Shape::new(vec![1, 1, plan.queries, 1])?)?);
            }
            None
        };
        let output_shape = Shape::new(vec![
            plan.batch.numel(),
            plan.query_heads,
            plan.queries,
            plan.value_depth,
        ])?;
        let fused_safe = fused_safe
            && additive
                .as_ref()
                .is_none_or(|m| m.shape.numel() <= i32::MAX as usize);
        let mut output = if fused_safe {
            let mode = if native_causal {
                c"causal"
            } else if additive.is_some() {
                c"array"
            } else {
                c""
            };
            let empty = Handle {
                ctx: std::ptr::null_mut(),
            };
            self.output(
                "scaled dot product attention",
                output_shape,
                MlxDtype::F32,
                |a, out| unsafe {
                    (a.fast_scaled_dot_product_attention)(
                        out,
                        query.array.raw,
                        key.array.raw,
                        value.array.raw,
                        plan.scale,
                        mode.as_ptr(),
                        additive.as_ref().map_or(empty, |m| m.array.raw),
                        empty,
                        self.context.stream,
                    )
                },
            )?
        } else {
            self.attention_graph(
                &query,
                &key,
                &value,
                additive.as_ref(),
                valid.as_ref(),
                &plan,
            )?
        };
        if value_shift != 0 {
            // Attention is a convex combination, so the exact result cannot
            // exceed finite V's range. Clamp rounding excursions before the
            // power-of-two restore instead of creating spurious infinity.
            let bound = f32::MAX * inverse_factor;
            output = self.binary(BinaryOp::Max, &output, &self.attention_scalar(-bound)?)?;
            output = self.binary(BinaryOp::Min, &output, &self.attention_scalar(bound)?)?;
            output = self.binary(
                BinaryOp::Multiply,
                &output,
                &self.attention_scalar(value_factor)?,
            )?;
        }
        if let Some(valid) = valid {
            output = self.select_values(&valid, &output, &zero)?;
        }
        self.reshape(&output, plan.output)
    }
}
