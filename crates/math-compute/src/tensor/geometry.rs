use super::{Result, TensorMath, TensorMathError};
use tensor_core::{
    BinaryOp, HasShape, MatmulPrecision, ReduceOp, TensorBackend, TensorReduceBackend, UnaryOp,
};

/// Axis bounds kept on the backend. Both endpoints have shape `[3]`.
pub struct TensorBounds<T> {
    pub samples: usize,
    pub min: T,
    pub max: T,
}

impl<B: TensorBackend> TensorMath<'_, B> {
    /// Applies `M*p+t` to row-major logical points. Matrix `[3,3]` and
    /// translation `[3]` are resident tensors; strided views are supported.
    /// Empty clouds remain empty. Inputs and arithmetic must be finite f32.
    pub fn transform(
        &self,
        points: &B::Tensor,
        matrix: &B::Tensor,
        translation: &B::Tensor,
        precision: MatmulPrecision,
    ) -> Result<B::Tensor, B::Error> {
        self.check_points(points)?;
        if matrix.shape().dims() != [3, 3] || translation.shape().dims() != [3] {
            return Err(TensorMathError::InvalidInput(
                "transform requires matrix [3,3] and translation [3]",
            ));
        }
        let transposed = self
            .backend
            .permute(matrix, &[1, 0])
            .map_err(TensorMathError::Backend)?;
        let product = self
            .backend
            .matmul(points, &transposed, precision)
            .map_err(TensorMathError::Backend)?;
        self.backend
            .binary(BinaryOp::Add, &product, translation)
            .map_err(TensorMathError::Backend)
    }

    /// Squared Euclidean distances between corresponding points, shape `[N]`.
    /// Counts must match; there is no point-count broadcasting. Empty pairs
    /// return `[0]`. Differences, squares and their sums must fit finite f32.
    pub fn squared_distance_pairs(
        &self,
        left: &B::Tensor,
        right: &B::Tensor,
    ) -> Result<B::Tensor, B::Error> {
        if self.check_points(left)? != self.check_points(right)? {
            return Err(TensorMathError::InvalidInput(
                "distance pairs require equal point counts",
            ));
        }
        let delta = self
            .backend
            .binary(BinaryOp::Subtract, left, right)
            .map_err(TensorMathError::Backend)?;
        let squares = self
            .backend
            .unary(UnaryOp::Square, &delta)
            .map_err(TensorMathError::Backend)?;
        self.backend
            .sum_axes(&squares, &[1], false)
            .map_err(TensorMathError::Backend)
    }

    /// Scalar sum of corresponding squared distances; empty pairs sum to zero.
    pub fn squared_distance_pair_sum(
        &self,
        left: &B::Tensor,
        right: &B::Tensor,
    ) -> Result<B::Tensor, B::Error> {
        let distances = self.squared_distance_pairs(left, right)?;
        self.backend
            .sum_axes(&distances, &[0], false)
            .map_err(TensorMathError::Backend)
    }
}

impl<B: TensorReduceBackend> TensorMath<'_, B> {
    /// Bounds of nonempty finite f32 points. Empty clouds are rejected, matching
    /// the CPU domain API; no sentinel values are introduced.
    pub fn bounds(&self, points: &B::Tensor) -> Result<TensorBounds<B::Tensor>, B::Error> {
        let samples = self.check_points(points)?;
        if samples == 0 {
            return Err(TensorMathError::InvalidInput(
                "point bounds require a nonempty cloud",
            ));
        }
        Ok(TensorBounds {
            samples,
            min: self
                .backend
                .reduce_f32(ReduceOp::Min, points, &[0], false)
                .map_err(TensorMathError::Backend)?,
            max: self
                .backend
                .reduce_f32(ReduceOp::Max, points, &[0], false)
                .map_err(TensorMathError::Backend)?,
        })
    }
}
