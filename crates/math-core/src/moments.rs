use crate::{Acceleration, Error, M3, Result, V3, dot, eigen, unit};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointMoments {
    pub samples: usize,
    pub centroid: V3,
    /// Mean outer product `E[p * p^T]`.
    pub second_moment: M3,
    /// Central covariance `E[(p-c) * (p-c)^T]`.
    pub covariance: M3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointPrincipalAxes {
    pub moments: PointMoments,
    /// Covariance eigenvalues in descending order.
    pub variances: V3,
    /// Unit eigenvectors in the same order as `variances`.
    pub axes: [V3; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointPlane {
    pub samples: usize,
    /// Unit normal of the least-squares plane. The sign is normalized
    /// deterministically from its largest component.
    pub normal: V3,
    /// Plane equation offset: `dot(normal, p) + offset = 0`.
    pub offset: f64,
    /// Root-mean-square orthogonal distance to the plane.
    pub rms_distance: f64,
    pub axes: PointPrincipalAxes,
}

impl PointMoments {
    /// Builds moments from raw sums; used by device reduction backends.
    pub fn from_sums(samples: usize, sum: V3, outer: [f64; 6]) -> Self {
        let n = samples as f64;
        let centroid = [sum[0] / n, sum[1] / n, sum[2] / n];
        let second_moment = [
            [outer[0] / n, outer[1] / n, outer[2] / n],
            [outer[1] / n, outer[3] / n, outer[4] / n],
            [outer[2] / n, outer[4] / n, outer[5] / n],
        ];
        let covariance = std::array::from_fn(|i| {
            std::array::from_fn(|j| second_moment[i][j] - centroid[i] * centroid[j])
        });
        Self {
            samples,
            centroid,
            second_moment,
            covariance,
        }
    }
}

/// Centroid of equally weighted finite points, without requiring representable moments.
pub fn point_centroid(points: &[V3]) -> Result<V3> {
    centroid_impl(points, None)
}

/// Centroid with finite, nonnegative weights and at least one positive weight.
/// Weights are normalized before summation, so their total may exceed f64::MAX.
pub fn weighted_point_centroid(points: &[V3], weights: &[f64]) -> Result<V3> {
    centroid_impl(points, Some(weights))
}

#[derive(Default)]
struct Sum {
    value: f64,
    correction: f64,
}
impl Sum {
    fn add(&mut self, value: f64) {
        let next = self.value + value;
        self.correction += if self.value.abs() >= value.abs() {
            (self.value - next) + value
        } else {
            (value - next) + self.value
        };
        self.value = next;
    }
    fn total(&self) -> f64 {
        self.value + self.correction
    }
}

fn centroid_impl(points: &[V3], weights: Option<&[f64]>) -> Result<V3> {
    if points.is_empty() || points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(Error::new(
            "invalid_point_moments_input",
            "Expected nonempty finite points",
        ));
    }
    let max_weight = if let Some(w) = weights {
        if w.len() != points.len() || w.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err(Error::new(
                "invalid_point_weights",
                "Expected one finite nonnegative weight per point",
            ));
        }
        let max = w.iter().copied().fold(0., f64::max);
        if max == 0. {
            return Err(Error::new(
                "invalid_point_weights",
                "At least one weight must be positive",
            ));
        }
        max
    } else {
        1.
    };
    let mut total = Sum::default();
    for i in 0..points.len() {
        total.add(weights.map_or(1., |w| w[i] / max_weight));
    }
    let total = total.total();
    let mut center = [0.; 3];
    for axis in 0..3 {
        let scale = points
            .iter()
            .enumerate()
            .filter(|(i, _)| weights.is_none_or(|w| w[*i] > 0.))
            .map(|(_, p)| p[axis].abs())
            .fold(0., f64::max);
        if scale == 0. {
            continue;
        }
        let mut sum = Sum::default();
        for (i, p) in points.iter().enumerate() {
            let weight = weights.map_or(1., |w| w[i] / max_weight);
            if weight > 0. {
                sum.add((p[axis] / scale) * weight);
            }
        }
        center[axis] = (sum.total() / total).clamp(-1., 1.) * scale;
    }
    Ok(center)
}

/// CPU population moments. Covariance is accumulated about the centroid to
/// avoid cancellation from subtracting raw second moments at large offsets.
/// Returns an error when any requested moment cannot be represented as f64.
pub fn point_moments(points: &[V3]) -> Result<PointMoments> {
    let centroid = point_centroid(points)?;
    let n = points.len() as f64;
    let mut second_moment = [[0.; 3]; 3];
    let mut covariance = [[0.; 3]; 3];
    for i in 0..3 {
        for j in i..3 {
            let mut raw = Sum::default();
            let mut central = Sum::default();
            for p in points {
                raw.add((p[i] / n) * p[j]);
                central.add(((p[i] - centroid[i]) / n) * (p[j] - centroid[j]));
            }
            let (r, c) = (raw.total(), central.total());
            if !r.is_finite() || !c.is_finite() {
                return Err(Error::new(
                    "point_moments_overflow",
                    "Requested moments exceed finite f64 range; use point_centroid for center only",
                ));
            }
            second_moment[i][j] = r;
            second_moment[j][i] = r;
            covariance[i][j] = c;
            covariance[j][i] = c;
        }
    }
    Ok(PointMoments {
        samples: points.len(),
        centroid,
        second_moment,
        covariance,
    })
}

/// [`point_moments`] with optional fused GPU/CUDA reductions. `Auto` currently
/// preserves the exact CPU reference; native `Gpu` uses binary64, while legacy
/// `Cuda` and browser adapters use f32 device execution and fall back to CPU if no device is available.
pub fn point_moments_accelerated(
    points: &[V3],
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Result<PointMoments> {
    if points.is_empty() {
        return Err(Error::new(
            "invalid_point_moments_input",
            "point_moments_accelerated expects at least one point",
        ));
    }
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Error::new(
            "invalid_point_moments_input",
            "point_moments_accelerated expects finite point coordinates",
        ));
    }
    if acceleration.is_gpu() && acceleration != Acceleration::Auto {
        if let Some(moments) =
            crate::device::kernels().and_then(|kernels| kernels.point_moments(acceleration, points))
        {
            return Ok(moments);
        }
    }
    point_moments(points)
}

