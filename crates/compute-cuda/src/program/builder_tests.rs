use super::*;

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn dense(dims: &[usize]) -> Layout {
    Layout::contiguous(shape(dims)).unwrap()
}
fn scratch_shapes(plan: &CudaProgramPlan) -> Vec<Shape> {
    plan.scratch
        .iter()
        .map(|spec| spec.shape().clone())
        .collect()
}
fn sizes(b: &CudaProgramPlanBuilder) -> (usize, usize, usize, usize) {
    (
        b.values.len(),
        b.plan.inputs.len(),
        b.plan.scratch.len(),
        b.plan.steps.len(),
    )
}

#[test]
fn views_alias_and_strided_reshape_inserts_one_logical_copy() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b
        .input(Layout::new(shape(&[2, 3]), vec![1, 2], 3).unwrap())
        .unwrap();
    let transposed = b.permute(x, &[1, 0]).unwrap();
    let flat_alias = b.reshape(transposed, shape(&[6])).unwrap();
    let flat_copy = b.reshape(x, shape(&[6])).unwrap();
    let still_copy = b.materialize(flat_copy).unwrap();
    assert_eq!(flat_copy, still_copy);
    let p = b.finish(&[x, flat_alias, flat_copy, flat_copy]).unwrap();
    assert_eq!(p.inputs.len(), 1);
    assert_eq!(scratch_shapes(&p), [shape(&[2, 3])]);
    assert_eq!(p.steps.len(), 1);
    let Step::Unary { source, op, .. } = &p.steps[0] else {
        panic!("reshape must materialize through the shared identity kernel")
    };
    assert_eq!(*op, 10);
    assert_eq!(
        (0..6)
            .map(|i| source.layout.element_offset(i).unwrap())
            .collect::<Vec<_>>(),
        [3, 5, 7, 4, 6, 8]
    );
    assert_eq!(p.outputs[0].buffer, BufferRef::Input(0));
    assert_eq!(p.outputs[1].buffer, BufferRef::Input(0));
    assert_eq!(p.outputs[1].layout.offset(), 3);
    assert_eq!(p.outputs[2].buffer, BufferRef::Scratch(0));
    assert_eq!(p.outputs[2], p.outputs[3]);
    assert_eq!(p.outputs[2].layout, dense(&[6]));
}

#[test]
fn binary_broadcast_preserves_offsets_and_records_every_operation() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b
        .input(Layout::new(shape(&[1, 3]), vec![99, 2], 5).unwrap())
        .unwrap();
    let x = b.broadcast_to(x, shape(&[2, 3])).unwrap();
    let scalar = b
        .input(Layout::new(shape(&[]), vec![], 7).unwrap())
        .unwrap();
    let ops = [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Min,
        BinaryOp::Max,
    ];
    for op in ops {
        b.binary(x, scalar, op).unwrap();
    }
    let p = b.finish(&[]).unwrap();
    assert_eq!(scratch_shapes(&p), vec![shape(&[2, 3]); 6]);
    for (step, expected_op) in p.steps.iter().zip(ops) {
        let Step::Binary {
            left, right, op, ..
        } = step
        else {
            panic!()
        };
        assert_eq!(*op, expected_op);
        assert_eq!(left.layout.strides(), [0, 2]);
        assert_eq!(right.layout.strides(), [0, 0]);
        assert_eq!(left.layout.offset(), 5);
        assert_eq!(right.layout.offset(), 7);
    }
}

#[test]
fn all_unary_operations_and_affine_keep_the_shared_kernel_abi() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[])).unwrap();
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
        b.unary(x, op).unwrap();
    }
    let last = b.affine(x, -2.5, 3.25).unwrap();
    let p = b.finish(&[last]).unwrap();
    assert_eq!(p.steps.len(), 10);
    for (expected, step) in p.steps.iter().enumerate() {
        let Step::Unary {
            op, scale, bias, ..
        } = step
        else {
            panic!()
        };
        assert_eq!(*op, expected as u32);
        assert_eq!(
            (*scale, *bias),
            if expected == 9 {
                (-2.5, 3.25)
            } else {
                (1., 0.)
            }
        );
    }
    assert_eq!(p.outputs[0].layout, dense(&[]));
}

