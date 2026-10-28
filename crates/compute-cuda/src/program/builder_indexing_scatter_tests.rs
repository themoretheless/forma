use super::*;

#[test]
fn scatter_modes_keep_base_shape_and_broadcast_updates_without_value_expansion() {
    for dtype in DTYPES {
        for op in [
            ScatterOp::Replace,
            ScatterOp::Add,
            ScatterOp::Multiply,
            ScatterOp::Min,
            ScatterOp::Max,
        ] {
            let mut b = CudaProgramPlanBuilder::new();
            let layout = Layout::new(shape(&[2, 3, 5]), vec![5, 10, 1], 7).unwrap();
            let x = input(&mut b, dtype, layout.clone());
            let ilayout = Layout::new(shape(&[7, 1]), vec![2, 0], 3).unwrap();
            let i = b.input_u32(ilayout.clone()).unwrap();
            let u = input(
                &mut b,
                dtype,
                Layout::new(shape(&[1, 7, 1, 5]), vec![0, 5, 0, 1], 11).unwrap(),
            );
            let r = scatter(&mut b, dtype, x, i, u, op, 1);
            assert_eq!(b.value(r.values).unwrap().layout, dense(&[2, 3, 5]));
            assert_eq!(b.value(r.values).unwrap().dtype, dtype);
            count(&b, r.invalid_count);
            let Step::Scatter {
                source,
                indices,
                updates,
                output,
                invalid_count,
                op: actual,
                axis,
            } = &b.plan.steps[0]
            else {
                panic!()
            };
            assert_eq!(source.layout, layout);
            assert_eq!(indices.layout, ilayout);
            assert_eq!(updates.layout.shape().dims(), &[2, 7, 1, 5]);
            assert_eq!(updates.layout.strides(), &[0, 5, 0, 1]);
            assert_eq!(updates.layout.offset(), 11);
            assert_eq!(updates.dtype, dtype);
            assert_eq!((*output, *invalid_count, *actual, *axis), (0, 1, op, 1));
            let final_cast =
                dtype.low_dtype().is_some() && matches!(op, ScatterOp::Add | ScatterOp::Multiply);
            assert_eq!(b.plan.steps.len(), if final_cast { 2 } else { 1 });
            assert_eq!(
                b.plan.scratch[0].dtype,
                if final_cast { CudaDtype::F32 } else { dtype }
            );
            if final_cast {
                let Step::Cast { source, to, output } = &b.plan.steps[1] else {
                    panic!()
                };
                assert_eq!(source.buffer, BufferRef::Scratch(0));
                assert_eq!((*to, *output), (dtype, 2));
            }
        }
    }
}

#[test]
fn every_low_scatter_mode_can_keep_f32_accumulator_without_rounding_to_low() {
    for dtype in [CudaDtype::F16, CudaDtype::Bf16] {
        for op in [
            ScatterOp::Replace,
            ScatterOp::Add,
            ScatterOp::Multiply,
            ScatterOp::Min,
            ScatterOp::Max,
        ] {
            let mut b = CudaProgramPlanBuilder::new();
            let x = input(&mut b, dtype, dense(&[5]));
            let i = b.input_u32(dense(&[])).unwrap();
            let u = input(&mut b, dtype, dense(&[]));
            let r = b.scatter_low_f32(x, i, u, op, 0).unwrap();
            assert_eq!(b.value(r.values).unwrap().dtype, CudaDtype::F32);
            count(&b, r.invalid_count);
            assert_eq!(b.plan.steps.len(), 1);
            let Step::Scatter {
                source, updates, ..
            } = &b.plan.steps[0]
            else {
                panic!()
            };
            assert_eq!((source.dtype, updates.dtype), (dtype, dtype));
            assert_eq!(updates.layout.shape().dims(), &[]);
        }
    }
}

#[test]
fn scalar_indices_and_empty_index_axes_use_shared_scatter_shape_rules() {
    for dtype in DTYPES {
        for (index_shape, update_shape, expected) in [
            (vec![], vec![3], vec![2, 3]),
            (vec![0], vec![1, 0, 3], vec![2, 0, 3]),
        ] {
            let mut b = CudaProgramPlanBuilder::new();
            let x = input(&mut b, dtype, dense(&[2, 5, 3]));
            let i = b.input_u32(dense(&index_shape)).unwrap();
            let u = input(&mut b, dtype, dense(&update_shape));
            let r = scatter(&mut b, dtype, x, i, u, ScatterOp::Max, 1);
            assert_eq!(b.value(r.values).unwrap().layout, dense(&[2, 5, 3]));
            count(&b, r.invalid_count);
            let Step::Scatter { updates, .. } = &b.plan.steps[0] else {
                panic!()
            };
            assert_eq!(updates.layout.shape().dims(), expected);
        }
    }
}

#[test]
fn count_outputs_compose_into_later_index_and_scan_nodes_with_aliasing_inputs() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b
        .input_low(tensor_core::LowDtype::Bf16, dense(&[7]))
        .unwrap();
    let mask = b.input_u32(dense(&[])).unwrap();
    let c = b.compact_low(x, mask).unwrap();
    let g = b.gather_low(c.values, c.count, 0).unwrap();
    let s = b
        .scatter_low(c.values, c.count, g.values, ScatterOp::Replace, 0)
        .unwrap();
    let scan = b
        .scan_low(
            s.values,
            0,
            ScanOptions {
                inclusive: false,
                reverse: true,
            },
        )
        .unwrap();
    let r = b.scatter_low(x, mask, x, ScatterOp::Min, 0);
    // Full x cannot broadcast to scalar updates: no accidental alias shortcut.
    assert!(r.is_err());
    let indices = b.input_u32(dense(&[7])).unwrap();
    let alias = b.scatter_low(x, indices, x, ScatterOp::Max, 0).unwrap();
    let Step::Scatter {
        source, updates, ..
    } = &b.plan.steps[5]
    else {
        panic!()
    };
    assert_eq!(source.buffer, updates.buffer);
    assert_ne!(b.value(alias.values).unwrap().buffer, source.buffer);
    let p = b
        .finish(&[scan, s.invalid_count, c.count, g.invalid_count, scan])
        .unwrap();
    assert_eq!(p.steps.len(), 6);
    assert_eq!(p.outputs[0], p.outputs[4]);
    assert!(p.outputs[1..4].iter().all(|v| v.dtype == CudaDtype::U32));
}
