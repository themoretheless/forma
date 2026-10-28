use crate::{Acceleration, Error, Result, V3, dot, sub};

/// One-to-one squared Euclidean distances for corresponding point pairs.
pub fn squared_distance_pairs(a: &[V3], b: &[V3]) -> Result<Vec<f64>> {
    if a.len() != b.len() {
        return Err(Error::new(
            "invalid_distance_pairs_input",
            "squared_distance_pairs expects equal point counts",
        ));
    }
    Ok(a.iter()
        .zip(b)
        .map(|(&a, &b)| {
            let d = sub(a, b);
            dot(d, d)
        })
        .collect())
}

/// Sum of one-to-one squared Euclidean distances for corresponding point pairs.
pub fn squared_distance_pair_sum(a: &[V3], b: &[V3]) -> Result<f64> {
    Ok(squared_distance_pairs(a, b)?.into_iter().sum())
}

/// [`squared_distance_pairs`] with optional GPU/CUDA batch kernels.
pub fn squared_distance_pairs_accelerated(
    a: &[V3],
    b: &[V3],
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Result<Vec<f64>> {
    if a.len() != b.len() {
        return Err(Error::new(
            "invalid_distance_pairs_input",
            "squared_distance_pairs_accelerated expects equal point counts",
        ));
    }
    if a.is_empty() {
        return Ok(Vec::new());
    }
    #[allow(unused_variables)]
    let acceleration = acceleration.resolve_for_distance_pairs(a.len());
    if acceleration.is_gpu() {
        if let Some(values) = crate::device::kernels()
            .and_then(|kernels| kernels.squared_distance_pairs(acceleration, a, b))
        {
            return Ok(values);
        }
    }
    squared_distance_pairs(a, b)
}

/// [`squared_distance_pair_sum`] with optional GPU/CUDA partial reduction.
pub fn squared_distance_pair_sum_accelerated(
    a: &[V3],
    b: &[V3],
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Result<f64> {
    if a.len() != b.len() {
        return Err(Error::new(
            "invalid_distance_pairs_input",
            "squared_distance_pair_sum_accelerated expects equal point counts",
        ));
    }
    if a.is_empty() {
        return Ok(0.);
    }
    #[allow(unused_variables)]
    let acceleration = acceleration.resolve_for_distance_pairs(a.len());
    if acceleration.is_gpu() {
        if let Some(value) = crate::device::kernels()
            .and_then(|kernels| kernels.squared_distance_pair_sum(acceleration, a, b))
        {
            return Ok(value);
        }
    }
    squared_distance_pair_sum(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(n: usize, offset: f64) -> Vec<V3> {
        (0..n)
            .map(|i| {
                let f = i as f64 + offset;
                [f * 0.13 - 4., (f * 0.07).sin(), (f * 0.11).cos()]
            })
            .collect()
    }

    #[test]
    fn squared_distance_pairs_matches_scalar_reference() {
        let a = vec![[0., 0., 0.], [1., 2., 3.]];
        let b = vec![[1., 0., 0.], [3., 2., -1.]];
        assert_eq!(squared_distance_pairs(&a, &b).unwrap(), vec![1., 20.]);
    }

    #[test]
    fn squared_distance_pairs_rejects_mismatched_lengths() {
        assert!(squared_distance_pairs(&[[0.; 3]], &[]).is_err());
        assert!(squared_distance_pairs_accelerated(&[[0.; 3]], &[], Acceleration::Gpu).is_err());
    }

    #[test]
    fn squared_distance_pairs_accelerated_cpu_matches_reference() {
        let a = points(64, 0.);
        let b = points(64, 1.);
        assert_eq!(
            squared_distance_pairs_accelerated(&a, &b, Acceleration::Cpu).unwrap(),
            squared_distance_pairs(&a, &b).unwrap()
        );
    }

    #[test]
    fn squared_distance_pair_sum_matches_vector_sum() {
        let a = points(64, 0.);
        let b = points(64, 1.);
        let vector_sum: f64 = squared_distance_pairs(&a, &b).unwrap().into_iter().sum();
        assert_eq!(squared_distance_pair_sum(&a, &b).unwrap(), vector_sum);
        assert_eq!(
            squared_distance_pair_sum_accelerated(&a, &b, Acceleration::Cpu).unwrap(),
            vector_sum
        );
    }
}
