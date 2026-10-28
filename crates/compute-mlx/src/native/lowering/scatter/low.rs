use super::*;
use crate::native::low_kernels::{WIDTH, element_launch, key};
use tensor_core::LowDtype;
const LISTS: &str = include_str!("../../../metal/low_scatter_lists.metal");
const FOLD: &str = include_str!("../../../metal/low_scatter_fold.metal");

fn list_shape(extent: usize, indices: usize) -> Result<Shape, MlxError> {
    // One-based u32 tokens reserve zero for end-of-list. Padding permits a
    // checked multidimensional allocation when total words exceed i32::MAX.
    u32::try_from(indices).map_err(|_| MlxError::TooLarge)?;
    u32::try_from(extent).map_err(|_| MlxError::TooLarge)?;
    let words = extent.checked_add(indices).ok_or(MlxError::TooLarge)?;
    let shape = Shape::new(vec![words.div_ceil(WIDTH), WIDTH])?;
    MlxBackend::dimensions(&shape)?;
    Ok(shape)
}

pub(in crate::native) fn scatter_low<L: Lowering>(
    g: &mut L,
    op: ScatterOp,
    input: L::Value,
    indices: L::Value,
    updates: L::Value,
    axis: usize,
    low_output: bool,
) -> Result<Scattered<L::Value, L::Value>, MlxError> {
    let (input_spec, update_spec) = matching(g, &input, &updates)?;
    let dtype = match input_spec.dtype {
        MlxDtype::F16 => LowDtype::F16,
        MlxDtype::Bf16 => LowDtype::Bf16,
        _ => return Err(MlxError::Dtype),
    };
    let index_spec = require(g, &indices, MlxDtype::U32)?;
    let update_shape = scatter_updates_shape(
        &input_spec.shape,
        &index_spec.shape,
        &update_spec.shape,
        axis,
    )?;
    MlxBackend::dimensions(&update_shape)?;
    if op == ScatterOp::Replace {
        let result = scatter(g, op, input, indices, updates, axis)?;
        return Ok(Scattered {
            values: if low_output {
                result.values
            } else {
                casts::cast_to_f32(g, result.values)?
            },
            invalid_count: result.invalid_count,
        });
    }
    let extent = input_spec.shape.dims()[axis];
    let checked = checked_indices(g, indices.clone(), extent)?;
    if input_spec.shape.is_empty() || index_spec.shape.is_empty() {
        return Ok(Scattered {
            values: if low_output {
                input
            } else {
                casts::cast_to_f32(g, input)?
            },
            invalid_count: checked.invalid_count,
        });
    }
    let lists_shape = list_shape(extent, index_spec.shape.numel())?;
    let inner = input_spec.shape.dims()[axis + 1..]
        .iter()
        .product::<usize>();
    let params = g.constant_u32(
        Shape::new(vec![4])?,
        &[
            extent as u32,
            index_spec.shape.numel() as u32,
            inner as u32,
            (inner as u64 >> 32) as u32,
        ],
    )?;
    let index_view = element_view(g, indices, &index_spec.shape)?;
    let mut list_key = key(LISTS.into(), &["indices", "params"]);
    list_key.zeroed_atomic_u32 = true;
    // Native custom-op init_value=0 is part of every replayed primitive. The
    // compiled payload holds the kernel/config, not a prior evaluated list.
    let lists = g.metal(
        list_key,
        &[index_view, params.clone()],
        TensorSpec {
            shape: lists_shape,
            dtype: MlxDtype::U32,
        },
        dtype == LowDtype::Bf16,
        element_launch(index_spec.shape.numel()),
    )?;
    let updates = element_view(g, updates, &update_shape)?;
    let store = if low_output {
        "out[i] = as_type<OUT>((OP == 3 || OP == 4) ? selected : low_encode(total, BF));"
    } else {
        "out[i] = (OP == 3 || OP == 4) ? low_decode(selected, BF) : total;"
    };
    let source = FOLD
        .replace("STORE", store)
        .replace("OP", &(op as u32).to_string());
    let values = g.metal(
        key(source, &["base", "updates", "lists", "params"]),
        &[input, updates, lists, params],
        TensorSpec {
            shape: input_spec.shape.clone(),
            dtype: if low_output {
                input_spec.dtype
            } else {
                MlxDtype::F32
            },
        },
        dtype == LowDtype::Bf16,
        element_launch(input_spec.shape.numel()),
    )?;
    Ok(Scattered {
        values,
        invalid_count: checked.invalid_count,
    })
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
