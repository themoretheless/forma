use crate::{HasShape, Shape, TensorError, TensorIndexBackend};

/// Scalar policy for scaled dot-product attention. No dropout is applied.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AttentionOptions {
    /// Defaults to 1/sqrt(query depth). Any finite explicit scale is valid,
    /// including zero and negative values.
    pub scale: Option<f32>,
    /// If present, key j is visible to query i only when j <= i + offset.
    /// Zero gives upper-left causal masking. For decoding, Lk-Lq gives
    /// lower-right alignment. Negative offsets can fully mask early rows.
    pub causal: Option<i32>,
}

/// Optional device-resident mask, broadcasting to [..., query heads, Lq, Lk].
/// Keep accepts any nonzero u32. Additive masks contain finite biases or -Inf;
/// -Inf excludes a key. NaN and +Inf masks are outside the shared contract.
#[derive(Clone, Copy, Debug)]
pub enum AttentionMask<'a, F, U> {
    None,
    Keep(&'a U),
    Additive(&'a F),
}
impl<F: HasShape, U: HasShape> AttentionMask<'_, F, U> {
    pub fn shape(&self) -> Option<&Shape> {
        match self {
            Self::None => None,
            Self::Keep(mask) => Some(mask.shape()),
            Self::Additive(mask) => Some(mask.shape()),
        }
    }
}

/// Device-resident scaled dot-product attention: softmax(scale*Q*K^T+mask)*V.
/// Inputs have shape [..., heads, sequence, depth], or [sequence, depth] for
/// one implicit head. Leading batch axes broadcast. Query heads must be a
/// multiple of the matching key/value head count; consecutive query-head
/// groups share one key/value head. If all three inputs have rank two, output
/// has rank two; otherwise it retains the head axis and broadcast batches.
///
/// Fully masked rows and zero-length key sequences return zeros. Zero query
/// lengths, value depths or batch dimensions return empty outputs. Head counts
/// and query/key depth must be positive, even for empty outputs. Inputs are
/// finite f32; dot products, scaled/biased allowed logits and normalized
/// weighted value sums must remain within f32 range. Constant finite values,
/// including f32::MAX, must not overflow an internal unnormalized numerator.
/// Reduction order, rounding and
/// underflow follow backend f32 limits. No tensor data is read back to the CPU.
pub trait TensorAttentionBackend: TensorIndexBackend {
    fn attention(
        &self,
        query: &Self::Tensor,
        key: &Self::Tensor,
        value: &Self::Tensor,
        mask: AttentionMask<'_, Self::Tensor, Self::UIntTensor>,
        options: AttentionOptions,
    ) -> Result<Self::Tensor, Self::Error>;
}

/// Checked logical attention geometry. Canonical shapes retain a head axis;
/// input batch dimensions are not expanded or materialized by this plan.
#[derive(Clone, Debug, PartialEq)]
pub struct AttentionPlan {
    pub query: Shape,
    pub key: Shape,
    pub value: Shape,
    pub batch: Shape,
    pub scores: Shape,
    pub canonical_output: Shape,
    pub output: Shape,
    pub query_heads: usize,
    pub key_heads: usize,
    pub queries: usize,
    pub keys: usize,
    pub depth: usize,
    pub value_depth: usize,
    pub group_size: usize,
    pub scale: f32,
    pub causal: Option<i32>,
}
impl AttentionPlan {
    pub fn new(
        query: &Shape,
        key: &Shape,
        value: &Shape,
        mask: Option<&Shape>,
        options: AttentionOptions,
    ) -> Result<Self, TensorError> {
        let matrices = query.rank() == 2 && key.rank() == 2 && value.rank() == 2;
        let query = promote(query)?;
        let key = promote(key)?;
        let value = promote(value)?;
        let q = query.dims();
        let k = key.dims();
        let v = value.dims();
        let (query_heads, queries, depth) = (q[q.len() - 3], q[q.len() - 2], q[q.len() - 1]);
        let (key_heads, keys, key_depth) = (k[k.len() - 3], k[k.len() - 2], k[k.len() - 1]);
        let (value_heads, value_keys, value_depth) =
            (v[v.len() - 3], v[v.len() - 2], v[v.len() - 1]);
        if depth == 0 || depth != key_depth {
            return Err(TensorError::InvalidAttention(
                "query/key depths must be equal and positive",
            ));
        }
        if query_heads == 0 || key_heads == 0 || value_heads != key_heads {
            return Err(TensorError::InvalidAttention(
                "head counts must be positive and key/value heads must match",
            ));
        }
        if !query_heads.is_multiple_of(key_heads) {
            return Err(TensorError::InvalidAttention(
                "query head count must be a multiple of key/value head count",
            ));
        }
        if keys != value_keys {
            return Err(TensorError::InvalidAttention(
                "key and value sequence lengths must match",
            ));
        }
        let batch = Shape::broadcast_all(&[
            &Shape::new(q[..q.len() - 3].to_vec())?,
            &Shape::new(k[..k.len() - 3].to_vec())?,
            &Shape::new(v[..v.len() - 3].to_vec())?,
        ])?;
        let mut dims = batch.dims().to_vec();
        dims.extend([query_heads, queries, keys]);
        let scores = Shape::new(dims)?;
        if let Some(mask) = mask
            && mask.broadcast(&scores)? != scores
        {
            return Err(TensorError::IncompatibleBroadcast {
                left: mask.dims().to_vec(),
                right: scores.dims().to_vec(),
            });
        }
        let mut dims = batch.dims().to_vec();
        dims.extend([query_heads, queries, value_depth]);
        let canonical_output = Shape::new(dims)?;
        let output = if matrices {
            Shape::new(vec![queries, value_depth])?
        } else {
            canonical_output.clone()
        };
        let scale = options
            .scale
            .unwrap_or_else(|| (depth as f32).sqrt().recip());
        if !scale.is_finite() {
            return Err(TensorError::InvalidAttentionScale);
        }
        Ok(Self {
            query,
            key,
            value,
            batch,
            scores,
            canonical_output,
            output,
            query_heads,
            key_heads,
            queries,
            keys,
            depth,
            value_depth,
            group_size: query_heads / key_heads,
            scale,
            causal: options.causal,
        })
    }
}

fn promote(shape: &Shape) -> Result<Shape, TensorError> {
    match shape.rank() {
        0 | 1 => Err(TensorError::InvalidAttention(
            "operands need rank at least two",
        )),
        2 => Shape::new(vec![1, shape.dims()[0], shape.dims()[1]]),
        _ => Ok(shape.clone()),
    }
}
