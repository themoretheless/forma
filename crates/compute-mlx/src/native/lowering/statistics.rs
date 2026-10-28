//! Stable f32 statistics recipes shared by eager execution and compiled traces.
use super::*;
use tensor_core::{Moments, normalization_shape, statistics_shape, validate_epsilon};

pub(super) fn f32_spec(shape: Shape) -> TensorSpec {
    TensorSpec {
        shape,
        dtype: MlxDtype::F32,
    }
}
pub(super) fn scalar<L: Lowering>(graph: &mut L, value: f32) -> Result<L::Value, MlxError> {
    let shape = Shape::new(vec![])?;
    let bits = graph.constant_u32(shape.clone(), &[value.to_bits()])?;
    graph.native(NativeOp::ViewF32, &[bits], f32_spec(shape))
}
pub(super) fn zeros<L: Lowering>(graph: &mut L, shape: Shape) -> Result<L::Value, MlxError> {
    graph.native(NativeOp::Zeros, &[], f32_spec(shape))
}
pub(super) fn axes_i32(axes: &[usize]) -> Result<Vec<i32>, MlxError> {
    axes.iter()
        .map(|&a| i32::try_from(a).map_err(|_| MlxError::TooLarge))
        .collect()
}
pub(super) fn reshape<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    shape: Shape,
) -> Result<L::Value, MlxError> {
    let dtype = graph.spec(&input)?.dtype;
    graph.native(NativeOp::Reshape, &[input], TensorSpec { shape, dtype })
}
fn require<L: Lowering>(graph: &L, input: &L::Value) -> Result<TensorSpec, MlxError> {
    let spec = graph.spec(input)?;
    if spec.dtype != MlxDtype::F32 {
        return Err(MlxError::Dtype);
    }
    Ok(spec)
}
fn unary<L: Lowering>(graph: &mut L, op: UnaryOp, value: &L::Value) -> Result<L::Value, MlxError> {
    let spec = graph.spec(value)?;
    graph.native(NativeOp::Unary(op), std::slice::from_ref(value), spec)
}
fn binary<L: Lowering>(
    graph: &mut L,
    op: BinaryOp,
    left: &L::Value,
    right: &L::Value,
) -> Result<L::Value, MlxError> {
    let shape = graph
        .spec(left)?
        .shape
        .broadcast(&graph.spec(right)?.shape)?;
    graph.native(
        NativeOp::Binary(op),
        &[left.clone(), right.clone()],
        f32_spec(shape),
    )
}
fn compare<L: Lowering>(
    graph: &mut L,
    op: CompareOp,
    left: &L::Value,
    right: &L::Value,
) -> Result<L::Value, MlxError> {
    let shape = graph
        .spec(left)?
        .shape
        .broadcast(&graph.spec(right)?.shape)?;
    graph.native(
        NativeOp::Compare(op),
        &[left.clone(), right.clone()],
        TensorSpec {
            shape,
            dtype: MlxDtype::U32,
        },
    )
}
fn select<L: Lowering>(
    graph: &mut L,
    mask: &L::Value,
    yes: &L::Value,
    no: &L::Value,
) -> Result<L::Value, MlxError> {
    let shape = tensor_core::select_shape(
        &graph.spec(mask)?.shape,
        &graph.spec(yes)?.shape,
        &graph.spec(no)?.shape,
    )?;
    graph.native(
        NativeOp::Select,
        &[mask.clone(), yes.clone(), no.clone()],
        f32_spec(shape),
    )
}
fn reduce<L: Lowering>(
    graph: &mut L,
    input: &L::Value,
    axes: &[usize],
    op: ReduceOp,
    mean: bool,
) -> Result<L::Value, MlxError> {
    let shape = graph.spec(input)?.shape.reduce(axes, true)?;
    let axes = axes_i32(axes)?;
    let op = if mean {
        NativeOp::Mean(axes, true)
    } else {
        NativeOp::Reduce(op, axes, true)
    };
    graph.native(op, std::slice::from_ref(input), f32_spec(shape))
}
struct ScaledMoments<V> {
    anchor: V,
    scale: V,
    mean: V,
    centered: V,
    variance: V,
}
fn scaled_moments<L: Lowering>(
    graph: &mut L,
    input: &L::Value,
    axes: &[usize],
) -> Result<ScaledMoments<L::Value>, MlxError> {
    let lo = reduce(graph, input, axes, ReduceOp::Min, false)?;
    let hi = reduce(graph, input, axes, ReduceOp::Max, false)?;
    let half = scalar(graph, 0.5)?;
    // Keep the established midpoint and centered scaling recipe in both routes.
    let lower_half = binary(graph, BinaryOp::Multiply, &lo, &half)?;
    let upper_half = binary(graph, BinaryOp::Multiply, &hi, &half)?;
    let anchor = binary(graph, BinaryOp::Add, &lower_half, &upper_half)?;
    let lower = binary(graph, BinaryOp::Subtract, &lo, &anchor)?;
    let lower = unary(graph, UnaryOp::Abs, &lower)?;
    let upper = binary(graph, BinaryOp::Subtract, &hi, &anchor)?;
    let upper = unary(graph, UnaryOp::Abs, &upper)?;
    let scale = binary(graph, BinaryOp::Max, &lower, &upper)?;
    let zero = scalar(graph, 0.)?;
    let one = scalar(graph, 1.)?;
    let constant = compare(graph, CompareOp::Equal, &scale, &zero)?;
    let safe_scale = select(graph, &constant, &one, &scale)?;
    let shifted = binary(graph, BinaryOp::Subtract, input, &anchor)?;
    let scaled = binary(graph, BinaryOp::Divide, &shifted, &safe_scale)?;
    let mean = reduce(graph, &scaled, axes, ReduceOp::Sum, true)?;
    let centered = binary(graph, BinaryOp::Subtract, &scaled, &mean)?;
    let squared = unary(graph, UnaryOp::Square, &centered)?;
    let variance = reduce(graph, &squared, axes, ReduceOp::Sum, true)?;
    Ok(ScaledMoments {
        anchor,
        scale,
        mean,
        centered,
        variance,
    })
}
pub(in crate::native) fn softmax<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
) -> Result<L::Value, MlxError> {
    let shape = normalization_shape(&require(graph, &input)?.shape, axes)?;
    if shape.is_empty() {
        return zeros(graph, shape);
    }
    if axes.is_empty() {
        return graph.native(NativeOp::Ones, &[], f32_spec(shape));
    }
    graph.native(
        NativeOp::Softmax(axes_i32(axes)?),
        &[input],
        f32_spec(shape),
    )
}
pub(in crate::native) fn log_softmax<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
) -> Result<L::Value, MlxError> {
    let shape = normalization_shape(&require(graph, &input)?.shape, axes)?;
    if shape.is_empty() || axes.is_empty() {
        return zeros(graph, shape);
    }
    let max = reduce(graph, &input, axes, ReduceOp::Max, false)?;
    let shifted = binary(graph, BinaryOp::Subtract, &input, &max)?;
    let normalizer = logsumexp(graph, shifted.clone(), axes, true)?;
    binary(graph, BinaryOp::Subtract, &shifted, &normalizer)
}
pub(in crate::native) fn logsumexp<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
    keep: bool,
) -> Result<L::Value, MlxError> {
    let shape = statistics_shape(&require(graph, &input)?.shape, axes, keep)?;
    if axes.is_empty() {
        return Ok(input);
    }
    if shape.is_empty() {
        return zeros(graph, shape);
    }
    graph.native(
        NativeOp::Logsumexp(axes_i32(axes)?, keep),
        &[input],
        f32_spec(shape),
    )
}
pub(in crate::native) fn moments<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
    keep: bool,
) -> Result<Moments<L::Value>, MlxError> {
    let shape = statistics_shape(&require(graph, &input)?.shape, axes, keep)?;
    if axes.is_empty() {
        return Ok(Moments {
            mean: input,
            variance: zeros(graph, shape)?,
        });
    }
    if shape.is_empty() {
        let empty = zeros(graph, shape)?;
        return Ok(Moments {
            mean: empty.clone(),
            variance: empty,
        });
    }
    let stats = scaled_moments(graph, &input, axes)?;
    let scaled_mean = binary(graph, BinaryOp::Multiply, &stats.scale, &stats.mean)?;
    let mean = binary(graph, BinaryOp::Add, &stats.anchor, &scaled_mean)?;
    // Apply normalized variance before the second scale multiplication.
    let variance = binary(graph, BinaryOp::Multiply, &stats.scale, &stats.variance)?;
    let variance = binary(graph, BinaryOp::Multiply, &stats.scale, &variance)?;
    Ok(Moments {
        mean: reshape(graph, mean, shape.clone())?,
        variance: reshape(graph, variance, shape)?,
    })
}
pub(in crate::native) fn layer_norm<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
    epsilon: f32,
) -> Result<L::Value, MlxError> {
    let shape = normalization_shape(&require(graph, &input)?.shape, axes)?;
    validate_epsilon(epsilon)?;
    if shape.is_empty() || axes.is_empty() {
        return zeros(graph, shape);
    }
    let stats = scaled_moments(graph, &input, axes)?;
    let root_epsilon = scalar(graph, epsilon.sqrt())?;
    let one = scalar(graph, 1.)?;
    let large = compare(graph, CompareOp::GreaterEqual, &stats.scale, &root_epsilon)?;
    // Both where branches evaluate, so both denominators must stay valid.
    let large_scale = select(graph, &large, &stats.scale, &root_epsilon)?;
    let ratio = binary(graph, BinaryOp::Divide, &root_epsilon, &large_scale)?;
    let squared = unary(graph, UnaryOp::Square, &ratio)?;
    let denominator = binary(graph, BinaryOp::Add, &stats.variance, &squared)?;
    let denominator = unary(graph, UnaryOp::Sqrt, &denominator)?;
    let large_result = binary(graph, BinaryOp::Divide, &stats.centered, &denominator)?;
    let small_scale = select(graph, &large, &root_epsilon, &stats.scale)?;
    let ratio = binary(graph, BinaryOp::Divide, &small_scale, &root_epsilon)?;
    let numerator = binary(graph, BinaryOp::Multiply, &stats.centered, &ratio)?;
    let squared = unary(graph, UnaryOp::Square, &ratio)?;
    let denominator = binary(graph, BinaryOp::Multiply, &stats.variance, &squared)?;
    let denominator = binary(graph, BinaryOp::Add, &one, &denominator)?;
    let denominator = unary(graph, UnaryOp::Sqrt, &denominator)?;
    let small_result = binary(graph, BinaryOp::Divide, &numerator, &denominator)?;
    select(graph, &large, &large_result, &small_result)
}
