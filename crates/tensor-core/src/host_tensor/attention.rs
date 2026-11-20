//! Scaled dot-product attention (2D/3D/batched with GQA & causal mask).

use super::Tensor;
use crate::{AttentionOptions, AttentionPlan, TensorError};

fn fma_dot(q_row: &[f32], k_row: &[f32]) -> f64 {
    let chunks4 = q_row.len() / 4;
    let mut acc = [0.0_f64; 4];
    for c in 0..chunks4 {
        let base = c * 4;
        for j in 0..4 {
            acc[j] = (q_row[base + j] as f64).mul_add(k_row[base + j] as f64, acc[j]);
        }
    }
    let mut dot = (acc[0] + acc[1]) + (acc[2] + acc[3]);
    for d in (chunks4 * 4)..q_row.len() {
        dot = (q_row[d] as f64).mul_add(k_row[d] as f64, dot);
    }
    dot
}

fn broadcast_batch_strides(operand_dims: &[usize], batch_dims: &[usize]) -> Vec<usize> {
    let batch_rank = batch_dims.len();
    let mut strides = vec![0usize; batch_rank];
    if operand_dims.len() <= 3 || batch_rank == 0 {
        return strides;
    }
    let op_batch = &operand_dims[..operand_dims.len() - 3];
    let offset = batch_rank - op_batch.len();
    let mut running_stride = 1usize;
    for (axis, &dim) in op_batch.iter().enumerate().rev() {
        if dim != 1 {
            strides[offset + axis] = running_stride;
        }
        running_stride *= dim;
    }
    strides
}

fn batch_offsets(
    linear_batch: usize,
    batch_dims: &[usize],
    q_strides: &[usize],
    k_strides: &[usize],
    v_strides: &[usize],
) -> (usize, usize, usize) {
    let mut rem = linear_batch;
    let (mut qb, mut kb, mut vb) = (0usize, 0usize, 0usize);
    for axis in (0..batch_dims.len()).rev() {
        let dim = batch_dims[axis];
        let coord = rem % dim;
        rem /= dim;
        qb += coord * q_strides[axis];
        kb += coord * k_strides[axis];
        vb += coord * v_strides[axis];
    }
    (qb, kb, vb)
}

struct HeadSlice<'a> {
    k: &'a [f32],
    v: &'a [f32],
    plan: &'a AttentionPlan,
    causal_mask: bool,
}

fn attend_query_row(
    q_row: &[f32],
    head: &HeadSlice<'_>,
    out_row: &mut [f32],
    logits: &mut [f64],
    qi: usize,
) {
    let plan = head.plan;
    let scale = plan.scale as f64;
    let max_allowed_j = if head.causal_mask {
        qi.min(plan.keys.saturating_sub(1))
    } else {
        plan.keys - 1
    };
    let mut max_logit = f64::NEG_INFINITY;
    for (kj, logit_slot) in logits.iter_mut().enumerate().take(plan.keys) {
        if head.causal_mask && kj > max_allowed_j {
            *logit_slot = f64::NEG_INFINITY;
            continue;
        }
        let k_row = &head.k[kj * plan.depth..(kj + 1) * plan.depth];
        let s = fma_dot(q_row, k_row) * scale;
        *logit_slot = s;
        max_logit = max_logit.max(s);
    }
    if !max_logit.is_finite() {
        return;
    }
    let mut sum_exp = 0.0_f64;
    for logit_slot in logits.iter_mut().take(plan.keys) {
        if logit_slot.is_finite() {
            let w = (*logit_slot - max_logit).exp();
            *logit_slot = w;
            sum_exp += w;
        } else {
            *logit_slot = 0.0;
        }
    }
    let inv_sum = if sum_exp > 0.0 { 1.0 / sum_exp } else { 0.0 };
    for (kj, &raw_w) in logits.iter().enumerate().take(plan.keys) {
        let w = raw_w * inv_sum;
        if w == 0.0 {
            continue;
        }
        let v_row = &head.v[kj * plan.value_depth..(kj + 1) * plan.value_depth];
        for (dst, &vv) in out_row.iter_mut().zip(v_row.iter()) {
            *dst = (w as f32).mul_add(vv, *dst);
        }
    }
}

impl Tensor {
    /// Reference CPU scaled dot-product attention $\operatorname{softmax}\!\left(\frac{Q K^T}{\sqrt{d_k}} + M\right) V$
    /// for 2D `[Lq, d_k]` or 3D `[H, Lq, d_k]` (and higher batch-prefixed) tensors, with optional
    /// upper-left causal masking (`j <= i`). Supports Grouped-Query Attention (GQA).
    pub fn scaled_dot_product_attention(
        q: &Self,
        k: &Self,
        v: &Self,
        causal_mask: bool,
    ) -> Result<Self, TensorError> {
        let options = AttentionOptions {
            scale: None,
            causal: if causal_mask { Some(0) } else { None },
        };
        let plan = AttentionPlan::new(q.shape(), k.shape(), v.shape(), None, options)?;
        if plan.output.numel() == 0 {
            return Ok(Self {
                shape: plan.output,
                data: Vec::new(),
            });
        }
        if plan.keys == 0 {
            return Ok(Self {
                shape: plan.output.clone(),
                data: vec![0.0; plan.output.numel()],
            });
        }
        let batch_dims = plan.batch.dims();
        let q_strides = broadcast_batch_strides(q.shape().dims(), batch_dims);
        let k_strides = broadcast_batch_strides(k.shape().dims(), batch_dims);
        let v_strides = broadcast_batch_strides(v.shape().dims(), batch_dims);
        let mut out = vec![0.0_f32; plan.output.numel()];
        let mut logits = vec![0.0_f64; plan.keys];
        for b in 0..plan.batch.numel().max(1) {
            let (qb, kb, vb) = batch_offsets(b, batch_dims, &q_strides, &k_strides, &v_strides);
            for h in 0..plan.query_heads {
                let kh = h / plan.group_size;
                let q_base = (qb * plan.query_heads + h) * plan.queries * plan.depth;
                let k_base = (kb * plan.key_heads + kh) * plan.keys * plan.depth;
                let v_base = (vb * plan.key_heads + kh) * plan.keys * plan.value_depth;
                let out_base = (b * plan.query_heads + h) * plan.queries * plan.value_depth;
                let head = HeadSlice {
                    k: &k.data[k_base..k_base + plan.keys * plan.depth],
                    v: &v.data[v_base..v_base + plan.keys * plan.value_depth],
                    plan: &plan,
                    causal_mask,
                };
                for qi in 0..plan.queries {
                    let q_row = &q.data[q_base + qi * plan.depth..q_base + (qi + 1) * plan.depth];
                    let out_row = &mut out
                        [out_base + qi * plan.value_depth..out_base + (qi + 1) * plan.value_depth];
                    attend_query_row(q_row, &head, out_row, &mut logits, qi);
                }
            }
        }
        Ok(Self {
            shape: plan.output,
            data: out,
        })
    }
}