/// Principal point-cloud axes from the covariance eigensystem. Moments can be
/// CPU, GPU or CUDA; the final 3x3 eigensolve runs on the CPU.
pub fn point_principal_axes(
    points: &[V3],
    acceleration: Acceleration,
) -> Result<PointPrincipalAxes> {
    let moments = point_moments_accelerated(points, acceleration)?;
    let (values, vectors) = eigen(moments.covariance);
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| values[b].total_cmp(&values[a]));
    let variances = order.map(|axis| values[axis].max(0.));
    let axes = order.map(|axis| [vectors[0][axis], vectors[1][axis], vectors[2][axis]]);
    Ok(PointPrincipalAxes {
        moments,
        variances,
        axes,
    })
}

fn orient_normal(mut normal: V3) -> V3 {
    normal = unit(normal);
    let axis = (0..3)
        .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
        .unwrap();
    if normal[axis] < 0. {
        normal = [-normal[0], -normal[1], -normal[2]];
    }
    normal
}

/// Least-squares plane fit from point-cloud PCA. Moment reduction can run on
/// CPU, GPU or CUDA; the final 3x3 eigensolve runs on CPU.
pub fn point_fit_plane(points: &[V3], acceleration: Acceleration) -> Result<PointPlane> {
    if points.len() < 3 {
        return Err(Error::new(
            "invalid_point_plane_input",
            "point_fit_plane expects at least 3 points",
        ));
    }
    let axes = point_principal_axes(points, acceleration)?;
    let normal = orient_normal(axes.axes[2]);
    let offset = -dot(normal, axes.moments.centroid);
    let mut residual = Sum::default();
    for p in points {
        let delta = std::array::from_fn(|i| p[i] - axes.moments.centroid[i]);
        let distance = dot(normal, delta);
        residual.add((distance / points.len() as f64) * distance);
    }
    Ok(PointPlane {
        samples: points.len(),
        normal,
        offset,
        rms_distance: residual.total().sqrt(),
        axes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_handle_extreme_coordinates_and_weights() {
        let m = f64::MAX;
        assert_eq!(point_centroid(&[[m; 3], [m; 3]]).unwrap(), [m; 3]);
        assert_eq!(point_centroid(&[[-m; 3], [m; 3]]).unwrap(), [0.; 3]);
        assert_eq!(
            weighted_point_centroid(&[[0.; 3], [8.; 3]], &[m / 2., m]).unwrap(),
            [16. / 3.; 3]
        );
        assert_eq!(
            weighted_point_centroid(&[[m; 3], [2.; 3]], &[0., 1.]).unwrap(),
            [2.; 3]
        );
        assert!(point_moments(&[[m; 3]]).is_err());
        assert_eq!(crate::point_bounds(&[[m; 3]]).unwrap().center, [m; 3]);
    }

    #[test]
    fn weighted_center_rejects_invalid_inputs() {
        for w in [
            vec![],
            vec![0., 0.],
            vec![-1., 2.],
            vec![f64::NAN, 1.],
            vec![f64::INFINITY, 1.],
        ] {
            assert!(weighted_point_centroid(&[[0.; 3], [1.; 3]], &w).is_err());
        }
    }

    #[test]
    fn covariance_preserves_small_spread_at_large_offset() {
        let offset = 1e12;
        let moments = point_moments(&[[offset - 1.; 3], [offset + 1.; 3]]).unwrap();
        assert_eq!(moments.centroid, [offset; 3]);
        assert_eq!(moments.covariance, [[1.; 3]; 3]);
    }

    fn points(n: usize) -> Vec<V3> {
        (0..n)
            .map(|i| {
                let f = i as f64;
                [
                    (f * 0.013).sin() * 20. - 3.,
                    (f * 0.017).cos() * 12. + 2.,
                    f * 0.001 - 5.,
                ]
            })
            .collect()
    }

    #[test]
    fn point_moments_reports_centroid_second_moment_and_covariance() {
        let got = point_moments(&[[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]]).unwrap();
        assert_eq!(got.samples, 3);
        assert_eq!(got.centroid, [2. / 3., 2. / 3., 0.]);
        assert_eq!(got.second_moment[0][0], 4. / 3.);
        assert_eq!(got.second_moment[1][1], 4. / 3.);
        assert_eq!(got.second_moment[0][1], 0.);
        assert!((got.covariance[0][0] - 8. / 9.).abs() < 1e-15);
        assert!((got.covariance[0][1] + 4. / 9.).abs() < 1e-15);
    }

    #[test]
    fn point_moments_rejects_empty_and_nonfinite_points() {
        assert!(point_moments(&[]).is_err());
        assert!(point_moments(&[[0., f64::INFINITY, 0.]]).is_err());
        assert!(point_moments_accelerated(&[], Acceleration::Gpu).is_err());
    }

    #[test]
    fn point_moments_auto_matches_cpu_reference() {
        let points = points(513);
        assert_eq!(
            point_moments_accelerated(&points, Acceleration::Auto).unwrap(),
            point_moments(&points).unwrap()
        );
    }

    #[test]
    fn point_principal_axes_orders_variance_descending() {
        let points = [
            [-3., 0., 0.],
            [-1., 0., 0.],
            [1., 0., 0.],
            [3., 0., 0.],
            [0., 0.2, 0.],
            [0., -0.2, 0.],
        ];
        let got = point_principal_axes(&points, Acceleration::Cpu).unwrap();
        assert!(got.variances[0] > got.variances[1]);
        assert!(got.variances[1] > got.variances[2]);
        assert!(got.axes[0][0].abs() > 0.99);
    }

    #[test]
    fn point_fit_plane_recovers_known_plane() {
        let points: Vec<_> = (0..32)
            .flat_map(|ix| {
                (0..16).map(move |iy| {
                    let x = ix as f64 * 0.1 - 1.5;
                    let y = iy as f64 * 0.1 - 0.7;
                    let z = 2. + 0.25 * x - 0.5 * y;
                    [x, y, z]
                })
            })
            .collect();
        let got = point_fit_plane(&points, Acceleration::Cpu).unwrap();
        let want = orient_normal([-0.25, 0.5, 1.]);
        for (axis, _) in want.iter().enumerate() {
            assert!((got.normal[axis] - want[axis]).abs() < 1e-10);
        }
        assert!(got.rms_distance < 1e-10);
        for point in &points {
            assert!((dot(got.normal, *point) + got.offset).abs() < 1e-10);
        }
    }

    #[test]
    fn point_fit_plane_rejects_too_few_points() {
        assert!(point_fit_plane(&[[0.; 3], [1.; 3]], Acceleration::Cpu).is_err());
    }
}
