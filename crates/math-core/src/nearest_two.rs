use crate::{Acceleration, V3, dot, nearest_neighbor_accelerated, sub};

pub type Neighbor = (u32, f64);
pub type TwoNearest = [Neighbor; 2];

/// Brute-force exact top-2 nearest-neighbor search for every query point.
pub fn nearest_two(queries: &[V3], targets: &[V3]) -> Vec<TwoNearest> {
    queries
        .iter()
        .map(|&q| {
            let mut best = (u32::MAX, f64::INFINITY);
            let mut second = (u32::MAX, f64::INFINITY);
            for (i, &t) in targets.iter().enumerate() {
                let d = sub(q, t);
                let dist = dot(d, d);
                if dist < best.1 {
                    second = best;
                    best = (i as u32, dist);
                } else if dist < second.1 {
                    second = (i as u32, dist);
                }
            }
            [best, second]
        })
        .collect()
}

/// [`nearest_two`] with optional GPU/CUDA batch kernels.
pub fn nearest_two_accelerated(
    queries: &[V3],
    targets: &[V3],
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Vec<TwoNearest> {
    if queries.is_empty() || targets.len() < 2 {
        return nearest_two(queries, targets);
    }
    #[allow(unused_variables)]
    let acceleration = acceleration.resolve_for_nearest_neighbor(queries.len(), targets.len());
    if acceleration.is_gpu() {
        if let Some(values) = crate::device::kernels()
            .and_then(|kernels| kernels.nearest_two(acceleration, queries, targets))
        {
            return values;
        }
    }
    nearest_two(queries, targets)
}

/// Exact nearest neighbor and second-nearest ratio for every query point.
pub fn nearest_two_ratios_accelerated(
    queries: &[V3],
    targets: &[V3],
    acceleration: Acceleration,
) -> Vec<(u32, f64, f64)> {
    nearest_two_accelerated(queries, targets, acceleration)
        .into_iter()
        .map(|[best, second]| {
            let ratio = if second.1.is_finite() && second.1 > 0. {
                (best.1 / second.1).sqrt()
            } else {
                f64::INFINITY
            };
            (best.0, best.1, ratio)
        })
        .collect()
}

/// Uses the existing nearest-neighbor kernel for workloads that only need top-1.
pub fn nearest_two_first_only(
    queries: &[V3],
    targets: &[V3],
    acceleration: Acceleration,
) -> Vec<Neighbor> {
    nearest_neighbor_accelerated(queries, targets, acceleration)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clouds() -> (Vec<V3>, Vec<V3>) {
        let queries = vec![[0., 0., 0.], [10., 0., 0.], [-5., -5., -5.]];
        let targets = vec![[1., 0., 0.], [0., 0., 0.9], [9., 0.5, 0.], [-4., -4., -4.]];
        (queries, targets)
    }

    #[test]
    fn nearest_two_finds_best_and_second_best() {
        let (queries, targets) = clouds();
        let got = nearest_two(&queries, &targets);
        assert_eq!(got[0][0], (1, 0.81));
        assert_eq!(got[0][1], (0, 1.0));
        assert_eq!(got[1][0].0, 2);
        assert_eq!(got[2][0].0, 3);
    }

    #[test]
    fn nearest_two_handles_empty_and_single_target() {
        assert!(nearest_two(&[], &[[0., 0., 0.]]).is_empty());
        assert_eq!(
            nearest_two(&[[0., 0., 0.]], &[]),
            vec![[(u32::MAX, f64::INFINITY), (u32::MAX, f64::INFINITY)]]
        );
        assert_eq!(
            nearest_two(&[[0., 0., 0.]], &[[1., 0., 0.]]),
            vec![[(0, 1.), (u32::MAX, f64::INFINITY)]]
        );
    }

    #[test]
    fn nearest_two_first_only_matches_nearest_neighbor() {
        let (queries, targets) = clouds();
        let got = nearest_two_first_only(&queries, &targets, Acceleration::Cpu);
        let want: Vec<_> = nearest_two(&queries, &targets)
            .into_iter()
            .map(|pair| pair[0])
            .collect();
        assert_eq!(got, want);
    }
}
