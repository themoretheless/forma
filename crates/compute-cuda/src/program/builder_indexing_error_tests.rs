use super::*;

#[test]
fn wrong_dtypes_indices_masks_and_axes_leave_every_slot_unchanged() {
    let mut b = CudaProgramPlanBuilder::new();
    let f = b.input(dense(&[0, 3])).unwrap();
    let u = b.input_u32(dense(&[0, 3])).unwrap();
    let i = b.input_u32(dense(&[])).unwrap();
    let l = b
        .input_low(tensor_core::LowDtype::F16, dense(&[0, 3]))
        .unwrap();
    let before = sizes(&b);
    assert!(matches!(
        b.scan(u, 1, ScanOptions::default()),
        Err(CudaError::Dtype)
    ));
    assert!(matches!(
        b.scan_u32(f, 1, ScanOptions::default()),
        Err(CudaError::Dtype)
    ));
    assert!(matches!(
        b.scan_low(f, 1, ScanOptions::default()),
        Err(CudaError::Dtype)
    ));
    assert!(matches!(b.gather(f, f, 1), Err(CudaError::Dtype)));
    assert!(matches!(b.gather_u32(f, i, 1), Err(CudaError::Dtype)));
    assert!(matches!(b.gather_low(f, i, 1), Err(CudaError::Dtype)));
    assert!(matches!(b.compact(f, f), Err(CudaError::Dtype)));
    assert!(matches!(b.compact_u32(f, i), Err(CudaError::Dtype)));
    assert!(matches!(b.compact_low(f, i), Err(CudaError::Dtype)));
    assert!(matches!(
        b.scatter(f, i, u, ScatterOp::Add, 1),
        Err(CudaError::Dtype)
    ));
    assert!(matches!(
        b.scatter_u32(u, f, u, ScatterOp::Add, 1),
        Err(CudaError::Dtype)
    ));
    assert!(matches!(
        b.scatter_low(l, f, l, ScatterOp::Add, 1),
        Err(CudaError::Dtype)
    ));
    assert!(b.gather(f, i, 2).is_err());
    assert!(b.scatter(f, i, f, ScatterOp::Replace, 2).is_err());
    assert_eq!(sizes(&b), before);
}

#[test]
fn low_scatter_requires_matching_dtype_even_when_outputs_are_empty() {
    let mut b = CudaProgramPlanBuilder::new();
    let f16 = b
        .input_low(tensor_core::LowDtype::F16, dense(&[0]))
        .unwrap();
    let bf16 = b
        .input_low(tensor_core::LowDtype::Bf16, dense(&[0]))
        .unwrap();
    let i = b.input_u32(dense(&[0])).unwrap();
    let before = sizes(&b);
    for op in [
        ScatterOp::Replace,
        ScatterOp::Add,
        ScatterOp::Multiply,
        ScatterOp::Min,
        ScatterOp::Max,
    ] {
        for result in [
            b.scatter_low(f16, i, bf16, op, 0),
            b.scatter_low_f32(f16, i, bf16, op, 0),
        ] {
            assert!(matches!(
                result,
                Err(CudaError::Contract(TensorError::LowDtypeMismatch { .. }))
            ));
        }
    }
    assert_eq!(sizes(&b), before);
}

#[test]
fn foreign_values_and_late_compound_errors_restore_both_results() {
    let mut foreign = CudaProgramPlanBuilder::new();
    let other = foreign.input_u32(dense(&[])).unwrap();
    let mut b = CudaProgramPlanBuilder::new();
    let x = b
        .input_low(tensor_core::LowDtype::F16, dense(&[3]))
        .unwrap();
    let i = b.input_u32(dense(&[])).unwrap();
    let u = b.input_low(tensor_core::LowDtype::F16, dense(&[])).unwrap();
    let before = sizes(&b);
    assert!(b.gather_low(x, other, 0).is_err());
    assert!(b.compact_low(x, other).is_err());
    assert!(b.scatter_low(x, other, u, ScatterOp::Min, 0).is_err());
    assert_eq!(sizes(&b), before);
    let failed: Result<(), CudaError> = b.transaction(|b| {
        let r = b.scatter_low(x, i, u, ScatterOp::Add, 0)?;
        assert_eq!(b.value(r.values)?.dtype, CudaDtype::F16);
        count(b, r.invalid_count);
        assert_eq!(b.plan.scratch.len(), 3);
        Err(CudaError::InvalidInput("injected late compound failure"))
    });
    assert!(failed.is_err());
    assert_eq!(sizes(&b), before);
    let r = b.gather_low(x, i, 0).unwrap();
    assert_eq!(r.values.index, before.0);
    assert_eq!(r.invalid_count.index, before.0 + 1);
    assert_eq!(b.value(r.values).unwrap().buffer, BufferRef::Scratch(0));
}

