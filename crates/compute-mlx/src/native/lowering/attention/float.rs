use super::*;

fn matmul<L: Lowering>(g: &mut L, left: L::Value, right: L::Value) -> Result<L::Value, MlxError> {
    let shape = tensor_core::matmul_shape(&g.spec(&left)?.shape, &g.spec(&right)?.shape)?;
    g.native(
        NativeOp::Matmul,
        &[left, right],
        TensorSpec {
            shape,
            dtype: MlxDtype::F32,
        },
    )
}

// Resident composition preserves post-dot scaling when pre-scaling Q could
// overflow. A separate group axis broadcasts K/V without explicit head copies.
fn composition<L: Lowering>(
    g: &mut L,
    inputs: [L::Value; 3],
    mask: Option<L::Value>,
    valid: Option<L::Value>,
    p: &AttentionPlan,
) -> Result<L::Value, MlxError> {
    let [q, k, v] = inputs;
    let b = p.batch.numel();
    let q = reshape(
        g,
        q,
        Shape::new(vec![b, p.key_heads, p.group_size, p.queries, p.depth])?,
    )?;
    let k = reshape(g, k, Shape::new(vec![b, p.key_heads, 1, p.keys, p.depth])?)?;
    let k = shape_op(
        g,
        k,
        NativeOp::Permute(vec![0, 1, 2, 4, 3]),
        Shape::new(vec![b, p.key_heads, 1, p.depth, p.keys])?,
    )?;
    let scores = matmul(g, q, k)?;
    let scores = reshape(
        g,
        scores,
        Shape::new(vec![b, p.query_heads, p.queries, p.keys])?,
    )?;
    let scale = scalar(g, p.scale)?;
    let mut scores = binary(g, scores, scale, BinaryOp::Multiply)?;
    if let Some(mask) = mask {
        scores = binary(g, scores, mask, BinaryOp::Add)?;
    }
    if let Some(valid) = valid {
        let zero = scalar(g, 0.)?;
        scores = select(g, valid, scores, zero)?;
    }
    let weights = g.native(
        NativeOp::Softmax(vec![3]),
        &[scores.clone()],
        g.spec(&scores)?,
    )?;
    let weights = reshape(
        g,
        weights,
        Shape::new(vec![b, p.key_heads, p.group_size, p.queries, p.keys])?,
    )?;
    let v = reshape(
        g,
        v,
        Shape::new(vec![b, p.key_heads, 1, p.keys, p.value_depth])?,
    )?;
    let output = matmul(g, weights, v)?;
    reshape(
        g,
        output,
        Shape::new(vec![b, p.query_heads, p.queries, p.value_depth])?,
    )
}

pub(in crate::native) fn attention<L: Lowering>(
    g: &mut L,
    query: L::Value,
    key: L::Value,
    value: L::Value,
    mask: AttentionMask<'_, L::Value, L::Value>,
    options: AttentionOptions,
) -> Result<L::Value, MlxError> {
    let p = plan(g, [&query, &key, &value], &mask, options, false)?;
    if p.output.is_empty() || p.keys == 0 {
        return zeros(g, p.output);
    }
    let query = operand(g, query, &p.query, &p.batch, true)?;
    let key = operand(g, key, &p.key, &p.batch, true)?;
    let mut value = operand(g, value, &p.value, &p.batch, true)?;
    // The numerator in native fused attention is unnormalized. A shape-only
    // power-of-two scale bounds it without suppressing masked small live V.
    let shift = u32::BITS - (p.keys as u32 - 1).leading_zeros();
    let factor = f32::from_bits((127 + shift) << 23);
    let inverse = f32::from_bits((127 - shift) << 23);
    if shift != 0 {
        let scalar = scalar(g, inverse)?;
        value = binary(g, value, scalar, BinaryOp::Multiply)?;
    }
    let fused_safe = p.scale.abs() <= 1.
        && [&query, &key, &value]
            .iter()
            .map(|v| g.spec(v))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .all(|s| s.shape.numel() <= i32::MAX as usize);
    let native_causal = fused_safe
        && matches!(mask, AttentionMask::None)
        && p.causal
            .is_some_and(|offset| i64::from(offset) == p.keys as i64 - p.queries as i64);
    let zero = scalar(g, 0.)?;
    let excluded = scalar(g, f32::NEG_INFINITY)?;
    let mut additive = match mask {
        AttentionMask::None => None,
        AttentionMask::Keep(mask) => Some(select(g, mask.clone(), zero.clone(), excluded.clone())?),
        AttentionMask::Additive(mask) => Some(mask.clone()),
    };
    if let Some(offset) = p.causal.filter(|_| !native_causal) {
        let keep = causal_mask(g, &p, offset)?;
        additive = Some(select(
            g,
            keep,
            additive.unwrap_or_else(|| zero.clone()),
            excluded.clone(),
        )?);
    }
    let mut valid = None;
    let additive = if let Some(additive) = additive {
        let additive = broadcast(g, additive, p.scores.clone())?;
        let additive = reshape(
            g,
            additive,
            Shape::new(vec![p.batch.numel(), p.query_heads, p.queries, p.keys])?,
        )?;
        let allowed = compare(g, additive.clone(), excluded, CompareOp::Greater)?;
        let shape = g.spec(&allowed)?.shape.reduce(&[3], true)?;
        valid = Some(g.native(
            NativeOp::Reduce(ReduceOp::Max, vec![3], true),
            &[allowed],
            TensorSpec {
                shape,
                dtype: MlxDtype::U32,
            },
        )?);
        Some(additive)
    } else {
        if native_causal && p.queries > p.keys {
            let rows = indices(g, p.queries)?;
            let threshold = g.constant_u32(Shape::new(vec![])?, &[(p.queries - p.keys) as u32])?;
            let allowed = compare(g, rows, threshold, CompareOp::GreaterEqual)?;
            valid = Some(reshape(g, allowed, Shape::new(vec![1, 1, p.queries, 1])?)?);
        }
        None
    };
    let output_shape = Shape::new(vec![
        p.batch.numel(),
        p.query_heads,
        p.queries,
        p.value_depth,
    ])?;
    let fused_safe = fused_safe
        && additive
            .as_ref()
            .map(|m| g.spec(m))
            .transpose()?
            .is_none_or(|m| m.shape.numel() <= i32::MAX as usize);
    let mut output = if fused_safe {
        let mut inputs = vec![query, key, value];
        if let Some(mask) = additive.as_ref() {
            inputs.push(mask.clone());
        }
        g.native(
            NativeOp::FastAttention {
                scale: p.scale,
                causal: native_causal,
                masked: additive.is_some(),
            },
            &inputs,
            TensorSpec {
                shape: output_shape,
                dtype: MlxDtype::F32,
            },
        )?
    } else {
        composition(g, [query, key, value], additive, valid.clone(), &p)?
    };
    if shift != 0 {
        // A convex combination of finite V cannot exceed finite V's range.
        // Clamp rounding excursions before restoring the power-of-two factor.
        let bound = f32::MAX * inverse;
        let lower = scalar(g, -bound)?;
        output = binary(g, output, lower, BinaryOp::Max)?;
        let upper = scalar(g, bound)?;
        output = binary(g, output, upper, BinaryOp::Min)?;
        let factor = scalar(g, factor)?;
        output = binary(g, output, factor, BinaryOp::Multiply)?;
    }
    if let Some(valid) = valid {
        output = select(g, valid, output, zero)?;
    }
    reshape(g, output, p.output)
}
