use crate::{
    Acceleration, Error, M3, PointBounds, PointMoments, Result, V3, add, mm, mv, point_bounds,
    point_moments, point_moments_accelerated, tr, transformed_point_bounds,
    transformed_point_bounds_accelerated,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointCloudStats {
    pub samples: usize,
    pub bounds: PointBounds,
    pub moments: PointMoments,
}

impl PointCloudStats {
    /// Combines reduced bounds and moments; used by device reduction backends.
    pub fn from_parts(bounds: PointBounds, moments: PointMoments) -> Self {
        debug_assert_eq!(bounds.samples, moments.samples);
        Self {
            samples: bounds.samples,
            bounds,
            moments,
        }
    }
}

/// Exact CPU point-cloud summary: bounds plus centroid/covariance moments.
pub fn point_cloud_stats(points: &[V3]) -> Result<PointCloudStats> {
    Ok(PointCloudStats::from_parts(
        point_bounds(points)?,
        point_moments(points)?,
    ))
}

fn transform_moments(moments: PointMoments, m: M3, t: V3) -> PointMoments {
    let mc = mv(m, moments.centroid);
    let centroid = add(mc, t);
    let rotated_second = mm(mm(m, moments.second_moment), tr(m));
    let second_moment = std::array::from_fn(|i| {
        std::array::from_fn(|j| rotated_second[i][j] + mc[i] * t[j] + t[i] * mc[j] + t[i] * t[j])
    });
    let covariance = mm(mm(m, moments.covariance), tr(m));
    PointMoments {
        samples: moments.samples,
        centroid,
        second_moment,
        covariance,
    }
}

/// Exact CPU transformed point-cloud summary: bounds plus centroid/covariance
/// moments for `M*p+t`, without materializing transformed points.
pub fn transformed_point_cloud_stats(points: &[V3], m: M3, t: V3) -> Result<PointCloudStats> {
    Ok(PointCloudStats::from_parts(
        transformed_point_bounds(points, m, t)?,
        transform_moments(point_moments(points)?, m, t),
    ))
}

/// [`point_cloud_stats`] with optional fused GPU/CUDA reduction. This combines
/// bounds and moments in one device pass/upload, which is preferable to running
/// `point_bounds_accelerated` and `point_moments_accelerated` separately when
/// callers need both summaries. `Auto` currently keeps the exact CPU reference;
/// native `Gpu` uses binary64; legacy `Cuda` and browser adapters use f32.
/// These convenience paths retain CPU fallback on device failure.
pub fn point_cloud_stats_accelerated(
    points: &[V3],
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Result<PointCloudStats> {
    if points.is_empty() {
        return Err(Error::new(
            "invalid_point_cloud_stats_input",
            "point_cloud_stats_accelerated expects at least one point",
        ));
    }
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Error::new(
            "invalid_point_cloud_stats_input",
            "point_cloud_stats_accelerated expects finite point coordinates",
        ));
    }
    #[allow(unused_variables)]
    let acceleration = acceleration.resolve_for_point_cloud_stats(points.len());
    if acceleration.is_gpu() {
        if let Some(stats) = crate::device::kernels()
            .and_then(|kernels| kernels.point_cloud_stats(acceleration, points))
        {
            return Ok(stats);
        }
    }
    point_cloud_stats(points)
}

/// [`transformed_point_cloud_stats`] with optional accelerated components.
///
/// Bounds use the transformed-bounds GPU/CUDA kernels when explicitly requested;
/// moments are computed by the existing moments reducer and transformed
/// analytically on the CPU, avoiding a second transformed point buffer.
pub fn transformed_point_cloud_stats_accelerated(
    points: &[V3],
    m: M3,
    t: V3,
    acceleration: Acceleration,
) -> Result<PointCloudStats> {
    if points.is_empty() {
        return Err(Error::new(
            "invalid_point_cloud_stats_input",
            "transformed_point_cloud_stats_accelerated expects at least one point",
        ));
    }
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Error::new(
            "invalid_point_cloud_stats_input",
            "transformed_point_cloud_stats_accelerated expects finite point coordinates",
        ));
    }
    Ok(PointCloudStats::from_parts(
        transformed_point_bounds_accelerated(points, m, t, acceleration)?,
        transform_moments(point_moments_accelerated(points, acceleration)?, m, t),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rotation, transform_points};

    fn points(n: usize) -> Vec<V3> {
        (0..n)
            .map(|i| {
                let f = i as f64;
                [
                    (f * 0.013).sin() * 200. - 7.,
                    (f * 0.017).cos() * 120. + 3.,
                    f * 0.00031 - 50.,
                ]
            })
            .collect()
    }

    #[test]
    fn point_cloud_stats_matches_separate_cpu_summaries() {
        let points = [[-1., 2., 3.], [4., -5., 6.], [0., 7., -8.]];
        let got = point_cloud_stats(&points).unwrap();
        assert_eq!(got.samples, 3);
        assert_eq!(got.bounds, point_bounds(&points).unwrap());
        assert_eq!(got.moments, point_moments(&points).unwrap());
    }

    #[test]
    fn transformed_point_cloud_stats_matches_materialized_reference() {
        let points = points(513);
        let m = rotation([0.2, -0.1, 0.3]);
        let t = [1., -0.5, 0.25];
        let transformed = transform_points(&points, m, t);
        assert_stats_close(
            transformed_point_cloud_stats(&points, m, t).unwrap(),
            point_cloud_stats(&transformed).unwrap(),
            1e-9,
        );
    }

    #[test]
    fn point_cloud_stats_rejects_empty_and_nonfinite_points() {
        assert!(point_cloud_stats(&[]).is_err());
        assert!(point_cloud_stats(&[[0., f64::NAN, 0.]]).is_err());
        assert!(point_cloud_stats_accelerated(&[], Acceleration::Gpu).is_err());
    }

    #[test]
    fn point_cloud_stats_auto_matches_cpu_reference() {
        let points = points(513);
        assert_eq!(
            point_cloud_stats_accelerated(&points, Acceleration::Auto).unwrap(),
            point_cloud_stats(&points).unwrap()
        );
    }

    #[test]
    fn transformed_point_cloud_stats_auto_matches_cpu_reference() {
        let points = points(513);
        let m = rotation([0.2, -0.1, 0.3]);
        let t = [1., -0.5, 0.25];
        assert_eq!(
            transformed_point_cloud_stats_accelerated(&points, m, t, Acceleration::Auto).unwrap(),
            transformed_point_cloud_stats(&points, m, t).unwrap()
        );
    }

    fn assert_stats_close(got: PointCloudStats, want: PointCloudStats, tol: f64) {
        assert_eq!(got.samples, want.samples);
        for axis in 0..3 {
            assert!((got.bounds.min[axis] - want.bounds.min[axis]).abs() < tol);
            assert!((got.bounds.max[axis] - want.bounds.max[axis]).abs() < tol);
            assert!((got.moments.centroid[axis] - want.moments.centroid[axis]).abs() < tol);
        }
        for i in 0..3 {
            for j in 0..3 {
                assert!((got.moments.covariance[i][j] - want.moments.covariance[i][j]).abs() < tol);
            }
        }
    }
}
