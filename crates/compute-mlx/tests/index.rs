#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use tensor_core::{CompareOp, HasShape, ScanOptions, Shape, TensorIndexBackend};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn backend() -> Option<MlxBackend> {
    match MlxBackend::new_gpu() {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("MLX unavailable: {e}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{e}");
            None
        }
    }
}

#[test]
fn device_gather_masks_invalid_indices_and_counts_each_index_once() {
    let Some(b) = backend() else { return };
    let data = b
        .upload_f32(shape(&[3, 2]), &[1., 4., 2., 5., 3., 6.])
        .unwrap();
    let input = b.permute(&data, &[1, 0]).unwrap();
    let raw_indices = b.upload_u32(shape(&[2, 2]), &[2, 0, u32::MAX, 3]).unwrap();
    let indices = b.permute_u32(&raw_indices, &[1, 0]).unwrap();
    let gathered = b.gather_f32(&input, &indices, 1).unwrap();
    assert_eq!(gathered.values.shape(), &shape(&[2, 2, 2]));
    assert_eq!(gathered.invalid_count.shape(), &shape(&[]));
    drop(data);
    drop(input);
    drop(raw_indices);
    drop(indices);
    assert_eq!(
        b.read_f32(&gathered.values).unwrap(),
        [3., 0., 1., 0., 6., 0., 4., 0.]
    );
    assert_eq!(b.read_u32(&gathered.invalid_count).unwrap(), [2]);

    let input = b
        .upload_u32(shape(&[2, 3]), &[u32::MAX, 2, 3, 4, 5, 6])
        .unwrap();
    let scalar = b.upload_u32(shape(&[]), &[0]).unwrap();
    let one = b.gather_u32(&input, &scalar, 1).unwrap();
    assert_eq!(one.values.shape(), &shape(&[2]));
    assert_eq!(b.read_u32(&one.values).unwrap(), [u32::MAX, 4]);
    assert_eq!(b.read_u32(&one.invalid_count).unwrap(), [0]);
    let invalid_scalar = b.upload_u32(shape(&[]), &[u32::MAX]).unwrap();
    let invalid = b.gather_u32(&input, &invalid_scalar, 1).unwrap();
    assert_eq!(b.read_u32(&invalid.values).unwrap(), [0, 0]);
    assert_eq!(b.read_u32(&invalid.invalid_count).unwrap(), [1]);

    let empty_axis = b.upload_f32(shape(&[2, 0, 3]), &[]).unwrap();
    let indices = b.upload_u32(shape(&[2]), &[0, u32::MAX]).unwrap();
    let gathered = b.gather_f32(&empty_axis, &indices, 1).unwrap();
    assert_eq!(gathered.values.shape(), &shape(&[2, 2, 3]));
    assert_eq!(b.read_f32(&gathered.values).unwrap(), [0.; 12]);
    assert_eq!(b.read_u32(&gathered.invalid_count).unwrap(), [2]);
    let empty_indices = b.upload_u32(shape(&[0, 2]), &[]).unwrap();
    let gathered = b.gather_f32(&empty_axis, &empty_indices, 1).unwrap();
    assert_eq!(gathered.values.shape(), &shape(&[2, 0, 2, 3]));
    assert!(b.read_f32(&gathered.values).unwrap().is_empty());
    assert_eq!(b.read_u32(&gathered.invalid_count).unwrap(), [0]);
    let empty_elsewhere = b.upload_u32(shape(&[0, 3]), &[]).unwrap();
    let gathered = b.gather_u32(&empty_elsewhere, &indices, 1).unwrap();
    assert!(b.read_u32(&gathered.values).unwrap().is_empty());
    assert_eq!(b.read_u32(&gathered.invalid_count).unwrap(), [1]);
}

#[test]
fn compaction_is_stable_for_strides_broadcast_masks_and_large_odd_counts() {
    let Some(b) = backend() else { return };
    let input = b
        .upload_f32(shape(&[3, 2]), &[1., 4., 2., 5., -0., 6.])
        .unwrap();
    let input = b.permute(&input, &[1, 0]).unwrap();
    let mask = b.upload_u32(shape(&[1, 3]), &[7, 0, u32::MAX]).unwrap();
    let compact = b.compact_f32(&input, &mask).unwrap();
    assert_eq!(compact.values.shape(), &shape(&[6]));
    assert_eq!(b.read_u32(&compact.count).unwrap(), [4]);
    let values = b.read_f32(&compact.values).unwrap();
    assert_eq!(values, [1., -0., 4., 6., 0., 0.]);
    assert_eq!(values[1].to_bits(), (-0.0_f32).to_bits());
    assert_eq!(values[4].to_bits(), 0);

    for n in [1, 2, 3, 31, 33, 257, 1025, 65_537] {
        let data: Vec<_> = (0..n).map(|i| u32::MAX - i as u32).collect();
        let mask: Vec<_> = (0..n)
            .map(|i| if i % 7 < 3 { u32::MAX } else { 0 })
            .collect();
        let input = b.upload_u32(shape(&[n]), &data).unwrap();
        let mask = b.upload_u32(shape(&[n]), &mask).unwrap();
        let selected = b.compact_u32(&input, &mask).unwrap();
        drop(input);
        drop(mask);
        let mut expected: Vec<_> = data
            .into_iter()
            .enumerate()
            .filter_map(|(i, v)| (i % 7 < 3).then_some(v))
            .collect();
        let count = expected.len();
        expected.resize(n, 0);
        assert_eq!(
            b.read_u32(&selected.count).unwrap(),
            [count as u32],
            "n={n}"
        );
        assert_eq!(b.read_u32(&selected.values).unwrap(), expected, "n={n}");
    }
}

