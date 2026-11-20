//! Shared wire packing, unpacking, and CPU partial folds for f32 GPU and CUDA math kernels.

pub(crate) mod wire;

#[cfg(test)]
mod tests {
    use super::wire;

    #[test]
    fn nearest_zip_and_chamfer_folds_preserve_indices_and_extrema() {
        let two = wire::zip_nearest_two(&[1, 3], &[0.5, 1.25], &[2, 0], &[2.0, 3.5]);
        assert_eq!(two, vec![[(1, 0.5), (2, 2.0)], [(3, 1.25), (0, 3.5)]]);

        let four = wire::zip_nearest_four(
            &[0, 1, 2, 3, 4, 5, 6, 7],
            &[1.0, 2.0, 3.0, 4.0, 0.25, 0.5, 0.75, 1.0],
            2,
        );
        assert_eq!(four.len(), 2);
        assert_eq!(four[0][2], (2, 3.0));
        assert_eq!(four[1][0], (4, 0.25));

        let chamfer = wire::fold_chamfer(&[1.5, 2.5], &[0.5, 3.0], 2);
        assert_eq!(chamfer.samples, 2);
        assert_eq!(chamfer.mean_squared_distance, 2.0);
        assert!((chamfer.rms_distance - 2.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(chamfer.max_squared_distance, 3.0);
    }

    #[cfg(any(target_arch = "wasm32", feature = "cuda"))]
    #[test]
    fn pack_and_aggregate_folds_handle_edges() {
        let flat = wire::pack_points_f32(&[[1.0, -2.5, 3.25]]);
        assert_eq!(flat, vec![1.0, -2.5, 3.25]);

        let tr = wire::pack_transform_f32(
            [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]],
            [4.0, 5.0, 6.0],
        );
        assert_eq!(
            tr,
            [1.0, 0.0, 0.0, 4.0, 0.0, 2.0, 0.0, 5.0, 0.0, 0.0, 3.0, 6.0]
        );

        let bounds = wire::fold_point_bounds(
            4,
            &[-1.0, 0.0, 2.0, -3.0, 1.0, 0.0],
            &[4.0, 2.0, 5.0, 1.0, 6.0, 3.0],
            2,
        );
        assert_eq!(bounds.samples, 4);
        assert_eq!(bounds.min, [-3.0, 0.0, 0.0]);
        assert_eq!(bounds.max, [4.0, 6.0, 5.0]);

        let moments =
            wire::fold_point_moments(2, &[2.0, 4.0, 6.0, 2.0, 8.0, 18.0, 0.0, 0.0, 0.0], 1);
        assert_eq!(moments.centroid, [1.0, 2.0, 3.0]);
        assert_eq!(wire::fold_f64_vec(&[0.5, 1.5]), vec![0.5, 1.5]);
        assert_eq!(wire::fold_f64_sum(&[1.25, 2.75]), 4.0);

        let stats = wire::fold_point_cloud_stats(
            2,
            &[
                0.0, 1.0, 2.0, 2.0, 3.0, 4.0, 2.0, 4.0, 6.0, 2.0, 8.0, 18.0, 0.0, 0.0, 0.0,
            ],
            1,
        );
        assert_eq!(stats.bounds.min, [0.0, 1.0, 2.0]);
        assert_eq!(stats.bounds.max, [2.0, 3.0, 4.0]);
        assert_eq!(stats.moments.centroid, [1.0, 2.0, 3.0]);
    }
}
