//! Rotary Position Embedding (RoPE).

use super::Tensor;
use crate::TensorError;

impl Tensor {
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

        // Precompute (cos, sin) table of shape [seq_len, half_dim].
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
