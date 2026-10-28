//! Shared host recipes. Eager calls execute them; builders record the same
//! views, parameters and original custom kernels for deferred native tracing.
use super::*;
mod matmul;
use crate::native::low_kernels::{self, LowReductionGeometry, element_launch, reduction_launch};
pub(in crate::native) use matmul::matmul_low_f32;
use tensor_core::{LowDtype, mean_shape, reduction_shape};

fn low_dtype(dtype: MlxDtype) -> Result<LowDtype, MlxError> {
    match dtype {
        MlxDtype::F16 => Ok(LowDtype::F16),
        MlxDtype::Bf16 => Ok(LowDtype::Bf16),
        _ => Err(MlxError::Dtype),
    }
}
fn matching(left: MlxDtype, right: MlxDtype) -> Result<LowDtype, MlxError> {
    let left = low_dtype(left)?;
    let right = low_dtype(right)?;
    if left != right {
        return Err(TensorError::LowDtypeMismatch { left, right }.into());
    }
    Ok(left)
}
fn view<L: Lowering>(graph: &mut L, input: L::Value, shape: Shape) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&input)?;
    MlxBackend::dimensions(&shape)?;
    if spec.shape == shape {
        return Ok(input);
    }
    if spec.shape.broadcast(&shape)? != shape {
        return Err(TensorError::IncompatibleBroadcast {
            left: spec.shape.dims().to_vec(),
            right: shape.dims().to_vec(),
        }
        .into());
    }
    graph.native(
        NativeOp::Broadcast,
        &[input],
        TensorSpec {
            shape,
            dtype: spec.dtype,
        },
    )
}
fn element_view<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    shape: &Shape,
) -> Result<L::Value, MlxError> {
    view(
        graph,
        input,
        if shape.rank() == 0 {
            Shape::new(vec![1])?
        } else {
            shape.clone()
        },
    )
}
pub(in crate::native) fn unary_low<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    op: UnaryOp,
) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&input)?;
    let dtype = low_dtype(spec.dtype)?;
    if spec.shape.is_empty() {
        return graph.native(NativeOp::Zeros, &[], spec);
    }
    let input = element_view(graph, input, &spec.shape)?;
    let launch = element_launch(spec.shape.numel());
    graph.metal(
        low_kernels::key(
            include_str!("../../metal/low_unary.metal").replace("OP", &(op as u32).to_string()),
            &["inp"],
        ),
        &[input],
        spec,
        dtype == LowDtype::Bf16,
        launch,
    )
}
pub(in crate::native) fn binary_low<L: Lowering>(
    graph: &mut L,
    left: L::Value,
    right: L::Value,
    op: BinaryOp,
) -> Result<L::Value, MlxError> {
    let a = graph.spec(&left)?;
    let b = graph.spec(&right)?;
    let dtype = matching(a.dtype, b.dtype)?;
    let spec = TensorSpec {
        shape: a.shape.broadcast(&b.shape)?,
        dtype: a.dtype,
    };
    MlxBackend::dimensions(&spec.shape)?;
    if spec.shape.is_empty() {
        return graph.native(NativeOp::Zeros, &[], spec);
    }
    let left = element_view(graph, left, &spec.shape)?;
    let right = element_view(graph, right, &spec.shape)?;
    let launch = element_launch(spec.shape.numel());
    graph.metal(
        low_kernels::key(
            include_str!("../../metal/low_binary.metal").replace("OP", &(op as u32).to_string()),
            &["lhs", "rhs"],
        ),
        &[left, right],
        spec,
        dtype == LowDtype::Bf16,
        launch,
    )
}
fn parameters<L: Lowering>(
    graph: &mut L,
    count: usize,
    rank: usize,
    parts: usize,
) -> Result<L::Value, MlxError> {
    graph.constant_u32(
        Shape::new(vec![4])?,
        &[
            u32::try_from(rank).map_err(|_| MlxError::TooLarge)?,
            count as u32,
            (count as u64 >> 32) as u32,
            u32::try_from(parts).map_err(|_| MlxError::TooLarge)?,
        ],
    )
}
pub(in crate::native) fn reduce_low<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    op: ReduceOp,
    axes: &[usize],
    keep: bool,
    mean: bool,
) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&input)?;
    let dtype = low_dtype(spec.dtype)?;
    let shape = if mean {
        mean_shape(&spec.shape, axes, keep)?
    } else {
        reduction_shape(op, &spec.shape, axes, keep)?
    };
    MlxBackend::dimensions(&shape)?;
    if axes.is_empty() {
        return casts::cast_to_f32(graph, input);
    }
    let result = TensorSpec {
        shape: shape.clone(),
        dtype: MlxDtype::F32,
    };
    if shape.is_empty() || spec.shape.is_empty() {
        let identity = if !shape.is_empty() && op == ReduceOp::Product {
            NativeOp::Ones
        } else {
            NativeOp::Zeros
        };
        return graph.native(identity, &[], result);
    }
    let plan = LowReductionGeometry::new(&spec.shape, axes)?;
    let input = graph.native(
        NativeOp::Permute(plan.order.iter().map(|&x| x as i32).collect()),
        &[input],
        TensorSpec {
            shape: spec.shape.permute(&plan.order)?,
            dtype: spec.dtype,
        },
    )?;
    let params = parameters(graph, plan.count, plan.row_shape.rank(), plan.parts)?;
    let partial = graph.metal(
        low_kernels::key(
            low_kernels::reduction_source(op, true, mean && plan.parts == 1, false),
            &["inp", "params"],
        ),
        &[input, params],
        TensorSpec {
            shape: plan.partial_shape(&shape)?,
            dtype: MlxDtype::F32,
        },
        dtype == LowDtype::Bf16,
        reduction_launch(plan.rows, plan.parts),
    )?;
    if plan.parts == 1 {
        return Ok(partial);
    }
    let params = parameters(graph, plan.count, plan.row_shape.rank(), 1)?;
    graph.metal(
        low_kernels::key(
            low_kernels::reduction_source(op, false, mean, false),
            &["inp", "params"],
        ),
        &[partial, params],
        result,
        false,
        reduction_launch(plan.rows, 1),
    )
}
