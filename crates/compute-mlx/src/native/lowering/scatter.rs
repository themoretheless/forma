//! One resident scatter recipe for eager execution and compiled replay.
use super::{indexing::*, *};
use tensor_core::{ScatterOp, Scattered, scatter_updates_shape};
mod low;
pub(in crate::native) use low::scatter_low;

pub(in crate::native) fn scatter<L: Lowering>(
    g: &mut L,
    op: ScatterOp,
    input: L::Value,
    indices: L::Value,
    updates: L::Value,
    axis: usize,
) -> Result<Scattered<L::Value, L::Value>, MlxError> {
    let (input_spec, update_spec) = matching(g, &input, &updates)?;
    let index_spec = require(g, &indices, MlxDtype::U32)?;
    let update_shape = scatter_updates_shape(
        &input_spec.shape,
        &index_spec.shape,
        &update_spec.shape,
        axis,
    )?;
    MlxBackend::dimensions(&update_shape)?;
    let checked = checked_indices(g, indices.clone(), input_spec.shape.dims()[axis])?;
    // Count original indices independently of empty base slices. Native
    // take/scatter must not receive an empty axis or an empty winner list.
    if input_spec.shape.is_empty() || index_spec.shape.is_empty() {
        return Ok(Scattered {
            values: input,
            invalid_count: checked.invalid_count,
        });
    }
    let updates = broadcast(g, updates, update_shape.clone())?;
    let values = if op == ScatterOp::Replace {
        replace(g, input, &checked, updates, axis)?
    } else {
        let mut mask_dims = vec![1; axis];
        mask_dims.extend_from_slice(index_spec.shape.dims());
        mask_dims.resize(update_shape.rank(), 1);
        let valid = reshape(g, checked.valid, Shape::new(mask_dims)?)?;
        let identity = identity(g, op, input_spec.dtype)?;
        let updates = select(g, valid, updates, identity)?;
        // Native update slices place index axes first and keep the indexed
        // axis as length one; the shared API inserts index axes at `axis`.
        let order: Vec<_> = (axis..axis + index_spec.shape.rank())
            .chain(0..axis)
            .chain(axis + index_spec.shape.rank()..update_shape.rank())
            .collect();
        let updates = permute(g, updates, &order)?;
        let mut dims = g.spec(&updates)?.shape.dims().to_vec();
        dims.insert(index_spec.shape.rank() + axis, 1);
        let updates = reshape(g, updates, Shape::new(dims)?)?;
        g.native(
            NativeOp::Scatter {
                op,
                axis: axis as i32,
            },
            &[input, checked.safe, updates],
            input_spec,
        )?
    };
    Ok(Scattered {
        values,
        invalid_count: checked.invalid_count,
    })
}

fn identity<L: Lowering>(g: &mut L, op: ScatterOp, dtype: MlxDtype) -> Result<L::Value, MlxError> {
    let bits = match dtype {
        MlxDtype::F16 | MlxDtype::Bf16 => return Err(MlxError::Dtype),
        MlxDtype::F32 => match op {
            // Redirected invalid adds preserve a negative-zero base.
            ScatterOp::Add => -0.0f32,
            ScatterOp::Multiply => 1.,
            ScatterOp::Min => f32::MAX,
            ScatterOp::Max => -f32::MAX,
            ScatterOp::Replace => unreachable!(),
        }
        .to_bits(),
        MlxDtype::U32 => match op {
            ScatterOp::Add | ScatterOp::Max => 0,
            ScatterOp::Multiply => 1,
            ScatterOp::Min => u32::MAX,
            ScatterOp::Replace => unreachable!(),
        },
    };
    let value = scalar_u32(g, bits)?;
    if dtype == MlxDtype::U32 {
        return Ok(value);
    }
    g.native(
        NativeOp::ViewF32,
        &[value],
        TensorSpec {
            shape: Shape::new(vec![])?,
            dtype,
        },
    )
}

fn replace<L: Lowering>(
    g: &mut L,
    input: L::Value,
    checked: &CheckedIndices<L::Value>,
    updates: L::Value,
    axis: usize,
) -> Result<L::Value, MlxError> {
    let input_spec = g.spec(&input)?;
    let n = g.spec(&checked.safe)?.shape.numel();
    let flat = Shape::new(vec![n])?;
    MlxBackend::dimensions(&flat)?;
    let positions = g.native(
        NativeOp::ArangeU32(n),
        &[],
        TensorSpec {
            shape: flat.clone(),
            dtype: MlxDtype::U32,
        },
    )?;
    let zero = scalar_u32(g, 0)?;
    let one = scalar_u32(g, 1)?;
    let positions = binary_u32(g, positions, one.clone(), BinaryOp::Add)?;
    let valid = reshape(g, checked.valid.clone(), flat.clone())?;
    let positions = select(g, valid, positions, zero.clone())?;
    let positions = reshape(g, positions, Shape::new(vec![n, 1])?)?;
    let indices = reshape(g, checked.safe.clone(), flat)?;
    let extent = input_spec.shape.dims()[axis];
    let owners_shape = Shape::new(vec![extent])?;
    let owners = zeros(g, owners_shape.clone(), MlxDtype::U32)?;
    let owners = g.native(
        NativeOp::Scatter {
            op: ScatterOp::Max,
            axis: 0,
        },
        &[owners, indices, positions],
        TensorSpec {
            shape: owners_shape,
            dtype: MlxDtype::U32,
        },
    )?;
    // Largest one-based logical position wins deterministically. Zero means
    // untouched, and must be sanitized before subtracting one/taking updates.
    let touched = compare(g, owners.clone(), zero, CompareOp::Greater)?;
    let winner = binary_u32(g, owners, one.clone(), BinaryOp::Max)?;
    let winner = binary_u32(g, winner, one, BinaryOp::Subtract)?;
    let mut dims = input_spec.shape.dims().to_vec();
    dims[axis] = n;
    let updates = reshape(g, updates, Shape::new(dims)?)?;
    let selected = g.native(
        NativeOp::TakeAxis(axis as i32),
        &[updates, winner],
        input_spec.clone(),
    )?;
    let mut dims = vec![1; input_spec.shape.rank()];
    dims[axis] = extent;
    let touched = reshape(g, touched, Shape::new(dims)?)?;
    select(g, touched, selected, input)
}
