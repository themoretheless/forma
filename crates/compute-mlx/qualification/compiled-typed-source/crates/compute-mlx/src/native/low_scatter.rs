use super::{low_kernels::*, *};
use tensor_core::{
    LowDtype, ScatterOp, Scattered, TensorLowBackend, TensorLowScatterBackend,
    low_scatter_updates_shape,
};
const LISTS: &str = include_str!("../metal/low_scatter_lists.metal");
const FOLD: &str = include_str!("../metal/low_scatter_fold.metal");

fn list_shape(extent: usize, indices: usize) -> Result<Shape, MlxError> {
    // Tokens are one-based u32 positions, with zero reserved for end-of-list.
    u32::try_from(indices).map_err(|_| MlxError::TooLarge)?;
    u32::try_from(extent).map_err(|_| MlxError::TooLarge)?;
    let words = extent.checked_add(indices).ok_or(MlxError::TooLarge)?;
    let blocks = words.div_ceil(WIDTH);
    let shape = Shape::new(vec![blocks, WIDTH])?;
    MlxBackend::dimensions(&shape)?;
    Ok(shape)
}

impl MlxBackend {
    fn scatter_low_values(
        &self,
        op: ScatterOp,
        input: &MlxLowTensor,
        indices: &MlxTensor,
        updates: &MlxLowTensor,
        axis: usize,
        low_output: bool,
    ) -> Result<Scattered<MlxTensor, MlxTensor>, MlxError> {
        self.check_low(input)?;
        self.check_low(updates)?;
        self.check(indices, Some(MlxDtype::U32))?;
        let update_shape = low_scatter_updates_shape(input, indices.shape(), updates, axis)?;
        Self::dimensions(&update_shape)?;
        if op == ScatterOp::Replace {
            // One-based owner max chooses the greatest logical index, and the
            // common native take/raw-select path preserves every low payload.
            let result = self.scatter_values(op, &input.tensor, indices, &updates.tensor, axis)?;
            return Ok(Scattered {
                values: if low_output {
                    result.values
                } else {
                    self.cast_to_f32(&MlxLowTensor {
                        tensor: result.values,
                        dtype: input.dtype,
                    })?
                },
                invalid_count: result.invalid_count,
            });
        }
        let extent = input.shape().dims()[axis];
        let checked = self.checked_indices(indices, extent)?;
        if input.shape().is_empty() || indices.shape.is_empty() {
            return Ok(Scattered {
                values: if low_output {
                    input.tensor.clone()
                } else {
                    self.cast_to_f32(input)?
                },
                invalid_count: checked.invalid_count,
            });
        }
        let list_shape = list_shape(extent, indices.shape.numel())?;
        let inner = input.shape().dims()[axis + 1..].iter().product::<usize>();
        let params = self.upload_u32(
            Shape::new(vec![4])?,
            &[
                extent as u32,
                indices.shape.numel() as u32,
                inner as u32,
                (inner as u64 >> 32) as u32,
            ],
        )?;
        let index_view = self.low_elementwise_view(indices, indices.shape())?;
        let mut list_key = key(LISTS.into(), &["indices", "params"]);
        list_key.zeroed_atomic_u32 = true;
        let lists = self.custom_metal(
            list_key,
            &[&index_view, &params],
            list_shape,
            MlxDtype::U32,
            input.dtype == LowDtype::Bf16,
            element_launch(indices.shape.numel()),
        )?;
        // Broadcasting changes only metadata; updates are decoded directly
        // from native low storage inside the destination's linked-list fold.
        let updates = self.low_elementwise_view(&updates.tensor, &update_shape)?;
        let store = if low_output {
            "out[i] = as_type<OUT>((OP == 3 || OP == 4) ? selected : low_encode(total, BF));"
        } else {
            "out[i] = (OP == 3 || OP == 4) ? low_decode(selected, BF) : total;"
        };
        let source = FOLD
            .replace("STORE", store)
            .replace("OP", &(op as u32).to_string());
        let values = self.custom_metal(
            key(source, &["base", "updates", "lists", "params"]),
            &[&input.tensor, &updates, &lists, &params],
            input.shape().clone(),
            if low_output {
                input.dtype.into()
            } else {
                MlxDtype::F32
            },
            input.dtype == LowDtype::Bf16,
            element_launch(input.shape().numel()),
        )?;
        Ok(Scattered {
            values,
            invalid_count: checked.invalid_count,
        })
    }
}

impl TensorLowScatterBackend for MlxBackend {
    fn scatter_low(
        &self,
        op: ScatterOp,
        input: &MlxLowTensor,
        indices: &MlxTensor,
        updates: &MlxLowTensor,
        axis: usize,
    ) -> Result<Scattered<MlxLowTensor, MlxTensor>, MlxError> {
        let result = self.scatter_low_values(op, input, indices, updates, axis, true)?;
        Ok(Scattered {
            values: MlxLowTensor {
                tensor: result.values,
                dtype: input.dtype,
            },
            invalid_count: result.invalid_count,
        })
    }
    fn scatter_low_f32(
        &self,
        op: ScatterOp,
        input: &MlxLowTensor,
        indices: &MlxTensor,
        updates: &MlxLowTensor,
        axis: usize,
    ) -> Result<Scattered<MlxTensor, MlxTensor>, MlxError> {
        self.scatter_low_values(op, input, indices, updates, axis, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn list_allocation_pads_safely_and_preserves_u32_token_capacity() {
        let mut cases = vec![(1, 1), (17, 65539)];
        if usize::BITS > 32 {
            cases.push((i32::MAX as usize, u32::MAX as usize));
        }
        for (extent, count) in cases {
            let shape = list_shape(extent, count).unwrap();
            assert!(shape.numel() >= extent + count);
            assert!(shape.numel() - extent - count < WIDTH);
            assert!(shape.dims().iter().all(|&x| x <= i32::MAX as usize));
        }
        if let Some(too_many) = (u32::MAX as usize).checked_add(1) {
            assert!(list_shape(1, too_many).is_err());
            assert!(list_shape(usize::MAX, 1).is_err());
        }
    }
}
