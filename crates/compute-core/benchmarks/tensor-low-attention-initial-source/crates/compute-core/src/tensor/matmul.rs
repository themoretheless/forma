use super::{TensorComputeError, checked_index};
use tensor_core::{Layout, MatmulPlan, Shape};

/// Shared layout-only lowering for f32 and packed low-precision matrix kernels.
pub(super) fn matmul_metadata(
    a: &Layout,
    b: &Layout,
    output: &Layout,
    max_groups: u32,
) -> Result<(Vec<u32>, u32), TensorComputeError> {
    let plan = MatmulPlan::new(a.shape(), b.shape())?;
    if plan.output.numel() == 0 {
        return Ok((Vec::new(), 0));
    }
    let shape = &plan.matrix_output;
    let rank = shape.rank();
    let batch_rank = rank - 2;
    let m = checked_index(shape.dims()[rank - 2])?;
    let n = checked_index(shape.dims()[rank - 1])?;
    let k = checked_index(a.shape().dims()[a.shape().rank() - 1])?;
    let mut a_dims = shape.dims()[..batch_rank].to_vec();
    a_dims.extend([m as usize, k as usize]);
    let mut b_dims = shape.dims()[..batch_rank].to_vec();
    b_dims.extend([k as usize, n as usize]);
    // Adding a length-one axis does not address new storage. Preserve a
    // strided vector's step rather than requiring a contiguous reshape.
    let promoted_a = if a.shape().rank() == 1 {
        Layout::new(plan.left, vec![0, a.strides()[0]], a.offset())?
    } else {
        a.clone()
    };
    let promoted_b = if b.shape().rank() == 1 {
        Layout::new(plan.right, vec![b.strides()[0], 0], b.offset())?
    } else {
        b.clone()
    };
    let av = promoted_a.broadcast_to(Shape::new(a_dims)?)?;
    let bv = promoted_b.broadcast_to(Shape::new(b_dims)?)?;
    let batches = Shape::new(shape.dims()[..batch_rank].to_vec())?.numel();
    let tiles_m = m.div_ceil(16);
    let tiles_n = n.div_ceil(16);
    let tiles = checked_index(
        batches
            .checked_mul(tiles_m as usize)
            .and_then(|x| x.checked_mul(tiles_n as usize))
            .ok_or(TensorComputeError::IndexTooLarge)?,
    )?;
    let groups = tiles.min(65535).min(max_groups);
    let mut meta = vec![
        m,
        n,
        k,
        checked_index(batch_rank)?,
        checked_index(av.offset())?,
        checked_index(bv.offset())?,
        checked_index(output.offset())?,
        tiles,
        tiles_m,
        tiles_n,
        groups,
        checked_index(av.strides()[batch_rank])?,
        checked_index(av.strides()[batch_rank + 1])?,
        checked_index(bv.strides()[batch_rank])?,
        checked_index(bv.strides()[batch_rank + 1])?,
        0,
    ];
    for axis in 0..batch_rank {
        meta.extend([
            checked_index(shape.dims()[axis])?,
            checked_index(av.strides()[axis])?,
            checked_index(bv.strides()[axis])?,
        ]);
    }
    Ok((meta, groups))
}
