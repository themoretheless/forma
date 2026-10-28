use super::*;
use tensor_core::{
    Moments, ReduceOp, TensorStatsBackend, normalization_shape, statistics_shape, validate_epsilon,
};

struct ScaledMoments {
    anchor: MlxTensor,
    scale: MlxTensor,
    mean: MlxTensor,
    centered: MlxTensor,
    variance: MlxTensor,
}

impl MlxBackend {
    fn stats_scalar(&self, value: f32) -> Result<MlxTensor, MlxError> {
        self.upload_f32(Shape::new(vec![])?, &[value])
    }

    fn stats_axes(
        &self,
        name: &'static str,
        input: &MlxTensor,
        axes: &[usize],
        shape: Shape,
        flag: bool,
        operation: ffi::Reduce,
    ) -> Result<MlxTensor, MlxError> {
        let axes: Vec<i32> = axes
            .iter()
            .map(|&axis| i32::try_from(axis).map_err(|_| MlxError::TooLarge))
            .collect::<Result<_, _>>()?;
        self.output(name, shape, MlxDtype::F32, |_, out| unsafe {
            operation(
                out,
                input.array.raw,
                axes.as_ptr(),
                axes.len(),
                flag,
                self.context.stream,
            )
        })
    }

    fn scaled_moments(&self, input: &MlxTensor, axes: &[usize]) -> Result<ScaledMoments, MlxError> {
        let lo = self.reduce_values(ReduceOp::Min, input, axes, true)?;
        let hi = self.reduce_values(ReduceOp::Max, input, axes, true)?;
        let half = self.stats_scalar(0.5)?;
        // lo/2 + hi/2 cannot overflow, even when hi-lo or lo+hi would.
        let anchor = self.binary(
            BinaryOp::Add,
            &self.binary(BinaryOp::Multiply, &lo, &half)?,
            &self.binary(BinaryOp::Multiply, &hi, &half)?,
        )?;
        let lower = self.unary(
            UnaryOp::Abs,
            &self.binary(BinaryOp::Subtract, &lo, &anchor)?,
        )?;
        let upper = self.unary(
            UnaryOp::Abs,
            &self.binary(BinaryOp::Subtract, &hi, &anchor)?,
        )?;
        let scale = self.binary(BinaryOp::Max, &lower, &upper)?;
        let zero = self.stats_scalar(0.)?;
        let one = self.stats_scalar(1.)?;
        let constant = self.compare(CompareOp::Equal, &scale, &zero)?;
        let safe_scale = self.select_values(&constant, &one, &scale)?;
        let scaled = self.binary(
            BinaryOp::Divide,
            &self.binary(BinaryOp::Subtract, input, &anchor)?,
            &safe_scale,
        )?;
        // Scaled values are bounded by one; neither mean nor squared centered
        // reductions can overflow for an MLX-representable element count.
        let mean = self.mean_axes(&scaled, axes, true)?;
        let centered = self.binary(BinaryOp::Subtract, &scaled, &mean)?;
        let variance = self.mean_axes(&self.unary(UnaryOp::Square, &centered)?, axes, true)?;
        Ok(ScaledMoments {
            anchor,
            scale,
            mean,
            centered,
            variance,
        })
    }
}

impl TensorStatsBackend for MlxBackend {
    fn softmax(&self, input: &MlxTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        let shape = normalization_shape(&input.shape, axes)?;
        if shape.is_empty() {
            return self.zeros(shape, MlxDtype::F32);
        }
        if axes.is_empty() {
            return self.broadcast_to(&self.stats_scalar(1.)?, shape);
        }
        self.stats_axes(
            "softmax",
            input,
            axes,
            shape,
            true,
            self.context.api.softmax_axes,
        )
    }

