use super::*;
use tensor_core::{Compacted, Gathered, ScanOptions, ScatterOp, Scattered};

const DTYPES: [CudaDtype; 4] = [
    CudaDtype::F32,
    CudaDtype::U32,
    CudaDtype::F16,
    CudaDtype::Bf16,
];
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn dense(dims: &[usize]) -> Layout {
    Layout::contiguous(shape(dims)).unwrap()
}
fn input(b: &mut CudaProgramPlanBuilder, dtype: CudaDtype, layout: Layout) -> CudaValue {
    b.input_typed(layout, dtype).unwrap()
}
fn sizes(b: &CudaProgramPlanBuilder) -> (usize, usize, usize, usize) {
    (
        b.values.len(),
        b.plan.inputs.len(),
        b.plan.scratch.len(),
        b.plan.steps.len(),
    )
}
fn scan(
    b: &mut CudaProgramPlanBuilder,
    dtype: CudaDtype,
    x: CudaValue,
    axis: usize,
    options: ScanOptions,
) -> CudaValue {
    match dtype {
        CudaDtype::F32 => b.scan(x, axis, options),
        CudaDtype::U32 => b.scan_u32(x, axis, options),
        _ => b.scan_low_f32(x, axis, options),
    }
    .unwrap()
}
fn gather(
    b: &mut CudaProgramPlanBuilder,
    dtype: CudaDtype,
    x: CudaValue,
    i: CudaValue,
    axis: usize,
) -> Gathered<CudaValue, CudaValue> {
    match dtype {
        CudaDtype::F32 => b.gather(x, i, axis),
        CudaDtype::U32 => b.gather_u32(x, i, axis),
        _ => b.gather_low(x, i, axis),
    }
    .unwrap()
}
fn compact(
    b: &mut CudaProgramPlanBuilder,
    dtype: CudaDtype,
    x: CudaValue,
    m: CudaValue,
) -> Compacted<CudaValue, CudaValue> {
    match dtype {
        CudaDtype::F32 => b.compact(x, m),
        CudaDtype::U32 => b.compact_u32(x, m),
        _ => b.compact_low(x, m),
    }
    .unwrap()
}
fn scatter(
    b: &mut CudaProgramPlanBuilder,
    dtype: CudaDtype,
    x: CudaValue,
    i: CudaValue,
    u: CudaValue,
    op: ScatterOp,
    axis: usize,
) -> Scattered<CudaValue, CudaValue> {
    match dtype {
        CudaDtype::F32 => b.scatter(x, i, u, op, axis),
        CudaDtype::U32 => b.scatter_u32(x, i, u, op, axis),
        _ => b.scatter_low(x, i, u, op, axis),
    }
    .unwrap()
}
fn count(b: &CudaProgramPlanBuilder, value: CudaValue) {
    let p = b.value(value).unwrap();
    assert_eq!(p.dtype, CudaDtype::U32);
    assert_eq!(p.layout, dense(&[]));
    assert!(matches!(p.buffer, BufferRef::Scratch(_)));
}

#[test]
fn scans_preserve_axis_modes_and_strides_with_explicit_low_final_rounding() {
    for dtype in DTYPES {
        for axis in 0..3 {
            for inclusive in [false, true] {
                for reverse in [false, true] {
                    let mut b = CudaProgramPlanBuilder::new();
                    let layout = Layout::new(shape(&[3, 513, 2]), vec![2, 0, 1], 7).unwrap();
                    let x = input(&mut b, dtype, layout.clone());
                    let options = ScanOptions { inclusive, reverse };
                    let y = scan(&mut b, dtype, x, axis, options);
                    let expected_dtype = if dtype == CudaDtype::U32 {
                        dtype
                    } else {
                        CudaDtype::F32
                    };
                    assert_eq!(b.value(y).unwrap().dtype, expected_dtype);
                    assert_eq!(b.value(y).unwrap().layout, dense(&[3, 513, 2]));
                    let Step::Scan {
                        source,
                        output,
                        axis: a,
                        options: o,
                    } = &b.plan.steps[0]
                    else {
                        panic!()
                    };
                    assert_eq!(source.layout, layout);
                    assert_eq!(source.dtype, dtype);
                    assert_eq!((*output, *a, *o), (0, axis, options));
                    if dtype.low_dtype().is_some() {
                        let low = b.scan_low(x, axis, options).unwrap();
                        assert_eq!(b.value(low).unwrap().dtype, dtype);
                        assert!(matches!(b.plan.steps[1], Step::Scan { .. }));
                        let Step::Cast { source, to, .. } = &b.plan.steps[2] else {
                            panic!()
                        };
                        assert_eq!(source.dtype, CudaDtype::F32);
                        assert_eq!(*to, dtype);
                    }
                }
            }
        }
    }
}

#[test]
fn gather_replaces_only_selected_axis_and_retains_once_per_index_count() {
    for dtype in DTYPES {
        for (index_dims, axis, expected) in [
            (vec![2, 3], 1, vec![5, 2, 3, 7]),
            (vec![], 0, vec![11, 7]),
            (vec![0, 3], 2, vec![5, 11, 0, 3]),
        ] {
            let mut b = CudaProgramPlanBuilder::new();
            let layout = Layout::new(shape(&[5, 11, 7]), vec![7, 35, 1], 3).unwrap();
            let x = input(&mut b, dtype, layout.clone());
            let ilayout = Layout::new(shape(&index_dims), vec![0; index_dims.len()], 9).unwrap();
            let i = b.input_u32(ilayout.clone()).unwrap();
            let r = gather(&mut b, dtype, x, i, axis);
            assert_eq!(b.value(r.values).unwrap().layout.shape().dims(), expected);
            assert_eq!(b.value(r.values).unwrap().dtype, dtype);
            count(&b, r.invalid_count);
            assert_eq!(b.plan.scratch.len(), 2);
            let Step::Gather {
                source,
                indices,
                output,
                invalid_count,
                axis: a,
            } = &b.plan.steps[0]
            else {
                panic!()
            };
            assert_eq!(
                (source.layout.clone(), indices.layout.clone()),
                (layout, ilayout)
            );
            assert_eq!((*output, *invalid_count, *a), (0, 1, axis));
            let p = b.finish(&[r.values, r.invalid_count, r.values]).unwrap();
            assert_eq!(p.outputs[0], p.outputs[2]);
        }
    }
}

