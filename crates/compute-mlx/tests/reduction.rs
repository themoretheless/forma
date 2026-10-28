#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use tensor_core::{HasShape, MatmulPrecision, ReduceOp, Shape, TensorError, TensorReduceBackend};

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
fn unsigned_reductions_remain_exact_for_strided_inputs_and_wrap_products() {
    let Some(b) = backend() else { return };
    let values: Vec<u32> = (0..2 * 513)
        .map(|i| match i % 7 {
            0 => u32::MAX,
            1 => 65_537,
            2 => 16_777_217,
            _ => 1,
        })
        .collect();
    let input = b.upload_u32(shape(&[2, 513]), &values).unwrap();
    let input = b.permute(&input, &[1, 0]).unwrap();
    for op in [
        ReduceOp::Sum,
        ReduceOp::Product,
        ReduceOp::Min,
        ReduceOp::Max,
    ] {
        let expected: Vec<u32> = values
            .chunks(513)
            .map(|row| match op {
                ReduceOp::Sum => row.iter().copied().fold(0, u32::wrapping_add),
                ReduceOp::Product => row.iter().copied().fold(1, u32::wrapping_mul),
                ReduceOp::Min => *row.iter().min().unwrap(),
                ReduceOp::Max => *row.iter().max().unwrap(),
            })
            .collect();
        let output = b.reduce_u32(op, &input, &[0], false).unwrap();
        assert_eq!(output.shape(), &shape(&[2]));
        assert_eq!(b.read_u32(&output).unwrap(), expected, "{op:?}");
        let output = b.reduce_u32(op, &input, &[0], true).unwrap();
        assert_eq!(output.shape(), &shape(&[1, 2]));
        assert_eq!(b.read_u32(&output).unwrap(), expected, "{op:?} keep_dims");
    }
    let pair = b.upload_u32(shape(&[2]), &[65_537, 65_537]).unwrap();
    let result = b.reduce_u32(ReduceOp::Product, &pair, &[0], false).unwrap();
    assert_eq!(b.read_u32(&result).unwrap(), [131_073]);
}

