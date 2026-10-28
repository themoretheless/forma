//! Geometry whose coordinates and intermediate arithmetic remain binary64.
use super::{
    Result, TensorBounds, TensorMathError, TensorPointCloudStats, TensorPointMoments, shape,
};
use tensor_core::{BinaryOp, HasShape, ReduceOp, TensorF64Backend, UnaryOp};

/// Explicit binary64 geometry. Requires a backend implementing binary64
/// arithmetic; a backend that only stores f64 or supports f32 cannot be used.
/// Upload/read are the only host value transfers. Inputs and intermediate
/// arithmetic must be finite. This API does not silently switch devices.
pub struct TensorMathF64<'a, B: TensorF64Backend> {
    backend: &'a B,
}
/// Population centroid `[3]` and centered covariance `[3,3]`, resident on device.
pub struct TensorCovariance<T> {
    pub samples: usize,
    pub centroid: T,
    pub covariance: T,
}
impl<'a, B: TensorF64Backend> TensorMathF64<'a, B> {
    pub fn new(backend: &'a B) -> Self {
        Self { backend }
    }
    pub fn backend(&self) -> &'a B {
        self.backend
    }
    fn check_points(&self, points: &B::F64Tensor) -> Result<usize, B::Error> {
        match points.shape().dims() {
            &[count, 3] => Ok(count),
            _ => Err(TensorMathError::InvalidInput(
                "point tensor must have shape [N, 3]",
            )),
        }
    }
    pub fn upload_points(&self, points: &[crate::V3]) -> Result<B::F64Tensor, B::Error> {
        if points.iter().flatten().any(|v| !v.is_finite()) {
            return Err(TensorMathError::InvalidInput(
                "point coordinates must be finite",
            ));
        }
        self.backend
            .upload_f64(shape(&[points.len(), 3])?, points.as_flattened())
            .map_err(TensorMathError::Backend)
    }
    pub fn read_points(&self, points: &B::F64Tensor) -> Result<Vec<crate::V3>, B::Error> {
        self.check_points(points)?;
        let values = self
            .backend
            .read_f64(points)
            .map_err(TensorMathError::Backend)?;
        Ok(values.as_chunks::<3>().0.to_vec())
    }
    /// Apply `M*p+t`; matrix `[3,3]` and translation `[3]` are resident.
    pub fn transform(
        &self,
        points: &B::F64Tensor,
        matrix: &B::F64Tensor,
        translation: &B::F64Tensor,
    ) -> Result<B::F64Tensor, B::Error> {
        self.check_points(points)?;
        if matrix.shape().dims() != [3, 3] || translation.shape().dims() != [3] {
            return Err(TensorMathError::InvalidInput(
                "transform requires matrix [3,3] and translation [3]",
            ));
        }
        let transposed = self
            .backend
            .permute_f64(matrix, &[1, 0])
            .map_err(TensorMathError::Backend)?;
        let product = self
            .backend
            .matmul_f64(points, &transposed)
            .map_err(TensorMathError::Backend)?;
        self.binary(BinaryOp::Add, &product, translation)
    }
    pub fn squared_distance_pairs(
        &self,
        left: &B::F64Tensor,
        right: &B::F64Tensor,
    ) -> Result<B::F64Tensor, B::Error> {
        if self.check_points(left)? != self.check_points(right)? {
            return Err(TensorMathError::InvalidInput(
                "distance pairs require equal point counts",
            ));
        }
        let delta = self.binary(BinaryOp::Subtract, left, right)?;
        let squares = self
            .backend
            .unary_f64(UnaryOp::Square, &delta)
            .map_err(TensorMathError::Backend)?;
        self.backend
            .reduce_f64(ReduceOp::Sum, &squares, &[1], false)
            .map_err(TensorMathError::Backend)
    }
    pub fn bounds(&self, points: &B::F64Tensor) -> Result<TensorBounds<B::F64Tensor>, B::Error> {
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
                .reduce_f64(ReduceOp::Min, points, &[0], false)
                .map_err(TensorMathError::Backend)?,
            max: self
                .backend
                .reduce_f64(ReduceOp::Max, points, &[0], false)
                .map_err(TensorMathError::Backend)?,
        })
    }
    /// Population covariance formed from centered coordinates, avoiding
    /// `E[pp^T]-E[p]E[p]^T` cancellation for tightly clustered large coordinates.
    /// The first point is a resident reference for the mean. Opposite extreme
    /// coordinates can overflow the differences, and squared centered distances
    /// and their sums must fit f64. No scaling or arbitrary-range promise is made.
    pub fn covariance(
        &self,
        points: &B::F64Tensor,
    ) -> Result<TensorCovariance<B::F64Tensor>, B::Error> {
        let samples = self.check_points(points)?;
        if samples == 0 {
            return Err(TensorMathError::InvalidInput(
                "point covariance requires a nonempty cloud",
            ));
        }
        let reference = self
            .backend
            .narrow_f64(points, 0, 0, 1)
            .map_err(TensorMathError::Backend)?;
        let relative = self.binary(BinaryOp::Subtract, points, &reference)?;
        let mean = self
            .backend
            .mean_f64(&relative, &[0], true)
            .map_err(TensorMathError::Backend)?;
        let centroid = self.binary(BinaryOp::Add, &reference, &mean)?;
        // Center relative coordinates instead of subtracting the rounded large
        // centroid. Small local differences survive reconstruction rounding.
        let centered = self.binary(BinaryOp::Subtract, &relative, &mean)?;
        let transpose = self
            .backend
            .permute_f64(&centered, &[1, 0])
            .map_err(TensorMathError::Backend)?;
        let gram = self
            .backend
            .matmul_f64(&transpose, &centered)
            .map_err(TensorMathError::Backend)?;
        let count = self
            .backend
            .upload_f64(shape(&[])?, &[samples as f64])
            .map_err(TensorMathError::Backend)?;
        let covariance = self.binary(BinaryOp::Divide, &gram, &count)?;
        let centroid = self
            .backend
            .reshape_f64(&centroid, shape(&[3])?)
            .map_err(TensorMathError::Backend)?;
        Ok(TensorCovariance {
            samples,
            centroid,
            covariance,
        })
    }
    /// Binary64 population moments. Raw products and their sums must fit f64.
    pub fn moments(
        &self,
        points: &B::F64Tensor,
    ) -> Result<TensorPointMoments<B::F64Tensor>, B::Error> {
        let centered = self.covariance(points)?;
        let transpose = self
            .backend
            .permute_f64(points, &[1, 0])
            .map_err(TensorMathError::Backend)?;
        let gram = self
            .backend
            .matmul_f64(&transpose, points)
            .map_err(TensorMathError::Backend)?;
        let count = self
            .backend
            .upload_f64(shape(&[])?, &[centered.samples as f64])
            .map_err(TensorMathError::Backend)?;
        let second_moment = self.binary(BinaryOp::Divide, &gram, &count)?;
        Ok(TensorPointMoments {
            samples: centered.samples,
            centroid: centered.centroid,
            covariance: centered.covariance,
            second_moment,
        })
    }
    /// Bounds and moments from one resident binary64 point tensor.
    pub fn point_cloud_stats(
        &self,
        points: &B::F64Tensor,
    ) -> Result<TensorPointCloudStats<B::F64Tensor>, B::Error> {
        Ok(TensorPointCloudStats {
            bounds: self.bounds(points)?,
            moments: self.moments(points)?,
        })
    }
    fn binary(
        &self,
        op: BinaryOp,
        left: &B::F64Tensor,
        right: &B::F64Tensor,
    ) -> Result<B::F64Tensor, B::Error> {
        self.backend
            .binary_f64(op, left, right)
            .map_err(TensorMathError::Backend)
    }
}