#[test]
fn invalid_records_and_foreign_values_preserve_all_slots() {
    let mut other = CudaProgramPlanBuilder::new();
    let foreign = other.input(dense(&[2, 3])).unwrap();
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[2, 3])).unwrap();
    let before = sizes(&b);
    assert!(b.unary(foreign, UnaryOp::Square).is_err());
    assert!(b.binary(x, foreign, BinaryOp::Add).is_err());
    assert!(b.permute(x, &[0, 0]).is_err());
    assert!(b.reshape(x, shape(&[7])).is_err());
    assert!(b.broadcast_to(x, shape(&[3, 3])).is_err());
    assert!(b.reduce(x, ReduceOp::Sum, &[1, 1], false).is_err());
    let invalid = CudaValue {
        owner: b.owner,
        index: 99,
    };
    assert!(b.materialize(invalid).is_err());
    assert_eq!(sizes(&b), before);
    let y = b.input(dense(&[])).unwrap();
    assert_eq!(b.value(y).unwrap().buffer, BufferRef::Input(1));
    assert!(b.finish(&[foreign]).is_err());
}

#[test]
fn compound_failure_rolls_back_new_inputs_values_scratch_and_steps() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[3])).unwrap();
    let before = sizes(&b);
    let failed: Result<(), _> = b.transaction(|b| {
        let y = b.input(dense(&[3]))?;
        let z = b.binary(x, y, BinaryOp::Multiply)?;
        b.unary(z, UnaryOp::Square)?;
        Err(CudaError::InvalidInput("injected late preparation failure"))
    });
    assert!(failed.is_err());
    assert_eq!(sizes(&b), before);
    let y = b.unary(x, UnaryOp::Negate).unwrap();
    let p = b.finish(&[y]).unwrap();
    assert_eq!(scratch_shapes(&p), [shape(&[3])]);
    assert_eq!(p.outputs[0].buffer, BufferRef::Scratch(0));
}

#[test]
fn broadcast_matmul_output_overflow_rolls_back_materialization() {
    let mut b = CudaProgramPlanBuilder::new();
    // Each operand fits the logical byte limit using one physical value;
    // their independent batch axes produce a representable element count
    // whose f32 byte count overflows. No large allocation is performed.
    let first = 1_usize << (usize::BITS / 2);
    let second = 1_usize << (usize::BITS - usize::BITS / 2 - 2);
    let a = b
        .input(Layout::new(shape(&[first, 1, 1, 1]), vec![0; 4], 0).unwrap())
        .unwrap();
    let c = b
        .input(Layout::new(shape(&[1, second, 1, 1]), vec![0; 4], 0).unwrap())
        .unwrap();
    let before = sizes(&b);
    assert!(b.matmul(a, c, MatmulPrecision::F32).is_err());
    assert_eq!(sizes(&b), before);
    assert!(b.finish(&[a, c]).unwrap().scratch.is_empty());
}

#[test]
fn reduction_keeps_axis_order_broadcast_count_and_mean_policy() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[2, 1, 3])).unwrap();
    let x = b.broadcast_to(x, shape(&[2, 5, 3])).unwrap();
    let mean = b.mean(x, &[2, 0], true).unwrap();
    let sum = b.sum_axes(x, &[0, 1, 2], false).unwrap();
    let p = b.finish(&[mean, sum]).unwrap();
    assert_eq!(p.outputs[0].layout.shape(), &shape(&[1, 5, 1]));
    assert_eq!(p.outputs[1].layout.shape(), &shape(&[]));
    let Step::Reduce {
        source,
        axes,
        mean,
        op,
        keep_dims,
        ..
    } = &p.steps[0]
    else {
        panic!()
    };
    assert_eq!(axes, &[2, 0]);
    assert_eq!(source.layout.strides(), [3, 0, 1]);
    assert_eq!(
        source.layout.shape().numel() / p.scratch[0].shape().numel(),
        6
    );
    assert!(*mean && *keep_dims);
    assert_eq!(*op, ReduceOp::Sum);
}

