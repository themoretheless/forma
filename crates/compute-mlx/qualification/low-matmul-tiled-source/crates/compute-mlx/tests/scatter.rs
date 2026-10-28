#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use tensor_core::{HasShape, ScatterOp, Shape, TensorScatterBackend};

const OPS: [ScatterOp; 5] = [
    ScatterOp::Replace,
    ScatterOp::Add,
    ScatterOp::Multiply,
    ScatterOp::Min,
    ScatterOp::Max,
];
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
fn replace_duplicate_winners_follow_logical_index_order_repeatedly() {
    let Some(b) = backend() else { return };
    let n = 4_101;
    let index_values: Vec<u32> = (0..n)
        .map(|i| {
            if i % 13 == 0 {
                u32::MAX
            } else {
                (i % 17) as u32
            }
        })
        .collect();
    let update_values: Vec<u32> = (0..n).map(|i| u32::MAX - i as u32 * 19).collect();
    let indices = b.upload_u32(shape(&[3, 1367]), &index_values).unwrap();
    let indices = b.permute(&indices, &[1, 0]).unwrap();
    let updates = b.upload_u32(shape(&[3, 1367]), &update_values).unwrap();
    let updates = b.permute(&updates, &[1, 0]).unwrap();
    let original = [11u32; 17];
    let base = b.upload_u32(shape(&[17]), &original).unwrap();
    let mut expected = original;
    let mut invalid = 0;
    for column in 0..1367 {
        for row in 0..3 {
            let flat = row * 1367 + column;
            let index = index_values[flat] as usize;
            if index < 17 {
                expected[index] = update_values[flat];
            } else {
                invalid += 1;
            }
        }
    }
    for _ in 0..5 {
        let result = b
            .scatter_u32(ScatterOp::Replace, &base, &indices, &updates, 0)
            .unwrap();
        assert_eq!(result.values.shape(), &shape(&[17]));
        assert_eq!(b.read_u32(&result.values).unwrap(), expected);
        assert_eq!(b.read_u32(&result.invalid_count).unwrap(), [invalid]);
    }
    assert_eq!(b.read_u32(&base).unwrap(), original);
}

#[test]
fn scatter_reductions_broadcast_updates_and_fold_exact_unsigned_values() {
    let Some(b) = backend() else { return };
    let base_values = [1, 2, u32::MAX, 4, 5, 16_777_217];
    let base = b
        .upload_u32(shape(&[3, 2]), &[1, 4, 2, 5, u32::MAX, 16_777_217])
        .unwrap();
    let base = b.permute(&base, &[1, 0]).unwrap();
    let indices = [2u32, 0, 2, u32::MAX];
    let update_values = [65_537u32, 3, u32::MAX, 7];
    let index_tensor = b.upload_u32(shape(&[2, 2]), &indices).unwrap();
    let updates = b.upload_u32(shape(&[2, 2]), &update_values).unwrap();
    for op in OPS {
        let mut expected = base_values;
        for row in 0..2 {
            for (&index, &update) in indices.iter().zip(&update_values) {
                if index >= 3 {
                    continue;
                }
                let value = &mut expected[row * 3 + index as usize];
                *value = match op {
                    ScatterOp::Replace => update,
                    ScatterOp::Add => value.wrapping_add(update),
                    ScatterOp::Multiply => value.wrapping_mul(update),
                    ScatterOp::Min => (*value).min(update),
                    ScatterOp::Max => (*value).max(update),
                };
            }
        }
        let result = b
            .scatter_u32(op, &base, &index_tensor, &updates, 1)
            .unwrap();
        assert_eq!(b.read_u32(&result.values).unwrap(), expected, "{op:?}");
        assert_eq!(b.read_u32(&result.invalid_count).unwrap(), [1]);
    }
}

#[test]
fn invalid_empty_and_scalar_scatter_inputs_preserve_contract() {
    let Some(b) = backend() else { return };
    let values = [-0.0_f32, 3.0, -7.0, f32::MIN_POSITIVE];
    let base = b.upload_f32(shape(&[4]), &values).unwrap();
    let invalid = b.upload_u32(shape(&[]), &[u32::MAX]).unwrap();
    let updates = b.upload_f32(shape(&[]), &[13.0]).unwrap();
    for op in OPS {
        let result = b.scatter_f32(op, &base, &invalid, &updates, 0).unwrap();
        let actual = b.read_f32(&result.values).unwrap();
        assert_eq!(
            actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            values.map(f32::to_bits),
            "{op:?}"
        );
        assert_eq!(b.read_u32(&result.invalid_count).unwrap(), [1]);
    }
    let empty_indices = b.upload_u32(shape(&[0, 2]), &[]).unwrap();
    let result = b
        .scatter_f32(ScatterOp::Replace, &base, &empty_indices, &updates, 0)
        .unwrap();
    assert_eq!(b.read_f32(&result.values).unwrap(), values);
    assert_eq!(b.read_u32(&result.invalid_count).unwrap(), [0]);
    let index = b.upload_u32(shape(&[]), &[2]).unwrap();
    let result = b
        .scatter_f32(ScatterOp::Replace, &base, &index, &updates, 0)
        .unwrap();
    assert_eq!(
        b.read_f32(&result.values).unwrap(),
        [-0.0, 3., 13., f32::MIN_POSITIVE]
    );
    let empty_axis = b.upload_f32(shape(&[2, 0, 3]), &[]).unwrap();
    let indices = b.upload_u32(shape(&[2]), &[0, 1]).unwrap();
    for op in OPS {
        let result = b
            .scatter_f32(op, &empty_axis, &indices, &updates, 1)
            .unwrap();
        assert!(b.read_f32(&result.values).unwrap().is_empty());
        assert_eq!(b.read_u32(&result.invalid_count).unwrap(), [2]);
    }
    let empty_slices = b.upload_f32(shape(&[0, 3]), &[]).unwrap();
    let result = b
        .scatter_f32(ScatterOp::Add, &empty_slices, &invalid, &updates, 1)
        .unwrap();
    assert!(b.read_f32(&result.values).unwrap().is_empty());
    assert_eq!(b.read_u32(&result.invalid_count).unwrap(), [1]);
}

#[test]
fn scatter_rejects_foreign_dtypes_and_incompatible_updates() {
    let Some(b) = backend() else { return };
    let base = b.upload_f32(shape(&[2, 3]), &[0.; 6]).unwrap();
    let indices = b.upload_u32(shape(&[2]), &[0, 1]).unwrap();
    let scalar = b.upload_f32(shape(&[]), &[2.]).unwrap();
    assert!(matches!(
        b.scatter_f32(ScatterOp::Replace, &base, &base, &scalar, 1),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        b.scatter_f32(ScatterOp::Replace, &base, &indices, &indices, 1),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        b.scatter_u32(ScatterOp::Replace, &base, &indices, &indices, 1),
        Err(MlxError::Dtype)
    ));
    let updates = b.upload_f32(shape(&[3]), &[1.; 3]).unwrap();
    assert!(matches!(
        b.scatter_f32(ScatterOp::Add, &base, &indices, &updates, 1),
        Err(MlxError::Contract(_))
    ));
    let other = MlxBackend::new_gpu().unwrap();
    let foreign = other.upload_u32(shape(&[2]), &[0, 1]).unwrap();
    assert!(matches!(
        b.scatter_f32(ScatterOp::Add, &base, &foreign, &scalar, 1),
        Err(MlxError::ForeignContext)
    ));
}

#[test]
fn common_scatter_backend_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_scatter_backend(&b).unwrap();
}
