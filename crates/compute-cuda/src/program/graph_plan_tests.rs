use super::*;
use crate::program::{builder::CudaProgramPlanBuilder, plan::CudaDtype};
use tensor_core::{
    AttentionMask, AttentionOptions, BinaryOp, CompareOp, LowDtype, MatmulPrecision, ScanOptions,
    ScatterOp, Shape, UnaryOp,
};
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn dense(dims: &[usize]) -> Layout {
    Layout::contiguous(shape(dims)).unwrap()
}
fn address(layout: &Layout, mut logical: usize) -> usize {
    let mut address = layout.offset();
    for (&dim, &stride) in layout.shape().dims().iter().zip(layout.strides()).rev() {
        address += (logical % dim) * stride;
        logical /= dim;
    }
    address
}
/// CPU address oracle: an input slot contains the original logical row-major
/// sequence. Materialization steps move integer address labels, not float data.
fn evaluate_copies(plan: &CudaProgramPlan, physical_inputs: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut scratch: Vec<Vec<usize>> = plan
        .scratch
        .iter()
        .map(|s| vec![usize::MAX; s.shape().numel()])
        .collect();
    let read = |value: &PlannedValue, scratch: &Vec<Vec<usize>>| {
        let storage = match value.buffer {
            BufferRef::Input(i) => &physical_inputs[i],
            BufferRef::Scratch(i) => &scratch[i],
        };
        (0..value.layout.shape().numel())
            .map(|i| storage[address(&value.layout, i)])
            .collect::<Vec<_>>()
    };
    for step in &plan.steps {
        match step {
            Step::Unary {
                source,
                output,
                op: 10,
                ..
            } => scratch[*output] = read(source, &scratch),
            _ => panic!("oracle only evaluates raw copies"),
        }
    }
    plan.outputs.iter().map(|v| read(v, &scratch)).collect()
}

#[test]
fn dense_slots_preserve_transpose_reshape_order_with_one_shared_copy() {
    let mut builder = CudaProgramPlanBuilder::new();
    let layout = Layout::new(shape(&[2, 3]), vec![1, 2], 7).unwrap();
    let input = builder.input(layout.clone()).unwrap();
    let permuted = builder.permute(input, &[1, 0]).unwrap();
    let reshaped = builder.reshape(permuted, shape(&[6])).unwrap();
    let plan = builder
        .finish(&[input, permuted, reshaped, reshaped])
        .unwrap();
    assert!(plan.steps.is_empty());
    let rebased = plan.dense_inputs().unwrap();
    assert_eq!(rebased.inputs[0].layout, dense(&[2, 3]));
    assert_eq!(rebased.steps.len(), 1);
    assert_eq!(rebased.scratch.len(), 1);
    let physical: Vec<_> = (0..13).collect();
    let packed: Vec<_> = (0..6).map(|i| physical[address(&layout, i)]).collect();
    assert_eq!(
        evaluate_copies(&plan, &[physical]),
        evaluate_copies(&rebased, std::slice::from_ref(&packed))
    );
    assert_eq!(rebased.outputs[2], rebased.outputs[3]);
    let again = rebased.dense_inputs().unwrap();
    assert_eq!(again.steps.len(), 1);
    assert_eq!(
        evaluate_copies(&again, std::slice::from_ref(&packed)),
        evaluate_copies(&rebased, &[packed])
    );
    // Ordinary preparation remains the original plan; capture remapping cannot
    // add a launch or scratch allocation to that path.
    assert!(plan.steps.is_empty());
    assert!(plan.scratch.is_empty());
}

