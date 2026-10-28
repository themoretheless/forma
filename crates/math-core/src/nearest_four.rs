use crate::{Acceleration, V3, dot, nearest_two_accelerated, sub};

pub type FourNearest = [crate::Neighbor; 4];

/// Brute-force exact top-4 nearest-neighbor search for every query point.
pub fn nearest_four(queries: &[V3], targets: &[V3]) -> Vec<FourNearest> {
    queries
        .iter()
        .map(|&q| {
            let mut best = [(u32::MAX, f64::INFINITY); 4];
            for (i, &t) in targets.iter().enumerate() {
                let d = sub(q, t);
                let dist = dot(d, d);
                let candidate = (i as u32, dist);
                if dist < best[0].1 {
                    best[3] = best[2];
                    best[2] = best[1];
                    best[1] = best[0];
                    best[0] = candidate;
                } else if dist < best[1].1 {
                    best[3] = best[2];
                    best[2] = best[1];
                    best[1] = candidate;
                } else if dist < best[2].1 {
                    best[3] = best[2];
                    best[2] = candidate;
                } else if dist < best[3].1 {
                    best[3] = candidate;
                }
            }
            best
        })
        .collect()
}

/// [`nearest_four`] with optional GPU/CUDA batch kernels.
pub fn nearest_four_accelerated(
    queries: &[V3],
    targets: &[V3],
    #[allow(unused_variables)] acceleration: Acceleration,
) -> Vec<FourNearest> {
    if queries.is_empty() || targets.len() < 4 {
        return nearest_four(queries, targets);
    }
    #[allow(unused_variables)]
    let acceleration = acceleration.resolve_for_nearest_neighbor(queries.len(), targets.len());
    if acceleration.is_gpu() {
        if let Some(values) = crate::device::kernels()
            .and_then(|kernels| kernels.nearest_four(acceleration, queries, targets))
        {
            return values;
        }
    }
    nearest_four(queries, targets)
}

/// Uses the existing top-2 kernel for workloads that only need two neighbors.
pub fn nearest_four_first_two(
    queries: &[V3],
    targets: &[V3],
    acceleration: Acceleration,
) -> Vec<crate::TwoNearest> {
    nearest_two_accelerated(queries, targets, acceleration)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clouds() -> (Vec<V3>, Vec<V3>) {
        let queries = vec![[0., 0., 0.], [10., 0., 0.], [-5., -5., -5.]];
        let targets = vec![
            [1., 0., 0.],
            [0., 0., 0.9],
            [9., 0.5, 0.],
            [-4., -4., -4.],
            [0.5, 0.5, 0.5],
        ];
        (queries, targets)
    }

    #[test]
    fn nearest_four_finds_sorted_neighbors() {
        let (queries, targets) = clouds();
        let got = nearest_four(&queries, &targets);
        assert_eq!(got[0][0], (4, 0.75));
        assert_eq!(got[0][1], (1, 0.81));
        assert_eq!(got[0][2], (0, 1.));
        assert_eq!(got[1][0].0, 2);
        assert_eq!(got[2][0].0, 3);
    }

    #[test]
    fn nearest_four_handles_short_targets() {
        assert!(nearest_four(&[], &[[0., 0., 0.]]).is_empty());
        assert_eq!(
            nearest_four(&[[0., 0., 0.]], &[]),
            vec![[(u32::MAX, f64::INFINITY); 4]]
        );
        let got = nearest_four(&[[0., 0., 0.]], &[[1., 0., 0.], [0., 2., 0.]]);
        assert_eq!(got[0][0], (0, 1.));
        assert_eq!(got[0][1], (1, 4.));
        assert_eq!(got[0][2], (u32::MAX, f64::INFINITY));
    }
}