#[test]
fn compact_broadcast_mask_retains_raw_values_full_capacity_and_scalar_count() {
    for dtype in DTYPES {
        let mut b = CudaProgramPlanBuilder::new();
        let source = Layout::new(shape(&[3, 5]), vec![1, 3], 11).unwrap();
        let x = input(&mut b, dtype, source.clone());
        let m = b
            .input_u32(Layout::new(shape(&[1, 5]), vec![0, 2], 4).unwrap())
            .unwrap();
        let r = compact(&mut b, dtype, x, m);
        assert_eq!(b.value(r.values).unwrap().layout, dense(&[15]));
        assert_eq!(b.value(r.values).unwrap().dtype, dtype);
        count(&b, r.count);
        let Step::Compact {
            source: s,
            mask,
            output,
            count,
        } = &b.plan.steps[0]
        else {
            panic!()
        };
        assert_eq!(s.layout, source);
        assert_eq!(mask.layout.shape().dims(), &[3, 5]);
        assert_eq!(mask.layout.strides(), &[0, 2]);
        assert_eq!(mask.layout.offset(), 4);
        assert_eq!((*output, *count), (0, 1));
        assert_eq!(b.plan.steps.len(), 1); // No value conversion/materialization.
    }
}

#[test]
fn empty_results_keep_index_count_steps_and_scan_validates_scalar_axis() {
    for dtype in DTYPES {
        let mut b = CudaProgramPlanBuilder::new();
        let x = input(&mut b, dtype, dense(&[0, 7]));
        let i = b.input_u32(dense(&[13])).unwrap();
        let u = input(&mut b, dtype, dense(&[]));
        let m = b.input_u32(dense(&[])).unwrap();
        let g = gather(&mut b, dtype, x, i, 1);
        let c = compact(&mut b, dtype, x, m);
        let s = scatter(&mut b, dtype, x, i, u, ScatterOp::Replace, 1);
        for (value, counter) in [
            (g.values, g.invalid_count),
            (c.values, c.count),
            (s.values, s.invalid_count),
        ] {
            assert!(b.value(value).unwrap().layout.shape().is_empty());
            count(&b, counter);
        }
        assert_eq!(b.plan.steps.len(), 3);
        let empty_scan = scan(&mut b, dtype, x, 0, ScanOptions::default());
        assert!(b.value(empty_scan).unwrap().layout.shape().is_empty());
        let scalar = input(&mut b, dtype, dense(&[]));
        let before = sizes(&b);
        let r = match dtype {
            CudaDtype::F32 => b.scan(scalar, 0, ScanOptions::default()),
            CudaDtype::U32 => b.scan_u32(scalar, 0, ScanOptions::default()),
            _ => b.scan_low(scalar, 0, ScanOptions::default()),
        };
        assert!(matches!(
            r,
            Err(CudaError::Contract(TensorError::AxisOutOfBounds { .. }))
        ));
        assert_eq!(sizes(&b), before);
    }
}

#[path = "builder_indexing_error_tests.rs"]
mod error_tests;
#[path = "builder_indexing_scatter_tests.rs"]
mod scatter_tests;

#[test]
fn zero_target_axis_keeps_nonempty_gather_and_empty_scatter_count_work() {
    for dtype in DTYPES {
        let mut b = CudaProgramPlanBuilder::new();
        let x = input(&mut b, dtype, dense(&[2, 0, 5]));
        let i = b.input_u32(dense(&[13])).unwrap();
        let u = input(&mut b, dtype, dense(&[]));
        let g = gather(&mut b, dtype, x, i, 1);
        let s = scatter(&mut b, dtype, x, i, u, ScatterOp::Replace, 1);
        assert_eq!(
            b.value(g.values).unwrap().layout.shape().dims(),
            &[2, 13, 5]
        );
        assert_eq!(b.value(s.values).unwrap().layout.shape().dims(), &[2, 0, 5]);
        count(&b, g.invalid_count);
        count(&b, s.invalid_count);
        assert_ne!(g.invalid_count, s.invalid_count);
        assert_eq!(b.plan.steps.len(), 2);
        let Step::Scatter { updates, .. } = &b.plan.steps[1] else {
            panic!()
        };
        assert_eq!(updates.layout.shape().dims(), &[2, 13, 5]);
        assert_eq!(updates.layout.strides(), &[0, 0, 0]);
    }
}

#[test]
fn scalar_compaction_preserves_source_offset_and_one_raw_slot() {
    for dtype in DTYPES {
        let mut b = CudaProgramPlanBuilder::new();
        let x = input(&mut b, dtype, Layout::new(shape(&[]), vec![], 5).unwrap());
        let mask = b.input_u32(dense(&[])).unwrap();
        let c = compact(&mut b, dtype, x, mask);
        assert_eq!(b.value(c.values).unwrap().layout, dense(&[1]));
        assert_eq!(b.value(c.values).unwrap().dtype, dtype);
        count(&b, c.count);
        let Step::Compact { source, mask, .. } = &b.plan.steps[0] else {
            panic!()
        };
        assert_eq!(source.layout.offset(), 5);
        assert_eq!(mask.layout.shape().dims(), &[]);
    }
}
