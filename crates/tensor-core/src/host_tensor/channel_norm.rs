//! Channel-axis normalizations (`batch_norm_1d`, `group_norm`).

use super::Tensor;
use crate::{TensorError, validate_epsilon};

fn check_channel_param(param: &[f32], channels: usize) -> Result<(), TensorError> {
    if param.len() != channels {
        return Err(TensorError::ElementCountMismatch {
            expected: channels,
            actual: param.len(),
        });
    }
    Ok(())
}

fn sum_f64_4wide(chunk: &[f32]) -> f64 {
    let chunks4 = chunk.len() / 4;
    let mut s = [0.0_f64; 4];
    for c in 0..chunks4 {
        let b = c * 4;
        for j in 0..4 {
            s[j] += chunk[b + j] as f64;
        }
    }
    let rem = chunk[chunks4 * 4..].iter().map(|&v| v as f64).sum::<f64>();
    (s[0] + s[1]) + (s[2] + s[3]) + rem
}

fn var_sum_f64_4wide(chunk: &[f32], mean: f64) -> f64 {
    let chunks4 = chunk.len() / 4;
    let mut v = [0.0_f64; 4];
    for c in 0..chunks4 {
        let b = c * 4;
        for j in 0..4 {
            let d = chunk[b + j] as f64 - mean;
            v[j] = d.mul_add(d, v[j]);
        }
    }
    let mut rem_sum = 0.0_f64;
    for &x in &chunk[chunks4 * 4..] {
        let d = x as f64 - mean;
        rem_sum = d.mul_add(d, rem_sum);
    }
    (v[0] + v[1]) + (v[2] + v[3]) + rem_sum
}

impl Tensor {
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
            _ => (dims[0], dims[1], dims[2]),
        };
        check_channel_param(running_mean, channels)?;
        check_channel_param(running_var, channels)?;
        if let Some(w) = weight {
            check_channel_param(w, channels)?;
        }
        if let Some(b) = bias {
            check_channel_param(b, channels)?;
        }
        if self.data.is_empty() {
            return Ok(self.clone());
        }

        let eps64 = eps as f64;
        let mut ch_scale = Vec::with_capacity(channels);
        let mut ch_offset = Vec::with_capacity(channels);
        for c in 0..channels {
            let var = running_var[c] as f64;
            if !var.is_finite() || var < 0.0 {
                return Err(TensorError::InvalidEpsilon);
            }
            let s = weight.map_or(1.0_f64, |w| w[c] as f64) / (var + eps64).sqrt();
            ch_scale.push(s);
            ch_offset.push(bias.map_or(0.0_f64, |b| b[c] as f64) - running_mean[c] as f64 * s);
        }

        let mut out = vec![0.0_f32; self.data.len()];
        for n in 0..batch {
            let n_base = n * channels * spatial;
            for c in 0..channels {
                let (s, o) = (ch_scale[c], ch_offset[c]);
                let c_base = n_base + c * spatial;
                for (dst, &x) in out[c_base..c_base + spatial]
                    .iter_mut()
                    .zip(&self.data[c_base..c_base + spatial])
                {
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
    pub fn group_norm(&self, num_groups: usize, eps: f32) -> Result<Self, TensorError> {
        validate_epsilon(eps)?;
        let rank = self.shape.rank();
        if rank == 0 {
            return Err(TensorError::AxisOutOfBounds { axis: 0, rank: 0 });
        }
        let dims = self.shape.dims();
        let (channels, spatial) = if rank == 1 {
            (dims[0], 1)
        } else {
            (dims[1], dims[2..].iter().product())
        };
        if num_groups == 0 || channels == 0 || !channels.is_multiple_of(num_groups) {
            return Err(TensorError::ElementCountMismatch {
                expected: num_groups.max(1),
                actual: channels,
            });
        }
        let group_elems = (channels / num_groups) * spatial;
        if self.data.is_empty() || group_elems == 0 {
            return Ok(self.clone());
        }
        let inv_n = 1.0 / (group_elems as f64);
        let eps64 = eps as f64;
        let mut out = vec![0.0_f32; self.data.len()];
        for (chunk, out_chunk) in self
            .data
            .chunks_exact(group_elems)
            .zip(out.chunks_exact_mut(group_elems))
        {
            let mean = sum_f64_4wide(chunk) * inv_n;
            let inv_std = 1.0 / (var_sum_f64_4wide(chunk, mean) * inv_n + eps64).sqrt();
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
