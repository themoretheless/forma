use tensor_core::{
    MatmulPlan, ReduceOp, Shape, TensorError, matmul_shape, mean_shape, reduction_shape,
};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

#[test]
fn reductions_validate_empty_contractions_without_inventing_values() {
    let input = shape(&[2, 0, 3]);
    for op in [ReduceOp::Sum, ReduceOp::Product] {
        assert_eq!(
            reduction_shape(op, &input, &[1], false).unwrap(),
            shape(&[2, 3])
        );
    }
    for op in [ReduceOp::Min, ReduceOp::Max] {
        assert_eq!(
            reduction_shape(op, &input, &[1], false),
            Err(TensorError::EmptyReduction)
        );
        assert_eq!(reduction_shape(op, &input, &[], true).unwrap(), input);
        assert_eq!(
            reduction_shape(op, &shape(&[0, 0]), &[1], false).unwrap(),
            shape(&[0])
        );
    }
    assert_eq!(
        mean_shape(&input, &[1], true),
        Err(TensorError::EmptyReduction)
    );
    assert_eq!(mean_shape(&shape(&[]), &[], false).unwrap(), shape(&[]));
    assert_eq!(
        mean_shape(&input, &[0, 2], true).unwrap(),
        shape(&[1, 0, 1])
    );
    assert_eq!(
        mean_shape(&shape(&[0, 0]), &[1], true).unwrap(),
        shape(&[0, 1])
    );
}

#[test]
fn reduction_validation_rejects_axes_and_output_overflow() {
    let input = shape(&[2, 3]);
    assert!(matches!(
        reduction_shape(ReduceOp::Sum, &input, &[0, 0], false),
        Err(TensorError::DuplicateAxis { .. })
    ));
    assert!(matches!(
        mean_shape(&input, &[2], false),
        Err(TensorError::AxisOutOfBounds { .. })
    ));
    let huge_empty = shape(&[usize::MAX, 0, usize::MAX]);
    assert_eq!(
        reduction_shape(ReduceOp::Sum, &huge_empty, &[1], false),
        Err(TensorError::ShapeOverflow)
    );
    assert_eq!(
        reduction_shape(ReduceOp::Min, &huge_empty, &[0, 2], false).unwrap(),
        shape(&[0])
    );
}

#[test]
fn matmul_promotes_vectors_and_preserves_batch_axes() {
    for (a, b, result) in [
        (vec![3], vec![3], vec![]),
        (vec![0], vec![0], vec![]),
        (vec![3], vec![3, 4], vec![4]),
        (vec![2, 3], vec![3], vec![2]),
        (vec![3], vec![2, 5, 3, 4], vec![2, 5, 4]),
        (vec![2, 5, 4, 3], vec![3], vec![2, 5, 4]),
        (vec![0], vec![2, 0, 4], vec![2, 4]),
        (vec![0, 3, 2], vec![2], vec![0, 3]),
    ] {
        let plan = MatmulPlan::new(&shape(&a), &shape(&b)).unwrap();
        assert_eq!(plan.output, shape(&result));
        assert_eq!(plan.output.numel(), plan.matrix_output.numel());
        assert!(plan.left.rank() >= 2 && plan.right.rank() >= 2);
        assert_eq!(plan.left.numel(), shape(&a).numel());
        assert_eq!(plan.right.numel(), shape(&b).numel());
    }
    assert_eq!(
        matmul_shape(&shape(&[]), &shape(&[1])),
        Err(TensorError::MatmulRank { left: 0, right: 1 })
    );
    assert!(matches!(
        matmul_shape(&shape(&[3]), &shape(&[2, 4])),
        Err(TensorError::MatmulInnerDimension { .. })
    ));
    assert!(matches!(
        matmul_shape(&shape(&[2, 3]), &shape(&[2])),
        Err(TensorError::MatmulInnerDimension { .. })
    ));
}
