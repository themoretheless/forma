use super::*;

#[test]
fn layer_norm_and_rms_norm_normalize_rows() {
    let t = Tensor::new(vec![2, 4], vec![1.0, 2.0, 3.0, 4.0, 10.0, 10.0, 10.0, 10.0]).unwrap();
    let ln = t.layer_norm(1e-5).unwrap();
    assert_eq!(ln.shape().dims(), &[2, 4]);
    let row0 = &ln.data()[0..4];
    let mean0: f32 = row0.iter().sum::<f32>() / 4.0;
    let var0: f32 = row0.iter().map(|v| v * v).sum::<f32>() / 4.0;
    assert!(mean0.abs() < 1e-6);
    assert!((var0 - 1.0).abs() < 1e-4);
    // Constant row normalizes to 0
    for &v in &ln.data()[4..8] {
        assert!(v.abs() < 1e-6);
    }

    let rms = t.rms_norm(1e-5).unwrap();
    let rms_sq0: f32 = rms.data()[0..4].iter().map(|v| v * v).sum::<f32>() / 4.0;
    assert!((rms_sq0 - 1.0).abs() < 1e-4);
}

#[test]
fn gelu_and_silu_match_reference_values() {
    let t = Tensor::new(vec![3], vec![-2.0, 0.0, 2.0]).unwrap();
    let g = t.gelu();
    let s = t.silu();
    assert!(g.data()[1].abs() < 1e-7);
    assert!(s.data()[1].abs() < 1e-7);
    assert!(g.data()[2] > 1.9 && g.data()[2] < 2.0);
    assert!(s.data()[2] > 1.7 && s.data()[2] < 1.8);
    assert!(g.data()[0] < 0.0 && g.data()[0] > -0.1);
}

#[test]
fn cosine_similarity_and_topk_compute_expected_outputs() {
    let a = Tensor::new(vec![2, 3], vec![1.0, 0.0, 0.0, 1.0, 2.0, 2.0]).unwrap();
    let b = Tensor::new(vec![2, 3], vec![0.0, 1.0, 0.0, 2.0, 4.0, 4.0]).unwrap();
    let sim = a.cosine_similarity(&b, 1e-8).unwrap();
    assert_eq!(sim.shape().dims(), &[2]);
    assert!(sim.data()[0].abs() < 1e-6);
    assert!((sim.data()[1] - 1.0).abs() < 1e-6);

    let top = a.topk(2).unwrap();
    assert_eq!(top.values.shape().dims(), &[2, 2]);
    assert_eq!(top.values.data(), &[1.0, 0.0, 2.0, 2.0]);
    assert_eq!(top.indices, vec![0, 1, 1, 2]);
}

#[test]
fn softmax_log_softmax_clamp_where_and_l2_normalize_work() {
    let t = Tensor::new(vec![2, 3], vec![1.0, 2.0, 3.0, 1000.0, 1000.0, 1000.0]).unwrap();
    let sm = t.softmax(1).unwrap();
    let row0_sum: f32 = sm.data()[0..3].iter().sum();
    assert!((row0_sum - 1.0).abs() < 1e-6);
    for &p in &sm.data()[3..6] {
        assert!((p - 1.0 / 3.0).abs() < 1e-6);
    }

    let lsm = t.log_softmax(1).unwrap();
    for (s, ls) in sm.data().iter().zip(lsm.data().iter()) {
        assert!((s.ln() - *ls).abs() < 1e-5);
    }

    let clamped = t.clamp(1.5, 2.5);
    assert_eq!(&clamped.data()[0..3], &[1.5, 2.0, 2.5]);

    let a = Tensor::new(vec![4], vec![10.0, 20.0, 30.0, 40.0]).unwrap();
    let b = Tensor::new(vec![4], vec![-1.0, -2.0, -3.0, -4.0]).unwrap();
    let sel = Tensor::where_select(&[true, false, true, false], &a, &b).unwrap();
    assert_eq!(sel.data(), &[10.0, -2.0, 30.0, -4.0]);

    let v = Tensor::new(vec![2, 2], vec![3.0, 4.0, 0.0, 0.0]).unwrap();
    let norm = v.l2_normalize(1, 1e-8).unwrap();
    assert!((norm.data()[0] - 0.6).abs() < 1e-6);
    assert!((norm.data()[1] - 0.8).abs() < 1e-6);
    assert_eq!(&norm.data()[2..4], &[0.0, 0.0]);
}

