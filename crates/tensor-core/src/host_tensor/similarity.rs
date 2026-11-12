//! Similarity and selection along the last axis.

use super::{Tensor, TopKResult};
use crate::{Shape, TensorError, validate_epsilon};

impl Tensor {
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
        for (a_chunk, b_chunk) in self
            .data
            .chunks(group_len)
            .zip(other.data.chunks(group_len))
        {
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
}
