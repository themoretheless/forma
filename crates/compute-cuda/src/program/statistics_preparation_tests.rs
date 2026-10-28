use super::{
    CudaPrepareOptions,
    builder::{CudaProgramPlanBuilder, CudaValue},
    operation::Operation,
    plan::BufferRef,
    preparation::{Expanded, expand},
    storage::ScratchSpec,
};
use crate::CudaError;
use tensor_core::{AttentionMask, AttentionOptions, Layout, LowDtype, Shape};
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn dense(d: &[usize]) -> Layout {
    Layout::contiguous(shape(d)).unwrap()
}
fn finish(b: CudaProgramPlanBuilder, out: &[CudaValue]) -> Expanded {
    expand(
        b.finish(out).unwrap(),
        1,
        CudaPrepareOptions::default(),
        |_| Ok(()),
    )
    .unwrap()
}
#[test]
fn stable_rows_budget_real_f64_state_and_bound_partial_storage() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[3, 8193])).unwrap();
    let y = b.softmax(x, &[1]).unwrap();
    let p = finish(b, &[y]);
    assert_eq!(p.stats.scratch_bytes, 3 * 8193 * 4 + (3 + 3 + 3 * 2) * 8);
    assert_eq!(p.stats.kernel_launches, 6);
    assert_eq!(p.stats.memset_calls, 0);
    let ops: Vec<_> = p
        .schedule
        .iter()
        .filter_map(|s| match &s.operation {
            Operation::StatisticsPartial {
                source,
                center,
                pass,
                op,
            } => Some((*source, *center, pass.chunks, *op)),
            _ => None,
        })
        .collect();
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].0, BufferRef::Input(0));
    assert_eq!(ops[0].1, ops[1].1);
    assert_eq!((ops[0].2, ops[0].3, ops[1].3), (2, 0, 1));
}
#[test]
fn moments_have_disjoint_outputs_and_centered_two_passes() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[3, 8193])).unwrap();
    let m = b.moments(x, &[1], false).unwrap();
    let p = finish(b, &[m.mean, m.variance]);
    assert_eq!(p.stats.scratch_bytes, 2 * 3 * 4 + 12 * 8);
    assert_eq!(p.stats.kernel_launches, 7);
    let ops: Vec<_> = p
        .schedule
        .iter()
        .filter_map(|s| match &s.operation {
            Operation::StatisticsPartial { op, .. } => Some(*op),
            _ => None,
        })
        .collect();
    assert_eq!(ops, [2, 3]);
    let s = p
        .schedule
        .iter()
        .find(|s| matches!(s.operation, Operation::StatisticsMoments { .. }))
        .unwrap();
    let super::operation::Destination::Scratch(mean) = s.destination else {
        panic!()
    };
    assert_ne!(Some(mean), s.operation.auxiliary_destination());
}
#[test]
fn low_statistics_load_native_inputs_and_round_after_f32_results() {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let x = b.input_low(dtype, dense(&[2, 5])).unwrap();
        let y = b.softmax_low(x, &[1]).unwrap();
        let p = finish(b, &[y]);
        assert_eq!(p.stats.scratch_bytes, 10 * 4 + 10 * 2 + 6 * 8);
        assert_eq!(p.stats.kernel_launches, 7);
        assert!(
            p.schedule
                .iter()
                .filter_map(|s| match &s.operation {
                    Operation::StatisticsPartial { source, .. }
                    | Operation::StatisticsEmit { source, .. } => Some(source),
                    _ => None,
                })
                .all(|s| *s == BufferRef::Input(0))
        );
        assert_eq!(
            p.schedule
                .iter()
                .filter(|s| matches!(s.operation, Operation::Cast { .. }))
                .count(),
            1
        );
    }
}
#[test]
fn singleton_statistics_write_identities_on_each_replay_without_wide_scratch() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[3, 1])).unwrap();
    let m = b.moments(x, &[1], false).unwrap();
    let a = b.softmax(x, &[1]).unwrap();
    let z = b.log_softmax(x, &[]).unwrap();
    let p = finish(b, &[m.mean, m.variance, a, z]);
    assert_eq!(p.stats.scratch_bytes, 4 * 3 * 4);
    assert_eq!(p.stats.memset_calls, 2);
    assert!(
        p.schedule
            .iter()
            .any(|s| matches!(s.operation,Operation::Fill{count:3,value} if value==1f32.to_bits()))
    );
    assert!(
        !p.schedule
            .iter()
            .any(|s| matches!(s.operation, Operation::StatisticsPartial { .. }))
    );
}
#[test]
fn attention_retains_only_output_sized_f64_state() {
    let mut b = CudaProgramPlanBuilder::new();
    let q = b.input(dense(&[2, 4, 5])).unwrap();
    let k = b.input(dense(&[1, 7, 5])).unwrap();
    let v = b.input(dense(&[1, 7, 3])).unwrap();
    let y = b
        .attention(q, k, v, AttentionMask::None, AttentionOptions::default())
        .unwrap();
    let p = finish(b, &[y]);
    assert_eq!(p.stats.scratch_bytes, 24 * (4 + 8));
    assert_eq!(p.stats.kernel_launches, 2);
    assert_eq!(p.stats.memset_calls, 0);
    assert_eq!(p.scratch.len(), 2);
    assert!(p.schedule[0].operation.auxiliary_destination().is_some());
}
#[test]
fn no_key_attention_reinitializes_outputs_without_an_accumulator() {
    let mut b = CudaProgramPlanBuilder::new();
    let q = b.input(dense(&[2, 4, 5])).unwrap();
    let k = b.input(dense(&[1, 0, 5])).unwrap();
    let v = b.input(dense(&[1, 0, 3])).unwrap();
    let y = b
        .attention(q, k, v, AttentionMask::None, AttentionOptions::default())
        .unwrap();
    let p = finish(b, &[y]);
    assert_eq!(p.stats.scratch_bytes, 24 * 4);
    assert_eq!(p.stats.memset_calls, 1);
    assert_eq!(p.stats.kernel_launches, 1);
    assert_eq!(p.scratch.len(), 1);
}
#[test]
fn budgets_reject_wide_scratch_before_device_allocation() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[2, 5])).unwrap();
    let y = b.softmax(x, &[1]).unwrap();
    let result = expand(
        b.finish(&[y]).unwrap(),
        1,
        CudaPrepareOptions {
            max_scratch_bytes: 87,
            ..Default::default()
        },
        |_| Ok(()),
    );
    assert!(matches!(
        result,
        Err(CudaError::PreparationBudget {
            kind: "scratch",
            required: 88,
            limit: 87
        })
    ));
    assert!(ScratchSpec::f64(shape(&[usize::MAX / 8 + 1])).is_err());
    assert_eq!(ScratchSpec::f64(shape(&[0])).unwrap().bytes().unwrap(), 8);
}