#[test]
fn empty_reduction_contracts_are_checked_before_native_evaluation() {
    let Some(b) = backend() else { return };
    let floats = b.upload_f32(shape(&[2, 0, 3]), &[]).unwrap();
    let integers = b.upload_u32(shape(&[2, 0, 3]), &[]).unwrap();
    for (op, identity) in [(ReduceOp::Sum, 0u32), (ReduceOp::Product, 1)] {
        let result = b.reduce_f32(op, &floats, &[1], false).unwrap();
        assert_eq!(result.shape(), &shape(&[2, 3]));
        assert_eq!(b.read_f32(&result).unwrap(), [identity as f32; 6]);
        let result = b.reduce_u32(op, &integers, &[1], true).unwrap();
        assert_eq!(result.shape(), &shape(&[2, 1, 3]));
        assert_eq!(b.read_u32(&result).unwrap(), [identity; 6]);
    }
    for op in [ReduceOp::Min, ReduceOp::Max] {
        assert!(matches!(
            b.reduce_f32(op, &floats, &[1], false),
            Err(MlxError::Contract(TensorError::EmptyReduction))
        ));
        assert!(matches!(
            b.reduce_u32(op, &integers, &[1], false),
            Err(MlxError::Contract(TensorError::EmptyReduction))
        ));
    }
    assert!(matches!(
        b.mean_axes(&floats, &[1], false),
        Err(MlxError::Contract(TensorError::EmptyReduction))
    ));
    let empty_output = b.upload_f32(shape(&[0, 0, 3]), &[]).unwrap();
    let empty_u = b.upload_u32(shape(&[0, 0, 3]), &[]).unwrap();
    for op in [
        ReduceOp::Sum,
        ReduceOp::Product,
        ReduceOp::Min,
        ReduceOp::Max,
    ] {
        assert!(
            b.read_f32(&b.reduce_f32(op, &empty_output, &[1], false).unwrap())
                .unwrap()
                .is_empty()
        );
        assert!(
            b.read_u32(&b.reduce_u32(op, &empty_u, &[1], false).unwrap())
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        b.read_f32(&b.mean_axes(&empty_output, &[1], false).unwrap())
            .unwrap()
            .is_empty()
    );
    let scalar = b.upload_f32(shape(&[]), &[-0.]).unwrap();
    for op in [
        ReduceOp::Sum,
        ReduceOp::Product,
        ReduceOp::Min,
        ReduceOp::Max,
    ] {
        let result = b.reduce_f32(op, &scalar, &[], true).unwrap();
        assert_eq!(result.shape(), &shape(&[]));
        assert_eq!(
            b.read_f32(&result).unwrap()[0].to_bits(),
            (-0.0_f32).to_bits()
        );
    }
    assert_eq!(
        b.read_f32(&b.mean_axes(&scalar, &[], false).unwrap())
            .unwrap()[0]
            .to_bits(),
        (-0.0_f32).to_bits()
    );
    assert!(matches!(
        b.reduce_u32(ReduceOp::Sum, &scalar, &[], false),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        b.reduce_f32(ReduceOp::Sum, &integers, &[], false),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        b.mean_axes(&integers, &[], false),
        Err(MlxError::Dtype)
    ));
    assert!(b.reduce_f32(ReduceOp::Sum, &scalar, &[0], false).is_err());
}

#[test]
fn vector_matmul_uses_native_promotion_with_broadcast_and_empty_contraction() {
    let Some(b) = backend() else { return };
    let vector = b.upload_f32(shape(&[3]), &[1., 2., 3.]).unwrap();
    let dot = b.matmul(&vector, &vector, MatmulPrecision::F32).unwrap();
    assert_eq!(dot.shape(), &shape(&[]));
    assert_eq!(b.read_f32(&dot).unwrap(), [14.]);
    let matrix = b
        .upload_f32(
            shape(&[2, 3, 2]),
            &[1., 0., 0., 1., 1., 1., 2., 0., 0., 2., 2., 2.],
        )
        .unwrap();
    let left = b.matmul(&vector, &matrix, MatmulPrecision::F32).unwrap();
    assert_eq!(left.shape(), &shape(&[2, 2]));
    assert_eq!(b.read_f32(&left).unwrap(), [4., 5., 8., 10.]);
    let transposed = b.permute(&matrix, &[0, 2, 1]).unwrap();
    let right = b
        .matmul(&transposed, &vector, MatmulPrecision::F32)
        .unwrap();
    assert_eq!(right.shape(), &shape(&[2, 2]));
    assert_eq!(b.read_f32(&right).unwrap(), [4., 5., 8., 10.]);
    let scalar = b.upload_f32(shape(&[]), &[2.]).unwrap();
    let broadcast = b.broadcast_to(&scalar, shape(&[3])).unwrap();
    assert_eq!(
        b.read_f32(&b.matmul(&vector, &broadcast, MatmulPrecision::F32).unwrap())
            .unwrap(),
        [12.]
    );
    let empty = b.upload_f32(shape(&[0]), &[]).unwrap();
    let dot = b.matmul(&empty, &empty, MatmulPrecision::F32).unwrap();
    assert_eq!(dot.shape(), &shape(&[]));
    assert_eq!(b.read_f32(&dot).unwrap(), [0.]);
    let empty_matrix = b.upload_f32(shape(&[2, 0, 3]), &[]).unwrap();
    let product = b
        .matmul(&empty, &empty_matrix, MatmulPrecision::F32)
        .unwrap();
    assert_eq!(product.shape(), &shape(&[2, 3]));
    assert_eq!(b.read_f32(&product).unwrap(), [0.; 6]);
    assert!(b.matmul(&scalar, &vector, MatmulPrecision::F32).is_err());
    assert!(matches!(
        b.matmul(&vector, &vector, MatmulPrecision::AllowF16),
        Err(MlxError::UnsupportedPrecision(_))
    ));
}

#[test]
fn means_use_logical_broadcast_counts_and_remain_resident() {
    let Some(b) = backend() else { return };
    let row = b.upload_f32(shape(&[1, 3]), &[-2., 0.5, 4.]).unwrap();
    let repeated = b.broadcast_to(&row, shape(&[4097, 3])).unwrap();
    let mean = b.mean_axes(&repeated, &[0], false).unwrap();
    drop(repeated);
    drop(row);
    let squared = b.unary(tensor_core::UnaryOp::Square, &mean).unwrap();
    let result = b.reduce_f32(ReduceOp::Sum, &squared, &[0], false).unwrap();
    assert!((b.read_f32(&result).unwrap()[0] - 20.25).abs() < 1e-4);
}

#[test]
fn common_reduce_backend_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_reduce_backend(&b).unwrap();
}

#[test]
fn vector_matmul_empty_batches_preserve_empty_shape() {
    let Some(b) = backend() else { return };
    let vector = b.upload_f32(shape(&[3]), &[2., -1., 3.]).unwrap();
    let empty_batch = b.upload_f32(shape(&[0, 2, 3]), &[]).unwrap();
    let result = b
        .matmul(&empty_batch, &vector, MatmulPrecision::F32)
        .unwrap();
    assert_eq!(result.shape(), &shape(&[0, 2]));
    assert!(b.read_f32(&result).unwrap().is_empty());
    let empty_batch = b.permute(&empty_batch, &[0, 2, 1]).unwrap();
    let result = b
        .matmul(&vector, &empty_batch, MatmulPrecision::F32)
        .unwrap();
    assert_eq!(result.shape(), &shape(&[0, 2]));
    assert!(b.read_f32(&result).unwrap().is_empty());
}
