use super::{
    builder::CudaProgramPlanBuilder,
    operation::{MatmulRequest, Operation},
    plan::{BufferRef, CudaDtype},
    preparation::{Expanded, expand},
    *,
};
use tensor_core::LowDtype;
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn dense(d: &[usize]) -> Layout {
    Layout::contiguous(shape(d)).unwrap()
}
fn finish(b: CudaProgramPlanBuilder, outputs: &[CudaValue]) -> Expanded {
    expand(
        b.finish(outputs).unwrap(),
        40,
        CudaPrepareOptions::default(),
        |_| Ok(()),
    )
    .unwrap()
}
#[test]
fn scratch_byte_counts_follow_all_four_storage_widths_including_sentinels() {
    for dims in [vec![], vec![3], vec![0, 7]] {
        for dtype in [
            CudaDtype::F32,
            CudaDtype::U32,
            CudaDtype::F16,
            CudaDtype::Bf16,
        ] {
            let mut b = CudaProgramPlanBuilder::new();
            let layout = dense(&dims);
            let x = match dtype {
                CudaDtype::F32 => b.input(layout),
                CudaDtype::U32 => b.input_u32(layout),
                _ => b.input_low(dtype.low_dtype().unwrap(), layout),
            }
            .unwrap();
            let y = match dtype {
                CudaDtype::F32 => b.unary(x, UnaryOp::Negate),
                CudaDtype::U32 => b.binary_u32(x, x, BinaryOp::Add),
                _ => b.unary_low(x, UnaryOp::Negate),
            }
            .unwrap();
            let plan = finish(b, &[y]);
            assert_eq!(
                plan.stats.scratch_bytes,
                shape(&dims).numel().max(1) * dtype.byte_width()
            );
            assert_eq!(plan.scratch[0].dtype, dtype);
            if dims.contains(&0) {
                assert_eq!(
                    (plan.stats.kernel_launches, plan.stats.metadata_bytes),
                    (0, 0)
                );
            }
            if dims.is_empty() {
                assert_eq!(plan.stats.metadata_bytes, 16);
            }
        }
    }
}
#[test]
fn low_partial_hierarchy_uses_f32_and_never_a_full_converted_input() {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let x = b
            .input_low(dtype, Layout::new(shape(&[131077]), vec![2], 3).unwrap())
            .unwrap();
        let y = b.mean_low(x, &[0], false).unwrap();
        let plan = finish(b, &[y]);
        assert_eq!(plan.stats.scratch_bytes, 270);
        assert!(plan.scratch.iter().all(|s| s.shape().numel() <= 65));
        let reductions: Vec<_> = plan
            .schedule
            .iter()
            .filter_map(|s| {
                if let Operation::Reduction { source, .. } = s.operation {
                    Some(source)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(reductions.len(), 2);
        assert!(matches!(reductions[0], BufferRef::Input(0)));
        let BufferRef::Scratch(partial) = reductions[1] else {
            panic!("second pass must read partials")
        };
        assert_eq!(plan.scratch[partial].dtype, CudaDtype::F32);
        assert!(matches!(
            plan.schedule[plan.schedule.len() - 2].operation,
            Operation::Cast { .. }
        ));
    }
}
#[test]
fn unsigned_partial_hierarchy_stays_unsigned_and_empty_product_fills_one() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input_u32(dense(&[131077])).unwrap();
    let y = b.reduce_u32(x, ReduceOp::Product, &[0], false).unwrap();
    let plan = finish(b, &[y]);
    assert_eq!(plan.stats.scratch_bytes, 264);
    assert!(plan.scratch.iter().all(|s| s.dtype == CudaDtype::U32));
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input_u32(dense(&[3, 0])).unwrap();
    let y = b.reduce_u32(x, ReduceOp::Product, &[1], false).unwrap();
    let plan = finish(b, &[y]);
    assert!(matches!(
        plan.schedule[0].operation,
        Operation::Fill { count: 3, value: 1 }
    ));
    assert_eq!(plan.scratch[0].dtype, CudaDtype::U32);
}
#[test]
fn low_axes_identity_decodes_but_raw_view_outputs_only_copy() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input_low(LowDtype::Bf16, dense(&[2, 3])).unwrap();
    let view = b.permute(x, &[1, 0]).unwrap();
    let y = b.reduce_low_f32(view, ReduceOp::Min, &[], false).unwrap();
    let plan = finish(b, &[view, y]);
    assert_eq!(plan.scratch.len(), 1);
    assert_eq!(plan.stats.scratch_bytes, 24);
    assert!(matches!(
        plan.schedule[0].operation,
        Operation::Cast {
            to: CudaDtype::F32,
            ..
        }
    ));
    assert!(matches!(plan.schedule[1].operation, Operation::Copy { .. }));
    assert_eq!(plan.outputs[0].dtype, CudaDtype::Bf16);
}
#[test]
fn typed_byte_budgets_reject_one_byte_short_without_resource_preparation() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input_low(LowDtype::F16, dense(&[3])).unwrap();
    let y = b.unary_low(x, UnaryOp::Abs).unwrap();
    let result = expand(
        b.finish(&[y]).unwrap(),
        40,
        CudaPrepareOptions {
            max_scratch_bytes: 5,
            max_metadata_bytes: usize::MAX,
        },
        |_| Ok(()),
    );
    assert!(matches!(
        result,
        Err(CudaError::PreparationBudget {
            kind: "scratch",
            required: 6,
            limit: 5
        })
    ));
}
#[test]
fn every_low_gemm_keeps_capability_checks_even_without_element_work() {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for dims in [(vec![0, 2, 3], vec![3, 4]), (vec![2, 0], vec![0, 4])] {
            let build = || {
                let mut b = CudaProgramPlanBuilder::new();
                let a = b.input_low(dtype, dense(&dims.0)).unwrap();
                let c = b.input_low(dtype, dense(&dims.1)).unwrap();
                let y = b.matmul_low_f32(a, c).unwrap();
                b.finish(&[y]).unwrap()
            };
            let mut checks = 0;
            let result = expand(build(), 40, CudaPrepareOptions::default(), |request| {
                checks += 1;
                assert_eq!(request, MatmulRequest::Low(dtype));
                Err(CudaError::UnsupportedPrecision(
                    "unsupported native low mode",
                ))
            });
            assert!(matches!(result, Err(CudaError::UnsupportedPrecision(_))));
            assert_eq!(checks, 1);
            let plan = expand(build(), 40, CudaPrepareOptions::default(), |_| Ok(())).unwrap();
            assert_eq!(plan.precisions, [MatmulRequest::Low(dtype)]);
            assert_eq!(plan.stats.gemm_calls, 0);
        }
    }
}
#[test]
fn strided_low_gemm_materializes_only_native_storage_and_rounds_final_result() {
    let mut b = CudaProgramPlanBuilder::new();
    let a = b
        .input_low(
            LowDtype::Bf16,
            Layout::new(shape(&[2, 3]), vec![1, 2], 0).unwrap(),
        )
        .unwrap();
    let c = b.input_low(LowDtype::Bf16, dense(&[3, 4])).unwrap();
    let y = b.matmul_low(a, c).unwrap();
    let plan = finish(b, &[y]);
    assert_eq!(plan.stats.scratch_bytes, 12 + 32 + 16);
    assert_eq!(plan.stats.gemm_calls, 1);
    assert_eq!(
        plan.scratch.iter().map(|s| s.dtype).collect::<Vec<_>>(),
        [CudaDtype::Bf16, CudaDtype::F32, CudaDtype::Bf16]
    );
}
