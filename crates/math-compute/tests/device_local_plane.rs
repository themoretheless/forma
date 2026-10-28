//! Device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]
use math_core::*;

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
fn local_point_planes_gpu_matches_cpu_reference() {
    math_compute::install();
    let support = plane_points(16, 16);
    let queries: Vec<_> = support.iter().step_by(17).copied().collect();
    let got = local_point_planes(&queries, &support, Acceleration::Gpu).unwrap();
    let want = local_point_planes(&queries, &support, Acceleration::Cpu).unwrap();
    assert_eq!(got.len(), want.len());
    for (got, want) in got.iter().zip(&want) {
        assert_eq!(got.neighbors, want.neighbors);
        for axis in 0..3 {
            assert!((got.plane.normal[axis] - want.plane.normal[axis]).abs() < 1e-4);
        }
    }
}

#[cfg(feature = "cuda")]
#[test]
fn local_point_planes_cuda_matches_cpu_reference() {
    math_compute::install();
    let support = plane_points(16, 16);
    let queries: Vec<_> = support.iter().step_by(17).copied().collect();
    let got = local_point_planes(&queries, &support, Acceleration::Cuda).unwrap();
    let want = local_point_planes(&queries, &support, Acceleration::Cpu).unwrap();
    assert_eq!(got.len(), want.len());
    for (got, want) in got.iter().zip(&want) {
        assert_eq!(got.neighbors, want.neighbors);
        for axis in 0..3 {
            assert!((got.plane.normal[axis] - want.plane.normal[axis]).abs() < 1e-4);
        }
    }
}
