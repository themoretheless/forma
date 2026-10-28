use crate::{Acceleration, Error, M3, Result, V3, add, mv};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointBounds {
    pub samples: usize,
    pub min: V3,
    pub max: V3,
    pub center: V3,
    pub extent: V3,
}

impl PointBounds {
    /// Builds bounds from reduced extrema; used by device reduction backends.
    pub fn new(samples: usize, min: V3, max: V3) -> Self {
        Self {
            samples,
            min,
            max,
            center: [
                min[0].midpoint(max[0]),
                min[1].midpoint(max[1]),
                min[2].midpoint(max[2]),
            ],
            extent: [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
        }
    }
}

/// Exact CPU axis-aligned bounds for a point cloud.
pub fn point_bounds(points: &[V3]) -> Result<PointBounds> {
    if points.is_empty() {
        return Err(Error::new(
            "invalid_point_bounds_input",
            "point_bounds expects at least one point",
        ));
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for &point in points {
        for axis in 0..3 {
            let value = point[axis];
            if !value.is_finite() {
                return Err(Error::new(
                    "invalid_point_bounds_input",
                    "point_bounds expects finite point coordinates",
                ));
            }
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    Ok(PointBounds::new(points.len(), min, max))
}

/// Exact CPU axis-aligned bounds after applying `q = M*p + t` to each point.
pub fn transformed_point_bounds(points: &[V3], m: M3, t: V3) -> Result<PointBounds> {
    if points.is_empty() {
        return Err(Error::new(
            "invalid_point_bounds_input",
            "transformed_point_bounds expects at least one point",
        ));
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for &point in points {
        if point.iter().any(|value| !value.is_finite()) {
            return Err(Error::new(
                "invalid_point_bounds_input",
                "transformed_point_bounds expects finite point coordinates",
            ));
        }
        let transformed = add(mv(m, point), t);
        for axis in 0..3 {
            let value = transformed[axis];
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    Ok(PointBounds::new(points.len(), min, max))
}

/// [`point_bounds`] with optional GPU/CUDA reduction kernels. `Auto` currently
/// keeps the CPU reference; explicit `Gpu`/`Cuda` force device execution and
/// fall back to CPU if no device is available.
pub fn point_bounds_accelerated(
    points: &[V3],
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Result<PointBounds> {
    if points.is_empty() {
        return Err(Error::new(
            "invalid_point_bounds_input",
            "point_bounds_accelerated expects at least one point",
        ));
    }
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Error::new(
            "invalid_point_bounds_input",
            "point_bounds_accelerated expects finite point coordinates",
        ));
    }
    if acceleration.is_gpu() && acceleration != Acceleration::Auto {
        if let Some(bounds) =
            crate::device::kernels().and_then(|kernels| kernels.point_bounds(acceleration, points))
        {
            return Ok(bounds);
        }
    }
    point_bounds(points)
}

/// [`transformed_point_bounds`] with optional GPU/CUDA reduction kernels.
pub fn transformed_point_bounds_accelerated(
    points: &[V3],
    m: M3,
    t: V3,
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Result<PointBounds> {
    if points.is_empty() {
        return Err(Error::new(
            "invalid_point_bounds_input",
            "transformed_point_bounds_accelerated expects at least one point",
        ));
    }
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Error::new(
            "invalid_point_bounds_input",
            "transformed_point_bounds_accelerated expects finite point coordinates",
        ));
    }
    #[allow(unused_variables)]
    let acceleration = acceleration.resolve_for_transformed_point_bounds(points.len());
    if acceleration.is_gpu() {
        if let Some(bounds) = crate::device::kernels()
            .and_then(|kernels| kernels.transformed_point_bounds(acceleration, points, m, t))
        {
            return Ok(bounds);
        }
    }
    transformed_point_bounds(points, m, t)
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
                    (f * 0.17).sin() * 20. - 3.,
                    (f * 0.07).cos() * 11. + 2.,
                    f * 0.013 - 5.,
                ]
            })
            .collect()
    }

    #[test]
    fn point_bounds_reports_min_max_center_and_extent() {
        let got = point_bounds(&[[-1., 2., 3.], [4., -5., 6.], [0., 7., -8.]]).unwrap();
        assert_eq!(got.samples, 3);
        assert_eq!(got.min, [-1., -5., -8.]);
        assert_eq!(got.max, [4., 7., 6.]);
        assert_eq!(got.center, [1.5, 1., -1.]);
        assert_eq!(got.extent, [5., 12., 14.]);
    }

    #[test]
    fn point_bounds_rejects_empty_and_nonfinite_points() {
        assert!(point_bounds(&[]).is_err());
        assert!(point_bounds(&[[f64::NAN, 0., 0.]]).is_err());
        assert!(point_bounds_accelerated(&[], Acceleration::Gpu).is_err());
    }

    #[test]
    fn point_bounds_auto_matches_cpu_reference() {
        let points = points(257);
        assert_eq!(
            point_bounds_accelerated(&points, Acceleration::Auto).unwrap(),
            point_bounds(&points).unwrap()
        );
    }

    #[test]
    fn transformed_point_bounds_matches_materialized_reference() {
        let points = points(257);
        let m = rotation([0.2, -0.1, 0.3]);
        let t = [1., -0.5, 0.25];
        let transformed = transform_points(&points, m, t);
        assert_eq!(
            transformed_point_bounds(&points, m, t).unwrap(),
            point_bounds(&transformed).unwrap()
        );
    }

    #[test]
    fn transformed_point_bounds_rejects_empty_and_nonfinite_points() {
        assert!(transformed_point_bounds(&[], [[0.; 3]; 3], [0.; 3]).is_err());
        assert!(
            transformed_point_bounds_accelerated(
                &[[f64::NAN, 0., 0.]],
                [[0.; 3]; 3],
                [0.; 3],
                Acceleration::Gpu
            )
            .is_err()
        );
    }

    #[test]
    fn transformed_point_bounds_auto_matches_cpu_reference() {
        let points = points(257);
        let m = rotation([0.2, -0.1, 0.3]);
        let t = [1., -0.5, 0.25];
        assert_eq!(
            transformed_point_bounds_accelerated(&points, m, t, Acceleration::Auto).unwrap(),
            transformed_point_bounds(&points, m, t).unwrap()
        );
    }
}
