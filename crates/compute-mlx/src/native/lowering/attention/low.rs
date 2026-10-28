use super::*;
use crate::native::low_kernels;

const HEADER: &str = concat!(
    include_str!("../../../metal/low_common.h"),
    "\n",
    include_str!("../../../metal/low_attention_common.h")
);
const SOURCE: &str = include_str!("../../../metal/low_attention.metal");
const WIDTH: usize = 64;

pub(in crate::native) fn attention_low_f32<L: Lowering>(
    g: &mut L,
    query: L::Value,
    key: L::Value,
    value: L::Value,
    mask: AttentionMask<'_, L::Value, L::Value>,
    options: AttentionOptions,
) -> Result<L::Value, MlxError> {
    let plan = plan(g, [&query, &key, &value], &mask, options, true)?;
    if plan.output.is_empty() || plan.keys == 0 {
        return zeros(g, plan.output);
    }
    let dtype = low_dtype(&g.spec(&query)?)?;
    let query = operand(g, query, &plan.query, &plan.batch, false)?;
    let key = operand(g, key, &plan.key, &plan.batch, false)?;
    let value = operand(g, value, &plan.value, &plan.batch, false)?;
    let (mode, mask) = match mask {
        AttentionMask::None => (0, g.constant_u32(Shape::new(vec![1])?, &[0])?),
        AttentionMask::Keep(m) => (1, broadcast(g, m.clone(), plan.scores.clone())?),
        AttentionMask::Additive(m) => (2, broadcast(g, m.clone(), plan.scores.clone())?),
    };
    let rows = plan.output.numel() / plan.value_depth;
    let tiles = plan.value_depth.div_ceil(WIDTH);
    let work = rows.checked_mul(tiles).ok_or(MlxError::TooLarge)?;
    let mut parameters = [
        plan.query_heads,
        plan.key_heads,
        plan.queries,
        plan.keys,
        plan.depth,
        plan.value_depth,
        plan.group_size,
        tiles,
    ]
    .into_iter()
    .map(|x| u32::try_from(x).map_err(|_| MlxError::TooLarge))
    .collect::<Result<Vec<_>, _>>()?;
    parameters.extend([
        rows as u32,
        (rows as u64 >> 32) as u32,
        plan.scale.to_bits(),
        u32::from(plan.causal.is_some()),
        plan.causal.unwrap_or(0) as u32,
    ]);
    let params = g.constant_u32(Shape::new(vec![parameters.len()])?, &parameters)?;
    let mut kernel = low_kernels::key(
        SOURCE.replace("MASK_MODE", &mode.to_string()),
        &["query", "key", "value", "mask", "params"],
    );
    kernel.header = HEADER;
    g.metal(
        kernel,
        &[query, key, value, mask, params],
        TensorSpec {
            shape: plan.output,
            dtype: MlxDtype::F32,
        },
        dtype == LowDtype::Bf16,
        (work.min(65535) * WIDTH, WIDTH),
    )
}
