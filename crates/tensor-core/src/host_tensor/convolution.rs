//! 1D convolution and pooling.

use super::Tensor;
use crate::{Shape, TensorError};

impl Tensor {
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
}
