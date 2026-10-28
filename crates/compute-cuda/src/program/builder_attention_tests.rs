use super::*;
use tensor_core::{AttentionMask, AttentionOptions, LowDtype};
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn dense(d: &[usize]) -> Layout {
    Layout::contiguous(shape(d)).unwrap()
}
fn sizes(b: &CudaProgramPlanBuilder) -> (usize, usize, usize) {
    (b.values.len(), b.plan.scratch.len(), b.plan.steps.len())
}

#[test]
fn attention_keeps_original_views_and_canonical_broadcast_gqa_geometry() {
    let mut b = CudaProgramPlanBuilder::new();
    let ql = Layout::new(shape(&[2, 4, 3, 5]), vec![0, 15, 5, 1], 7).unwrap();
    let kl = Layout::new(shape(&[1, 2, 7, 5]), vec![0, 35, 1, 7], 3).unwrap();
    let vl = Layout::new(shape(&[2, 7, 11]), vec![77, 11, 1], 9).unwrap();
    let q = b.input(ql.clone()).unwrap();
    let k = b.input(kl.clone()).unwrap();
    let v = b.input(vl.clone()).unwrap();
    let mask = b.input_u32(dense(&[])).unwrap();
    for offset in [i32::MIN, -2, 0, i32::MAX] {
        let y = b
            .attention(
                q,
                k,
                v,
                AttentionMask::Keep(&mask),
                AttentionOptions {
                    scale: Some(-2.),
                    causal: Some(offset),
                },
            )
            .unwrap();
        assert_eq!(b.value(y).unwrap().layout.shape(), &shape(&[2, 4, 3, 11]));
        let Step::Attention {
            query,
            key,
            value,
            mask,
            plan,
            ..
        } = b.plan.steps.last().unwrap()
        else {
            panic!()
        };
        assert_eq!((&query.layout, &key.layout, &value.layout), (&ql, &kl, &vl));
        assert!(mask.as_ref().unwrap().1);
        assert_eq!(plan.scores.dims(), &[2, 4, 3, 7]);
        assert_eq!(plan.group_size, 2);
        assert_eq!(plan.causal, Some(offset));
        assert_eq!(plan.scale, -2.);
    }
}
#[test]
fn attention_low_final_cast_and_rank_two_output() {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut b = CudaProgramPlanBuilder::new();
        let q = b.input_low(dtype, dense(&[3, 5])).unwrap();
        let k = b.input_low(dtype, dense(&[7, 5])).unwrap();
        let v = b.input_low(dtype, dense(&[7, 2])).unwrap();
        let mask = b.input(dense(&[7])).unwrap();
        let y = b
            .attention_low(
                q,
                k,
                v,
                AttentionMask::Additive(&mask),
                AttentionOptions::default(),
            )
            .unwrap();
        assert_eq!(b.value(y).unwrap().dtype, dtype.into());
        assert_eq!(b.value(y).unwrap().layout.shape(), &shape(&[3, 2]));
        assert_eq!(b.plan.steps.len(), 2);
        let Step::Attention {
            query,
            mask,
            output,
            plan,
            ..
        } = &b.plan.steps[0]
        else {
            panic!()
        };
        assert_eq!(query.dtype, dtype.into());
        assert!(!mask.as_ref().unwrap().1);
        assert_eq!(b.plan.scratch[*output].dtype, CudaDtype::F32);
        assert_eq!(plan.query.dims(), &[1, 3, 5]);
        assert!(matches!(b.plan.steps[1],Step::Cast{to,..}if to==dtype.into()));
    }
}
#[test]
fn attention_validates_types_masks_and_geometry_even_for_empty_results() {
    let mut b = CudaProgramPlanBuilder::new();
    let q = b.input_low(LowDtype::F16, dense(&[0, 2])).unwrap();
    let k = b.input_low(LowDtype::F16, dense(&[0, 2])).unwrap();
    let v = b.input_low(LowDtype::Bf16, dense(&[0, 3])).unwrap();
    let f = b.input(dense(&[0, 2])).unwrap();
    let u = b.input_u32(dense(&[])).unwrap();
    let before = sizes(&b);
    assert!(matches!(
        b.attention_low(q, k, v, AttentionMask::None, AttentionOptions::default()),
        Err(CudaError::Contract(TensorError::LowDtypeMismatch { .. }))
    ));
    assert!(
        b.attention(
            f,
            f,
            f,
            AttentionMask::Keep(&f),
            AttentionOptions::default()
        )
        .is_err()
    );
    assert!(
        b.attention(
            f,
            f,
            f,
            AttentionMask::Additive(&u),
            AttentionOptions::default()
        )
        .is_err()
    );
    assert!(
        b.attention(q, k, q, AttentionMask::None, AttentionOptions::default())
            .is_err()
    );
    let mut other = CudaProgramPlanBuilder::new();
    let foreign = other.input_u32(dense(&[])).unwrap();
    assert!(
        b.attention(
            f,
            f,
            f,
            AttentionMask::Keep(&foreign),
            AttentionOptions::default()
        )
        .is_err()
    );
    assert!(
        b.attention(
            f,
            f,
            f,
            AttentionMask::None,
            AttentionOptions {
                scale: Some(f32::INFINITY),
                causal: None
            }
        )
        .is_err()
    );
    assert_eq!(sizes(&b), before);
    let zero = b
        .attention(f, f, f, AttentionMask::None, AttentionOptions::default())
        .unwrap();
    assert!(b.value(zero).unwrap().layout.shape().is_empty());
    assert!(matches!(b.plan.steps.last(), Some(Step::Attention { .. })));
    let bad = b.input(dense(&[0, 0])).unwrap();
    let before = sizes(&b);
    assert!(
        b.attention(
            bad,
            bad,
            bad,
            AttentionMask::None,
            AttentionOptions::default()
        )
        .is_err()
    );
    assert_eq!(sizes(&b), before);
}
#[test]
fn attention_scores_do_not_require_dense_storage_but_output_bytes_are_checked() {
    let mut b = CudaProgramPlanBuilder::new();
    let q = b
        .input(Layout::new(shape(&[65537, 1]), vec![0, 0], 0).unwrap())
        .unwrap();
    let k = b
        .input(Layout::new(shape(&[65539, 1]), vec![0, 0], 0).unwrap())
        .unwrap();
    let m = b.input_u32(dense(&[])).unwrap();
    let y = b
        .attention(
            q,
            k,
            k,
            AttentionMask::Keep(&m),
            AttentionOptions::default(),
        )
        .unwrap();
    let Step::Attention { plan, .. } = &b.plan.steps[0] else {
        panic!()
    };
    assert!(plan.scores.numel() > u32::MAX as usize);
    assert_eq!(b.value(y).unwrap().layout.shape(), &shape(&[65537, 1]));
    let n = 1usize << (usize::BITS / 2 - 1);
    let q = b
        .input(Layout::new(shape(&[n, 1, 1, 1, 1]), vec![0; 5], 0).unwrap())
        .unwrap();
    let k = b
        .input(Layout::new(shape(&[1, n, 1, 1, 1]), vec![0; 5], 0).unwrap())
        .unwrap();
    let before = sizes(&b);
    assert!(
        b.attention(q, k, k, AttentionMask::None, AttentionOptions::default())
            .is_err()
    );
    assert_eq!(sizes(&b), before);
}
