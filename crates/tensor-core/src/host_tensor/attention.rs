//! Attention and positional encoding.

use super::Tensor;
use crate::TensorError;

impl Tensor {
    /// Reference CPU scaled dot-product attention $\operatorname{softmax}\!\left(\frac{Q K^T}{\sqrt{d_k}} + M\right) V$
    /// for 2D `[Lq, d_k]` or 3D `[H, Lq, d_k]` (and higher batch-prefixed) tensors, with optional
    /// upper-left causal masking (`j <= i`). Supports Grouped-Query Attention (GQA) when `Q` has a
    /// head count that is a multiple of `K`/`V` head count.
    pub fn scaled_dot_product_attention(
        q: &Self,
        k: &Self,
        v: &Self,
        causal_mask: bool,
    ) -> Result<Self, TensorError> {
        let options = crate::AttentionOptions {
            scale: None,
            causal: if causal_mask { Some(0) } else { None },
        };
        let plan = crate::AttentionPlan::new(q.shape(), k.shape(), v.shape(), None, options)?;
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
        let batch_rank = batch_dims.len();
        let batch_elems = plan.batch.numel().max(1);
        let q_batch_strides = broadcast_batch_strides(q.shape().dims(), batch_dims);
        let k_batch_strides = broadcast_batch_strides(k.shape().dims(), batch_dims);
        let v_batch_strides = broadcast_batch_strides(v.shape().dims(), batch_dims);

        let scale = plan.scale as f64;
        let mut out = vec![0.0_f32; plan.output.numel()];
        let mut logits = vec![0.0_f64; plan.keys];

        for b in 0..batch_elems {
            let mut rem = b;
            let mut qb = 0usize;
            let mut kb = 0usize;
            let mut vb = 0usize;
            for axis in (0..batch_rank).rev() {
                let dim = batch_dims[axis];
                let coord = rem % dim;
                rem /= dim;
                qb += coord * q_batch_strides[axis];
                kb += coord * k_batch_strides[axis];
                vb += coord * v_batch_strides[axis];
            }

            for h in 0..plan.query_heads {
                let kh = h / plan.group_size;
                let q_head_base = (qb * plan.query_heads + h) * plan.queries * plan.depth;
                let k_head_base = (kb * plan.key_heads + kh) * plan.keys * plan.depth;
                let v_head_base = (vb * plan.key_heads + kh) * plan.keys * plan.value_depth;
                let out_head_base = (b * plan.query_heads + h) * plan.queries * plan.value_depth;

                for qi in 0..plan.queries {
                    let q_row =
                        &q.data[q_head_base + qi * plan.depth..q_head_base + (qi + 1) * plan.depth];
                    let mut max_logit = f64::NEG_INFINITY;
                    let max_allowed_j = if causal_mask {
                        qi.min(plan.keys.saturating_sub(1))
                    } else {
                        plan.keys - 1
                    };

                    for (kj, logit_slot) in logits.iter_mut().enumerate().take(plan.keys) {
                        if causal_mask && kj > max_allowed_j {
                            *logit_slot = f64::NEG_INFINITY;
                            continue;
                        }
                        let k_row = &k.data
                            [k_head_base + kj * plan.depth..k_head_base + (kj + 1) * plan.depth];
                        // 4-wide unrolled FMA dot product for NEON/AVX auto-vectorization
                        let mut acc0 = 0.0_f64;
                        let mut acc1 = 0.0_f64;
                        let mut acc2 = 0.0_f64;
                        let mut acc3 = 0.0_f64;
                        let chunks4 = plan.depth / 4;
                        for c in 0..chunks4 {
                            let base = c * 4;
                            acc0 = (q_row[base] as f64).mul_add(k_row[base] as f64, acc0);
                            acc1 = (q_row[base + 1] as f64).mul_add(k_row[base + 1] as f64, acc1);
                            acc2 = (q_row[base + 2] as f64).mul_add(k_row[base + 2] as f64, acc2);
                            acc3 = (q_row[base + 3] as f64).mul_add(k_row[base + 3] as f64, acc3);
                        }
                        let mut dot = (acc0 + acc1) + (acc2 + acc3);
                        for d in (chunks4 * 4)..plan.depth {
                            dot = (q_row[d] as f64).mul_add(k_row[d] as f64, dot);
                        }
                        let s = dot * scale;
                        *logit_slot = s;
                        if s > max_logit {
                            max_logit = s;
                        }
                    }

                    if !max_logit.is_finite() {
                        continue;
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
                    let out_row = &mut out[out_head_base + qi * plan.value_depth
                        ..out_head_base + (qi + 1) * plan.value_depth];
                    for (kj, &raw_w) in logits.iter().enumerate().take(plan.keys) {
                        let w = raw_w * inv_sum;
                        if w == 0.0 {
                            continue;
                        }
                        let v_row = &v.data[v_head_base + kj * plan.value_depth
                            ..v_head_base + (kj + 1) * plan.value_depth];
                        for (dst, &vv) in out_row.iter_mut().zip(v_row.iter()) {
                            *dst = (w as f32).mul_add(vv, *dst);
                        }
                    }
                }
            }
        }

        Ok(Self {
            shape: plan.output,
            data: out,
        })
    }

    /// Apply Rotary Position Embedding (RoPE) along the sequence dimension (`dims[rank - 2]`) and
    /// even-sized feature/head dimension (`dims[rank - 1]`) for 2D `[L, D]` or higher `[..., L, D]` tensors.
    ///
    /// Pairs of adjacent features `(x_{2i}, x_{2i+1})` at sequence position `m` are rotated by
    /// $\theta_{m, i} = m \cdot \text{base\_theta}^{-2i / D}$.
    pub fn apply_rotary_pos_emb(&self, base_theta: f32) -> Result<Self, TensorError> {
        if !base_theta.is_finite() || base_theta <= 0.0 {
            return Err(TensorError::InvalidAttention(
                "apply_rotary_pos_emb base_theta must be finite and positive",
            ));
        }
        let rank = self.shape.rank();
        if rank < 2 {
            return Err(TensorError::InvalidAttention(
                "apply_rotary_pos_emb expects rank >= 2 ([..., seq_len, head_dim])",
            ));
        }
        let dims = self.shape.dims();
        let seq_len = dims[rank - 2];
        let head_dim = dims[rank - 1];
        if !head_dim.is_multiple_of(2) || head_dim == 0 {
            return Err(TensorError::InvalidAttention(
                "apply_rotary_pos_emb requires a positive even head_dim",
            ));
        }
        if self.data.is_empty() {
            return Ok(self.clone());
        }

        let half_dim = head_dim / 2;
        let base64 = base_theta as f64;
        let inv_d = 1.0 / (head_dim as f64);
        let inv_freq: Vec<f64> = (0..half_dim)
            .map(|i| base64.powf(-((2 * i) as f64) * inv_d))
            .collect();

        // Precompute (cos, sin) table of shape [seq_len, half_dim]
        let mut cos_table = Vec::with_capacity(seq_len * half_dim);
        let mut sin_table = Vec::with_capacity(seq_len * half_dim);
        for m in 0..seq_len {
            let m_f = m as f64;
            for &freq in &inv_freq {
                let (s, c) = (m_f * freq).sin_cos();
                cos_table.push(c);
                sin_table.push(s);
            }
        }

        let outer: usize = dims[..rank - 2].iter().product();
        let mut out = vec![0.0_f32; self.data.len()];
        for b in 0..outer {
            let b_base = b * seq_len * head_dim;
            for m in 0..seq_len {
                let row_base = b_base + m * head_dim;
                let trig_base = m * half_dim;
                let src_row = &self.data[row_base..row_base + head_dim];
                let dst_row = &mut out[row_base..row_base + head_dim];
                for i in 0..half_dim {
                    let x0 = src_row[2 * i] as f64;
                    let x1 = src_row[2 * i + 1] as f64;
                    let c = cos_table[trig_base + i];
                    let s = sin_table[trig_base + i];
                    dst_row[2 * i] = (x0 * c - x1 * s) as f32;
                    dst_row[2 * i + 1] = (x0 * s + x1 * c) as f32;
                }
            }
        }

        Ok(Self {
            shape: self.shape.clone(),
            data: out,
        })
    }
}

/// Computes right-aligned broadcast batch strides (in units of per-batch slice index)
/// for an operand's shape against `batch_dims`.
/// Leading batch axes live in `operand_dims[..rank - 3]` when `rank >= 3` (2D operands have no batch axes).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_dot_product_attention_broadcasts_multi_axis_batches() {
        // 5D multi-axis batch broadcast: Q=[2, 1, 1, 2, 2], K=V=[1, 3, 1, 2, 2] -> output [2, 3, 1, 2, 2]
        // Also test 4D batch+head broadcast: Q=[2, 1, 2, 2] (batch=2, heads=1) vs K=V=[1, 3, 2, 2] is invalid GQA (1 % 3 != 0),
        // whereas Q=[2, 1, 1, 2, 2] x K=V=[1, 3, 1, 2, 2] has batch [2, 3] and heads=1, S=2, D=2!
        let q_5d = Tensor::new(
            vec![2, 1, 1, 2, 2],
            vec![
                // q batch (0, 0): identity queries
                1.0, 0.0, 0.0, 1.0, // q batch (1, 0): swapped queries
                0.0, 1.0, 1.0, 0.0,
            ],
        )
        .unwrap();
        let k_5d = Tensor::new(
            vec![1, 3, 1, 2, 2],
            vec![
                // k batch (0, 0)
                2.0, 0.0, 0.0, 2.0, // k batch (0, 1)
                0.0, 2.0, 2.0, 0.0, // k batch (0, 2): zero keys -> uniform attention
                0.0, 0.0, 0.0, 0.0,
            ],
        )
        .unwrap();
        let v_5d = Tensor::new(
            vec![1, 3, 1, 2, 2],
            vec![
                // v batch (0, 0)
                10.0, 20.0, 30.0, 40.0, // v batch (0, 1)
                100.0, 200.0, 300.0, 400.0, // v batch (0, 2)
                5.0, 7.0, 9.0, 11.0,
            ],
        )
        .unwrap();