#[test]
fn scaled_dot_product_attention_conv1d_and_max_pool1d_work() {
    // 2D attention: Q, K, V of shape [2, 2]
    let q = Tensor::new(vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]).unwrap();
    let k = Tensor::new(vec![2, 2], vec![1.0, 0.0, 0.0, 1.0]).unwrap();
    let v = Tensor::new(vec![2, 2], vec![10.0, 20.0, 30.0, 40.0]).unwrap();
    let attn = Tensor::scaled_dot_product_attention(&q, &k, &v, false).unwrap();
    assert_eq!(attn.shape().dims(), &[2, 2]);
    // Query 0 aligns more strongly with Key 0 ([10, 20]) than Key 1 ([30, 40])
    assert!(attn.data()[0] < 20.0 && attn.data()[0] > 10.0);

    // Causal attention: row 0 can only attend to key 0, so output row 0 equals V row 0 exactly!
    let causal_attn = Tensor::scaled_dot_product_attention(&q, &k, &v, true).unwrap();
    assert!((causal_attn.data()[0] - 10.0).abs() < 1e-5);
    assert!((causal_attn.data()[1] - 20.0).abs() < 1e-5);

    // 1D convolution: input [1, 1, 5], weight [1, 1, 3], bias [0.5], stride 1, padding 1
    let x = Tensor::new(vec![1, 1, 5], vec![1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
    let w = Tensor::new(vec![1, 1, 3], vec![1.0, 1.0, 1.0]).unwrap();
    let conv = x.conv1d(&w, Some(&[0.5]), 1, 1).unwrap();
    assert_eq!(conv.shape().dims(), &[1, 1, 5]);
    assert_eq!(conv.data(), &[3.5, 6.5, 9.5, 12.5, 9.5]);

    // 1D max pooling: window 2, stride 2 on [1, 1, 4]
    let p_in = Tensor::new(vec![1, 1, 4], vec![1.0, 5.0, 3.0, 2.0]).unwrap();
    let pooled = p_in.max_pool1d(2, 2).unwrap();
    assert_eq!(pooled.shape().dims(), &[1, 1, 2]);
    assert_eq!(pooled.data(), &[5.0, 3.0]);

    // 1D avg pooling: window 2, stride 2 on [1, 1, 4]
    let avg_pooled = p_in.avg_pool1d(2, 2).unwrap();
    assert_eq!(avg_pooled.shape().dims(), &[1, 1, 2]);
    assert_eq!(avg_pooled.data(), &[3.0, 2.5]);
}

#[test]
fn batch_norm_1d_and_apply_rotary_pos_emb_work() {
    // [N=2, C=2, L=2]
    let x = Tensor::new(
        vec![2, 2, 2],
        vec![
            1.0, 3.0, // n=0, c=0
            10.0, 20.0, // n=0, c=1
            5.0, 7.0, // n=1, c=0
            30.0, 40.0, // n=1, c=1
        ],
    )
    .unwrap();
    let mean = [1.0, 10.0];
    let var = [4.0, 100.0];
    let weight = [2.0, 5.0];
    let bias = [0.5, -1.0];
    let bn = x
        .batch_norm_1d(&mean, &var, Some(&weight), Some(&bias), 1e-6)
        .unwrap();
    assert_eq!(bn.shape().dims(), &[2, 2, 2]);
    // For c=0: ((x - 1) / 2) * 2 + 0.5 = x - 0.5 -> [0.5, 2.5]
    assert!((bn.data()[0] - 0.5).abs() < 1e-4);
    assert!((bn.data()[1] - 2.5).abs() < 1e-4);
    // For c=1: ((x - 10) / 10) * 5 - 1 = 0.5*(x - 10) - 1 -> [-1.0, 4.0]
    assert!((bn.data()[2] - (-1.0)).abs() < 1e-4);
    assert!((bn.data()[3] - 4.0).abs() < 1e-4);

    // RoPE on [seq_len=2, head_dim=4]
    let rope_in = Tensor::new(
        vec![2, 4],
        vec![
            1.0, 0.0, 2.0, 0.0, // m=0 (angle=0 -> unchanged)
            1.0, 0.0, 0.0, 1.0, // m=1 (pair 0 rotated by 1.0 rad)
        ],
    )
    .unwrap();
    let rope_out = rope_in.apply_rotary_pos_emb(10000.0).unwrap();
    assert_eq!(rope_out.shape().dims(), &[2, 4]);
    // Position m=0 is unrotated
    assert!((rope_out.data()[0] - 1.0).abs() < 1e-6);
    assert!((rope_out.data()[1] - 0.0).abs() < 1e-6);
    assert!((rope_out.data()[2] - 2.0).abs() < 1e-6);
    // Position m=1, pair 0 has theta = 1.0 rad -> (cos(1), sin(1))
    assert!((rope_out.data()[4] - 1.0_f32.cos()).abs() < 1e-6);
    assert!((rope_out.data()[5] - 1.0_f32.sin()).abs() < 1e-6);
    // RoPE preserves L2 norm of each position vector
    let norm_in: f32 = rope_in.data()[4..8].iter().map(|v| v * v).sum();
    let norm_out: f32 = rope_out.data()[4..8].iter().map(|v| v * v).sum();
    assert!((norm_in - norm_out).abs() < 1e-5);

    // Group normalization on [N=1, C=4, L=2] with num_groups=2 (each group has 2*2 = 4 elements)
    let gn_in = Tensor::new(
        vec![1, 4, 2],
        vec![
            1.0, 2.0, 3.0, 4.0, // group 0
            10.0, 10.0, 10.0, 10.0, // group 1 (constant -> 0)
        ],
    )
    .unwrap();
    let gn_out = gn_in.group_norm(2, 1e-5).unwrap();
    let g0 = &gn_out.data()[0..4];
    let g0_mean: f32 = g0.iter().sum::<f32>() / 4.0;
    let g0_var: f32 = g0.iter().map(|v| v * v).sum::<f32>() / 4.0;
    assert!(g0_mean.abs() < 1e-6);
    assert!((g0_var - 1.0).abs() < 1e-4);
    for &v in &gn_out.data()[4..8] {
        assert!(v.abs() < 1e-6);
    }

    // Fused SwiGLU matches self.silu() * gate
    let x_val = Tensor::new(vec![3], vec![-1.0, 0.0, 2.0]).unwrap();
    let g_val = Tensor::new(vec![3], vec![2.0, 5.0, 0.5]).unwrap();
    let sw = x_val.swiglu(&g_val).unwrap();
    let silu_ref = x_val.silu();
    for i in 0..3 {
        assert!((sw.data()[i] - silu_ref.data()[i] * g_val.data()[i]).abs() < 1e-6);
    }
}