#[test]
fn compaction_all_false_all_true_empty_scalar_and_resident_count() {
    let Some(b) = backend() else { return };
    let input = b.upload_u32(shape(&[5]), &[u32::MAX, 0, 9, 2, 7]).unwrap();
    for mask in [0, 1, u32::MAX] {
        let mask_tensor = b.upload_u32(shape(&[]), &[mask]).unwrap();
        let output = b.compact_u32(&input, &mask_tensor).unwrap();
        let expected = if mask == 0 {
            vec![0; 5]
        } else {
            vec![u32::MAX, 0, 9, 2, 7]
        };
        assert_eq!(b.read_u32(&output.values).unwrap(), expected);
        assert_eq!(
            b.read_u32(&output.count).unwrap(),
            [if mask == 0 { 0 } else { 5 }]
        );
        // The count can be consumed as a device scalar without reading it.
        let rescan = b
            .scan_u32(
                &output.values,
                0,
                ScanOptions {
                    inclusive: false,
                    reverse: true,
                },
            )
            .unwrap();
        let mask = b
            .compare_u32(CompareOp::Less, &rescan, &output.count)
            .unwrap();
        let again = b.compact_u32(&output.values, &mask).unwrap();
        assert_eq!(again.values.shape(), &shape(&[5]));
        b.eval(&again.values).unwrap();
    }
    let empty = b.upload_f32(shape(&[2, 0]), &[]).unwrap();
    let mask = b.upload_u32(shape(&[]), &[1]).unwrap();
    let output = b.compact_f32(&empty, &mask).unwrap();
    assert_eq!(output.values.shape(), &shape(&[0]));
    assert!(b.read_f32(&output.values).unwrap().is_empty());
    assert_eq!(b.read_u32(&output.count).unwrap(), [0]);
    let scalar = b.upload_f32(shape(&[]), &[-0.]).unwrap();
    let output = b.compact_f32(&scalar, &mask).unwrap();
    assert_eq!(output.values.shape(), &shape(&[1]));
    assert_eq!(
        b.read_f32(&output.values).unwrap()[0].to_bits(),
        (-0.0_f32).to_bits()
    );
    assert_eq!(b.read_u32(&output.count).unwrap(), [1]);
}

#[test]
fn index_methods_enforce_dtype_ownership_and_mask_shape() {
    let Some(b) = backend() else { return };
    let floats = b.upload_f32(shape(&[2]), &[1., 2.]).unwrap();
    let integers = b.upload_u32(shape(&[2]), &[0, 1]).unwrap();
    assert!(matches!(
        tensor_core::TensorBackend::materialize(&b, &integers),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        tensor_core::TensorBackend::sum_axes(&b, &integers, &[0], false),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(b.materialize_u32(&floats), Err(MlxError::Dtype)));
    assert!(matches!(
        b.scan_f32(&integers, 0, ScanOptions::default()),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        b.compare_u32(CompareOp::Less, &floats, &integers),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        b.select_f32(&integers, &floats, &integers),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        b.gather_u32(&integers, &floats, 0),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        b.compact_u32(&integers, &floats),
        Err(MlxError::Dtype)
    ));
    let expanded_mask = b.upload_u32(shape(&[2, 2]), &[1; 4]).unwrap();
    assert!(matches!(
        b.compact_u32(&integers, &expanded_mask),
        Err(MlxError::Contract(_))
    ));
    let other = MlxBackend::new_gpu().unwrap();
    let foreign = other.upload_u32(shape(&[2]), &[0, 1]).unwrap();
    assert!(matches!(
        b.select_u32(&foreign, &integers, &integers),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        b.gather_u32(&integers, &foreign, 0),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        b.compact_u32(&integers, &foreign),
        Err(MlxError::ForeignContext)
    ));
    assert_eq!(
        b.read_u32(&b.select_u32(&integers, &integers, &expanded_mask).unwrap())
            .unwrap(),
        [1, 1, 1, 1]
    );
}

#[test]
fn common_index_backend_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_index_backend(&b).unwrap();
}