        let out = Tensor::scaled_dot_product_attention(&q_5d, &k_5d, &v_5d, false).unwrap();
        assert_eq!(out.shape().dims(), &[2, 3, 1, 2, 2]);

        // Verify each (i, j) in 2x3 matches independent 3D [1, 2, 2] slice attention!
        for qi in 0..2 {
            let q_slice =
                Tensor::new(vec![1, 2, 2], q_5d.data()[qi * 4..(qi + 1) * 4].to_vec()).unwrap();
            for kj in 0..3 {
                let k_slice =
                    Tensor::new(vec![1, 2, 2], k_5d.data()[kj * 4..(kj + 1) * 4].to_vec()).unwrap();
                let v_slice =
                    Tensor::new(vec![1, 2, 2], v_5d.data()[kj * 4..(kj + 1) * 4].to_vec()).unwrap();
                let expected =
                    Tensor::scaled_dot_product_attention(&q_slice, &k_slice, &v_slice, false)
                        .unwrap();
                let b_idx = qi * 3 + kj;
                let actual_slice = &out.data()[b_idx * 4..(b_idx + 1) * 4];
                for (a, e) in actual_slice.iter().zip(expected.data().iter()) {
                    assert!(
                        (a - e).abs() < 1e-6,
                        "mismatch at batch ({qi}, {kj}): {a} != {e}"
                    );
                }
            }
        }
    }
}
