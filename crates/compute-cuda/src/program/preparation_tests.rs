use super::{
    builder::CudaProgramPlanBuilder,
    operation::{MatmulRequest, Operation},
    preparation::*,
    *,
};
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn dense(dims: &[usize]) -> Layout {
    Layout::contiguous(shape(dims)).unwrap()
}
fn expanded(builder: CudaProgramPlanBuilder, outputs: &[CudaValue]) -> Expanded {
    expand(
        builder.finish(outputs).unwrap(),
        40,
        CudaPrepareOptions::default(),
        |_| Ok(()),
    )
    .unwrap()
}
#[test]
fn scalar_metadata_sentinel_and_duplicate_terminal_copies_are_counted() {
    let mut builder = CudaProgramPlanBuilder::new();
    let input = builder.input(dense(&[])).unwrap();
    let output = builder.unary(input, UnaryOp::Square).unwrap();
    let plan = expanded(builder, &[output, output, input]);
    assert_eq!(
        plan.stats,
        CudaProgramStats {
            memset_calls: 0,
            kernel_launches: 4,
            gemm_calls: 0,
            scratch_bytes: 4,
            metadata_bytes: 32
        }
    );
}
#[test]
fn full_reduction_expansion_and_mean_include_every_partial_and_division() {
    for mean in [false, true] {
        let mut builder = CudaProgramPlanBuilder::new();
        let input = builder.input(dense(&[131077])).unwrap();
        let output = if mean {
            builder.mean(input, &[0], false)
        } else {
            builder.reduce(input, ReduceOp::Sum, &[0], false)
        }
        .unwrap();
        let plan = expanded(builder, &[output]);
        // 65 f32 partials, completed scalar; mean adds a separate sum scalar.
        assert_eq!(plan.stats.scratch_bytes, 264 + usize::from(mean) * 4);
        assert_eq!(plan.stats.kernel_launches, 3 + usize::from(mean));
        assert_eq!(plan.stats.metadata_bytes, 40 + usize::from(mean) * 8);
        assert_eq!(
            plan.schedule
                .iter()
                .filter(|s| matches!(s.operation, Operation::Reduction { .. }))
                .count(),
            2
        );
    }
}
#[test]
fn empty_contractions_and_k_zero_have_explicit_repeatable_identity_writes() {
    let mut builder = CudaProgramPlanBuilder::new();
    let input = builder.input(dense(&[2, 0])).unwrap();
    let sum = builder.reduce(input, ReduceOp::Sum, &[1], false).unwrap();
    let product = builder
        .reduce(input, ReduceOp::Product, &[1], false)
        .unwrap();
    let right = builder.input(dense(&[0, 3])).unwrap();
    let gemm = builder.matmul(input, right, MatmulPrecision::F32).unwrap();
    let plan = expanded(builder, &[sum, product, gemm]);
    let fills: Vec<_> = plan
        .schedule
        .iter()
        .filter_map(|s| match &s.operation {
            Operation::Fill { count, value } => Some((*count, *value)),
            _ => None,
        })
        .collect();
    assert_eq!(fills, [(2, 0), (2, 1), (6, 0)]);
    assert_eq!(plan.stats.kernel_launches, 6);
    assert_eq!(plan.stats.gemm_calls, 0);
    assert_eq!(plan.stats.scratch_bytes, 40);
    // Identities are explicit scheduled writes, independent of allocation state.
}
#[test]
fn empty_scratch_counts_sentinel_but_issues_no_dispatch_or_metadata() {
    let mut builder = CudaProgramPlanBuilder::new();
    let input = builder.input(dense(&[0, 3])).unwrap();
    let output = builder.unary(input, UnaryOp::Negate).unwrap();
    let plan = expanded(builder, &[output]);
    assert_eq!(
        plan.stats,
        CudaProgramStats {
            memset_calls: 0,
            scratch_bytes: 4,
            ..Default::default()
        }
    );
}
#[test]
fn budgets_include_expanded_scratch_and_scalar_metadata_before_resource_preparation() {
    for (scratch, metadata, expected) in
        [(263, usize::MAX, "scratch"), (usize::MAX, 39, "metadata")]
    {
        let mut builder = CudaProgramPlanBuilder::new();
        let input = builder.input(dense(&[131077])).unwrap();
        let output = builder.reduce(input, ReduceOp::Sum, &[0], false).unwrap();
        let error = expand(
            builder.finish(&[output]).unwrap(),
            40,
            CudaPrepareOptions {
                max_scratch_bytes: scratch,
                max_metadata_bytes: metadata,
            },
            |_| Ok(()),
        )
        .err()
        .unwrap();
        assert!(matches!(error,CudaError::PreparationBudget{kind,..} if kind==expected));
    }
    let mut builder = CudaProgramPlanBuilder::new();
    let input = builder.input(dense(&[131077])).unwrap();
    let output = builder.reduce(input, ReduceOp::Sum, &[0], false).unwrap();
    assert!(
        expand(
            builder.finish(&[output]).unwrap(),
            40,
            CudaPrepareOptions {
                max_scratch_bytes: 264,
                max_metadata_bytes: 40
            },
            |_| Ok(())
        )
        .is_ok()
    );
}
#[test]
fn zero_work_matmul_still_validates_precision_before_preparing_resources() {
    for (left, right) in [(vec![0, 2, 3], vec![3, 4]), (vec![2, 0], vec![0, 4])] {
        let mut builder = CudaProgramPlanBuilder::new();
        let left = builder.input(dense(&left)).unwrap();
        let right = builder.input(dense(&right)).unwrap();
        let output = builder
            .matmul(left, right, MatmulPrecision::AllowTf32)
            .unwrap();
        let mut checked = false;
        let error = expand(
            builder.finish(&[output]).unwrap(),
            40,
            CudaPrepareOptions::default(),
            |precision| {
                checked = true;
                assert_eq!(precision, MatmulRequest::F32(MatmulPrecision::AllowTf32));
                Err(CudaError::UnsupportedPrecision("mock policy rejection"))
            },
        )
        .err()
        .unwrap();
        assert!(checked);
        assert!(matches!(error, CudaError::UnsupportedPrecision(_)));
    }
}

#[test]
fn no_work_matmul_retains_precision_for_every_replay_validation() {
    for (left, right) in [(vec![0, 2, 3], vec![3, 4]), (vec![2, 0], vec![0, 4])] {
        let mut builder = CudaProgramPlanBuilder::new();
        let left = builder.input(dense(&left)).unwrap();
        let right = builder.input(dense(&right)).unwrap();
        let output = builder
            .matmul(left, right, MatmulPrecision::AllowTf32)
            .unwrap();
        let plan = expanded(builder, &[output]);
        assert_eq!(
            plan.precisions,
            [MatmulRequest::F32(MatmulPrecision::AllowTf32)]
        );
        assert_eq!(plan.stats.gemm_calls, 0);
    }
}
