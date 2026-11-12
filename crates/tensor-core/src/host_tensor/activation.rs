//! Element-wise activations and selections.

use super::Tensor;
use crate::TensorError;

impl Tensor {
    /// Gaussian Error Linear Unit (exact erf-based formula via tanh approximation or exact polynomial).
    /// Uses `0.5 * x * (1 + tanh(sqrt(2/pi) * (x + 0.044715 * x^3)))`.
    pub fn gelu(&self) -> Self {
        const SQRT_2_OVER_PI: f64 = 0.797_884_560_802_865_4;
        const COEFF: f64 = 0.044_715;
        let data = self
            .data
            .iter()
            .map(|&v| {
                let x = v as f64;
                let inner = SQRT_2_OVER_PI * (x + COEFF * x * x * x);
                (0.5 * x * (1.0 + inner.tanh())) as f32
            })
            .collect();
        Self {
            shape: self.shape.clone(),
            data,
        }
    }

    /// Sigmoid Linear Unit (Swish-1): `x / (1 + exp(-x))`.
    pub fn silu(&self) -> Self {
        let data = self
            .data
            .iter()
            .map(|&v| {
                let x = v as f64;
                (x / (1.0 + (-x).exp())) as f32
            })
            .collect();
        Self {
            shape: self.shape.clone(),
            data,
        }
    }

    /// Fused SwiGLU gated activation $\operatorname{SwiGLU}(x, g) = \operatorname{SiLU}(x) \odot g = \frac{x}{1 + e^{-x}} \cdot g$
    /// in a single pass over identically-shaped tensors.
    pub fn swiglu(&self, gate: &Self) -> Result<Self, TensorError> {
        if self.shape != gate.shape {
            return Err(TensorError::IncompatibleBroadcast {
                left: self.shape.dims().to_vec(),
                right: gate.shape.dims().to_vec(),
            });
        }
        let data = self
            .data
            .iter()
            .zip(gate.data.iter())
            .map(|(&x, &g)| {
                let xf = x as f64;
                let gf = g as f64;
                let silu = xf / (1.0 + (-xf).exp());
                (silu * gf) as f32
            })
            .collect();
        Ok(Self {
            shape: self.shape.clone(),
            data,
        })
    }

    /// Elementwise clamp each value to `[min_val, max_val]`.
    pub fn clamp(&self, min_val: f32, max_val: f32) -> Self {
        let (lo, hi) = if min_val <= max_val {
            (min_val, max_val)
        } else {
            (max_val, min_val)
        };
        let data = self.data.iter().map(|&v| v.clamp(lo, hi)).collect();
        Self {
            shape: self.shape.clone(),
            data,
        }
    }

    /// Elementwise ternary selection `condition[i] ? a[i] : b[i]` for identically-shaped tensors.
    pub fn where_select(condition: &[bool], a: &Self, b: &Self) -> Result<Self, TensorError> {
        if a.shape != b.shape {
            return Err(TensorError::IncompatibleBroadcast {
                left: a.shape.dims().to_vec(),
                right: b.shape.dims().to_vec(),
            });
        }
        if condition.len() != a.data.len() {
            return Err(TensorError::ElementCountMismatch {
                expected: a.data.len(),
                actual: condition.len(),
            });
        }
        let data = condition
            .iter()
            .zip(a.data.iter().zip(b.data.iter()))
            .map(|(&cond, (&av, &bv))| if cond { av } else { bv })
            .collect();
        Ok(Self {
            shape: a.shape.clone(),
            data,
        })
    }

    /// Numerically stable softmax `exp(x - max(x)) / sum(exp(x - max(x)))` along `axis`.
    pub fn softmax(&self, axis: usize) -> Result<Self, TensorError> {
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
        let mut out = vec![0.0_f32; self.data.len()];
        for o in 0..outer {
            let base_o = o * axis_len * inner;
            for i in 0..inner {
                let mut max_val = f64::NEG_INFINITY;
                for k in 0..axis_len {
                    let v = self.data[base_o + k * inner + i] as f64;
                    if v > max_val {
                        max_val = v;
                    }
                }
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
        let mut out = vec![0.0_f32; self.data.len()];
        for o in 0..outer {
            let base_o = o * axis_len * inner;
            for i in 0..inner {
                let mut max_val = f64::NEG_INFINITY;
                for k in 0..axis_len {
                    let v = self.data[base_o + k * inner + i] as f64;
                    if v > max_val {
                        max_val = v;
                    }
                }
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
