//! Direct low-input statistics. The same Metal sources, row geometry, and f32
//! summary passes serve eager graphs and compiled callbacks.
use super::{casts, ops, statistics::*, *};
use crate::native::low_kernels::{self, LowReductionGeometry, element_launch, reduction_launch};
use tensor_core::{Moments, normalization_shape, statistics_shape, validate_epsilon};

const HEADER: &str = concat!(
    include_str!("../../metal/low_common.h"),
    "\n",
    include_str!("../../metal/low_stats_common.h")
);
const STATE: &str = include_str!("../../metal/low_stats_state.metal");
const EMIT: &str = include_str!("../../metal/low_stats_emit.metal");
const ROWS: &str = include_str!("../../metal/low_stats_rows.metal");
fn key(source: String, inputs: &'static [&'static str]) -> KernelKey {
    let mut key = low_kernels::key(source, inputs);
    key.header = HEADER;
    key
}
fn require<L: Lowering>(graph: &L, input: &L::Value) -> Result<TensorSpec, MlxError> {
    let spec = graph.spec(input)?;
    if !matches!(spec.dtype, MlxDtype::F16 | MlxDtype::Bf16) {
        return Err(MlxError::Dtype);
    }
    Ok(spec)
}
fn singleton(shape: &Shape, axes: &[usize]) -> bool {
    axes.iter().all(|&axis| shape.dims()[axis] == 1)
}
struct Plan<V> {
    input: V,
    spec: TensorSpec,
    geometry: LowReductionGeometry,
}
impl<V> std::ops::Deref for Plan<V> {
    type Target = LowReductionGeometry;
    fn deref(&self) -> &Self::Target {
        &self.geometry
    }
}
fn plan<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
) -> Result<Plan<L::Value>, MlxError> {
    let spec = require(graph, &input)?;
    let geometry = LowReductionGeometry::new(&spec.shape, axes)?;
    let spec = TensorSpec {
        shape: spec.shape.permute(&geometry.order)?,
        dtype: spec.dtype,
    };
    let input = graph.native(
        NativeOp::Permute(axes_i32(&geometry.order)?),
        &[input],
        spec.clone(),
    )?;
    Ok(Plan {
        input,
        spec,
        geometry,
    })
}
fn element_view<L: Lowering>(graph: &mut L, input: &L::Value) -> Result<L::Value, MlxError> {
    let spec = graph.spec(input)?;
    if spec.shape.rank() != 0 {
        return Ok(input.clone());
    }
    graph.native(
        NativeOp::Broadcast,
        std::slice::from_ref(input),
        TensorSpec {
            shape: Shape::new(vec![1])?,
            dtype: spec.dtype,
        },
    )
}
fn params<L: Lowering>(
    graph: &mut L,
    plan: &Plan<L::Value>,
    parts: usize,
) -> Result<L::Value, MlxError> {
    graph.constant_u32(
        Shape::new(vec![4])?,
        &[
            u32::try_from(plan.row_shape.rank()).map_err(|_| MlxError::TooLarge)?,
            plan.count as u32,
            (plan.count as u64 >> 32) as u32,
            u32::try_from(parts).map_err(|_| MlxError::TooLarge)?,
        ],
    )
}
fn reduce<L: Lowering>(
    graph: &mut L,
    plan: &Plan<L::Value>,
    state: &L::Value,
    center: &L::Value,
    mode: u32,
) -> Result<L::Value, MlxError> {
    let (prepare, transform) = match mode {
        0 => ("", "value = metal::precise::exp(value - state[row]);"),
        1 => (
            "float anchor = state[row * 4], scale = state[row * 4 + 1]; bool lifted = state[row * 4 + 2] != 0.0f;",
            "if (lifted) value = low_power2(value, 64); value = stats_scaled(value, anchor, scale);",
        ),
        _ => (
            "float anchor = state[row * 4], scale = state[row * 4 + 1]; bool lifted = state[row * 4 + 2] != 0.0f;",
            "if (lifted) value = low_power2(value, 64); value = stats_scaled(value, anchor, scale) - center[row]; value *= value;",
        ),
    };
    let mean = mode != 0;
    let parameters = params(graph, plan, plan.parts)?;
    let state = element_view(graph, state)?;
    let center = element_view(graph, center)?;
    let source = low_kernels::transformed_reduction_source(
        ReduceOp::Sum,
        true,
        mean && plan.parts == 1,
        false,
        prepare,
        transform,
    );
    let partial = graph.metal(
        key(source, &["inp", "params", "state", "center"]),
        &[plan.input.clone(), parameters, state, center],
        f32_spec(plan.partial_shape(&plan.row_shape)?),
        plan.spec.dtype == MlxDtype::Bf16,
        reduction_launch(plan.rows, plan.parts),
    )?;
    if plan.parts == 1 {
        return Ok(partial);
    }
    let parameters = params(graph, plan, 1)?;
    graph.metal(
        low_kernels::key(
            low_kernels::reduction_source(ReduceOp::Sum, false, mean, false),
            &["inp", "params"],
        ),
        &[partial, parameters],
        f32_spec(plan.row_shape.clone()),
        false,
        reduction_launch(plan.rows, 1),
    )
}
fn distribution<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
    plan: &Plan<L::Value>,
) -> Result<(L::Value, L::Value), MlxError> {
    let maximum = ops::reduce_low(graph, input, ReduceOp::Max, axes, false, false)?;
    let sum = reduce(graph, plan, &maximum, &maximum, 0)?;
    Ok((maximum, sum))
}
struct LowMoments<V> {
    state: V,
    mean: V,
    variance: V,
}
fn scaled_moments<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
    plan: &Plan<L::Value>,
) -> Result<LowMoments<L::Value>, MlxError> {
    let lo = ops::reduce_low(graph, input.clone(), ReduceOp::Min, axes, false, false)?;
    let hi = ops::reduce_low(graph, input, ReduceOp::Max, axes, false, false)?;
    let lo = element_view(graph, &lo)?;
    let hi = element_view(graph, &hi)?;
    let shape = Shape::new(
        plan.row_shape
            .dims()
            .iter()
            .copied()
            .chain([4])
            .collect::<Vec<_>>(),
    )?;
    let state = graph.metal(
        key(STATE.into(), &["lo", "hi"]),
        &[lo.clone(), hi],
        f32_spec(shape),
        plan.spec.dtype == MlxDtype::Bf16,
        element_launch(plan.rows),
    )?;
    let mean = reduce(graph, plan, &state, &lo, 1)?;
    let variance = reduce(graph, plan, &state, &mean, 2)?;
    Ok(LowMoments {
        state,
        mean,
        variance,
    })
}
fn rows<L: Lowering>(
    graph: &mut L,
    state: &L::Value,
    center: &L::Value,
    shape: Shape,
    mode: u32,
) -> Result<L::Value, MlxError> {
    let state = element_view(graph, state)?;
    let center = element_view(graph, center)?;
    let launch = element_launch(shape.numel());
    graph.metal(
        key(
            ROWS.replace("MODE", &mode.to_string()),
            &["state", "center"],
        ),
        &[state, center],
        f32_spec(shape),
        false,
        launch,
    )
}
fn emit<L: Lowering>(
    graph: &mut L,
    plan: &Plan<L::Value>,
    summaries: [&L::Value; 3],
    mode: u32,
    epsilon: f32,
) -> Result<L::Value, MlxError> {
    let parameters = params(graph, plan, 1)?;
    let state = element_view(graph, summaries[0])?;
    let center = element_view(graph, summaries[1])?;
    let variance = element_view(graph, summaries[2])?;
    // Only the public scalar epsilon is processed on the host.
    let epsilon = scalar(graph, epsilon.sqrt())?;
    let epsilon = element_view(graph, &epsilon)?;
    let output = graph.metal(
        key(
            EMIT.replace("MODE", &mode.to_string()),
            &["inp", "params", "state", "center", "variance", "epsilon"],
        ),
        &[
            plan.input.clone(),
            parameters,
            state,
            center,
            variance,
            epsilon,
        ],
        f32_spec(plan.spec.shape.clone()),
        plan.spec.dtype == MlxDtype::Bf16,
        element_launch(plan.spec.shape.numel()),
    )?;
    graph.native(
        NativeOp::Permute(axes_i32(&plan.inverse)?),
        &[output],
        f32_spec(plan.spec.shape.permute(&plan.inverse)?),
    )
}
pub(in crate::native) fn softmax<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
    logarithm: bool,
) -> Result<L::Value, MlxError> {
    let spec = require(graph, &input)?;
    let shape = normalization_shape(&spec.shape, axes)?;
    if shape.is_empty() {
        return zeros(graph, shape);
    }
    if singleton(&spec.shape, axes) {
        let op = if logarithm {
            NativeOp::Zeros
        } else {
            NativeOp::Ones
        };
        return graph.native(op, &[], f32_spec(shape));
    }
    let plan = plan(graph, input.clone(), axes)?;
    let (max, sum) = distribution(graph, input, axes, &plan)?;
    emit(graph, &plan, [&max, &sum, &sum], u32::from(logarithm), 1.)
}
pub(in crate::native) fn logsumexp<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
    keep: bool,
) -> Result<L::Value, MlxError> {
    let spec = require(graph, &input)?;
    let shape = statistics_shape(&spec.shape, axes, keep)?;
    if shape.is_empty() {
        return zeros(graph, shape);
    }
    if singleton(&spec.shape, axes) {
        let wide = casts::cast_to_f32(graph, input)?;
        return reshape(graph, wide, shape);
    }
    let plan = plan(graph, input.clone(), axes)?;
    let (max, sum) = distribution(graph, input, axes, &plan)?;
    rows(graph, &max, &sum, shape, 0)
}
pub(in crate::native) fn moments<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
    keep: bool,
) -> Result<Moments<L::Value>, MlxError> {
    let spec = require(graph, &input)?;
    let shape = statistics_shape(&spec.shape, axes, keep)?;
    if shape.is_empty() {
        let empty = zeros(graph, shape)?;
        return Ok(Moments {
            mean: empty.clone(),
            variance: empty,
        });
    }
    if singleton(&spec.shape, axes) {
        let wide = casts::cast_to_f32(graph, input)?;
        return Ok(Moments {
            mean: reshape(graph, wide, shape.clone())?,
            variance: zeros(graph, shape)?,
        });
    }
    let plan = plan(graph, input.clone(), axes)?;
    let stats = scaled_moments(graph, input, axes, &plan)?;
    Ok(Moments {
        mean: rows(graph, &stats.state, &stats.mean, shape.clone(), 1)?,
        variance: rows(graph, &stats.state, &stats.variance, shape, 2)?,
    })
}
pub(in crate::native) fn layer_norm<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axes: &[usize],
    epsilon: f32,
) -> Result<L::Value, MlxError> {
    let spec = require(graph, &input)?;
    let shape = normalization_shape(&spec.shape, axes)?;
    validate_epsilon(epsilon)?;
    if shape.is_empty() || singleton(&spec.shape, axes) {
        return zeros(graph, shape);
    }
    let plan = plan(graph, input.clone(), axes)?;
    let stats = scaled_moments(graph, input, axes, &plan)?;
    emit(
        graph,
        &plan,
        [&stats.state, &stats.mean, &stats.variance],
        2,
        epsilon,
    )
}
