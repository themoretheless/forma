use super::{
    CudaPrepareOptions,
    builder::{CudaProgramPlanBuilder, CudaValue},
    operation::{Destination, Operation},
    plan::{BufferRef, CudaDtype},
    preparation::{Expanded, expand},
};
use crate::CudaError;
use tensor_core::{Layout, LowDtype, ScanOptions, ScatterOp, Shape};
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn dense(dims: &[usize]) -> Layout {
    Layout::contiguous(shape(dims)).unwrap()
}
fn finish(b: CudaProgramPlanBuilder, outputs: &[CudaValue]) -> Expanded {
    expand(
        b.finish(outputs).unwrap(),
        24,
        CudaPrepareOptions::default(),
        |_| Ok(()),
    )
    .unwrap()
}
#[test]
fn scan_retains_full_hierarchy_and_uses_f32_after_native_low_first_pass() {
    for dtype in [
        CudaDtype::F32,
        CudaDtype::U32,
        CudaDtype::F16,
        CudaDtype::Bf16,
    ] {
        let mut b = CudaProgramPlanBuilder::new();
        let n = 131077;
        let layout = Layout::new(shape(&[n]), vec![2], 7).unwrap();
        let options = ScanOptions {
            inclusive: false,
            reverse: true,
        };
        let (x, y) = match dtype {
            CudaDtype::F32 => {
                let x = b.input(layout).unwrap();
                (x, b.scan(x, 0, options).unwrap())
            }
            CudaDtype::U32 => {
                let x = b.input_u32(layout).unwrap();
                (x, b.scan_u32(x, 0, options).unwrap())
            }
            _ => {
                let x = b.input_low(dtype.low_dtype().unwrap(), layout).unwrap();
                (x, b.scan_low_f32(x, 0, options).unwrap())
            }
        };
        let _ = x;
        let p = finish(b, &[y]);
        assert_eq!(p.stats.kernel_launches, 6);
        assert_eq!(p.stats.memset_calls, 0);
        assert_eq!(p.stats.scratch_bytes, (n + 2 * 513 + 2 * 3 + 1) * 4);
        let accumulator = if dtype == CudaDtype::U32 {
            CudaDtype::U32
        } else {
            CudaDtype::F32
        };
        assert!(p.scratch.iter().all(|s| s.dtype == accumulator));
        let scans: Vec<_> = p
            .schedule
            .iter()
            .filter_map(|s| {
                if let Operation::Scan {
                    source,
                    totals,
                    pass,
                } = &s.operation
                {
                    Some((source, totals, pass))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(scans.len(), 3);
        assert_eq!(*scans[0].0, BufferRef::Input(0));
        assert_eq!(scans[0].2.totals_shape, shape(&[1, 513]));
        for pair in scans.windows(2) {
            assert_eq!(*pair[1].0, BufferRef::Scratch(*pair[0].1));
        }
        assert_eq!(
            p.schedule
                .iter()
                .filter(|s| matches!(s.operation, Operation::ScanCarry { .. }))
                .count(),
            2
        );
    }
}
#[test]
fn compaction_resets_both_outputs_and_retains_u32_prefix_scratch() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input_low(LowDtype::Bf16, dense(&[513])).unwrap();
    let mask = b.input_u32(dense(&[])).unwrap();
    let y = b.compact_low(x, mask).unwrap();
    let p = finish(b, &[y.values, y.count]);
    assert_eq!(p.stats.memset_calls, 2);
    assert_eq!(p.stats.kernel_launches, 7);
    assert_eq!(p.stats.scratch_bytes, 513 * 10 + 32);
    assert!(matches!(p.schedule[0].operation, Operation::Zero));
    assert!(matches!(p.schedule[1].operation, Operation::Zero));
    assert_eq!(p.scratch[0].dtype, CudaDtype::Bf16);
    assert!(p.scratch[1..].iter().all(|s| s.dtype == CudaDtype::U32));
    let compact = p
        .schedule
        .iter()
        .find(|s| matches!(s.operation, Operation::Compact { .. }))
        .unwrap();
    assert_eq!(compact.operation.auxiliary_destination(), Some(1));
    assert!(matches!(compact.destination, Destination::Scratch(0)));
}
#[test]
fn invalid_counts_survive_empty_values_and_zero_axis_lengths() {
    for (dims, axis) in [(vec![0, 3], 1), (vec![0], 0)] {
        let mut b = CudaProgramPlanBuilder::new();
        let x = b.input(dense(&dims)).unwrap();
        let indices = b.input_u32(dense(&[4])).unwrap();
        let y = b.gather(x, indices, axis).unwrap();
        let p = finish(b, &[y.values, y.invalid_count]);
        assert_eq!(p.stats.memset_calls, 1);
        assert_eq!(
            p.schedule
                .iter()
                .filter(|s| matches!(s.operation, Operation::InvalidIndices { .. }))
                .count(),
            1
        );
        let gather_count = p
            .schedule
            .iter()
            .filter(|s| matches!(s.operation, Operation::Gather { .. }))
            .count();
        assert_eq!(gather_count, usize::from(dims.len() == 1));
        assert_eq!(p.stats.kernel_launches, if dims.len() == 1 { 4 } else { 2 });
    }
}
#[test]
fn raw_low_scatter_pads_only_its_private_cas_output_and_resets_owners() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input_low(LowDtype::F16, dense(&[3])).unwrap();
    let i = b.input_u32(dense(&[2])).unwrap();
    let u = b.input_low(LowDtype::F16, dense(&[2])).unwrap();
    let y = b.scatter_low(x, i, u, ScatterOp::Replace, 0).unwrap();
    let p = finish(b, &[y.values, y.invalid_count]);
    assert_eq!(p.scratch[0].shape(), &shape(&[3]));
    assert_eq!(p.scratch[0].physical_len, 4);
    assert_eq!(p.stats.scratch_bytes, 24);
    assert_eq!(p.stats.memset_calls, 2);
    assert_eq!(p.stats.kernel_launches, 6);
    assert_eq!(p.outputs[0].shape(), &shape(&[3]));
    let election = p
        .schedule
        .iter()
        .position(|s| matches!(s.operation, Operation::ScatterOwners { .. }))
        .unwrap();
    assert!(matches!(
        p.schedule[election - 1].operation,
        Operation::Zero
    ));
    assert!(matches!(
        p.schedule[election + 1].operation,
        Operation::Scatter { .. }
    ));
}
#[test]
fn low_scatter_add_widens_base_only_and_rounds_after_f32_accumulation() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input_low(LowDtype::Bf16, dense(&[3])).unwrap();
    let i = b.input_u32(dense(&[2])).unwrap();
    let u = b.input_low(LowDtype::Bf16, dense(&[2])).unwrap();
    let y = b.scatter_low(x, i, u, ScatterOp::Add, 0).unwrap();
    let p = finish(b, &[y.values, y.invalid_count]);
    assert_eq!(p.stats.scratch_bytes, 22);
    assert_eq!(p.stats.memset_calls, 1);
    assert_eq!(p.scratch.len(), 3);
    let scatter = p
        .schedule
        .iter()
        .position(|s| matches!(s.operation, Operation::Scatter { .. }))
        .unwrap();
    match p.schedule[scatter].operation {
        Operation::Scatter { updates, .. } => assert_eq!(updates, BufferRef::Input(2)),
        _ => unreachable!(),
    }
    assert!(matches!(
        p.schedule[0].operation,
        Operation::Cast {
            to: CudaDtype::F32,
            ..
        }
    ));
    assert!(matches!(
        p.schedule[scatter + 1].operation,
        Operation::Cast {
            to: CudaDtype::Bf16,
            ..
        }
    ));
}
#[test]
fn empty_index_scatter_copies_base_without_allocating_owners_or_cas_padding() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input_low(LowDtype::F16, dense(&[3])).unwrap();
    let i = b.input_u32(dense(&[0])).unwrap();
    let u = b.input_low(LowDtype::F16, dense(&[0])).unwrap();
    let y = b.scatter_low(x, i, u, ScatterOp::Replace, 0).unwrap();
    let p = finish(b, &[y.values, y.invalid_count]);
    assert_eq!(p.scratch[0].physical_len, 3);
    assert_eq!(p.stats.scratch_bytes, 10);
    assert_eq!(p.stats.memset_calls, 1);
    assert_eq!(p.stats.kernel_launches, 3);
    assert!(!p.schedule.iter().any(|s| matches!(
        s.operation,
        Operation::Scatter { .. }
            | Operation::ScatterOwners { .. }
            | Operation::InvalidIndices { .. }
    )));
}
#[test]
fn complete_indexing_budget_checks_cas_padding_before_device_work() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input_low(LowDtype::F16, dense(&[3])).unwrap();
    let i = b.input_u32(dense(&[2])).unwrap();
    let u = b.input_low(LowDtype::F16, dense(&[2])).unwrap();
    let y = b.scatter_low(x, i, u, ScatterOp::Replace, 0).unwrap();
    let plan = b.finish(&[y.values, y.invalid_count]).unwrap();
    let result = expand(
        plan,
        24,
        CudaPrepareOptions {
            max_scratch_bytes: 23,
            ..Default::default()
        },
        |_| Ok(()),
    );
    assert!(matches!(
        result,
        Err(CudaError::PreparationBudget {
            kind: "scratch",
            required: 24,
            limit: 23
        })
    ));
}
