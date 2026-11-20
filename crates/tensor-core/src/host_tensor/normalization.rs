//! Sequence and vector normalizations (`layer_norm`, `rms_norm`, `l2_normalize`).

use super::Tensor;
use crate::{TensorError, validate_epsilon};

impl Tensor {
    /// Layer normalization `(x - mean) / sqrt(var + eps)` over the trailing dimension (or all elements if rank <= 1).
    pub fn layer_norm(&self, epsilon: f32) -> Result<Self, TensorError> {
        validate_epsilon(epsilon)?;
        if self.data.is_empty() {
            return Ok(self.clone());
        }
        let group_len = *self.shape.dims().last().unwrap_or(&self.data.len());
        if group_len == 0 {
            return Ok(self.clone());
        }
        let eps = epsilon as f64;
        let inv_n = 1.0 / (group_len as f64);
        let mut out = vec![0.0_f32; self.data.len()];
        for (chunk, out_chunk) in self
            .data
            .chunks_exact(group_len)
            .zip(out.chunks_exact_mut(group_len))
        {
            let sum: f64 = chunk.iter().map(|&v| v as f64).sum();
            let mean = sum * inv_n;
            // Two-pass variance for numerical stability on large offsets.
            let mut var_acc = 0.0_f64;
            for &v in chunk {
                let d = v as f64 - mean;
                var_acc = d.mul_add(d, var_acc);
            }
            let var = var_acc * inv_n;
            let inv_std = 1.0 / (var + eps).sqrt();
            for (dst, &v) in out_chunk.iter_mut().zip(chunk.iter()) {
                *dst = ((v as f64 - mean) * inv_std) as f32;
            }
        }
        Ok(Self {
            shape: self.shape.clone(),
            data: out,
        })
    }

    /// Root Mean Square normalization `x / sqrt(mean(x^2) + eps)` over the trailing dimension.
    pub fn rms_norm(&self, epsilon: f32) -> Result<Self, TensorError> {
        validate_epsilon(epsilon)?;
        if self.data.is_empty() {
            return Ok(self.clone());
        }
        let group_len = *self.shape.dims().last().unwrap_or(&self.data.len());
        if group_len == 0 {
            return Ok(self.clone());
        }
        let eps = epsilon as f64;
        let inv_n = 1.0 / (group_len as f64);
        let mut out = vec![0.0_f32; self.data.len()];
        for (chunk, out_chunk) in self
            .data
            .chunks_exact(group_len)
            .zip(out.chunks_exact_mut(group_len))
        {
            let mut sum_sq = 0.0_f64;
            for &v in chunk {
                let x = v as f64;
                sum_sq = x.mul_add(x, sum_sq);
            }
            let mean_sq = sum_sq * inv_n;
            let inv_rms = 1.0 / (mean_sq + eps).sqrt();
            for (dst, &v) in out_chunk.iter_mut().zip(chunk.iter()) {
                *dst = (v as f64 * inv_rms) as f32;
            }
        }
        Ok(Self {
            shape: self.shape.clone(),
            data: out,
        })
    }

    /// L2 normalization `x / max(||x||_2, eps)` along `axis`.
    pub fn l2_normalize(&self, axis: usize, eps: f32) -> Result<Self, TensorError> {
        validate_epsilon(eps)?;
        let rank = self.shape.rank();
        if axis >= rank {
            return Err(TensorError::AxisOutOfBounds { axis, rank });
        }
        let dims = self.shape.dims();
        let axis_len = dims[axis];
        if axis_len == 0 {
            return Err(TensorError::EmptyReduction);
        }
        if self.data.is_empty() {
            return Ok(self.clone());
        }
        let outer: usize = dims[..axis].iter().product();
        let inner: usize = dims[axis + 1..].iter().product();
        let eps64 = eps as f64;
        let mut out = vec![0.0_f32; self.data.len()];
        for o in 0..outer {
            let base_o = o * axis_len * inner;
            for i in 0..inner {
                let mut sum_sq = 0.0_f64;
                for k in 0..axis_len {
                    let v = self.data[base_o + k * inner + i] as f64;
                    sum_sq += v * v;
                }
                let denom = sum_sq.sqrt().max(eps64);
                let inv = 1.0 / denom;
                for k in 0..axis_len {
                    let idx = base_o + k * inner + i;
                    out[idx] = (self.data[idx] as f64 * inv) as f32;
                }
            }
        }
        Ok(Self {
            shape: self.shape.clone(),
            data: out,
        })
    }
}
