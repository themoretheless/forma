use super::super::plan::StatisticsKind;
use super::*;
use tensor_core::LowDtype;
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn dense(d: &[usize]) -> Layout {
    Layout::contiguous(shape(d)).unwrap()
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
fn statistics_record_all_kinds_axes_and_both_moments_results() {
    let mut b = CudaProgramPlanBuilder::new();
    let layout = Layout::new(shape(&[2, 3, 5]), vec![1, 0, 7], 11).unwrap();
    let x = b.input(layout.clone()).unwrap();
    let s = b.softmax(x, &[2, 0]).unwrap();
    let l = b.log_softmax(x, &[2, 0]).unwrap();
    let e = b.logsumexp(x, &[2, 0], true).unwrap();
    let m = b.moments(x, &[2, 0], false).unwrap();
    let n = b.layer_norm(x, &[2, 0], f32::from_bits(1)).unwrap();
    assert_eq!(b.plan.steps.len(), 5);
    for (step, kind) in b.plan.steps.iter().zip([
        StatisticsKind::Softmax,
        StatisticsKind::LogSoftmax,
        StatisticsKind::Logsumexp,
        StatisticsKind::Moments,
        StatisticsKind::LayerNorm {
            epsilon: f32::from_bits(1),
        },
    ]) {
        let Step::Statistics {
            source,
            axes,
            kind: actual,
            variance,
            output,
        } = step
        else {
            panic!()
        };
        assert_eq!(source.layout, layout);
        assert_eq!(axes, &[2, 0]);
        assert_eq!(*actual, kind);
        assert_eq!(variance.is_some(), kind == StatisticsKind::Moments);
        if let Some(v) = variance {
            assert_ne!(*v, *output);
            assert_eq!(b.plan.scratch[*v], b.plan.scratch[*output]);
        }
    }
    for v in [s, l, n] {
        assert_eq!(b.value(v).unwrap().layout.shape(), &shape(&[2, 3, 5]));
    }
    assert_eq!(b.value(e).unwrap().layout.shape(), &shape(&[1, 3, 1]));
    for v in [m.mean, m.variance] {
        assert_eq!(b.value(v).unwrap().layout.shape(), &shape(&[3]));
    }
    assert_ne!(m.mean, m.variance);
    let p = b.finish(&[m.mean, m.variance, m.mean]).unwrap();
    assert_eq!(p.outputs[0], p.outputs[2]);
}

#[test]
fn low_statistics_load_directly_then_round_each_completed_result_once() {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let x = b.input_low(dtype, dense(&[2, 257])).unwrap();
        let wide = [
            b.softmax_low_f32(x, &[1]).unwrap(),
            b.log_softmax_low_f32(x, &[1]).unwrap(),
            b.logsumexp_low_f32(x, &[1], false).unwrap(),
            b.layer_norm_low_f32(x, &[1], 1e-5).unwrap(),
        ];
        let m = b.moments_low_f32(x, &[1], true).unwrap();
        for v in wide.into_iter().chain([m.mean, m.variance]) {
            assert_eq!(b.value(v).unwrap().dtype, CudaDtype::F32);
        }
        assert_eq!(b.plan.steps.len(), 5);
        assert!(b.plan.steps.iter().all(|s|matches!(s,Step::Statistics{source,..} if source.dtype==dtype.into() && source.buffer==BufferRef::Input(0))));
        let start = b.plan.steps.len();
        let low = [
            b.softmax_low(x, &[1]).unwrap(),
            b.log_softmax_low(x, &[1]).unwrap(),
            b.logsumexp_low(x, &[1], false).unwrap(),
            b.layer_norm_low(x, &[1], 1e-5).unwrap(),
        ];
        for v in low {
            assert_eq!(b.value(v).unwrap().dtype, dtype.into());
        }
        for pair in b.plan.steps[start..].as_chunks::<2>().0 {
            assert!(matches!(pair[0], Step::Statistics { .. }));
            assert!(matches!(pair[1],Step::Cast{to,..} if to==dtype.into()));
        }
        let start = b.plan.steps.len();
        let m = b.moments_low(x, &[1], false).unwrap();
        assert_eq!(b.plan.steps.len(), start + 3);
        assert!(matches!(
            b.plan.steps[start],
            Step::Statistics {
                variance: Some(_),
                ..
            }
        ));
        for v in [m.mean, m.variance] {
            assert_eq!(b.value(v).unwrap().dtype, dtype.into());
        }
    }
}

#[test]
fn empty_and_singleton_contracts_validate_before_recording() {
    let mut b = CudaProgramPlanBuilder::new();
    let x = b.input(dense(&[2, 0])).unwrap();
    assert!(b.softmax(x, &[1]).is_ok());
    assert!(b.log_softmax(x, &[1]).is_ok());
    assert!(b.layer_norm(x, &[1], 1e-5).is_ok());
    let before = sizes(&b);
    assert!(matches!(
        b.logsumexp(x, &[1], false),
        Err(CudaError::Contract(TensorError::EmptyReduction))
    ));
    assert!(matches!(
        b.moments(x, &[1], false),
        Err(CudaError::Contract(TensorError::EmptyReduction))
    ));
    for epsilon in [0., -0., -1., f32::INFINITY, f32::NAN] {
        assert!(b.layer_norm(x, &[], epsilon).is_err());
    }
    assert!(b.softmax(x, &[0, 0]).is_err());
    assert!(b.moments(x, &[2], false).is_err());
    assert_eq!(sizes(&b), before);
    let empty = b.moments(x, &[0], false).unwrap();
    assert_eq!(b.value(empty.mean).unwrap().layout.shape(), &shape(&[0]));
    let scalar = b.input(dense(&[])).unwrap();
    let m = b.moments(scalar, &[], true).unwrap();
    assert_eq!(b.value(m.mean).unwrap().layout, dense(&[]));
    assert_eq!(b.value(m.variance).unwrap().layout, dense(&[]));
}

#[test]
fn statistics_type_foreign_overflow_and_compound_rollback() {
    let mut b = CudaProgramPlanBuilder::new();
    let u = b.input_u32(dense(&[0])).unwrap();
    let f = b.input(dense(&[0])).unwrap();
    let low = b.input_low(LowDtype::Bf16, dense(&[2])).unwrap();
    let mut other = CudaProgramPlanBuilder::new();
    let foreign = other.input(dense(&[2])).unwrap();
    let before = sizes(&b);
    assert!(matches!(b.softmax(u, &[]), Err(CudaError::Dtype)));
    assert!(matches!(
        b.moments_low(f, &[], false),
        Err(CudaError::Dtype)
    ));
    assert!(b.logsumexp(foreign, &[], false).is_err());
    let failed: Result<(), CudaError> = b.transaction(|b| {
        let m = b.moments_low(low, &[0], false)?;
        assert_ne!(m.mean, m.variance);
        Err(CudaError::InvalidInput("late failure"))
    });
    assert!(failed.is_err());
    assert_eq!(sizes(&b), before);
    let huge = b
        .input_low(
            LowDtype::Bf16,
            Layout::new(shape(&[usize::MAX / 4 + 1]), vec![0], 0).unwrap(),
        )
        .unwrap();
    let before = sizes(&b);
    assert!(b.softmax_low(huge, &[]).is_err());
    assert!(b.moments_low_f32(huge, &[], false).is_err());
    assert_eq!(sizes(&b), before);
    assert!(b.moments_low_f32(huge, &[0], false).is_ok());
}
