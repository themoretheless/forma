//! Numerically stable softmax and log-softmax reductions along an axis.

use super::Tensor;
use crate::TensorError;

fn softmax_dims(tensor: &Tensor, axis: usize) -> Result<(usize, usize, usize), TensorError> {
    let rank = tensor.shape.rank();
    if axis >= rank {
        return Err(TensorError::AxisOutOfBounds { axis, rank });
    }
    let dims = tensor.shape.dims();
    let axis_len = dims[axis];
    if axis_len == 0 {
        return Err(TensorError::EmptyReduction);
    }
    let outer: usize = dims[..axis].iter().product();
    let inner: usize = dims[axis + 1..].iter().product();
    Ok((outer, axis_len, inner))
}

fn lane_max(data: &[f32], base_o: usize, axis_len: usize, inner: usize, i: usize) -> f64 {
    let mut max_val = f64::NEG_INFINITY;
    for k in 0..axis_len {
        let v = data[base_o + k * inner + i] as f64;
        if v > max_val {
            max_val = v;
        }
    }
    max_val
}

impl Tensor {
    /// Numerically stable softmax `exp(x - max(x)) / sum(exp(x - max(x)))` along `axis`.
    pub fn softmax(&self, axis: usize) -> Result<Self, TensorError> {
        let (outer, axis_len, inner) = softmax_dims(self, axis)?;
        if self.data.is_empty() {
            return Ok(self.clone());
        }
        let mut out = vec![0.0_f32; self.data.len()];
        for o in 0..outer {
            let base_o = o * axis_len * inner;
            for i in 0..inner {
                let max_val = lane_max(&self.data, base_o, axis_len, inner, i);
                let mut sum = 0.0_f64;
                for k in 0..axis_len {
                    let idx = base_o + k * inner + i;
                    let e = (self.data[idx] as f64 - max_val).exp();
                    out[idx] = e as f32;
                    sum += e;
                }
                let inv_sum = if sum > 0.0 { 1.0 / sum } else { 0.0 };
                for k in 0..axis_len {
                    let idx = base_o + k * inner + i;
                    out[idx] = (out[idx] as f64 * inv_sum) as f32;
                }
            }
        }
        Ok(Self {
            shape: self.shape.clone(),
            data: out,
        })
    }

    /// Numerically stable log-softmax `(x - max(x)) - ln(sum(exp(x - max(x))))` along `axis`.
    pub fn log_softmax(&self, axis: usize) -> Result<Self, TensorError> {
        let (outer, axis_len, inner) = softmax_dims(self, axis)?;
        if self.data.is_empty() {
            return Ok(self.clone());
        }
        let mut out = vec![0.0_f32; self.data.len()];
        for o in 0..outer {
            let base_o = o * axis_len * inner;
            for i in 0..inner {
                let max_val = lane_max(&self.data, base_o, axis_len, inner, i);
                let mut sum_exp = 0.0_f64;
                for k in 0..axis_len {
                    let idx = base_o + k * inner + i;
                    sum_exp += (self.data[idx] as f64 - max_val).exp();
                }
                let log_sum_exp = max_val + sum_exp.ln();
                for k in 0..axis_len {
                    let idx = base_o + k * inner + i;
                    out[idx] = (self.data[idx] as f64 - log_sum_exp) as f32;
                }
            }
        }
        Ok(Self {
            shape: self.shape.clone(),
            data: out,
        })
    }
}
