#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxDtype, MlxError};
use tensor_core::{BinaryOp, CompareOp, HasShape, MatmulPrecision, Shape, TensorError, UnaryOp};
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
fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        assert!((a - e).abs() <= 1e-5 * e.abs().max(1.), "{a} != {e}");
    }
}

#[test]
fn strided_views_broadcast_and_lazy_lifetime_follow_logical_values() {
    let Some(b) = backend() else { return };
    eprintln!("MLX runtime={} GPU stream", b.version());
    let a = b
        .upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])
        .unwrap();
    let trans = b.permute(&a, &[1, 0]).unwrap();
    drop(a);
    assert_eq!(trans.shape(), &shape(&[3, 2]));
    close(&b.read_f32(&trans).unwrap(), &[1., 4., 2., 5., 3., 6.]);
    let reshaped = b.reshape(&trans, shape(&[6])).unwrap();
    drop(trans);
    close(&b.read_f32(&reshaped).unwrap(), &[1., 4., 2., 5., 3., 6.]);
    let row = b.upload_f32(shape(&[1, 3]), &[10., 20., 30.]).unwrap();
    let broadcast = b.broadcast_to(&row, shape(&[2, 3])).unwrap();
    let scalar = b.upload_f32(shape(&[]), &[2.]).unwrap();
    let output = b.binary(BinaryOp::Multiply, &broadcast, &scalar).unwrap();
    drop(row);
    drop(broadcast);
    drop(scalar);
    close(
        &b.read_f32(&output).unwrap(),
        &[20., 40., 60., 20., 40., 60.],
    );
}

#[test]
fn arithmetic_comparison_and_axis_reduction_match_references() {
    let Some(b) = backend() else { return };
    let input = b
        .upload_f32(shape(&[2, 3]), &[0.5, 1., 2., 3., 4., 5.])
        .unwrap();
    for op in [
        UnaryOp::Negate,
        UnaryOp::Abs,
        UnaryOp::Square,
        UnaryOp::Sqrt,
        UnaryOp::Reciprocal,
        UnaryOp::Exp,
        UnaryOp::Log,
        UnaryOp::Sin,
        UnaryOp::Cos,
    ] {
        let expected: Vec<f32> = [0.5_f32, 1., 2., 3., 4., 5.]
            .into_iter()
            .map(|x| match op {
                UnaryOp::Negate => -x,
                UnaryOp::Abs => x.abs(),
                UnaryOp::Square => x * x,
                UnaryOp::Sqrt => x.sqrt(),
                UnaryOp::Reciprocal => x.recip(),
                UnaryOp::Exp => x.exp(),
                UnaryOp::Log => x.ln(),
                UnaryOp::Sin => x.sin(),
                UnaryOp::Cos => x.cos(),
            })
            .collect();
        close(
            &b.read_f32(&b.unary(op, &input).unwrap()).unwrap(),
            &expected,
        );
    }
    let two = b.upload_f32(shape(&[]), &[2.]).unwrap();
    for op in [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Min,
        BinaryOp::Max,
    ] {
        let expected: Vec<f32> = [0.5_f32, 1., 2., 3., 4., 5.]
            .into_iter()
            .map(|x| match op {
                BinaryOp::Add => x + 2.,
                BinaryOp::Subtract => x - 2.,
                BinaryOp::Multiply => x * 2.,
                BinaryOp::Divide => x / 2.,
                BinaryOp::Min => x.min(2.),
                BinaryOp::Max => x.max(2.),
            })
            .collect();
        close(
            &b.read_f32(&b.binary(op, &input, &two).unwrap()).unwrap(),
            &expected,
        );
    }
    for op in [
        CompareOp::Equal,
        CompareOp::NotEqual,
        CompareOp::Less,
        CompareOp::LessEqual,
        CompareOp::Greater,
        CompareOp::GreaterEqual,
    ] {
        let expected: Vec<u32> = [0.5_f32, 1., 2., 3., 4., 5.]
            .into_iter()
            .map(|x| {
                u32::from(match op {
                    CompareOp::Equal => x == 2.,
                    CompareOp::NotEqual => x != 2.,
                    CompareOp::Less => x < 2.,
                    CompareOp::LessEqual => x <= 2.,
                    CompareOp::Greater => x > 2.,
                    CompareOp::GreaterEqual => x >= 2.,
                })
            })
            .collect();
        assert_eq!(
            b.read_u32(&b.compare(op, &input, &two).unwrap()).unwrap(),
            expected
        );
    }
    close(
        &b.read_f32(&b.sum_axes(&input, &[0], false).unwrap())
            .unwrap(),
        &[3.5, 5., 7.],
    );
    let total = b.sum_axes(&input, &[0, 1], true).unwrap();
    assert_eq!(total.shape(), &shape(&[1, 1]));
    close(&b.read_f32(&total).unwrap(), &[15.5]);
    close(
        &b.read_f32(&b.sum_axes(&input, &[], false).unwrap())
            .unwrap(),
        &[0.5, 1., 2., 3., 4., 5.],
    );
}