    fn log_softmax(&self, input: &MlxTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        let shape = normalization_shape(&input.shape, axes)?;
        if shape.is_empty() || axes.is_empty() {
            return self.zeros(shape, MlxDtype::F32);
        }
        let max = self.reduce_values(ReduceOp::Max, input, axes, true)?;
        let shifted = self.binary(BinaryOp::Subtract, input, &max)?;
        // Keep the shift through the subtraction. x-logsumexp(x) would lose
        // log(N) for equal logits whose common offset is much larger than it.
        let normalizer = self.logsumexp(&shifted, axes, true)?;
        self.binary(BinaryOp::Subtract, &shifted, &normalizer)
    }

    fn logsumexp(
        &self,
        input: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        let shape = statistics_shape(&input.shape, axes, keep_dims)?;
        if axes.is_empty() {
            return Ok(input.clone());
        }
        if shape.is_empty() {
            return self.zeros(shape, MlxDtype::F32);
        }
        self.stats_axes(
            "logsumexp",
            input,
            axes,
            shape,
            keep_dims,
            self.context.api.logsumexp_axes,
        )
    }

    fn moments(
        &self,
        input: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        let shape = statistics_shape(&input.shape, axes, keep_dims)?;
        if axes.is_empty() {
            return Ok(Moments {
                mean: input.clone(),
                variance: self.zeros(shape, MlxDtype::F32)?,
            });
        }
        if shape.is_empty() {
            let empty = self.zeros(shape, MlxDtype::F32)?;
            return Ok(Moments {
                mean: empty.clone(),
                variance: empty,
            });
        }
        let stats = self.scaled_moments(input, axes)?;
        let mean = self.binary(
            BinaryOp::Add,
            &stats.anchor,
            &self.binary(BinaryOp::Multiply, &stats.scale, &stats.mean)?,
        )?;
        // Delaying the second multiplication avoids squaring scale before its
        // normalized variance is applied. Truly unrepresentable variance is +inf.
        let variance = self.binary(
            BinaryOp::Multiply,
            &stats.scale,
            &self.binary(BinaryOp::Multiply, &stats.scale, &stats.variance)?,
        )?;
        Ok(Moments {
            mean: self.reshape(&mean, shape.clone())?,
            variance: self.reshape(&variance, shape)?,
        })
    }

    fn layer_norm(
        &self,
        input: &MlxTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        let shape = normalization_shape(&input.shape, axes)?;
        validate_epsilon(epsilon)?;
        if shape.is_empty() || axes.is_empty() {
            return self.zeros(shape, MlxDtype::F32);
        }
        let stats = self.scaled_moments(input, axes)?;
        // A host scalar parameter is allowed; input data stays on the GPU.
        // sqrt(epsilon) is normal even for the smallest positive f32 epsilon.
        let root_epsilon = self.stats_scalar(epsilon.sqrt())?;
        let one = self.stats_scalar(1.)?;
        let large = self.compare(CompareOp::GreaterEqual, &stats.scale, &root_epsilon)?;
        // Both branches must have valid denominators because native graph
        // evaluation computes both before where selects the required values.
        let large_scale = self.select_values(&large, &stats.scale, &root_epsilon)?;
        let ratio = self.binary(BinaryOp::Divide, &root_epsilon, &large_scale)?;
        let denominator = self.unary(
            UnaryOp::Sqrt,
            &self.binary(
                BinaryOp::Add,
                &stats.variance,
                &self.unary(UnaryOp::Square, &ratio)?,
            )?,
        )?;
        let large_result = self.binary(BinaryOp::Divide, &stats.centered, &denominator)?;
        let small_scale = self.select_values(&large, &root_epsilon, &stats.scale)?;
        let ratio = self.binary(BinaryOp::Divide, &small_scale, &root_epsilon)?;
        let numerator = self.binary(BinaryOp::Multiply, &stats.centered, &ratio)?;
        let denominator = self.unary(
            UnaryOp::Sqrt,
            &self.binary(
                BinaryOp::Add,
                &one,
                &self.binary(
                    BinaryOp::Multiply,
                    &stats.variance,
                    &self.unary(UnaryOp::Square, &ratio)?,
                )?,
            )?,
        )?;
        let small_result = self.binary(BinaryOp::Divide, &numerator, &denominator)?;
        self.select_values(&large, &large_result, &small_result)
    }
}
