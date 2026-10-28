use super::*;
use tensor_core::{CompareOp, LowDtype};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn dense(dims: &[usize]) -> Layout {
    Layout::contiguous(shape(dims)).unwrap()
}
fn sizes(b: &CudaProgramPlanBuilder) -> (usize, usize, usize, usize) {
    (
        b.values.len(),
        b.plan.inputs.len(),
        b.plan.scratch.len(),
        b.plan.steps.len(),
    )
}
fn input(b: &mut CudaProgramPlanBuilder, dtype: CudaDtype, layout: Layout) -> CudaValue {
    match dtype {
        CudaDtype::F32 => b.input(layout),
        CudaDtype::U32 => b.input_u32(layout),
        CudaDtype::F16 | CudaDtype::Bf16 => b.input_low(dtype.low_dtype().unwrap(), layout),
    }
    .unwrap()
}

#[test]
fn raw_typed_views_keep_dtype_offsets_and_materialize_only_when_needed() {
    for dtype in [CudaDtype::U32, CudaDtype::F16, CudaDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let x = input(
            &mut b,
            dtype,
            Layout::new(shape(&[2, 3]), vec![1, 2], 5).unwrap(),
        );
        let view = b.permute(x, &[1, 0]).unwrap();
        let dense_alias = b.reshape(view, shape(&[6])).unwrap();
        let copy = b.reshape(x, shape(&[6])).unwrap();
        let p = b.finish(&[x, dense_alias, copy, copy]).unwrap();
        assert_eq!(p.inputs[0].dtype, dtype);
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.scratch[0].dtype, dtype);
        let Step::Unary { source, op: 10, .. } = &p.steps[0] else {
            panic!()
        };
        assert_eq!(source.dtype, dtype);
        assert_eq!(source.layout.offset(), 5);
        assert!(p.outputs.iter().all(|o| o.dtype == dtype));
        assert_eq!(p.outputs[0].buffer, BufferRef::Input(0));
        assert_eq!(p.outputs[1].buffer, BufferRef::Input(0));
        assert_eq!(p.outputs[1].layout.offset(), 5);
        assert_eq!(p.outputs[2].buffer, BufferRef::Scratch(0));
        assert_eq!(p.outputs[2], p.outputs[3]);
    }
}

#[test]
fn ordinary_f32_entry_points_reject_typed_inputs_before_recording() {
    for dtype in [CudaDtype::U32, CudaDtype::F16, CudaDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let x = input(&mut b, dtype, dense(&[0, 2]));
        let f = b.input(dense(&[0, 2])).unwrap();
        let mask = b.input_u32(dense(&[])).unwrap();
        let before = sizes(&b);
        assert!(matches!(b.unary(x, UnaryOp::Negate), Err(CudaError::Dtype)));
        assert!(matches!(b.affine(x, 1., 0.), Err(CudaError::Dtype)));
        assert!(matches!(
            b.binary(f, x, BinaryOp::Add),
            Err(CudaError::Dtype)
        ));
        assert!(matches!(
            b.compare(x, f, CompareOp::Equal),
            Err(CudaError::Dtype)
        ));
        assert!(matches!(b.select(mask, x, f), Err(CudaError::Dtype)));
        assert!(matches!(
            b.reduce(x, ReduceOp::Sum, &[], false),
            Err(CudaError::Dtype)
        ));
        assert!(matches!(b.mean(x, &[], false), Err(CudaError::Dtype)));
        assert!(matches!(
            b.matmul(x, f, MatmulPrecision::F32),
            Err(CudaError::Dtype)
        ));
        assert!(matches!(
            b.cast_to_low(x, LowDtype::F16),
            Err(CudaError::Dtype)
        ));
        assert_eq!(sizes(&b), before);
    }
}

#[test]
fn mixed_low_types_are_rejected_even_for_empty_results() {
    let mut b = CudaProgramPlanBuilder::new();
    let a = b.input_low(LowDtype::F16, dense(&[0, 2])).unwrap();
    let c = b.input_low(LowDtype::Bf16, dense(&[2, 0])).unwrap();
    let mask = b.input_u32(dense(&[])).unwrap();
    let before = sizes(&b);
    for result in [
        b.binary_low(a, c, BinaryOp::Multiply),
        b.compare_low(a, c, CompareOp::Equal),
        b.select_low(mask, a, c),
        b.matmul_low_f32(a, c),
        b.matmul_low(a, c),
    ] {
        assert!(matches!(
            result,
            Err(CudaError::Contract(TensorError::LowDtypeMismatch {
                left: LowDtype::F16,
                right: LowDtype::Bf16
            }))
        ));
    }
    assert_eq!(sizes(&b), before);
}

