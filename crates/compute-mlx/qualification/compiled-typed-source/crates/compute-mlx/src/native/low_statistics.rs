use super::{custom_metal::KernelKey, low_kernels::*, *};
use tensor_core::{
    LowDtype, Moments, ReduceOp, TensorLowBackend, TensorLowOpsBackend, TensorLowStatsBackend,
    normalization_shape, statistics_shape, validate_epsilon,
};

const HEADER: &str = concat!(
    include_str!("../metal/low_common.h"),
    "\n",
    include_str!("../metal/low_stats_common.h")
);
const STATE: &str = include_str!("../metal/low_stats_state.metal");
const EMIT: &str = include_str!("../metal/low_stats_emit.metal");
const ROWS: &str = include_str!("../metal/low_stats_rows.metal");

fn stats_key(source: String, inputs: &'static [&'static str]) -> KernelKey {
    let mut key = key(source, inputs);
    key.header = HEADER;
    key
}

struct LowMoments {
    state: MlxTensor,
    mean: MlxTensor,
    variance: MlxTensor,
}

impl MlxBackend {
    fn low_stats_singleton(&self, input: &MlxLowTensor, axes: &[usize]) -> bool {
        // Empty tensors are handled first, so this product cannot overflow.
        axes.iter().all(|&axis| input.shape().dims()[axis] == 1)
    }

    /// Direct low loads reuse the ordinary low reducer's row/partial geometry,
    /// shared tree and final f32 fold. Only register values are transformed.
    fn low_stats_reduce(
        &self,
        plan: &LowReductionPlan,
        state: &MlxTensor,
        center: &MlxTensor,
        mode: u32,
    ) -> Result<MlxTensor, MlxError> {
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
        let params = self.low_parameters(plan.count, plan.row_shape.rank(), plan.parts)?;
        let state = self.low_elementwise_view(state, state.shape())?;
        let center = self.low_elementwise_view(center, center.shape())?;
        let source = transformed_reduction_source(
            ReduceOp::Sum,
            true,
            mean && plan.parts == 1,
            false,
            prepare,
            transform,
        );
        let partial = self.custom_metal(
            stats_key(source, &["inp", "params", "state", "center"]),
            &[&plan.input, &params, &state, &center],
            plan.partial_shape(&plan.row_shape)?,
            MlxDtype::F32,
            plan.input.dtype == MlxDtype::Bf16,
            reduction_launch(plan.rows, plan.parts),
        )?;
        self.low_fold_partials(partial, plan, plan.row_shape.clone(), ReduceOp::Sum, mean)
    }

    fn low_stats_distribution(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        plan: &LowReductionPlan,
    ) -> Result<(MlxTensor, MlxTensor), MlxError> {
        let maximum = self.reduce_low_f32(ReduceOp::Max, input, axes, false)?;
        let sum = self.low_stats_reduce(plan, &maximum, &maximum, 0)?;
        Ok((maximum, sum))
    }

    fn low_stats_moments(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        plan: &LowReductionPlan,
    ) -> Result<LowMoments, MlxError> {
        let lo = self.reduce_low_f32(ReduceOp::Min, input, axes, false)?;
        let hi = self.reduce_low_f32(ReduceOp::Max, input, axes, false)?;
        let lo = self.low_elementwise_view(&lo, lo.shape())?;
        let hi = self.low_elementwise_view(&hi, hi.shape())?;
        let state_shape = Shape::new(
            plan.row_shape
                .dims()
                .iter()
                .copied()
                .chain([4])
                .collect::<Vec<_>>(),
        )?;
        let state = self.custom_metal(
            stats_key(STATE.into(), &["lo", "hi"]),
            &[&lo, &hi],
            state_shape,
            MlxDtype::F32,
            input.dtype == LowDtype::Bf16,
            element_launch(plan.rows),
        )?;
        let mean = self.low_stats_reduce(plan, &state, &lo, 1)?;
        let variance = self.low_stats_reduce(plan, &state, &mean, 2)?;
        Ok(LowMoments {
            state,
            mean,
            variance,
        })
    }

    fn low_stats_rows(
        &self,
        state: &MlxTensor,
        center: &MlxTensor,
        shape: Shape,
        mode: u32,
    ) -> Result<MlxTensor, MlxError> {
        let state = self.low_elementwise_view(state, state.shape())?;
        let center = self.low_elementwise_view(center, center.shape())?;
        let rows = shape.numel();
        self.custom_metal(
            stats_key(
                ROWS.replace("MODE", &mode.to_string()),
                &["state", "center"],
            ),
            &[&state, &center],
            shape,
            MlxDtype::F32,
            false,
            element_launch(rows),
        )
    }

