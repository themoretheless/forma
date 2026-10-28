use crate::{Acceleration, Error, M3, Result, V3, add, dot, mv, sub};

/// Sum of squared one-to-one distances after applying `q = M*p + t` to `source`.
///
/// This is the ICP/registration scoring form: transform the moving/source point
/// and compare it to the corresponding target point without materializing the
/// transformed cloud.
pub fn transformed_squared_distance_pair_sum(
    source: &[V3],
    target: &[V3],
    m: M3,
    t: V3,
) -> Result<f64> {
    if source.len() != target.len() {
        return Err(Error::new(
            "invalid_transform_error_input",
            "transformed_squared_distance_pair_sum expects equal point counts",
        ));
    }
    Ok(source
        .iter()
        .zip(target)
        .map(|(&p, &q)| {
            let d = sub(add(mv(m, p), t), q);
            dot(d, d)
        })
        .sum())
}

/// Root-mean-square one-to-one distance after applying `q = M*p + t` to `source`.
pub fn transformed_squared_distance_pair_rmse(
    source: &[V3],
    target: &[V3],
    m: M3,
    t: V3,
) -> Result<f64> {
    if source.is_empty() {
        return Ok(0.);
    }
    Ok((transformed_squared_distance_pair_sum(source, target, m, t)? / source.len() as f64).sqrt())
}

/// [`transformed_squared_distance_pair_sum`] with optional fused GPU/CUDA reduction.
pub fn transformed_squared_distance_pair_sum_accelerated(
    source: &[V3],
    target: &[V3],
    m: M3,
    t: V3,
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Result<f64> {
    if source.len() != target.len() {
        return Err(Error::new(
            "invalid_transform_error_input",
            "transformed_squared_distance_pair_sum_accelerated expects equal point counts",
        ));
    }
    if source.is_empty() {
        return Ok(0.);
    }
    #[allow(unused_variables)]
    let acceleration = acceleration.resolve_for_distance_pairs(source.len());
    if acceleration.is_gpu() {
        if let Some(value) = crate::device::kernels().and_then(|kernels| {
            kernels.transformed_squared_distance_pair_sum(acceleration, source, target, m, t)
        }) {
            return Ok(value);
        }
    }
    transformed_squared_distance_pair_sum(source, target, m, t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ID, rotation, transform_points};

    fn points(n: usize, offset: f64) -> Vec<V3> {
        (0..n)
            .map(|i| {
                let f = i as f64 + offset;
                [f * 0.13 - 4., (f * 0.07).sin(), (f * 0.11).cos()]
            })
            .collect()
    }

    #[test]
    fn transformed_sum_matches_materialized_reference() {
        let source = points(64, 0.);
        let target = points(64, 1.);
        let m = rotation([0.2, -0.1, 0.3]);
        let t = [1., -0.5, 0.25];
        let transformed = transform_points(&source, m, t);
        let expected: f64 = transformed
            .iter()
            .zip(&target)
            .map(|(&a, &b)| {
                let d = sub(a, b);
                dot(d, d)
            })
            .sum();
        let got = transformed_squared_distance_pair_sum(&source, &target, m, t).unwrap();
        assert!((got - expected).abs() < 1e-12);
    }

    #[test]
    fn transformed_sum_rejects_mismatched_lengths() {
        assert!(transformed_squared_distance_pair_sum(&[[0.; 3]], &[], ID, [0.; 3]).is_err());
        assert!(
            transformed_squared_distance_pair_sum_accelerated(
                &[[0.; 3]],
                &[],
                ID,
                [0.; 3],
                Acceleration::Gpu
            )
            .is_err()
        );
    }

    #[test]
    fn transformed_rmse_handles_empty_input() {
        assert_eq!(
            transformed_squared_distance_pair_rmse(&[], &[], ID, [0.; 3]).unwrap(),
            0.
        );
    }

    #[test]
    fn transformed_sum_auto_matches_cpu_reference() {
        let source = points(128, 0.);
        let target = points(128, 1.);
        let m = rotation([0.2, -0.1, 0.3]);
        let t = [1., -0.5, 0.25];
        assert_eq!(
            transformed_squared_distance_pair_sum_accelerated(
                &source,
                &target,
                m,
                t,
                Acceleration::Auto
            )
            .unwrap(),
            transformed_squared_distance_pair_sum(&source, &target, m, t).unwrap()
        );
    }
}
