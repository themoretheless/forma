//! Normalization layers.

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
            let mut sum = 0.0_f64;
            let mut sum_sq = 0.0_f64;
            for &v in chunk {
                let x = v as f64;
                sum += x;
                sum_sq = x.mul_add(x, sum_sq);
            }
            let mean = sum * inv_n;
            // Two-pass variance for numerical stability on large offsets
            let mut var_acc = 0.0_f64;
            for &v in chunk {
                let d = v as f64 - mean;
                var_acc = d.mul_add(d, var_acc);
            }
            let _ = sum_sq;
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

    /// 1D inference batch normalization over channel axis (`axis = 0` for 1D `[C]`, `axis = 1` for
    /// 2D `[N, C]` or 3D `[N, C, L]`):
    /// `y = ((x - running_mean[c]) / sqrt(running_var[c] + eps)) * weight[c] + bias[c]`.
    pub fn batch_norm_1d(
        &self,
        running_mean: &[f32],
        running_var: &[f32],
        weight: Option<&[f32]>,
        bias: Option<&[f32]>,
        eps: f32,
    ) -> Result<Self, TensorError> {
        validate_epsilon(eps)?;
        let rank = self.shape.rank();
        if rank == 0 || rank > 3 {
            return Err(TensorError::RankMismatch {
                expected: 3,
                actual: rank,
            });
        }
        let dims = self.shape.dims();
        let (batch, channels, spatial) = match rank {
            1 => (1, dims[0], 1),
            2 => (dims[0], dims[1], 1),
            3 => (dims[0], dims[1], dims[2]),
            _ => unreachable!(),
        };
        if running_mean.len() != channels {
            return Err(TensorError::ElementCountMismatch {
                expected: channels,
                actual: running_mean.len(),
            });
        }
        if running_var.len() != channels {
            return Err(TensorError::ElementCountMismatch {
                expected: channels,
                actual: running_var.len(),
            });
        }
        if let Some(w) = weight {
            if w.len() != channels {
                return Err(TensorError::ElementCountMismatch {
                    expected: channels,
                    actual: w.len(),
                });
            }
        }
        if let Some(b) = bias {
            if b.len() != channels {
                return Err(TensorError::ElementCountMismatch {
                    expected: channels,
                    actual: b.len(),
                });
            }
        }
        if self.data.is_empty() {
            return Ok(self.clone());
        }

        // Precompute per-channel fused affine scale & shift: y = x * scale[c] + offset[c]
        let eps64 = eps as f64;
        let mut ch_scale = Vec::with_capacity(channels);
        let mut ch_offset = Vec::with_capacity(channels);
        for c in 0..channels {
            let mean = running_mean[c] as f64;
            let var = running_var[c] as f64;
            if !var.is_finite() || var < 0.0 {
                return Err(TensorError::InvalidEpsilon);
            }
            let gamma = weight.map_or(1.0_f64, |w| w[c] as f64);
            let beta = bias.map_or(0.0_f64, |b| b[c] as f64);
            let s = gamma / (var + eps64).sqrt();
            let o = beta - mean * s;
            ch_scale.push(s);
            ch_offset.push(o);
        }

        let mut out = vec![0.0_f32; self.data.len()];
        for n in 0..batch {
            let n_base = n * channels * spatial;
            for c in 0..channels {
                let s = ch_scale[c];
                let o = ch_offset[c];
                let c_base = n_base + c * spatial;
                let src_slice = &self.data[c_base..c_base + spatial];
                let dst_slice = &mut out[c_base..c_base + spatial];
                for (dst, &x) in dst_slice.iter_mut().zip(src_slice.iter()) {
                    *dst = (x as f64).mul_add(s, o) as f32;
                }
            }
        }

        Ok(Self {
            shape: self.shape.clone(),
            data: out,
        })
    }

    /// Group normalization over `num_groups` channel groups (`axis = 0` for 1D `[C]`, `axis = 1`
    /// for `[N, C, ...]` tensors of rank $\ge 2$).
    ///
    /// Divides the `C` channels into `num_groups` equal-sized groups (`C % num_groups == 0`) and
    /// normalizes each `(n, g)` group slice of length `(C / num_groups) * spatial` to zero mean and
    /// unit variance using 4-wide FMA reductions.
    pub fn group_norm(&self, num_groups: usize, eps: f32) -> Result<Self, TensorError> {
        validate_epsilon(eps)?;
        let rank = self.shape.rank();
        if rank == 0 {
            return Err(TensorError::AxisOutOfBounds { axis: 0, rank: 0 });
        }
        let dims = self.shape.dims();
        let (batch, channels, spatial) = if rank == 1 {
            (1, dims[0], 1)
        } else {
            let sp: usize = dims[2..].iter().product();
            (dims[0], dims[1], sp)
        };
        if num_groups == 0 || channels == 0 || !channels.is_multiple_of(num_groups) {
            return Err(TensorError::ElementCountMismatch {
                expected: num_groups.max(1),
                actual: channels,
            });
        }
        if self.data.is_empty() {
            return Ok(self.clone());
        }

        let group_elems = (channels / num_groups) * spatial;
        if group_elems == 0 {
            return Ok(self.clone());
        }
        let inv_n = 1.0 / (group_elems as f64);
        let eps64 = eps as f64;
        let mut out = vec![0.0_f32; self.data.len()];
        let chunks4 = group_elems / 4;

        for (chunk, out_chunk) in self
            .data
            .chunks_exact(group_elems)
            .zip(out.chunks_exact_mut(group_elems))
        {
            let _ = batch;
            // Pass 1: 4-wide FMA mean reduction
            let mut s0 = 0.0_f64;
            let mut s1 = 0.0_f64;
            let mut s2 = 0.0_f64;
            let mut s3 = 0.0_f64;
            for c in 0..chunks4 {
                let b = c * 4;
                s0 += chunk[b] as f64;
                s1 += chunk[b + 1] as f64;
                s2 += chunk[b + 2] as f64;
                s3 += chunk[b + 3] as f64;
            }
            let mut sum = (s0 + s1) + (s2 + s3);
            for &v in &chunk[chunks4 * 4..] {
                sum += v as f64;
            }
            let mean = sum * inv_n;

            // Pass 2: 4-wide FMA centered variance reduction
            let mut v0 = 0.0_f64;
            let mut v1 = 0.0_f64;
            let mut v2 = 0.0_f64;
            let mut v3 = 0.0_f64;
            for c in 0..chunks4 {
                let b = c * 4;
                let d0 = chunk[b] as f64 - mean;
                let d1 = chunk[b + 1] as f64 - mean;
                let d2 = chunk[b + 2] as f64 - mean;
                let d3 = chunk[b + 3] as f64 - mean;
                v0 = d0.mul_add(d0, v0);
                v1 = d1.mul_add(d1, v1);
                v2 = d2.mul_add(d2, v2);
                v3 = d3.mul_add(d3, v3);
            }
            let mut var_sum = (v0 + v1) + (v2 + v3);
            for &v in &chunk[chunks4 * 4..] {
                let d = v as f64 - mean;
                var_sum = d.mul_add(d, var_sum);
            }
            let var = var_sum * inv_n;
            let inv_std = 1.0 / (var + eps64).sqrt();

            for (dst, &v) in out_chunk.iter_mut().zip(chunk.iter()) {
                *dst = ((v as f64 - mean) * inv_std) as f32;
            }
        }

        Ok(Self {
            shape: self.shape.clone(),
            data: out,
        })
    }
}