#[test]
fn casts_keep_each_explicit_rounding_boundary_and_dtype() {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let x = b
            .input(Layout::new(shape(&[3]), vec![2], 1).unwrap())
            .unwrap();
        let low = b.cast_to_low(x, dtype).unwrap();
        let back = b.cast_to_f32(low).unwrap();
        let p = b.finish(&[back, low]).unwrap();
        assert_eq!(p.steps.len(), 2);
        let Step::Cast { source, to, output } = &p.steps[0] else {
            panic!()
        };
        assert_eq!(
            (source.dtype, *to, *output),
            (CudaDtype::F32, dtype.into(), 0)
        );
        assert_eq!(source.layout.strides(), [2]);
        let Step::Cast { source, to, output } = &p.steps[1] else {
            panic!()
        };
        assert_eq!(
            (source.dtype, *to, *output),
            (dtype.into(), CudaDtype::F32, 1)
        );
        assert_eq!(source.buffer, BufferRef::Scratch(0));
        assert_eq!(p.outputs[0].buffer, BufferRef::Scratch(1));
        assert_eq!(p.outputs[1].buffer, BufferRef::Scratch(0));
    }
}

#[test]
fn unsigned_arithmetic_comparison_and_selection_never_record_float_storage() {
    let mut b = CudaProgramPlanBuilder::new();
    let a = b.input_u32(dense(&[2, 1])).unwrap();
    let c = b.input_u32(dense(&[3])).unwrap();
    for op in [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Min,
        BinaryOp::Max,
    ] {
        b.binary_u32(a, c, op).unwrap();
    }
    let before = sizes(&b);
    assert!(matches!(
        b.binary_u32(a, c, BinaryOp::Divide),
        Err(CudaError::InvalidInput("u32 division is not supported"))
    ));
    assert_eq!(sizes(&b), before);
    for op in [
        CompareOp::Equal,
        CompareOp::NotEqual,
        CompareOp::Less,
        CompareOp::LessEqual,
        CompareOp::Greater,
        CompareOp::GreaterEqual,
    ] {
        let mask = b.compare_u32(a, c, op).unwrap();
        b.select_u32(mask, a, c).unwrap();
    }
    let p = b.finish(&[a, c]).unwrap();
    assert_eq!(p.steps.len(), 17);
    assert!(
        p.inputs
            .iter()
            .chain(&p.scratch)
            .all(|s| s.dtype == CudaDtype::U32)
    );
    assert!(p.scratch.iter().all(|s| s.shape() == &shape(&[2, 3])));
}

#[test]
fn low_arithmetic_comparison_and_selection_record_direct_low_values() {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let a = b.input_low(dtype, dense(&[2, 1])).unwrap();
        let c = b.input_low(dtype, dense(&[3])).unwrap();
        let y = b.unary_low(a, UnaryOp::Abs).unwrap();
        let z = b.binary_low(y, c, BinaryOp::Multiply).unwrap();
        let mask = b.compare_low(z, c, CompareOp::NotEqual).unwrap();
        let selected = b.select_low(mask, z, c).unwrap();
        let p = b.finish(&[selected, mask]).unwrap();
        assert_eq!(p.steps.len(), 4);
        assert_eq!(
            p.scratch.iter().map(|s| s.dtype).collect::<Vec<_>>(),
            [dtype.into(), dtype.into(), CudaDtype::U32, dtype.into()]
        );
        let Step::Select { mask, yes, no, .. } = &p.steps[3] else {
            panic!()
        };
        assert_eq!(mask.dtype, CudaDtype::U32);
        assert_eq!((yes.dtype, no.dtype), (dtype.into(), dtype.into()));
        assert_eq!(no.layout.strides(), [0, 1]);
        assert!(p.steps.iter().all(|s| !matches!(s, Step::Cast { .. })));
    }
}

