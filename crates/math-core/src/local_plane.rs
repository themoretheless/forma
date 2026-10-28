use crate::{
    Acceleration, Error, PointPlane, Result, V3, finite, nearest_four_accelerated, point_fit_plane,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalPlane {
    pub query: V3,
    pub plane: PointPlane,
    pub neighbors: usize,
    pub max_squared_neighbor_distance: f64,
}

/// Fits a small least-squares plane around every query from its nearest support
/// points. The top-4 nearest-neighbor stage can run on CPU, wgpu/Metal or CUDA;
/// each tiny neighborhood PCA fit runs on CPU.
pub fn local_point_planes(
    queries: &[V3],
    support: &[V3],
    acceleration: Acceleration,
) -> Result<Vec<LocalPlane>> {
    if support.len() < 3 {
        return Err(Error::new(
            "invalid_local_plane_input",
            "local_point_planes expects at least 3 support points",
        ));
    }
    if queries.iter().chain(support).any(|&p| !finite(p)) {
        return Err(Error::new(
            "invalid_local_plane_input",
            "local_point_planes expects finite and bounded point coordinates",
        ));
    }
    let neighborhoods = nearest_four_accelerated(queries, support, acceleration);
    let mut out = Vec::with_capacity(queries.len());
    for (&query, neighbors) in queries.iter().zip(neighborhoods) {
        let mut points = Vec::with_capacity(4);
        let mut max_squared_neighbor_distance = 0.;
        for (index, squared) in neighbors {
            if index == u32::MAX {
                continue;
            }
            points.push(support[index as usize]);
            max_squared_neighbor_distance = squared.max(max_squared_neighbor_distance);
        }
        if points.len() < 3 {
            return Err(Error::new(
                "insufficient_local_plane_neighbors",
                "local_point_planes found fewer than 3 neighbors for a query",
            ));
        }
        out.push(LocalPlane {
            query,
            plane: point_fit_plane(&points, Acceleration::Cpu)?,
            neighbors: points.len(),
            max_squared_neighbor_distance,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dot;

    fn plane_points(nx: usize, ny: usize) -> Vec<V3> {
        (0..nx)
            .flat_map(|ix| {
                (0..ny).map(move |iy| {
                    let x = ix as f64 * 0.2 - 2.;
                    let y = iy as f64 * 0.2 - 2.;
                    let z = 1. + 0.2 * x - 0.1 * y;
                    [x, y, z]
                })
            })
            .collect()
    }

    #[test]
    fn local_point_planes_fit_query_neighborhoods() {
        let support = plane_points(12, 12);
        let queries = vec![support[50], support[75], [0.1, -0.2, 1.04]];
        let planes = local_point_planes(&queries, &support, Acceleration::Cpu).unwrap();
        assert_eq!(planes.len(), queries.len());
        let want = {
            let mut n = [-0.2, 0.1, 1.];
            let len = dot(n, n).sqrt();
            n = [n[0] / len, n[1] / len, n[2] / len];
            n
        };
        for local in planes {
            assert!(local.neighbors >= 3);
            assert!(local.max_squared_neighbor_distance.is_finite());
            for (axis, _) in want.iter().enumerate() {
                assert!((local.plane.normal[axis] - want[axis]).abs() < 1e-10);
            }
            for point in &support {
                assert!((dot(local.plane.normal, *point) + local.plane.offset).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn local_point_planes_reject_invalid_inputs() {
        assert!(local_point_planes(&[[0.; 3]], &[[0.; 3], [1.; 3]], Acceleration::Cpu).is_err());
        assert!(
            local_point_planes(
                &[[0., f64::INFINITY, 0.]],
                &[[0.; 3], [1., 0., 0.], [0., 1., 0.]],
                Acceleration::Cpu
            )
            .is_err()
        );
    }
}