#[test]
fn batched_matmul_zero_inner_and_empty_reductions() {
    let Some(b) = backend() else { return };
    let left = b
        .upload_f32(
            shape(&[2, 2, 3]),
            &[1., 2., 3., 4., 5., 6., 2., 4., 6., 8., 10., 12.],
        )
        .unwrap();
    let right = b
        .upload_f32(shape(&[3, 2]), &[7., 8., 9., 10., 11., 12.])
        .unwrap();
    let product = b.matmul(&left, &right, MatmulPrecision::F32).unwrap();
    assert_eq!(product.shape(), &shape(&[2, 2, 2]));
    close(
        &b.read_f32(&product).unwrap(),
        &[58., 64., 139., 154., 116., 128., 278., 308.],
    );
    let left = b.upload_f32(shape(&[2, 0]), &[]).unwrap();
    let right = b.upload_f32(shape(&[0, 3]), &[]).unwrap();
    close(
        &b.read_f32(&b.matmul(&left, &right, MatmulPrecision::F32).unwrap())
            .unwrap(),
        &[0.; 6],
    );
    let empty = b.upload_f32(shape(&[2, 0, 3]), &[]).unwrap();
    assert!(b.read_f32(&empty).unwrap().is_empty());
    close(
        &b.read_f32(&b.sum_axes(&empty, &[1], false).unwrap())
            .unwrap(),
        &[0.; 6],
    );
    let empty_broadcast = b
        .broadcast_to(&b.upload_f32(shape(&[1]), &[2.]).unwrap(), shape(&[0]))
        .unwrap();
    assert!(b.read_f32(&empty_broadcast).unwrap().is_empty());
}

#[test]
fn unsigned_scans_gather_and_dtype_boundaries() {
    let Some(b) = backend() else { return };
    let floats = b
        .upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])
        .unwrap();
    close(
        &b.read_f32(&b.scan(&floats, 0, false, false).unwrap())
            .unwrap(),
        &[0., 0., 0., 1., 2., 3.],
    );
    close(
        &b.read_f32(&b.gather_axis(&floats, &[2, 0], 1).unwrap())
            .unwrap(),
        &[3., 1., 6., 4.],
    );
    let input = b.upload_u32(shape(&[5]), &[3, 0, 4, 1, 2]).unwrap();
    assert_eq!(input.dtype(), MlxDtype::U32);
    assert_eq!(
        b.read_u32(&b.scan(&input, 0, false, false).unwrap())
            .unwrap(),
        vec![0, 3, 3, 7, 8]
    );
    assert_eq!(
        b.read_u32(&b.scan(&input, 0, true, true).unwrap()).unwrap(),
        vec![10, 7, 7, 3, 2]
    );
    assert_eq!(
        b.read_u32(&b.gather_axis(&input, &[4, 0, 2], 0).unwrap())
            .unwrap(),
        vec![2, 3, 4]
    );
    assert_eq!(
        b.read_u32(&b.sum_axes(&input, &[0], false).unwrap())
            .unwrap(),
        vec![10]
    );
    let overflow = b.upload_u32(shape(&[3]), &[u32::MAX, 1, 2]).unwrap();
    assert_eq!(
        b.read_u32(&b.scan(&overflow, 0, true, false).unwrap())
            .unwrap(),
        vec![u32::MAX, 0, 2]
    );
    let empty = b.upload_u32(shape(&[0]), &[]).unwrap();
    assert!(
        b.read_u32(&b.scan(&empty, 0, false, false).unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(matches!(b.read_f32(&input), Err(MlxError::Dtype)));
    assert!(matches!(
        b.gather_axis(&input, &[5], 0),
        Err(MlxError::InvalidIndices)
    ));
}

#[test]
fn validation_failure_leaves_backend_usable_and_owner_checks_apply() {
    let Some(b) = backend() else { return };
    let other = MlxBackend::new_gpu().unwrap();
    let a = b.upload_f32(shape(&[2, 2]), &[1., 2., 3., 4.]).unwrap();
    let foreign = other.upload_f32(shape(&[2, 2]), &[1.; 4]).unwrap();
    assert!(matches!(
        b.binary(BinaryOp::Add, &a, &foreign),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        b.upload_f32(shape(&[2]), &[1.]),
        Err(MlxError::Contract(TensorError::ElementCountMismatch { .. }))
    ));
    assert!(matches!(
        b.permute(&a, &[0, 0]),
        Err(MlxError::Contract(TensorError::DuplicateAxis { .. }))
    ));
    assert!(matches!(
        b.sum_axes(&a, &[2], false),
        Err(MlxError::Contract(TensorError::AxisOutOfBounds { .. }))
    ));
    assert!(matches!(
        b.matmul(&a, &a, MatmulPrecision::AllowF16),
        Err(MlxError::UnsupportedPrecision(_))
    ));
    close(
        &b.read_f32(&b.matmul(&a, &a, MatmulPrecision::F32).unwrap())
            .unwrap(),
        &[7., 10., 15., 22.],
    );
    b.synchronize().unwrap();
}

#[test]
fn common_tensor_backend_conformance() {
    let Some(backend) = backend() else { return };
    tensor_core::conformance::check_backend(&backend).unwrap();
}