#[test]
fn three_way_select_allows_empty_mask_to_bound_oversized_value_broadcast() {
    for dtype in [
        CudaDtype::F32,
        CudaDtype::U32,
        CudaDtype::F16,
        CudaDtype::Bf16,
    ] {
        let mut b = CudaProgramPlanBuilder::new();
        let large = 1_usize << (usize::BITS / 2);
        let a = input(
            &mut b,
            dtype,
            Layout::new(shape(&[1, large, 1]), vec![0; 3], 3).unwrap(),
        );
        let c = input(
            &mut b,
            dtype,
            Layout::new(shape(&[1, 1, large]), vec![0; 3], 4).unwrap(),
        );
        let mask = b.input_u32(dense(&[0, 1, 1])).unwrap();
        let selected = match dtype {
            CudaDtype::F32 => b.select(mask, a, c),
            CudaDtype::U32 => b.select_u32(mask, a, c),
            _ => b.select_low(mask, a, c),
        }
        .unwrap();
        let p = b.finish(&[selected]).unwrap();
        assert_eq!(p.outputs[0].layout.shape(), &shape(&[0, large, large]));
        assert_eq!(p.outputs[0].dtype, dtype);
        assert_eq!(p.steps.len(), 1);
    }
}

#[test]
fn reductions_choose_accumulator_dtype_and_low_output_rounds_once() {
    let mut b = CudaProgramPlanBuilder::new();
    let unsigned = b.input_u32(dense(&[2, 3])).unwrap();
    for op in [
        ReduceOp::Sum,
        ReduceOp::Product,
        ReduceOp::Min,
        ReduceOp::Max,
    ] {
        let value = b.reduce_u32(unsigned, op, &[1], true).unwrap();
        assert_eq!(b.value(value).unwrap().dtype, CudaDtype::U32);
    }
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let low = b.input_low(dtype, dense(&[2, 3])).unwrap();
        for mean in [false, true] {
            let start = b.plan.steps.len();
            let value = if mean {
                b.mean_low(low, &[1], false)
            } else {
                b.reduce_low(low, ReduceOp::Max, &[1], false)
            }
            .unwrap();
            assert_eq!(b.plan.steps.len(), start + 2);
            let Step::Reduce {
                source,
                output,
                mean: actual_mean,
                ..
            } = &b.plan.steps[start]
            else {
                panic!()
            };
            assert_eq!(source.dtype, dtype.into());
            assert_eq!(b.plan.scratch[*output].dtype, CudaDtype::F32);
            assert_eq!(*actual_mean, mean);
            let Step::Cast { source, to, .. } = &b.plan.steps[start + 1] else {
                panic!()
            };
            assert_eq!(source.dtype, CudaDtype::F32);
            assert_eq!(*to, dtype.into());
            assert_eq!(b.value(value).unwrap().dtype, dtype.into());
        }
    }
}

#[test]
fn empty_axis_low_reductions_decode_but_unsigned_keeps_raw_identity() {
    let mut b = CudaProgramPlanBuilder::new();
    let u = b.input_u32(dense(&[3])).unwrap();
    assert_eq!(b.reduce_u32(u, ReduceOp::Sum, &[], false).unwrap(), u);
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let low = b.input_low(dtype, dense(&[3])).unwrap();
        for mean in [false, true] {
            let start = b.plan.steps.len();
            let out = if mean {
                b.mean_low_f32(low, &[], true)
            } else {
                b.reduce_low_f32(low, ReduceOp::Sum, &[], false)
            }
            .unwrap();
            assert!(matches!(
                b.plan.steps[start],
                Step::Cast {
                    to: CudaDtype::F32,
                    ..
                }
            ));
            assert_eq!(b.plan.steps.len(), start + 1);
            assert_eq!(b.value(out).unwrap().dtype, CudaDtype::F32);
        }
    }
}

