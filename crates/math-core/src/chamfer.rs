use crate::{Acceleration, Error, Result, V3, nearest_neighbor_accelerated};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectedChamfer {
    pub samples: usize,
    pub mean_squared_distance: f64,
    pub rms_distance: f64,
    pub max_squared_distance: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChamferDistance {
    pub a_to_b: DirectedChamfer,
    pub b_to_a: DirectedChamfer,
    pub symmetric_mean_squared_distance: f64,
    pub symmetric_rms_distance: f64,
    pub symmetric_max_squared_distance: f64,
    pub hausdorff_distance: f64,
}

/// Mean nearest-neighbor squared distance from every `queries` point into
/// `targets`.
pub fn directed_chamfer_distance(
    queries: &[V3],
    targets: &[V3],
    acceleration: Acceleration,
) -> Result<DirectedChamfer> {
    if queries.is_empty() || targets.is_empty() {
        return Err(Error::new(
            "invalid_chamfer_input",
            "directed_chamfer_distance expects non-empty point clouds",
        ));
    }
    #[allow(unused_variables)]
    let acceleration = acceleration.resolve_for_nearest_neighbor(queries.len(), targets.len());
    if acceleration.is_gpu() {
        if let Some(summary) = crate::device::kernels()
            .and_then(|kernels| kernels.directed_chamfer(acceleration, queries, targets))
        {
            return Ok(summary);
        }
    }
    let distances = nearest_neighbor_accelerated(queries, targets, acceleration);
    let mut sum = 0.;
    let mut max_squared_distance = 0.;
    for (_, distance) in distances {
        sum += distance;
        max_squared_distance = f64::max(max_squared_distance, distance);
    }
    let mean_squared_distance = sum / queries.len() as f64;
    Ok(DirectedChamfer {
        samples: queries.len(),
        mean_squared_distance,
        rms_distance: mean_squared_distance.sqrt(),
        max_squared_distance,
    })
}

/// Symmetric point-cloud Chamfer distance: both directed nearest-neighbor
/// means plus their average. Distances are squared to match the accelerated
/// nearest-neighbor kernels and avoid unnecessary square roots per point.
pub fn chamfer_distance(a: &[V3], b: &[V3], acceleration: Acceleration) -> Result<ChamferDistance> {
    let a_to_b = directed_chamfer_distance(a, b, acceleration)?;
    let b_to_a = directed_chamfer_distance(b, a, acceleration)?;
    let symmetric_mean_squared_distance =
        0.5 * (a_to_b.mean_squared_distance + b_to_a.mean_squared_distance);
    let symmetric_max_squared_distance =
        f64::max(a_to_b.max_squared_distance, b_to_a.max_squared_distance);
    Ok(ChamferDistance {
        a_to_b,
        b_to_a,
        symmetric_mean_squared_distance,
        symmetric_rms_distance: symmetric_mean_squared_distance.sqrt(),
        symmetric_max_squared_distance,
        hausdorff_distance: symmetric_max_squared_distance.sqrt(),
    })
}

/// Directed Hausdorff distance from `queries` into `targets`: the largest
/// nearest-neighbor distance across the query cloud.
pub fn directed_hausdorff_distance(
    queries: &[V3],
    targets: &[V3],
    acceleration: Acceleration,
) -> Result<f64> {
    Ok(directed_chamfer_distance(queries, targets, acceleration)?
        .max_squared_distance
        .sqrt())
}

/// Symmetric Hausdorff distance: the maximum of both directed nearest-neighbor
/// distances. Uses the same accelerated nearest-neighbor backend as
/// [`chamfer_distance`].
pub fn hausdorff_distance(a: &[V3], b: &[V3], acceleration: Acceleration) -> Result<f64> {
    Ok(chamfer_distance(a, b, acceleration)?.hausdorff_distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(n: usize, offset: f64) -> Vec<V3> {
        (0..n)
            .map(|i| {
                let f = i as f64 + offset;
                [f * 0.17 - 4., (f * 0.07).sin(), (f * 0.11).cos()]
            })
            .collect()
    }

    #[test]
    fn directed_chamfer_reports_mean_rms_and_max() {
        let queries = [[0., 0., 0.], [2., 0., 0.]];
        let targets = [[1., 0., 0.], [10., 0., 0.]];
        let got = directed_chamfer_distance(&queries, &targets, Acceleration::Cpu).unwrap();
        assert_eq!(got.samples, 2);
        assert_eq!(got.mean_squared_distance, 1.);
        assert_eq!(got.rms_distance, 1.);
        assert_eq!(got.max_squared_distance, 1.);
    }

    #[test]
    fn symmetric_chamfer_averages_both_directions() {
        let a = [[0., 0., 0.], [2., 0., 0.]];
        let b = [[1., 0., 0.], [10., 0., 0.]];
        let got = chamfer_distance(&a, &b, Acceleration::Cpu).unwrap();
        assert_eq!(got.a_to_b.mean_squared_distance, 1.);
        assert_eq!(got.b_to_a.mean_squared_distance, 32.5);
        assert_eq!(got.symmetric_mean_squared_distance, 16.75);
        assert_eq!(got.symmetric_rms_distance, 16.75_f64.sqrt());
        assert_eq!(got.symmetric_max_squared_distance, 64.);
        assert_eq!(got.hausdorff_distance, 8.);
    }

    #[test]
    fn hausdorff_reports_worst_case_distance() {
        let a = [[0., 0., 0.], [2., 0., 0.]];
        let b = [[1., 0., 0.], [10., 0., 0.]];
        assert_eq!(
            directed_hausdorff_distance(&a, &b, Acceleration::Cpu).unwrap(),
            1.
        );
        assert_eq!(hausdorff_distance(&a, &b, Acceleration::Cpu).unwrap(), 8.);
    }

    #[test]
    fn chamfer_rejects_empty_clouds() {
        assert!(directed_chamfer_distance(&[], &[[0.; 3]], Acceleration::Cpu).is_err());
        assert!(directed_chamfer_distance(&[[0.; 3]], &[], Acceleration::Cpu).is_err());
        assert!(chamfer_distance(&[], &[[0.; 3]], Acceleration::Cpu).is_err());
    }

    #[test]
    fn chamfer_auto_matches_cpu_reference() {
        let a = points(96, 0.);
        let b = points(64, 100.);
        let got = chamfer_distance(&a, &b, Acceleration::Auto).unwrap();
        let want = chamfer_distance(&a, &b, Acceleration::Cpu).unwrap();
        assert!(
            (got.symmetric_mean_squared_distance - want.symmetric_mean_squared_distance).abs()
                < 1e-9
        );
    }
}