    fn low_stats_emit(
        &self,
        plan: &LowReductionPlan,
        state: &MlxTensor,
        center: &MlxTensor,
        variance: &MlxTensor,
        mode: u32,
        epsilon: f32,
    ) -> Result<MlxTensor, MlxError> {
        let params = self.low_parameters(plan.count, plan.row_shape.rank(), 1)?;
        let state = self.low_elementwise_view(state, state.shape())?;
        let center = self.low_elementwise_view(center, center.shape())?;
        let variance = self.low_elementwise_view(variance, variance.shape())?;
        // The host sees only this public scalar parameter, never tensor data.
        let epsilon = self.upload_f32(Shape::new(vec![1])?, &[epsilon.sqrt()])?;
        let output = self.custom_metal(
            stats_key(
                EMIT.replace("MODE", &mode.to_string()),
                &["inp", "params", "state", "center", "variance", "epsilon"],
            ),
            &[&plan.input, &params, &state, &center, &variance, &epsilon],
            plan.input.shape.clone(),
            MlxDtype::F32,
            plan.input.dtype == MlxDtype::Bf16,
            element_launch(plan.input.shape.numel()),
        )?;
        self.permute(&output, &plan.inverse)
    }

    fn low_distribution_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        logarithm: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        let shape = normalization_shape(input.shape(), axes)?;
        if shape.is_empty() {
            return self.zeros(shape, MlxDtype::F32);
        }
        if self.low_stats_singleton(input, axes) {
            let value = self.upload_f32(Shape::new(vec![])?, &[if logarithm { 0. } else { 1. }])?;
            return self.broadcast_to(&value, shape);
        }
        let plan = self.low_reduction_plan(input, axes)?;
        let (max, sum) = self.low_stats_distribution(input, axes, &plan)?;
        self.low_stats_emit(&plan, &max, &sum, &sum, u32::from(logarithm), 1.)
    }
}

impl TensorLowStatsBackend for MlxBackend {
    fn softmax_low_f32(&self, input: &MlxLowTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.low_distribution_f32(input, axes, false)
    }

    fn log_softmax_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
    ) -> Result<MlxTensor, MlxError> {
        self.low_distribution_f32(input, axes, true)
    }

    fn logsumexp_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        if shape.is_empty() {
            return self.zeros(shape, MlxDtype::F32);
        }
        if self.low_stats_singleton(input, axes) {
            return self.reshape(&self.cast_to_f32(input)?, shape);
        }
        let plan = self.low_reduction_plan(input, axes)?;
        let (max, sum) = self.low_stats_distribution(input, axes, &plan)?;
        self.low_stats_rows(&max, &sum, shape, 0)
    }

    fn moments_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<MlxTensor>, MlxError> {
        self.check_low(input)?;
        let shape = statistics_shape(input.shape(), axes, keep_dims)?;
        if shape.is_empty() {
            let empty = self.zeros(shape, MlxDtype::F32)?;
            return Ok(Moments {
                mean: empty.clone(),
                variance: empty,
            });
        }
        if self.low_stats_singleton(input, axes) {
            return Ok(Moments {
                mean: self.reshape(&self.cast_to_f32(input)?, shape.clone())?,
                variance: self.zeros(shape, MlxDtype::F32)?,
            });
        }
        let plan = self.low_reduction_plan(input, axes)?;
        let stats = self.low_stats_moments(input, axes, &plan)?;
        Ok(Moments {
            mean: self.low_stats_rows(&stats.state, &stats.mean, shape.clone(), 1)?,
            variance: self.low_stats_rows(&stats.state, &stats.variance, shape, 2)?,
        })
    }

    fn layer_norm_low_f32(
        &self,
        input: &MlxLowTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        let shape = normalization_shape(input.shape(), axes)?;
        validate_epsilon(epsilon)?;
        if shape.is_empty() || self.low_stats_singleton(input, axes) {
            return self.zeros(shape, MlxDtype::F32);
        }
        let plan = self.low_reduction_plan(input, axes)?;
        let stats = self.low_stats_moments(input, axes, &plan)?;
        self.low_stats_emit(
            &plan,
            &stats.state,
            &stats.mean,
            &stats.variance,
            2,
            epsilon,
        )
    }
}
