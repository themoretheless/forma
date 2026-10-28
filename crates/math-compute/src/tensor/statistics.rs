use super::{Result, TensorBounds, TensorMath, TensorMathError, shape};
use tensor_core::{
    BinaryOp, MatmulPrecision, ReduceOp, TensorReduceBackend, TensorStatsBackend, UnaryOp,
};

/// Population moments of resident `[N,3]` points. Centroid has shape `[3]`;
/// second moment `E[ppᵀ]` and centered covariance have shape `[3,3]`.
pub struct TensorPointMoments<T> {
    pub samples: usize,
    pub centroid: T,
    pub second_moment: T,
    pub covariance: T,
}

/// Bounds and moments sharing the same resident point input.
pub struct TensorPointCloudStats<T> {
    pub bounds: TensorBounds<T>,
    pub moments: TensorPointMoments<T>,
}

impl<B: TensorReduceBackend + TensorStatsBackend> TensorMath<'_, B> {
    /// Computes centered, per-coordinate scaled covariance. The rounded f32
    /// centroid is refined by adding its residual mean. Covariance subtracts
    /// the residual mean outer product in scaled coordinates. Raw second moments
    /// use a separate normalized Gram product, avoiding an overflowing raw square
    /// or unnormalized sum.
    ///
    /// Finite f32 inputs, requested moments and intermediate arithmetic are
    /// required. Underflow and reduction order follow the backend. This does
    /// not extend the finite f64 domain of the CPU API, recover differences
    /// lost at upload, or guarantee positive semidefiniteness after rounding.
    /// Very small normalized cross-coordinate terms may underflow before unit
    /// restoration even when the final cross-moment would be normal; accuracy
    /// is assessed against the corresponding diagonal-moment scales.
    /// Centroid error is relative to the mean absolute coordinate magnitude;
    /// cancellation does not permit a relative-error guarantee near zero mean.
    /// No readback or host-side numerical fold is performed.
    pub fn moments(&self, points: &B::Tensor) -> Result<TensorPointMoments<B::Tensor>, B::Error> {
        let samples = self.check_points(points)?;
        if samples == 0 {
            return Err(TensorMathError::InvalidInput(
                "point moments require a nonempty cloud",
            ));
        }
        // Reuse each backend's scaled/wider centered mean instead of a raw sum.
        let center = self
            .backend
            .moments(points, &[0], true)
            .map_err(TensorMathError::Backend)?
            .mean;
        let delta = self.binary(BinaryOp::Subtract, points, &center)?;
        let (normalized, scale) = self.normalize_columns(&delta)?;
        let residual = self
            .backend
            .mean_axes(&normalized, &[0], true)
            .map_err(TensorMathError::Backend)?;
        // Reconstructing a midpoint-based mean can cancel large terms for a
        // sparse, asymmetric cloud. The residual pass already measures this
        // error, so apply its correction to the reported center as well.
        let center_correction = self.binary(BinaryOp::Multiply, &scale, &residual)?;
        let refined_center = self.binary(BinaryOp::Add, &center, &center_correction)?;
        let gram = self.mean_gram(&normalized, samples)?;
        let residual_t = self
            .backend
            .permute(&residual, &[1, 0])
            .map_err(TensorMathError::Backend)?;
        let correction = self
            .backend
            .matmul(&residual_t, &residual, MatmulPrecision::F32)
            .map_err(TensorMathError::Backend)?;
        let covariance = self.binary(BinaryOp::Subtract, &gram, &correction)?;
        let covariance = self.rescale_gram(&covariance, &scale)?;
        let (raw_normalized, raw_scale) = self.normalize_columns(points)?;
        let second_moment = self.mean_gram(&raw_normalized, samples)?;
        let second_moment = self.rescale_gram(&second_moment, &raw_scale)?;
        Ok(TensorPointMoments {
            samples,
            centroid: self
                .backend
                .reshape(&refined_center, shape(&[3])?)
                .map_err(TensorMathError::Backend)?,
            second_moment,
            covariance,
        })
    }

    pub fn point_cloud_stats(
        &self,
        points: &B::Tensor,
    ) -> Result<TensorPointCloudStats<B::Tensor>, B::Error> {
        Ok(TensorPointCloudStats {
            bounds: self.bounds(points)?,
            moments: self.moments(points)?,
        })
    }

    fn binary(&self, op: BinaryOp, a: &B::Tensor, b: &B::Tensor) -> Result<B::Tensor, B::Error> {
        self.backend
            .binary(op, a, b)
            .map_err(TensorMathError::Backend)
    }
    fn scalar(&self, value: f32) -> Result<B::Tensor, B::Error> {
        self.backend
            .upload_f32(shape(&[])?, &[value])
            .map_err(TensorMathError::Backend)
    }
    fn normalize_columns(&self, input: &B::Tensor) -> Result<(B::Tensor, B::Tensor), B::Error> {
        let magnitude = self
            .backend
            .unary(UnaryOp::Abs, input)
            .map_err(TensorMathError::Backend)?;
        let scale = self
            .backend
            .reduce_f32(ReduceOp::Max, &magnitude, &[0], true)
            .map_err(TensorMathError::Backend)?;
        // Constants, zero coordinates and tiny columns must never divide by 0.
        // Keeping the divisor normal also avoids an overflowing reciprocal.
        let safe = self.binary(BinaryOp::Max, &scale, &self.scalar(f32::MIN_POSITIVE)?)?;
        Ok((self.binary(BinaryOp::Divide, input, &safe)?, safe))
    }
    fn mean_gram(&self, values: &B::Tensor, count: usize) -> Result<B::Tensor, B::Error> {
        let weighted = self.binary(
            BinaryOp::Multiply,
            values,
            &self.scalar(1.0 / count as f32)?,
        )?;
        let transpose = self
            .backend
            .permute(values, &[1, 0])
            .map_err(TensorMathError::Backend)?;
        let gram = self
            .backend
            .matmul(&transpose, &weighted, MatmulPrecision::F32)
            .map_err(TensorMathError::Backend)?;
        // Weighting only the right operand may introduce asymmetric rounding.
        // Symmetrize while values are bounded, before restoring physical units.
        let transpose = self
            .backend
            .permute(&gram, &[1, 0])
            .map_err(TensorMathError::Backend)?;
        let symmetric = self.binary(BinaryOp::Add, &gram, &transpose)?;
        self.binary(BinaryOp::Multiply, &symmetric, &self.scalar(0.5)?)
    }
    fn rescale_gram(&self, gram: &B::Tensor, scale: &B::Tensor) -> Result<B::Tensor, B::Error> {
        let column = self
            .backend
            .permute(scale, &[1, 0])
            .map_err(TensorMathError::Backend)?;
        let larger = self.binary(BinaryOp::Max, scale, &column)?;
        let smaller = self.binary(BinaryOp::Min, scale, &column)?;
        // Normalized coefficients are bounded by one in exact arithmetic.
        // Applying the larger scale first preserves normal cross-moments when
        // one axis is tiny and the other huge. Do not first form scale_i*scale_j:
        // it can overflow even when the requested moment is representable.
        let result = self.binary(BinaryOp::Multiply, gram, &larger)?;
        self.binary(BinaryOp::Multiply, &result, &smaller)
    }
}