#[test]
fn empty_axes_alias_and_empty_contractions_remain_explicit_per_replay() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[2, 0, 3])).unwrap();
    assert_eq!(b.reduce(x, ReduceOp::Min, &[], false).unwrap(), x);
    assert_eq!(b.mean(x, &[], true).unwrap(), x);
    let sum = b.reduce(x, ReduceOp::Sum, &[1], false).unwrap();
    let product = b.reduce(x, ReduceOp::Product, &[1], false).unwrap();
    let before = sizes(&b);
    assert!(b.reduce(x, ReduceOp::Min, &[1], false).is_err());
    assert!(b.reduce(x, ReduceOp::Max, &[1], false).is_err());
    assert!(b.mean(x, &[1], false).is_err());
    assert_eq!(sizes(&b), before);
    let empty_mean = b.mean(x, &[0], false).unwrap();
    let p = b.finish(&[sum, product, empty_mean]).unwrap();
    assert_eq!(
        scratch_shapes(&p),
        [shape(&[2, 3]), shape(&[2, 3]), shape(&[0, 3])]
    );
    assert!(matches!(
        p.steps[0],
        Step::Reduce {
            op: ReduceOp::Sum,
            ..
        }
    ));
    assert!(matches!(
        p.steps[1],
        Step::Reduce {
            op: ReduceOp::Product,
            ..
        }
    ));
    assert!(matches!(p.steps[2], Step::Reduce { mean: true, .. }));
}

#[test]
fn matmul_materializes_strides_and_preserves_broadcast_vector_shape() {
    let mut b = CudaProgramPlanBuilder::new();
    let a = b
        .input(Layout::new(shape(&[2, 1, 3, 5]), vec![15, 0, 1, 3], 4).unwrap())
        .unwrap();
    let v = b
        .input(Layout::new(shape(&[5]), vec![2], 1).unwrap())
        .unwrap();
    let y = b.matmul(a, v, MatmulPrecision::AllowTf32).unwrap();
    let p = b.finish(&[y]).unwrap();
    assert_eq!(p.steps.len(), 3);
    assert!(matches!(p.steps[0], Step::Unary { op: 10, .. }));
    assert!(matches!(p.steps[1], Step::Unary { op: 10, .. }));
    let Step::Matmul {
        left,
        right,
        precision,
        ..
    } = &p.steps[2]
    else {
        panic!()
    };
    assert_eq!(*precision, MatmulPrecision::AllowTf32);
    assert_eq!(left.layout, dense(&[2, 1, 3, 5]));
    assert_eq!(right.layout.shape(), &shape(&[5, 1]));
    assert_eq!(right.layout.strides(), [1, 0]);
    assert_eq!(p.outputs[0].layout, dense(&[2, 1, 3]));
}

#[test]
fn empty_matmul_retains_precision_and_skips_unused_materialization() {
    let mut b = CudaProgramPlanBuilder::new();
    let empty = b.input(dense(&[0])).unwrap();
    let dot = b.matmul(empty, empty, MatmulPrecision::AllowBf16).unwrap();
    let a = b.input(dense(&[0, 3, 5])).unwrap();
    let rhs = b
        .input(Layout::new(shape(&[5, 7]), vec![1, 5], 0).unwrap())
        .unwrap();
    let y = b.matmul(a, rhs, MatmulPrecision::F32).unwrap();
    let p = b.finish(&[dot, y]).unwrap();
    assert_eq!(p.steps.len(), 2);
    assert!(matches!(
        p.steps[0],
        Step::Matmul {
            precision: MatmulPrecision::AllowBf16,
            ..
        }
    ));
    assert!(matches!(
        p.steps[1],
        Step::Matmul {
            precision: MatmulPrecision::F32,
            ..
        }
    ));
    assert_eq!(p.outputs[0].layout.shape(), &shape(&[]));
    assert_eq!(p.outputs[1].layout.shape(), &shape(&[0, 3, 7]));
}

#[test]
fn oversized_inputs_broadcasts_and_blas_dimensions_fail_before_recording() {
    let mut b = CudaProgramPlanBuilder::new();
    assert!(
        b.input(Layout::new(shape(&[usize::MAX / 4 + 1]), vec![0], 0).unwrap())
            .is_err()
    );
    assert_eq!(sizes(&b), (0, 0, 0, 0));
    let scalar = b.input(dense(&[])).unwrap();
    assert_eq!(b.value(scalar).unwrap().buffer, BufferRef::Input(0));
    assert!(
        b.broadcast_to(scalar, shape(&[usize::MAX / 4 + 1]))
            .is_err()
    );
    let huge = b
        .input(Layout::new(shape(&[i32::MAX as usize + 1, 1]), vec![0, 0], 0).unwrap())
        .unwrap();
    let one = b.input(dense(&[1, 1])).unwrap();
    let before = sizes(&b);
    assert!(b.matmul(huge, one, MatmulPrecision::F32).is_err());
    assert!(b.matmul(scalar, one, MatmulPrecision::F32).is_err());
    assert_eq!(sizes(&b), before);
    assert!(b.finish(&[]).unwrap().steps.is_empty());
}