#[test]
fn low_matmul_preserves_native_inputs_and_applies_only_final_cast() {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let a = b
            .input_low(dtype, Layout::new(shape(&[3, 5]), vec![1, 3], 7).unwrap())
            .unwrap();
        let c = b
            .input_low(dtype, Layout::new(shape(&[5]), vec![2], 1).unwrap())
            .unwrap();
        let out = b.matmul_low(a, c).unwrap();
        let p = b.finish(&[out]).unwrap();
        assert_eq!(p.steps.len(), 4);
        assert!(matches!(p.steps[0], Step::Unary { op: 10, .. }));
        assert!(matches!(p.steps[1], Step::Unary { op: 10, .. }));
        let Step::Matmul {
            left,
            right,
            output,
            precision,
        } = &p.steps[2]
        else {
            panic!()
        };
        assert_eq!((left.dtype, right.dtype), (dtype.into(), dtype.into()));
        assert_eq!(*precision, MatmulPrecision::F32);
        assert_eq!(p.scratch[*output].dtype, CudaDtype::F32);
        assert_eq!(right.layout.shape(), &shape(&[5, 1]));
        assert!(matches!(p.steps[3], Step::Cast { to, .. } if to==dtype.into()));
        assert_eq!(p.outputs[0].layout.shape(), &shape(&[3]));
    }
}

#[test]
fn empty_low_matmul_keeps_dtype_for_native_capability_preflight() {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let empty = b.input_low(dtype, dense(&[0])).unwrap();
        let out = b.matmul_low_f32(empty, empty).unwrap();
        let p = b.finish(&[out]).unwrap();
        let Step::Matmul { left, right, .. } = &p.steps[0] else {
            panic!()
        };
        assert_eq!((left.dtype, right.dtype), (dtype.into(), dtype.into()));
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.outputs[0].dtype, CudaDtype::F32);
        assert_eq!(p.outputs[0].layout.shape(), &shape(&[]));
    }
}

#[test]
fn widening_and_compare_output_limits_fail_without_recording() {
    let mut b = CudaProgramPlanBuilder::new();
    let layout = Layout::new(shape(&[usize::MAX / 4 + 1]), vec![0], 0).unwrap();
    let low = b.input_low(LowDtype::Bf16, layout).unwrap();
    let before = sizes(&b);
    assert!(b.cast_to_f32(low).is_err());
    assert!(b.compare_low(low, low, CompareOp::Equal).is_err());
    assert!(b.reduce_low(low, ReduceOp::Sum, &[], false).is_err());
    assert!(b.mean_low(low, &[], false).is_err());
    assert_eq!(sizes(&b), before);
    let value = b.reduce_low(low, ReduceOp::Sum, &[0], false).unwrap();
    assert_eq!(b.value(value).unwrap().dtype, CudaDtype::Bf16);
}

#[test]
fn broadcast_mask_does_not_impose_a_full_size_u32_allocation_on_low_selection() {
    let mut b = CudaProgramPlanBuilder::new();
    let count = usize::MAX / 4 + 1;
    let low = b
        .input_low(
            LowDtype::Bf16,
            Layout::new(shape(&[count]), vec![0], 0).unwrap(),
        )
        .unwrap();
    let mask = b.input_u32(dense(&[])).unwrap();
    let selected = b.select_low(mask, low, low).unwrap();
    let p = b.finish(&[selected]).unwrap();
    assert_eq!(p.inputs[1].shape(), &shape(&[]));
    assert_eq!(p.outputs[0].dtype, CudaDtype::Bf16);
    assert_eq!(p.scratch[0].shape(), &shape(&[count]));
    let Step::Select { mask, .. } = &p.steps[0] else {
        panic!()
    };
    assert_eq!(mask.layout.strides(), [0]);
    assert_eq!(mask.layout.required_storage_len().unwrap(), 1);
}

#[test]
fn typed_foreign_and_invalid_mask_values_leave_no_partial_graph() {
    let mut foreign = CudaProgramPlanBuilder::new();
    let other = foreign.input_low(LowDtype::F16, dense(&[3])).unwrap();
    let mut b = CudaProgramPlanBuilder::new();
    let low = b.input_low(LowDtype::F16, dense(&[3])).unwrap();
    let f = b.input(dense(&[])).unwrap();
    let before = sizes(&b);
    assert!(b.binary_low(low, other, BinaryOp::Add).is_err());
    assert!(b.matmul_low(low, other).is_err());
    assert!(b.cast_to_f32(other).is_err());
    assert!(matches!(b.select_low(f, low, low), Err(CudaError::Dtype)));
    assert!(matches!(
        b.reduce_u32(low, ReduceOp::Sum, &[], false),
        Err(CudaError::Dtype)
    ));
    assert!(matches!(b.cast_to_f32(f), Err(CudaError::Dtype)));
    assert_eq!(sizes(&b), before);
}
