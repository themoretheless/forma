//! Resident binary64 recipes behind existing native geometry entrypoints.
use super::{GpuMathError, MathGpuSession};
use crate::{M3, V3};
use compute_core::binary64::GpuF64Tensor;
use tensor_core::{ReduceOp, Shape, TensorF64Backend};

impl MathGpuSession {
    /// Geometry with binary64 inputs and intermediate arithmetic on this GPU.
    pub fn tensor_f64(
        &self,
    ) -> Result<crate::tensor::TensorMathF64<'_, compute_core::ComputeRuntime>, GpuMathError> {
        self.tensor()
    }
    fn f64_points(
        &self,
        points: &[V3],
        transform: Option<(M3, V3)>,
    ) -> Result<GpuF64Tensor, GpuMathError> {
        let math = self.tensor_f64()?;
        let points = math.upload_points(points)?;
        if let Some((matrix, translation)) = transform {
            if matrix
                .iter()
                .flatten()
                .chain(&translation)
                .any(|v| !v.is_finite())
            {
                return Err(GpuMathError::InvalidInput("transform must be finite"));
            }
            let backend = math.backend();
            let matrix = backend.upload_f64(
                Shape::new(vec![3, 3]).expect("fixed shape"),
                matrix.as_flattened(),
            )?;
            let translation =
                backend.upload_f64(Shape::new(vec![3]).expect("fixed shape"), &translation)?;
            Ok(math.transform(&points, &matrix, &translation)?)
        } else {
            Ok(points)
        }
    }
    pub(super) fn f64_distances(
        &self,
        source: &[V3],
        target: &[V3],
        transform: Option<(M3, V3)>,
        sum: bool,
    ) -> Result<Vec<f64>, GpuMathError> {
        if source.len() != target.len() {
            return Err(GpuMathError::InvalidInput("incompatible point counts"));
        }
        let math = self.tensor_f64()?;
        let source = self.f64_points(source, transform)?;
        let target = math.upload_points(target)?;
        let distances = math.squared_distance_pairs(&source, &target)?;
        let output = if sum {
            math.backend()
                .reduce_f64(ReduceOp::Sum, &distances, &[0], false)?
        } else {
            distances
        };
        Ok(math.backend().read_f64(&output)?)
    }
    fn read_f64_moments(&self, points: &GpuF64Tensor) -> Result<crate::PointMoments, GpuMathError> {
        let math = self.tensor_f64()?;
        let backend = math.backend();
        let moments = math.moments(points)?;
        let centroid = backend.read_f64(&moments.centroid)?;
        let covariance = backend.read_f64(&moments.covariance)?;
        let second = backend.read_f64(&moments.second_moment)?;
        Ok(crate::PointMoments {
            samples: moments.samples,
            centroid: [centroid[0], centroid[1], centroid[2]],
            covariance: std::array::from_fn(|i| std::array::from_fn(|j| covariance[i * 3 + j])),
            second_moment: std::array::from_fn(|i| std::array::from_fn(|j| second[i * 3 + j])),
        })
    }
    pub(super) fn f64_moments(&self, points: &[V3]) -> Result<crate::PointMoments, GpuMathError> {
        let points = self.f64_points(points, None)?;
        self.read_f64_moments(&points)
    }
    pub(super) fn f64_stats(&self, points: &[V3]) -> Result<crate::PointCloudStats, GpuMathError> {
        let points = self.f64_points(points, None)?;
        Ok(crate::PointCloudStats::from_parts(
            self.read_f64_bounds(&points)?,
            self.read_f64_moments(&points)?,
        ))
    }
    fn read_f64_bounds(&self, points: &GpuF64Tensor) -> Result<crate::PointBounds, GpuMathError> {
        let math = self.tensor_f64()?;
        let bounds = math.bounds(points)?;
        let min = math.backend().read_f64(&bounds.min)?;
        let max = math.backend().read_f64(&bounds.max)?;
        Ok(crate::PointBounds::new(
            bounds.samples,
            [min[0], min[1], min[2]],
            [max[0], max[1], max[2]],
        ))
    }

    pub(super) fn f64_bounds(
        &self,
        points: &[V3],
        transform: Option<(M3, V3)>,
    ) -> Result<crate::PointBounds, GpuMathError> {
        if points.is_empty() {
            return Err(GpuMathError::InvalidInput("empty point cloud"));
        }
        let points = self.f64_points(points, transform)?;
        self.read_f64_bounds(&points)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_geometry_calls_preserve_binary64() {
        let Some(context) = gpu_compute::GpuContext::new() else {
            assert!(
                std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
                "GPU required"
            );
            return;
        };
        let session = MathGpuSession::new(&context);
        let base = 2f64.powi(40);
        let source = [
            [base + 0.25, -base + 0.5, 1e100],
            [base + 0.5, -base + 0.25, 1e100],
        ];
        let target = [[base, -base, 1e100]; 2];
        let distances = session
            .try_squared_distance_pairs(&source, &target)
            .unwrap();
        assert_eq!(
            distances.arithmetic,
            crate::gpu::GpuArithmetic::SoftwareBinary64
        );
        assert_eq!(distances.value, vec![0.3125, 0.3125]);
        assert_eq!(
            session
                .try_squared_distance_pair_sum(&source, &target)
                .unwrap()
                .value,
            0.625
        );
        let bounds = session.try_point_bounds(&source).unwrap();
        assert_eq!(
            bounds.value,
            crate::PointBounds::new(
                2,
                [base + 0.25, -base + 0.25, 1e100],
                [base + 0.5, -base + 0.5, 1e100]
            )
        );
        let identity = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        let translation = [-base, base, -1e100];
        let local = [[0.25, 0.5, 0.], [0.5, 0.25, 0.]];
        assert_eq!(
            session
                .try_transformed_squared_distance_pair_sum(&source, &local, identity, translation)
                .unwrap()
                .value,
            0.
        );
        assert_eq!(
            session
                .try_transformed_point_bounds(&source, identity, translation)
                .unwrap()
                .value,
            crate::PointBounds::new(2, [0.25, 0.25, 0.], [0.5, 0.5, 0.])
        );
        assert_eq!(
            session
                .try_squared_distance_pair_sum(&[], &[])
                .unwrap()
                .value,
            0.
        );
        assert!(session.try_point_bounds(&[[f64::NAN, 0., 0.]]).is_err());
    }
}