#[test]
fn zero_strides_singleton_axes_and_broadcast_use_logical_provenance() {
    for dtype in [
        CudaDtype::F32,
        CudaDtype::U32,
        CudaDtype::F16,
        CudaDtype::Bf16,
    ] {
        let mut builder = CudaProgramPlanBuilder::new();
        let layout = Layout::new(shape(&[2, 1, 3]), vec![0, usize::MAX / 8, 2], 11).unwrap();
        let input = match dtype {
            CudaDtype::F32 => builder.input(layout.clone()).unwrap(),
            CudaDtype::U32 => builder.input_u32(layout.clone()).unwrap(),
            _ => builder
                .input_low(dtype.low_dtype().unwrap(), layout.clone())
                .unwrap(),
        };
        let value = builder.permute(input, &[2, 1, 0]).unwrap();
        let value = builder.broadcast_to(value, shape(&[4, 3, 5, 2])).unwrap();
        let value = builder.reshape(value, shape(&[12, 10])).unwrap();
        let plan = builder.finish(&[value]).unwrap();
        let rebased = plan.dense_inputs().unwrap();
        assert_eq!(rebased.inputs[0].layout, dense(&[2, 1, 3]));
        assert_eq!(rebased.inputs[0].shape().numel(), 6);
        let physical: Vec<_> = (0..16).collect();
        let packed: Vec<_> = (0..6).map(|i| physical[address(&layout, i)]).collect();
        assert_eq!(
            evaluate_copies(&plan, &[physical]),
            evaluate_copies(&rebased, &[packed])
        );
    }
}

#[test]
fn active_gemm_materializes_rebased_views_but_empty_and_k_zero_do_not() {
    let mut builder = CudaProgramPlanBuilder::new();
    let source = builder
        .input(Layout::new(shape(&[2, 3]), vec![1, 2], 0).unwrap())
        .unwrap();
    let left = builder.permute(source, &[1, 0]).unwrap();
    let right = builder.input(dense(&[2, 4])).unwrap();
    let y = builder.matmul(left, right, MatmulPrecision::F32).unwrap();
    let plan = builder.finish(&[y]).unwrap();
    assert_eq!(plan.steps.len(), 1);
    let rebased = plan.dense_inputs().unwrap();
    assert_eq!(rebased.steps.len(), 2);
    match &rebased.steps[1] {
        Step::Matmul { left, right, .. } => {
            assert!(left.layout.is_contiguous());
            assert!(right.layout.is_contiguous());
            assert!(matches!(left.buffer, BufferRef::Scratch(_)));
        }
        _ => panic!("GEMM retained"),
    }
    for (a, b) in [(vec![0, 3], vec![3, 2]), (vec![2, 0], vec![0, 3])] {
        let mut builder = CudaProgramPlanBuilder::new();
        let left = builder
            .input(Layout::new(shape(&a), vec![7, 2], 9).unwrap())
            .unwrap();
        let right = builder
            .input(Layout::new(shape(&b), vec![8, 3], 4).unwrap())
            .unwrap();
        let y = builder
            .matmul(left, right, MatmulPrecision::AllowTf32)
            .unwrap();
        let rebased = builder.finish(&[y]).unwrap().dense_inputs().unwrap();
        assert_eq!(rebased.steps.len(), 1);
        assert!(matches!(
            rebased.steps[0],
            Step::Matmul {
                precision: MatmulPrecision::AllowTf32,
                ..
            }
        ));
    }
}

#[test]
fn vector_gemm_promotions_and_empty_huge_spans_keep_dense_slot_bounds() {
    let mut builder = CudaProgramPlanBuilder::new();
    let left = builder
        .input_low(LowDtype::F16, Layout::new(shape(&[3]), vec![2], 5).unwrap())
        .unwrap();
    let right = builder.input_low(LowDtype::F16, dense(&[3])).unwrap();
    let y = builder.matmul_low_f32(left, right).unwrap();
    let rebased = builder.finish(&[y]).unwrap().dense_inputs().unwrap();
    assert_eq!(rebased.inputs[0].layout, dense(&[3]));
    match rebased.steps.last().unwrap() {
        Step::Matmul { left, right, .. } => {
            assert_eq!(left.layout.shape(), &shape(&[1, 3]));
            assert_eq!(right.layout.shape(), &shape(&[3, 1]));
        }
        _ => panic!("GEMM retained"),
    }
    let mut builder = CudaProgramPlanBuilder::new();
    let x = builder
        .input(Layout::new(shape(&[0, usize::MAX]), vec![0, 0], 0).unwrap())
        .unwrap();
    let plan = builder.finish(&[x]).unwrap().dense_inputs().unwrap();
    assert_eq!(plan.inputs[0].layout.offset(), 0);
    assert_eq!(plan.inputs[0].shape().numel(), 0);
    assert!(plan.steps.is_empty());
    let mut builder = CudaProgramPlanBuilder::new();
    let layout = Layout::new(shape(&[2]), vec![usize::MAX / 16], 7).unwrap();
    let x = builder.input(layout).unwrap();
    let plan = builder.finish(&[x]).unwrap().dense_inputs().unwrap();
    assert_eq!(plan.inputs[0].layout.required_storage_len().unwrap(), 2);
}

