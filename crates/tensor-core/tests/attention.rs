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

#[test]
fn host_tensor_attention_broadcasts_multi_axis_batches() {
    use tensor_core::Tensor;
    // Q=[2, 1, 1, 2, 2] x K=V=[1, 3, 1, 2, 2] -> [2, 3, 1, 2, 2]
    let q = Tensor::new(
        vec![2, 1, 1, 2, 2],
        vec![
            1.0, 0.0, 0.0, 1.0, // q batch 0
            0.0, 1.0, 1.0, 0.0, // q batch 1
        ],
    )
    .unwrap();
    let k = Tensor::new(
        vec![1, 3, 1, 2, 2],
        vec![
            2.0, 0.0, 0.0, 2.0, // kv batch 0
            0.0, 2.0, 2.0, 0.0, // kv batch 1
            1.0, 1.0, 1.0, 1.0, // kv batch 2
        ],
    )
    .unwrap();
    let v = Tensor::new(
        vec![1, 3, 1, 2, 2],
        vec![
            10.0, 20.0, 30.0, 40.0, // kv batch 0
            100.0, 200.0, 300.0, 400.0, // kv batch 1
            5.0, 7.0, 9.0, 11.0, // kv batch 2
        ],
    )
    .unwrap();
    let out = Tensor::scaled_dot_product_attention(&q, &k, &v, false).unwrap();
    assert_eq!(out.shape().dims(), &[2, 3, 1, 2, 2]);
    for qi in 0..2 {
        let q_sub = Tensor::new(vec![1, 2, 2], q.data()[qi * 4..(qi + 1) * 4].to_vec()).unwrap();
        for kj in 0..3 {
            let k_sub =
                Tensor::new(vec![1, 2, 2], k.data()[kj * 4..(kj + 1) * 4].to_vec()).unwrap();
            let v_sub =
                Tensor::new(vec![1, 2, 2], v.data()[kj * 4..(kj + 1) * 4].to_vec()).unwrap();
            let want = Tensor::scaled_dot_product_attention(&q_sub, &k_sub, &v_sub, false).unwrap();
            let b = qi * 3 + kj;
            let got = &out.data()[b * 4..(b + 1) * 4];
            for (g, w) in got.iter().zip(want.data().iter()) {
                assert!((g - w).abs() < 1e-6);
            }
        }
    }

    // Also test 4D batch broadcast with GQA: Q=[2, 3, 2, 2] (batch=2, q_heads=3) x K=V=[1, 3, 2, 2] (batch=1, kv_heads=3)
    // and Q=[2, 1, 3, 2, 2] x K=V=[1, 3, 1, 2, 2] (batch=[2, 3], q_heads=3, kv_heads=1)
    let q_4d = Tensor::new(vec![2, 1, 2, 2], q.data().to_vec()).unwrap();
    let kv_4d = Tensor::new(vec![1, 1, 2, 2], k.data()[0..4].to_vec()).unwrap();
    let out_4d = Tensor::scaled_dot_product_attention(&q_4d, &kv_4d, &kv_4d, false).unwrap();
    assert_eq!(out_4d.shape().dims(), &[2, 1, 2, 2]);
}