#[test]
fn oversized_index_counts_are_checked_even_for_empty_values_outputs() {
    if usize::BITS <= 32 {
        return;
    }
    let n = (u32::MAX as usize).checked_add(1).unwrap();
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[0, 7])).unwrap();
    let i = b
        .input_u32(Layout::new(shape(&[n]), vec![0], 0).unwrap())
        .unwrap();
    let update = b.input(dense(&[])).unwrap();
    let large = b
        .input_low(
            tensor_core::LowDtype::Bf16,
            Layout::new(shape(&[n]), vec![0], 0).unwrap(),
        )
        .unwrap();
    let mask = b.input_u32(dense(&[])).unwrap();
    let before = sizes(&b);
    assert!(
        matches!(b.gather(x, i, 1), Err(CudaError::Contract(TensorError::IndexCountOverflow { count })) if count == n)
    );
    assert!(
        matches!(b.scatter(x, i, update, ScatterOp::Add, 1), Err(CudaError::Contract(TensorError::IndexCountOverflow { count })) if count == n)
    );
    assert!(
        matches!(b.compact_low(large, mask), Err(CudaError::Contract(TensorError::IndexCountOverflow { count })) if count == n)
    );
    assert_eq!(sizes(&b), before);
}

#[test]
fn gather_expansion_and_low_accumulator_byte_limits_fail_without_partial_counts() {
    let mut b = CudaProgramPlanBuilder::new();
    let n = usize::MAX / 4;
    let x = b
        .input(Layout::new(shape(&[n, 1]), vec![0, 0], 0).unwrap())
        .unwrap();
    let i = b.input_u32(dense(&[2])).unwrap();
    let low_n = n + 1;
    let low = b
        .input_low(
            tensor_core::LowDtype::Bf16,
            Layout::new(shape(&[low_n]), vec![0], 0).unwrap(),
        )
        .unwrap();
    let scalar_i = b.input_u32(dense(&[])).unwrap();
    let update = b
        .input_low(tensor_core::LowDtype::Bf16, dense(&[]))
        .unwrap();
    let before = sizes(&b);
    assert!(b.gather(x, i, 1).is_err());
    assert!(b.scan_low_f32(low, 0, ScanOptions::default()).is_err());
    assert!(b.scan_low(low, 0, ScanOptions::default()).is_err());
    assert!(
        b.scatter_low_f32(low, scalar_i, update, ScatterOp::Min, 0)
            .is_err()
    );
    assert!(
        b.scatter_low(low, scalar_i, update, ScatterOp::Add, 0)
            .is_err()
    );
    assert_eq!(sizes(&b), before);
    // Raw Min needs only native16 result storage and remains a valid plan.
    let raw = b
        .scatter_low(low, scalar_i, update, ScatterOp::Min, 0)
        .unwrap();
    assert_eq!(b.value(raw.values).unwrap().dtype, CudaDtype::Bf16);
}

#[test]
fn raw_low_scatter_rejects_word_padding_overflow_before_recording() {
    let mut b = CudaProgramPlanBuilder::new();
    let n = usize::MAX / 2;
    let x = b
        .input_low(
            tensor_core::LowDtype::F16,
            Layout::new(shape(&[n]), vec![0], 0).unwrap(),
        )
        .unwrap();
    let i = b.input_u32(dense(&[])).unwrap();
    let u = b.input_low(tensor_core::LowDtype::F16, dense(&[])).unwrap();
    let before = sizes(&b);
    for op in [ScatterOp::Replace, ScatterOp::Min, ScatterOp::Max] {
        assert!(b.scatter_low(x, i, u, op, 0).is_err());
    }
    assert_eq!(sizes(&b), before);
}

#[test]
fn expanded_updates_and_mask_must_fit_without_expanding_base_shape() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[2, 3])).unwrap();
    let i = b.input_u32(dense(&[5])).unwrap();
    let u = b.input(dense(&[4, 5])).unwrap();
    let mask = b.input_u32(dense(&[4, 2, 3])).unwrap();
    let before = sizes(&b);
    assert!(matches!(
        b.scatter(x, i, u, ScatterOp::Add, 1),
        Err(CudaError::Contract(
            TensorError::IncompatibleBroadcast { .. }
        ))
    ));
    assert!(matches!(
        b.compact(x, mask),
        Err(CudaError::Contract(
            TensorError::IncompatibleBroadcast { .. }
        ))
    ));
    assert_eq!(sizes(&b), before);

    // A tiny broadcast backing allocation cannot waive the expanded update
    // tensor's logical byte limit, matching the eager CUDA view contract.
    let wide = usize::MAX / 4;
    let x = b
        .input(Layout::new(shape(&[wide, 1]), vec![0, 0], 0).unwrap())
        .unwrap();
    let u = b.input(dense(&[])).unwrap();
    let before = sizes(&b);
    assert!(b.scatter(x, i, u, ScatterOp::Add, 1).is_err());
    assert_eq!(sizes(&b), before);
}