#[test]
fn all_step_operand_families_are_rebased_and_scratch_views_stay_unchanged() {
    let mut builder = CudaProgramPlanBuilder::new();
    let layout = Layout::new(shape(&[2, 3]), vec![1, 2], 5).unwrap();
    let x = builder.input(layout.clone()).unwrap();
    let u = builder.input_u32(layout.clone()).unwrap();
    let low = builder.input_low(LowDtype::Bf16, layout).unwrap();
    let a = builder.unary(x, UnaryOp::Square).unwrap();
    let _ = builder.binary(x, a, BinaryOp::Add).unwrap();
    let mask = builder.compare(x, a, CompareOp::Less).unwrap();
    let _ = builder.select(mask, x, a).unwrap();
    let _ = builder.cast_to_f32(low).unwrap();
    let _ = builder.sum_axes(x, &[1], true).unwrap();
    let _ = builder.scan(x, 1, ScanOptions::default()).unwrap();
    let _ = builder.gather(x, u, 1).unwrap();
    let _ = builder.compact(x, u).unwrap();
    let _ = builder.scatter(x, u, x, ScatterOp::Replace, 1).unwrap();
    let indices = builder.input_u32(dense(&[3])).unwrap();
    let _ = builder.scatter(x, indices, x, ScatterOp::Add, 1).unwrap();
    let moments = builder.moments(x, &[1], false).unwrap();
    let _ = builder
        .attention(
            x,
            x,
            x,
            AttentionMask::Keep(&u),
            AttentionOptions::default(),
        )
        .unwrap_err();
    let _ = builder
        .attention(x, x, x, AttentionMask::None, AttentionOptions::default())
        .unwrap();
    let output = builder.permute(a, &[1, 0]).unwrap();
    let plan = builder.finish(&[output, moments.variance]).unwrap();
    let rebased = plan.dense_inputs().unwrap();
    assert_eq!(rebased.steps.len(), plan.steps.len());
    assert_eq!(rebased.outputs, plan.outputs);
    for step in &rebased.steps {
        let mut step = step.clone();
        for value in operands(&mut step) {
            if let BufferRef::Input(slot) = value.buffer {
                assert_eq!(value.layout.offset(), 0);
                let mut expected = Layout::contiguous(plan.inputs[slot].shape().clone()).unwrap();
                for view in &value.input_views {
                    expected = view.apply(&expected).unwrap();
                }
                assert_eq!(value.layout, expected);
            }
        }
    }
}

#[test]
fn missing_provenance_is_rejected_instead_of_silently_rebasing_a_wrong_view() {
    let mut builder = CudaProgramPlanBuilder::new();
    let x = builder.input(dense(&[2, 3])).unwrap();
    let mut plan = builder.finish(&[x]).unwrap();
    plan.outputs[0].layout = plan.outputs[0].layout.permute(&[1, 0]).unwrap();
    assert!(plan.dense_inputs().is_err());
}

#[test]
fn rebasing_does_not_allocate_a_broadcast_mask_at_result_width() {
    let mut builder = CudaProgramPlanBuilder::new();
    let count = usize::MAX / 4 + 1;
    let low = builder
        .input_low(
            LowDtype::Bf16,
            Layout::new(shape(&[count]), vec![0], 0).unwrap(),
        )
        .unwrap();
    let mask = builder.input_u32(dense(&[])).unwrap();
    let selected = builder.select_low(mask, low, low).unwrap();
    let plan = builder.finish(&[selected]).unwrap().dense_inputs().unwrap();
    assert_eq!(plan.inputs[1].shape().numel(), 1);
    let Step::Select { mask, .. } = &plan.steps[0] else {
        panic!("selection retained")
    };
    assert_eq!(mask.layout.shape().numel(), count);
    assert_eq!(mask.layout.required_storage_len().unwrap(), 1);
}
