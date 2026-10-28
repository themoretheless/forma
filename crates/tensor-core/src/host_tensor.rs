use crate::{HasShape, Shape, TensorError, validate_epsilon};

/// Result of a `topk` selection along the last axis.
#[derive(Clone, Debug, PartialEq)]
pub struct TopKResult {
    pub values: Tensor,
    pub indices: Vec<u32>,
}

/// Host reference tensor with checked [`Shape`] and contiguous `f32` storage.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    shape: Shape,
    data: Vec<f32>,
}

impl HasShape for Tensor {
    fn shape(&self) -> &Shape {
        &self.shape
    }
}

impl Tensor {
    pub fn new(dims: impl Into<Vec<usize>>, data: impl Into<Vec<f32>>) -> Result<Self, TensorError> {
        let shape = Shape::new(dims)?;
        let data = data.into();
        if data.len() != shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: data.len(),
            });
        }
        Ok(Self { shape, data })
    }

    pub fn from_shape(shape: Shape, data: Vec<f32>) -> Result<Self, TensorError> {
        if data.len() != shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: data.len(),
            });
        }
        Ok(Self { shape, data })
    }

    #[inline]
    pub fn shape(&self) -> &Shape {
        &self.shape
    }

    #[inline]
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    #[inline]
    pub fn into_data(self) -> Vec<f32> {
        self.data
    }

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
        for (chunk, out_chunk) in self.data.chunks_exact(group_len).zip(out.chunks_exact_mut(group_len)) {
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
        for (chunk, out_chunk) in self.data.chunks_exact(group_len).zip(out.chunks_exact_mut(group_len)) {
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

    /// Cosine similarity between two identically-shaped tensors along the last dimension.
    pub fn cosine_similarity(&self, other: &Self, epsilon: f32) -> Result<Self, TensorError> {
        validate_epsilon(epsilon)?;
        if self.shape != other.shape {
            return Err(TensorError::IncompatibleBroadcast {
                left: self.shape.dims().to_vec(),
                right: other.shape.dims().to_vec(),
            });
        }
        if self.shape.rank() == 0 {
            return Err(TensorError::AxisOutOfBounds { axis: 0, rank: 0 });
        }
        let last_axis = self.shape.rank() - 1;
        let out_shape = self.shape.reduce(&[last_axis], false)?;
        let group_len = self.shape.dims()[last_axis];
        if group_len == 0 {
            return Err(TensorError::EmptyReduction);
        }
        let eps = epsilon as f64;
        let mut out = Vec::with_capacity(out_shape.numel());
        for (a_chunk, b_chunk) in self.data.chunks(group_len).zip(other.data.chunks(group_len)) {
            let mut dot = 0.0_f64;
            let mut norm_a = 0.0_f64;
            let mut norm_b = 0.0_f64;
            for (&a, &b) in a_chunk.iter().zip(b_chunk.iter()) {
                let af = a as f64;
                let bf = b as f64;
                dot += af * bf;
                norm_a += af * af;
                norm_b += bf * bf;
            }
            let denom = (norm_a.sqrt() * norm_b.sqrt()).max(eps);
            out.push((dot / denom) as f32);
        }
        Ok(Self {
            shape: out_shape,
            data: out,
        })
    }

    /// Select the top `k` largest elements (sorted descending, tie-breaking by lowest index) along the last axis.
    pub fn topk(&self, k: usize) -> Result<TopKResult, TensorError> {
        if self.shape.rank() == 0 {
            return Err(TensorError::AxisOutOfBounds { axis: 0, rank: 0 });
        }
        let rank = self.shape.rank();
        let last_dim = self.shape.dims()[rank - 1];
        if k > last_dim {
            return Err(TensorError::InvalidSlice {
                axis: rank - 1,
                start: 0,
                len: k,
                dimension: last_dim,
            });
        }
        let mut out_dims = self.shape.dims().to_vec();
        out_dims[rank - 1] = k;
        let out_shape = Shape::new(out_dims)?;
        if k == 0 || self.data.is_empty() {
            return Ok(TopKResult {
                values: Self {
                    shape: out_shape,
                    data: Vec::new(),
                },
                indices: Vec::new(),
            });
        }
        let num_groups = self.data.len() / last_dim;
        let mut values = Vec::with_capacity(num_groups * k);
        let mut indices = Vec::with_capacity(num_groups * k);
        let mut row: Vec<(u32, f32)> = Vec::with_capacity(last_dim);
        for chunk in self.data.chunks(last_dim) {
            row.clear();
            row.extend(
                chunk
                    .iter()
                    .enumerate()
                    .map(|(idx, &val)| (idx as u32, val)),
            );
            row.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            for &(idx, val) in &row[..k] {
                values.push(val);
                indices.push(idx);
            }
        }
        Ok(TopKResult {
            values: Self {
                shape: out_shape,
                data: values,
            },
            indices,
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

        let batch_elems = plan.batch.numel().max(1);
        let q_batch_elems = q.data.len() / (plan.query_heads * plan.queries * plan.depth).max(1);
        let k_batch_elems = k.data.len() / (plan.key_heads * plan.keys * plan.depth).max(1);
        let v_batch_elems = v.data.len() / (plan.key_heads * plan.keys * plan.value_depth).max(1);

        let scale = plan.scale as f64;
        let mut out = vec![0.0_f32; plan.output.numel()];
        let mut logits = vec![0.0_f64; plan.keys];

        for b in 0..batch_elems {
            let qb = if q_batch_elems <= 1 { 0 } else { b % q_batch_elems };
            let kb = if k_batch_elems <= 1 { 0 } else { b % k_batch_elems };
            let vb = if v_batch_elems <= 1 { 0 } else { b % v_batch_elems };

            for h in 0..plan.query_heads {
                let kh = h / plan.group_size;
                let q_head_base = (qb * plan.query_heads + h) * plan.queries * plan.depth;
                let k_head_base = (kb * plan.key_heads + kh) * plan.keys * plan.depth;
                let v_head_base = (vb * plan.key_heads + kh) * plan.keys * plan.value_depth;
                let out_head_base = (b * plan.query_heads + h) * plan.queries * plan.value_depth;

                for qi in 0..plan.queries {
                    let q_row = &q.data[q_head_base + qi * plan.depth..q_head_base + (qi + 1) * plan.depth];
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
                        let k_row = &k.data[k_head_base + kj * plan.depth..k_head_base + (kj + 1) * plan.depth];
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

    /// 1D cross-correlation (`conv1d`) over a 3D input `[N, C_in, L]` with weight `[C_out, C_in, K]`,
    /// optional per-channel `bias` of length `C_out`, `stride >= 1`, and symmetric zero `padding`.
    pub fn conv1d(
        &self,
        weight: &Self,
        bias: Option<&[f32]>,
        stride: usize,
        padding: usize,
    ) -> Result<Self, TensorError> {
        let mut options = crate::ConvOptions::new(1);
        options.strides[0] = stride;
        options.padding_before[0] = padding;
        options.padding_after[0] = padding;
        let plan = crate::ConvPlan::new(&self.shape, &weight.shape, &options)?;
        let out_channels = weight.shape.dims()[0];
        if let Some(b) = bias {
            if b.len() != out_channels {
                return Err(TensorError::ElementCountMismatch {
                    expected: out_channels,
                    actual: b.len(),
                });
            }
        }
        if plan.output.numel() == 0 {
            return Ok(Self {
                shape: plan.output,
                data: Vec::new(),
            });
        }

        let n_batch = self.shape.dims()[0];
        let in_channels = self.shape.dims()[1];
        let in_len = self.shape.dims()[2];
        let kernel_size = weight.shape.dims()[2];
        let out_len = plan.output.dims()[2];
        let mut out = vec![0.0_f32; plan.output.numel()];

        for n in 0..n_batch {
            let n_in_base = n * in_channels * in_len;
            let n_out_base = n * out_channels * out_len;
            for oc in 0..out_channels {
                let b_val = bias.map_or(0.0_f64, |b| b[oc] as f64);
                let out_base = n_out_base + oc * out_len;
                let oc_w_base = oc * in_channels * kernel_size;
                for ol in 0..out_len {
                    let mut acc = b_val;
                    let in_start = (ol * stride) as isize - (padding as isize);
                    // Compute valid kernel range [k_min, k_max) once per output position to eliminate inner branches
                    let k_min = (-in_start).max(0) as usize;
                    let k_max = ((in_len as isize - in_start).max(0) as usize).min(kernel_size);
                    if k_min < k_max {
                        let valid_len = k_max - k_min;
                        let in_offset = (in_start + k_min as isize) as usize;
                        for ic in 0..in_channels {
                            let in_slice = &self.data[n_in_base + ic * in_len + in_offset
                                ..n_in_base + ic * in_len + in_offset + valid_len];
                            let w_slice = &weight.data[oc_w_base + ic * kernel_size + k_min
                                ..oc_w_base + ic * kernel_size + k_max];
                            for (&x, &w) in in_slice.iter().zip(w_slice.iter()) {
                                acc = (x as f64).mul_add(w as f64, acc);
                            }
                        }
                    }
                    out[out_base + ol] = acc as f32;
                }
            }
        }

        Ok(Self {
            shape: plan.output,
            data: out,
        })
    }

    /// 1D max pooling over the trailing spatial dimension of a 2D `[C, L]` or 3D `[N, C, L]` tensor
    /// with window `kernel_size >= 1` and `stride >= 1`.
    pub fn max_pool1d(&self, kernel_size: usize, stride: usize) -> Result<Self, TensorError> {
        let rank = self.shape.rank();
        if rank != 2 && rank != 3 {
            return Err(TensorError::InvalidConvolution(
                "max_pool1d expects a 2D [C, L] or 3D [N, C, L] tensor",
            ));
        }
        if kernel_size == 0 || stride == 0 {
            return Err(TensorError::InvalidConvolution(
                "max_pool1d kernel_size and stride must be positive",
            ));
        }
        let dims = self.shape.dims();
        let in_len = dims[rank - 1];
        if in_len < kernel_size {
            return Err(TensorError::InvalidConvolution(
                "max_pool1d input length must be at least kernel_size",
            ));
        }
        let out_len = (in_len - kernel_size) / stride + 1;
        let mut out_dims = dims.to_vec();
        out_dims[rank - 1] = out_len;
        let out_shape = Shape::new(out_dims)?;
        if out_shape.numel() == 0 {
            return Ok(Self {
                shape: out_shape,
                data: Vec::new(),
            });
        }

        let outer: usize = dims[..rank - 1].iter().product();
        let mut out = Vec::with_capacity(outer * out_len);
        for c in 0..outer {
            let row = &self.data[c * in_len..(c + 1) * in_len];
            for ol in 0..out_len {
                let start = ol * stride;
                let window = &row[start..start + kernel_size];
                let mut max_val = f32::NEG_INFINITY;
                for &v in window {
                    if v > max_val || max_val.is_nan() {
                        max_val = v;
                    }
                }
                out.push(max_val);
            }
        }
        Ok(Self {
            shape: out_shape,
            data: out,
        })
    }

    /// 1D average pooling over the trailing spatial dimension of a 2D `[C, L]` or 3D `[N, C, L]` tensor
    /// with window `kernel_size >= 1` and `stride >= 1`.
    pub fn avg_pool1d(&self, kernel_size: usize, stride: usize) -> Result<Self, TensorError> {
        let rank = self.shape.rank();
        if rank != 2 && rank != 3 {
            return Err(TensorError::InvalidConvolution(
                "avg_pool1d expects a 2D [C, L] or 3D [N, C, L] tensor",
            ));
        }
        if kernel_size == 0 || stride == 0 {
            return Err(TensorError::InvalidConvolution(
                "avg_pool1d kernel_size and stride must be positive",
            ));
        }
        let dims = self.shape.dims();
        let in_len = dims[rank - 1];
        if in_len < kernel_size {
            return Err(TensorError::InvalidConvolution(
                "avg_pool1d input length must be at least kernel_size",
            ));
        }
        let out_len = (in_len - kernel_size) / stride + 1;
        let mut out_dims = dims.to_vec();
        out_dims[rank - 1] = out_len;
        let out_shape = Shape::new(out_dims)?;
        if out_shape.numel() == 0 {
            return Ok(Self {
                shape: out_shape,
                data: Vec::new(),
            });
        }

        let inv_k = 1.0 / (kernel_size as f64);
        let outer: usize = dims[..rank - 1].iter().product();
        let mut out = Vec::with_capacity(outer * out_len);
        for c in 0..outer {
            let row = &self.data[c * in_len..(c + 1) * in_len];
            for ol in 0..out_len {
                let start = ol * stride;
                let window = &row[start..start + kernel_size];
                let mut sum = 0.0_f64;
                for &v in window {
                    sum += v as f64;
                }
                out.push((sum * inv_k) as f32);
            }
        }
        Ok(Self {
            shape: out_shape,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_norm_and_rms_norm_normalize_rows() {
        let t = Tensor::new(vec![2, 4], vec![1.0, 2.0, 3.0, 4.0, 10.0, 10.0, 10.0, 10.0]).unwrap();
        let ln = t.layer_norm(1e-5).unwrap();
        assert_eq!(ln.shape().dims(), &[2, 4]);
        let row0 = &ln.data()[0..4];
        let mean0: f32 = row0.iter().sum::<f32>() / 4.0;
        let var0: f32 = row0.iter().map(|v| v * v).sum::<f32>() / 4.0;
        assert!(mean0.abs() < 1e-6);
        assert!((var0 - 1.0).abs() < 1e-4);
        // Constant row normalizes to 0
        for &v in &ln.data()[4..8] {
            assert!(v.abs() < 1e-6);
        }

        let rms = t.rms_norm(1e-5).unwrap();
        let rms_sq0: f32 = rms.data()[0..4].iter().map(|v| v * v).sum::<f32>() / 4.0;
        assert!((rms_sq0 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn gelu_and_silu_match_reference_values() {
        let t = Tensor::new(vec![3], vec![-2.0, 0.0, 2.0]).unwrap();
        let g = t.gelu();
        let s = t.silu();
        assert!(g.data()[1].abs() < 1e-7);
        assert!(s.data()[1].abs() < 1e-7);
        assert!(g.data()[2] > 1.9 && g.data()[2] < 2.0);
        assert!(s.data()[2] > 1.7 && s.data()[2] < 1.8);
        assert!(g.data()[0] < 0.0 && g.data()[0] > -0.1);
    }

    #[test]
    fn cosine_similarity_and_topk_compute_expected_outputs() {
        let a = Tensor::new(vec![2, 3], vec![1.0, 0.0, 0.0, 1.0, 2.0, 2.0]).unwrap();
        let b = Tensor::new(vec![2, 3], vec![0.0, 1.0, 0.0, 2.0, 4.0, 4.0]).unwrap();
        let sim = a.cosine_similarity(&b, 1e-8).unwrap();
        assert_eq!(sim.shape().dims(), &[2]);
        assert!(sim.data()[0].abs() < 1e-6);
        assert!((sim.data()[1] - 1.0).abs() < 1e-6);

        let top = a.topk(2).unwrap();
        assert_eq!(top.values.shape().dims(), &[2, 2]);
        assert_eq!(top.values.data(), &[1.0, 0.0, 2.0, 2.0]);
        assert_eq!(top.indices, vec![0, 1, 1, 2]);
    }

    #[test]
    fn softmax_log_softmax_clamp_where_and_l2_normalize_work() {
        let t = Tensor::new(vec![2, 3], vec![1.0, 2.0, 3.0, 1000.0, 1000.0, 1000.0]).unwrap();
        let sm = t.softmax(1).unwrap();
        let row0_sum: f32 = sm.data()[0..3].iter().sum();
        assert!((row0_sum - 1.0).abs() < 1e-6);
        for &p in &sm.data()[3..6] {
            assert!((p - 1.0 / 3.0).abs() < 1e-6);
        }

        let lsm = t.log_softmax(1).unwrap();
        for (s, ls) in sm.data().iter().zip(lsm.data().iter()) {
            assert!((s.ln() - *ls).abs() < 1e-5);
        }

        let clamped = t.clamp(1.5, 2.5);
        assert_eq!(&clamped.data()[0..3], &[1.5, 2.0, 2.5]);

        let a = Tensor::new(vec![4], vec![10.0, 20.0, 30.0, 40.0]).unwrap();
        let b = Tensor::new(vec![4], vec![-1.0, -2.0, -3.0, -4.0]).unwrap();
        let sel = Tensor::where_select(&[true, false, true, false], &a, &b).unwrap();
        assert_eq!(sel.data(), &[10.0, -2.0, 30.0, -4.0]);

        let v = Tensor::new(vec![2, 2], vec![3.0, 4.0, 0.0, 0.0]).unwrap();
        let norm = v.l2_normalize(1, 1e-8).unwrap();
        assert!((norm.data()[0] - 0.6).abs() < 1e-6);
        assert!((norm.data()[1] - 0.8).abs() < 1e-6);
        assert_eq!(&norm.data()[2..4], &[0.0, 0.0]);
    }

    #[test]
    fn scaled_dot_product_attention_conv1d_and_max_pool1d_work() {
        // 2D attention: Q, K, V of shape [2, 2]
        let q = Tensor::new(vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]).unwrap();
        let k = Tensor::new(vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]).unwrap();
        let v = Tensor::new(vec![2, 2], vec![10.0, 20.0, 30.0, 40.0]).unwrap();
        let attn = Tensor::scaled_dot_product_attention(&q, &k, &v, false).unwrap();
        assert_eq!(attn.shape().dims(), &[2, 2]);
        // Query 0 aligns more strongly with Key 0 ([10, 20]) than Key 1 ([30, 40])
        assert!(attn.data()[0] < 20.0 && attn.data()[0] > 10.0);

        // Causal attention: row 0 can only attend to key 0, so output row 0 equals V row 0 exactly!
        let causal_attn = Tensor::scaled_dot_product_attention(&q, &k, &v, true).unwrap();
        assert!((causal_attn.data()[0] - 10.0).abs() < 1e-5);
        assert!((causal_attn.data()[1] - 20.0).abs() < 1e-5);

        // 1D convolution: input [1, 1, 5], weight [1, 1, 3], bias [0.5], stride 1, padding 1
        let x = Tensor::new(vec![1, 1, 5], vec![1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        let w = Tensor::new(vec![1, 1, 3], vec![1.0, 1.0, 1.0]).unwrap();
        let conv = x.conv1d(&w, Some(&[0.5]), 1, 1).unwrap();
        assert_eq!(conv.shape().dims(), &[1, 1, 5]);
        assert_eq!(conv.data(), &[3.5, 6.5, 9.5, 12.5, 9.5]);

        // 1D max pooling: window 2, stride 2 on [1, 1, 4]
        let p_in = Tensor::new(vec![1, 1, 4], vec![1.0, 5.0, 3.0, 2.0]).unwrap();
        let pooled = p_in.max_pool1d(2, 2).unwrap();
        assert_eq!(pooled.shape().dims(), &[1, 1, 2]);
        assert_eq!(pooled.data(), &[5.0, 3.0]);

        // 1D avg pooling: window 2, stride 2 on [1, 1, 4]
        let avg_pooled = p_in.avg_pool1d(2, 2).unwrap();
        assert_eq!(avg_pooled.shape().dims(), &[1, 1, 2]);
        assert_eq!(avg_pooled.data(), &[3.0, 2.5]);
    }

    #[test]
    fn batch_norm_1d_and_apply_rotary_pos_emb_work() {
        // [N=2, C=2, L=2]
        let x = Tensor::new(
            vec![2, 2, 2],
            vec![
                1.0, 3.0, // n=0, c=0
                10.0, 20.0, // n=0, c=1
                5.0, 7.0, // n=1, c=0
                30.0, 40.0, // n=1, c=1
            ],
        )
        .unwrap();
        let mean = [1.0, 10.0];
        let var = [4.0, 100.0];
        let weight = [2.0, 5.0];
        let bias = [0.5, -1.0];
        let bn = x
            .batch_norm_1d(&mean, &var, Some(&weight), Some(&bias), 1e-6)
            .unwrap();
        assert_eq!(bn.shape().dims(), &[2, 2, 2]);
        // For c=0: ((x - 1) / 2) * 2 + 0.5 = x - 0.5 -> [0.5, 2.5]
        assert!((bn.data()[0] - 0.5).abs() < 1e-4);
        assert!((bn.data()[1] - 2.5).abs() < 1e-4);
        // For c=1: ((x - 10) / 10) * 5 - 1 = 0.5*(x - 10) - 1 -> [-1.0, 4.0]
        assert!((bn.data()[2] - (-1.0)).abs() < 1e-4);
        assert!((bn.data()[3] - 4.0).abs() < 1e-4);

        // RoPE on [seq_len=2, head_dim=4]
        let rope_in = Tensor::new(
            vec![2, 4],
            vec![
                1.0, 0.0, 2.0, 0.0, // m=0 (angle=0 -> unchanged)
                1.0, 0.0, 0.0, 1.0, // m=1 (pair 0 rotated by 1.0 rad)
            ],
        )
        .unwrap();
        let rope_out = rope_in.apply_rotary_pos_emb(10000.0).unwrap();
        assert_eq!(rope_out.shape().dims(), &[2, 4]);
        // Position m=0 is unrotated
        assert!((rope_out.data()[0] - 1.0).abs() < 1e-6);
        assert!((rope_out.data()[1] - 0.0).abs() < 1e-6);
        assert!((rope_out.data()[2] - 2.0).abs() < 1e-6);
        // Position m=1, pair 0 has theta = 1.0 rad -> (cos(1), sin(1))
        assert!((rope_out.data()[4] - 1.0_f32.cos()).abs() < 1e-6);
        assert!((rope_out.data()[5] - 1.0_f32.sin()).abs() < 1e-6);
        // RoPE preserves L2 norm of each position vector
        let norm_in: f32 = rope_in.data()[4..8].iter().map(|v| v * v).sum();
        let norm_out: f32 = rope_out.data()[4..8].iter().map(|v| v * v).sum();
        assert!((norm_in - norm_out).abs() < 1e-5);

        // Group normalization on [N=1, C=4, L=2] with num_groups=2 (each group has 2*2 = 4 elements)
        let gn_in = Tensor::new(
            vec![1, 4, 2],
            vec![
                1.0, 2.0, 3.0, 4.0, // group 0
                10.0, 10.0, 10.0, 10.0, // group 1 (constant -> 0)
            ],
        )
        .unwrap();
        let gn_out = gn_in.group_norm(2, 1e-5).unwrap();
        let g0 = &gn_out.data()[0..4];
        let g0_mean: f32 = g0.iter().sum::<f32>() / 4.0;
        let g0_var: f32 = g0.iter().map(|v| v * v).sum::<f32>() / 4.0;
        assert!(g0_mean.abs() < 1e-6);
        assert!((g0_var - 1.0).abs() < 1e-4);
        for &v in &gn_out.data()[4..8] {
            assert!(v.abs() < 1e-6);
        }

        // Fused SwiGLU matches self.silu() * gate
        let x_val = Tensor::new(vec![3], vec![-1.0, 0.0, 2.0]).unwrap();
        let g_val = Tensor::new(vec![3], vec![2.0, 5.0, 0.5]).unwrap();
        let sw = x_val.swiglu(&g_val).unwrap();
        let silu_ref = x_val.silu();
        for i in 0..3 {
            assert!((sw.data()[i] - silu_ref.data()[i] * g_val.data()[i]).abs() < 1e-6);
        }
    }
}
