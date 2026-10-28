use tensor_core::{AttentionOptions, AttentionPlan, Shape, TensorError};
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
#[test]
fn matrix_attention_and_grouped_broadcast_geometry() {
    let plan = AttentionPlan::new(
        &shape(&[3, 4]),
        &shape(&[7, 4]),
        &shape(&[7, 5]),
        Some(&shape(&[3, 7])),
        AttentionOptions::default(),
    )
    .unwrap();
    assert_eq!(plan.output, shape(&[3, 5]));
    assert_eq!(plan.query, shape(&[1, 3, 4]));
    assert_eq!(plan.scores, shape(&[1, 3, 7]));
    assert_eq!(plan.scale, 0.5);
    let plan = AttentionPlan::new(
        &shape(&[2, 1, 6, 3, 4]),
        &shape(&[1, 5, 2, 7, 4]),
        &shape(&[5, 2, 7, 9]),
        Some(&shape(&[1, 6, 3, 7])),
        AttentionOptions {
            scale: Some(-2.0),
            causal: Some(-3),
        },
    )
    .unwrap();
    assert_eq!(plan.batch, shape(&[2, 5]));
    assert_eq!(plan.output, shape(&[2, 5, 6, 3, 9]));
    assert_eq!(plan.group_size, 3);
    assert_eq!(plan.causal, Some(-3));
    assert_eq!(plan.scale, -2.0);
}
#[test]
fn invalid_geometry_and_masks_fail_before_empty_execution() {
    let q = shape(&[4, 3, 8]);
    let k = shape(&[2, 7, 8]);
    let v = shape(&[2, 7, 5]);
    let options = AttentionOptions::default();
    for (q, k, v) in [
        (shape(&[]), k.clone(), v.clone()),
        (shape(&[8]), k.clone(), v.clone()),
        (shape(&[3, 3, 8]), k.clone(), v.clone()),
        (q.clone(), shape(&[2, 7, 9]), v.clone()),
        (q.clone(), k.clone(), shape(&[2, 6, 5])),
        (q.clone(), k.clone(), shape(&[1, 7, 5])),
        (shape(&[0, 3, 8]), k.clone(), v.clone()),
        (shape(&[4, 0, 0]), shape(&[2, 7, 0]), v.clone()),
        (shape(&[2, 4, 3, 8]), shape(&[3, 2, 7, 8]), v.clone()),
    ] {
        assert!(AttentionPlan::new(&q, &k, &v, None, options).is_err());
    }
    for mask in [shape(&[5, 3, 7]), shape(&[2, 4, 3, 7]), shape(&[3, 8])] {
        assert!(AttentionPlan::new(&q, &k, &v, Some(&mask), options).is_err());
    }
    for scale in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(matches!(
            AttentionPlan::new(
                &q,
                &k,
                &v,
                None,
                AttentionOptions {
                    scale: Some(scale),
                    causal: None
                }
            ),
            Err(TensorError::InvalidAttentionScale)
        ));
    }
}
#[test]
fn empty_sequence_value_depth_and_batch_keep_checked_shapes() {
    for (q, k, v, expected) in [
        (vec![3, 4], vec![0, 4], vec![0, 5], vec![3, 5]),
        (vec![0, 4], vec![7, 4], vec![7, 5], vec![0, 5]),
        (vec![3, 4], vec![7, 4], vec![7, 0], vec![3, 0]),
        (
            vec![0, 4, 3, 8],
            vec![1, 2, 7, 8],
            vec![2, 7, 5],
            vec![0, 4, 3, 5],
        ),
    ] {
        let plan = AttentionPlan::new(
            &shape(&q),
            &shape(&k),
            &shape(&v),
            None,
            AttentionOptions {
                scale: Some(0.0),
                causal: Some(i32::MIN),
            },
        )
        .unwrap();
        assert_eq!(plan.output, shape(&expected));
    }
    assert!(
        AttentionPlan::new(
            &shape(&[3, 4]),
            &shape(&[0, 4]),
            &shape(&[0, 5]),
            Some(&shape(&[3, 2])),
            AttentionOptions::default()
        )
        .is_err()
    );
}
