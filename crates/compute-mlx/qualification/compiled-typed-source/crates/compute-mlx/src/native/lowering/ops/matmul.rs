use super::*;
use tensor_core::MatmulPlan;

pub(in crate::native) fn matmul_low_f32<L: Lowering>(
    graph: &mut L,
    left: L::Value,
    right: L::Value,
    plan: MatmulPlan,
) -> Result<L::Value, MlxError> {
    let a = graph.spec(&left)?;
    let b = graph.spec(&right)?;
    let dtype = matching(a.dtype, b.dtype)?;
    MlxBackend::dimensions(&plan.output)?;
    let rank = plan.matrix_output.rank();
    let batch = &plan.matrix_output.dims()[..rank - 2];
    let m = plan.left.dims()[plan.left.rank() - 2];
    let k = plan.left.dims()[plan.left.rank() - 1];
    let n = plan.right.dims()[plan.right.rank() - 1];
    let result = TensorSpec {
        shape: plan.output.clone(),
        dtype: MlxDtype::F32,
    };
    if plan.output.is_empty() || k == 0 {
        return graph.native(NativeOp::Zeros, &[], result);
    }
    let mut dims = batch.to_vec();
    dims.extend([m, k]);
    let left = view(graph, left, Shape::new(dims)?)?;
    let right = if b.shape.rank() == 1 {
        let row = view(graph, right, Shape::new(vec![1, k])?)?;
        graph.native(
            NativeOp::Permute(vec![1, 0]),
            &[row],
            TensorSpec {
                shape: Shape::new(vec![k, 1])?,
                dtype: b.dtype,
            },
        )?
    } else {
        right
    };
    let mut dims = batch.to_vec();
    dims.extend([k, n]);
    let right = view(graph, right, Shape::new(dims)?)?;
    let tiles_m = m.div_ceil(16);
    let tiles_n = n.div_ceil(16);
    let batches = plan.output.numel() / m / n;
    let vector = m == 1 || n == 1;
    let work = if vector {
        plan.output.numel()
    } else {
        batches
            .checked_mul(tiles_m)
            .and_then(|x| x.checked_mul(tiles_n))
            .ok_or(MlxError::TooLarge)?
    };
    let mut words = [m, n, k, tiles_m, tiles_n]
        .into_iter()
        .map(|v| u32::try_from(v).map_err(|_| MlxError::TooLarge))
        .collect::<Result<Vec<_>, _>>()?;
    words.extend([work as u32, (work as u64 >> 32) as u32]);
    let params = graph.constant_u32(Shape::new(vec![words.len()])?, &words)?;
    let threads = if vector { 256 } else { 128 };
    let source = if vector {
        include_str!("../../../metal/low_matvec.metal")
    } else {
        include_str!("../../../metal/low_matmul.metal")
    };
    let mut key = low_kernels::key(source.to_owned(), &["left", "right", "params"]);
    if !vector {
        key.header = concat!(
            "#include <metal_simdgroup_matrix>\n",
            include_str!("../../../metal/low_common.h")
        );
    }
    graph.metal(
        key,
        &[left, right, params],
        result,
        dtype == LowDtype::Bf16,
        (work.min(65535) * threads, threads),
    )
}
