use crate::{
    Acceleration, Error, ID, M3, Result, V3, add, det, finite, mm, mv,
    nearest_neighbor_accelerated, scale, sub, svd, tr, transform_points,
    transformed_squared_distance_pair_sum_accelerated,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigidTransform {
    pub rotation: M3,
    pub translation: V3,
}

impl Default for RigidTransform {
    fn default() -> Self {
        Self {
            rotation: ID,
            translation: [0.; 3],
        }
    }
}

impl RigidTransform {
    #[inline]
    pub fn apply(self, point: V3) -> V3 {
        add(mv(self.rotation, point), self.translation)
    }

    #[inline]
    pub fn then(self, next: Self) -> Self {
        Self {
            rotation: mm(next.rotation, self.rotation),
            translation: add(mv(next.rotation, self.translation), next.translation),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IcpOptions {
    pub max_iterations: usize,
    /// Stop once the absolute mean-squared-error improvement is at or below
    /// this value.
    pub tolerance: f64,
    /// Optional Euclidean correspondence-distance cap; rejected pairs are not
    /// used by the rigid fit.
    pub max_correspondence_distance: Option<f64>,
    pub acceleration: Acceleration,
}

impl Default for IcpOptions {
    fn default() -> Self {
        Self {
            max_iterations: 32,
            tolerance: 1e-10,
            max_correspondence_distance: None,
            acceleration: Acceleration::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IcpReport {
    pub transform: RigidTransform,
    pub iterations: usize,
    pub correspondences: usize,
    pub mean_squared_error: f64,
}

fn centroid(points: &[V3]) -> V3 {
    let sum = points.iter().fold([0.; 3], |acc, &p| add(acc, p));
    scale(sum, 1. / points.len() as f64)
}

fn checked_registration_points(source: &[V3], target: &[V3]) -> Result<()> {
    if source.len() != target.len() || source.len() < 3 {
        return Err(Error::new(
            "invalid_registration_input",
            "Rigid registration expects equal point counts and at least 3 correspondences",
        ));
    }
    if source.iter().chain(target).any(|&p| !finite(p)) {
        return Err(Error::new(
            "invalid_registration_input",
            "Rigid registration coordinates must be finite and bounded",
        ));
    }
    Ok(())
}

/// Least-squares rigid transform (Kabsch): returns `R,t` minimizing
/// `sum(|R*source[i] + t - target[i]|^2)` for known correspondences.
pub fn rigid_transform(source: &[V3], target: &[V3]) -> Result<RigidTransform> {
    checked_registration_points(source, target)?;
    let ps = centroid(source);
    let qs = centroid(target);
    let mut h = [[0.; 3]; 3];
    for (&p, &q) in source.iter().zip(target) {
        let p = sub(p, ps);
        let q = sub(q, qs);
        for i in 0..3 {
            for j in 0..3 {
                h[i][j] += p[i] * q[j];
            }
        }
    }
    let (u, _s, mut v) = svd(h);
    let mut r = mm(v, tr(u));
    if det(r) < 0. {
        for row in &mut v {
            row[2] = -row[2];
        }
        r = mm(v, tr(u));
    }
    Ok(RigidTransform {
        rotation: r,
        translation: sub(qs, mv(r, ps)),
    })
}

/// Point-to-point Iterative Closest Point. The nearest-neighbor stage is the
/// expensive step and uses [`nearest_neighbor_accelerated`]; the fitted delta
/// is scored with [`transformed_squared_distance_pair_sum_accelerated`], so
/// `Gpu`/`Cuda` or `Auto` can accelerate large registrations while preserving
/// the CPU reference path.
pub fn icp_register(source: &[V3], target: &[V3], options: IcpOptions) -> Result<IcpReport> {
    if source.len() < 3 || target.len() < 3 {
        return Err(Error::new(
            "invalid_registration_input",
            "ICP expects at least 3 source points and 3 target points",
        ));
    }
    if source.iter().chain(target).any(|&p| !finite(p)) {
        return Err(Error::new(
            "invalid_registration_input",
            "ICP coordinates must be finite and bounded",
        ));
    }
    if options.max_iterations == 0 || !options.tolerance.is_finite() || options.tolerance < 0. {
        return Err(Error::new(
            "invalid_registration_input",
            "ICP max_iterations must be positive and tolerance must be finite and non-negative",
        ));
    }
    let max_squared = options.max_correspondence_distance.map(|d| {
        if d.is_finite() && d > 0. {
            d * d
        } else {
            f64::NAN
        }
    });
    if max_squared.is_some_and(|d| !d.is_finite()) {
        return Err(Error::new(
            "invalid_registration_input",
            "ICP max_correspondence_distance must be finite and positive",
        ));
    }

    let mut current = source.to_vec();
    let mut transform = RigidTransform::default();
    let mut last_mse = f64::INFINITY;
    let mut report = IcpReport {
        transform,
        iterations: 0,
        correspondences: 0,
        mean_squared_error: f64::INFINITY,
    };
    for iteration in 0..options.max_iterations {
        let pairs = nearest_neighbor_accelerated(&current, target, options.acceleration);
        let mut from = Vec::with_capacity(source.len());
        let mut to = Vec::with_capacity(source.len());
        for (i, &(j, squared)) in pairs.iter().enumerate() {
            if j == u32::MAX || max_squared.is_some_and(|cap| squared > cap) {
                continue;
            }
            from.push(current[i]);
            to.push(target[j as usize]);
        }
        if from.len() < 3 {
            return Err(Error::new(
                "insufficient_correspondences",
                "ICP found fewer than 3 correspondences",
            ));
        }
        let delta = rigid_transform(&from, &to)?;
        let mse = transformed_squared_distance_pair_sum_accelerated(
            &from,
            &to,
            delta.rotation,
            delta.translation,
            options.acceleration,
        )? / from.len() as f64;
        let next_transform = transform.then(delta);
        report = IcpReport {
            transform: next_transform,
            iterations: iteration + 1,
            correspondences: from.len(),
            mean_squared_error: mse,
        };
        if last_mse.is_finite() && (last_mse - mse).abs() <= options.tolerance {
            break;
        }
        last_mse = mse;
        current = transform_points(&current, delta.rotation, delta.translation);
        transform = next_transform;
    }
    Ok(report)
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    use crate::rotation;

    fn sample_cloud() -> Vec<V3> {
        (0..128)
            .map(|i| {
                let f = i as f64;
                [
                    (f * 0.17).sin() * 2. + f * 0.01,
                    (f * 0.11).cos() * 1.5,
                    (f * 0.07).sin() * (f * 0.03).cos(),
                ]
            })
            .collect()
    }

    fn assert_transform_close(got: RigidTransform, want: RigidTransform, tol: f64) {
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (got.rotation[i][j] - want.rotation[i][j]).abs() < tol,
                    "rotation[{i}][{j}]: got {}, want {}",
                    got.rotation[i][j],
                    want.rotation[i][j]
                );
            }
            assert!(
                (got.translation[i] - want.translation[i]).abs() < tol,
                "translation[{i}]: got {}, want {}",
                got.translation[i],
                want.translation[i]
            );
        }
    }

    #[test]
    fn rigid_transform_recovers_known_correspondences() {
        let source = sample_cloud();
        let want = RigidTransform {
            rotation: rotation([0.08, -0.05, 0.11]),
            translation: [0.4, -0.2, 0.15],
        };
        let target: Vec<_> = source.iter().map(|&p| want.apply(p)).collect();
        let got = rigid_transform(&source, &target).unwrap();
        assert_transform_close(got, want, 1e-8);
    }

    #[test]
    fn icp_register_recovers_small_rigid_offset() {
        let source = sample_cloud();
        let want = RigidTransform {
            rotation: rotation([0.02, -0.015, 0.03]),
            translation: [0.03, -0.02, 0.015],
        };
        let target: Vec<_> = source.iter().map(|&p| want.apply(p)).collect();
        let report = icp_register(
            &source,
            &target,
            IcpOptions {
                acceleration: Acceleration::Auto,
                max_iterations: 20,
                tolerance: 1e-14,
                max_correspondence_distance: Some(0.5),
            },
        )
        .unwrap();
        assert!(report.mean_squared_error < 1e-8);
        assert!(report.iterations <= 20);
        assert_transform_close(report.transform, want, 1e-4);
    }

    #[test]
    fn icp_reports_post_fit_error_for_iteration() {
        let source = vec![
            [0., 0., 0.],
            [10., 0., 0.],
            [0., 10., 0.],
            [0., 0., 10.],
            [10., 10., 10.],
        ];
        let want = RigidTransform {
            rotation: rotation([0.002, -0.001, 0.0015]),
            translation: [0.01, -0.012, 0.006],
        };
        let target: Vec<_> = source.iter().map(|&p| want.apply(p)).collect();
        let report = icp_register(
            &source,
            &target,
            IcpOptions {
                acceleration: Acceleration::Cpu,
                max_iterations: 1,
                tolerance: 0.,
                max_correspondence_distance: Some(0.1),
            },
        )
        .unwrap();
        assert_eq!(report.iterations, 1);
        assert!(report.mean_squared_error < 1e-10);
        assert_transform_close(report.transform, want, 1e-5);
    }

    #[test]
    fn icp_rejects_invalid_inputs() {
        assert!(icp_register(&[[0.; 3]; 2], &[[0.; 3]; 3], IcpOptions::default()).is_err());
        assert!(rigid_transform(&[[0.; 3], [1.; 3], [2.; 3]], &[[0.; 3], [1.; 3]]).is_err());
    }
}
