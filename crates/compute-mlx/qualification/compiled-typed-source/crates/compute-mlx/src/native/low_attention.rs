use super::{low_kernels, *};
use tensor_core::{
    AttentionMask, AttentionOptions, LowDtype, TensorLowAttentionBackend, low_attention_plan,
};

const HEADER: &str = concat!(
    include_str!("../metal/low_common.h"),
    "\n",
    include_str!("../metal/low_attention_common.h")
);
const SOURCE: &str = include_str!("../metal/low_attention.metal");
const WIDTH: usize = 64;

impl MlxBackend {
    /// Expanding leading batches and an implicit rank-two head changes only
    /// native shape/stride metadata. No flattening or contiguous copy is needed.
    fn low_attention_view(
        &self,
        input: &MlxTensor,
        promoted: &Shape,
        batch: &Shape,
    ) -> Result<MlxTensor, MlxError> {
        let mut dims = batch.dims().to_vec();
        dims.extend_from_slice(&promoted.dims()[promoted.rank() - 3..]);
        self.broadcast_to(input, Shape::new(dims)?)
    }
}

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
        self.check_attention_mask(&mask)?;
        let plan = low_attention_plan(query, key, value, mask.shape(), options)?;
        Self::dimensions(&plan.output)?;
        if plan.output.is_empty() || plan.keys == 0 {
            return self.zeros(plan.output, MlxDtype::F32);
        }
        let dtype = query.dtype;
        let query = self.low_attention_view(&query.tensor, &plan.query, &plan.batch)?;
        let key = self.low_attention_view(&key.tensor, &plan.key, &plan.batch)?;
        let value = self.low_attention_view(&value.tensor, &plan.value, &plan.batch)?;
        let (mask_mode, mask) = match mask {
            AttentionMask::None => (0, self.upload_u32(Shape::new(vec![1])?, &[0])?),
            AttentionMask::Keep(mask) => (1, self.broadcast_to(mask, plan.scores.clone())?),
            AttentionMask::Additive(mask) => (2, self.broadcast_to(mask, plan.scores.clone())?),
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
        let params = self.upload_u32(Shape::new(vec![parameters.len()])?, &parameters)?;
        let mut kernel = low_kernels::key(
            SOURCE.replace("MASK_MODE", &mask_mode.to_string()),
            &["query", "key", "value", "mask", "params"],
        );
        kernel.header = HEADER;
        self.custom_metal(
            kernel,
            &[&query, &key, &value, &mask, &params],
            plan.output,
            MlxDtype::F32,
            dtype == LowDtype::Bf16,
            (work.min(65535) * WIDTH, WIDTH),
        )
    }
}
