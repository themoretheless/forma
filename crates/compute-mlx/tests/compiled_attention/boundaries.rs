use super::*;

#[test]
fn native_causal_closed_rows_and_finite_maximum_values() {
    let Some(b) = backend() else { return };
    for queries in [1, 5, 17] {
        let keys = if queries == 5 { 3 } else { 65 };
        let options = AttentionOptions {
            scale: None,
            causal: Some(keys as i32 - queries as i32),
        };
        let mut g = b.program();
        let q = g.input(shape(&[queries, 64])).unwrap();
        let k = g.input(shape(&[keys, 64])).unwrap();
        let v = g.input(shape(&[keys, 64])).unwrap();
        let out = g.attention(q, k, v, AttentionMask::None, options).unwrap();
        let program = g.compile(&[out]).unwrap();
        let qt = b
            .upload_f32(shape(&[queries, 64]), &vec![0.; queries * 64])
            .unwrap();
        let kt = b
            .upload_f32(shape(&[keys, 64]), &vec![0.; keys * 64])
            .unwrap();
        for (run, value) in [f32::MAX, 1.].into_iter().enumerate() {
            let vt = b
                .upload_f32(shape(&[keys, 64]), &vec![value; keys * 64])
                .unwrap();
            let result = program.run(&[&qt, &kt, &vt]).unwrap();
            let eager = b
                .attention(&qt, &kt, &vt, AttentionMask::None, options)
                .unwrap();
            let expected: Vec<_> = (0..queries)
                .flat_map(|row| {
                    std::iter::repeat_n(if row + keys < queries { 0. } else { value }, 64)
                })
                .collect();
            close(&b.read_f32(&result[0]).unwrap(), &expected);
            close(&b.read_f32(&eager).unwrap(), &expected);
            traces(&program, run + 1);
        }
    }
}

#[test]
fn low_tiny_times_large_dot_is_normal_and_lazy_resources_survive() {
    let Some(b) = backend() else { return };
    for reverse in [false, true] {
        let dtype = LowDtype::Bf16;
        let mut g = b.program();
        let q = g.input_low(dtype, shape(&[1, 1])).unwrap();
        let k = g.input_low(dtype, shape(&[2, 1])).unwrap();
        let v = g.input_low(dtype, shape(&[2, 1])).unwrap();
        let options = AttentionOptions {
            scale: Some(1.),
            causal: None,
        };
        let out = g
            .attention_low_f32(q, k, v, AttentionMask::None, options)
            .unwrap();
        let program = g.compile(&[out]).unwrap();
        let tiny = half::bf16::from_bits(1).to_f32();
        let huge = half::bf16::from_bits(0x7f7f).to_f32();
        let qt = b
            .upload_low(dtype, shape(&[1, 1]), &[if reverse { 0x7f7f } else { 1 }])
            .unwrap();
        let kt = b
            .upload_low(
                dtype,
                shape(&[2, 1]),
                &[0, if reverse { 1 } else { 0x7f7f }],
            )
            .unwrap();
        let vt = b.upload_low(dtype, shape(&[2, 1]), &[0, 0x3f80]).unwrap();
        let out = program
            .run_typed(&[(&qt).into(), (&kt).into(), (&vt).into()])
            .unwrap();
        traces(&program, 1);
        drop((program, qt, kt, vt));
        let x = f64::from(tiny) * f64::from(huge);
        close(
            &b.read_f32(out[0].as_tensor().unwrap()).unwrap(),
            &[(1. / (1. + (-x).exp())) as f32],
        );
    }
}

#[test]
fn zero_keys_and_rejected_nodes_leave_graph_usable() {
    let Some(b) = backend() else { return };
    let mut foreign = b.program();
    let foreign_mask = foreign.input_u32(shape(&[])).unwrap();
    let mut g = b.program();
    let q = g.input(shape(&[2, 3])).unwrap();
    let k = g.input(shape(&[0, 3])).unwrap();
    let v = g.input(shape(&[0, 4])).unwrap();
    let wrong_mask = g.input_u32(shape(&[])).unwrap();
    let options = AttentionOptions::default();
    assert!(matches!(
        g.attention(q, k, v, AttentionMask::Keep(&foreign_mask), options),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        g.attention(q, k, v, AttentionMask::Additive(&wrong_mask), options),
        Err(MlxError::Dtype)
    ));
    assert!(
        g.attention(
            q,
            k,
            v,
            AttentionMask::None,
            AttentionOptions {
                scale: Some(f32::NAN),
                causal: None
            }
        )
        .is_err()
    );
    let out = g
        .attention(q, k, v, AttentionMask::Keep(&wrong_mask), options)
        .unwrap();
    let program = g.compile(&[out]).unwrap();
    let qt = b.upload_f32(shape(&[2, 3]), &[1.; 6]).unwrap();
    let kt = b.upload_f32(shape(&[0, 3]), &[]).unwrap();
    let vt = b.upload_f32(shape(&[0, 4]), &[]).unwrap();
    let mask = b.upload_u32(shape(&[]), &[9]).unwrap();
    let result = program
        .run_typed(&[(&qt).into(), (&kt).into(), (&vt).into(), (&mask).into()])
        .unwrap();
    assert_eq!(result[0].shape(), &shape(&[2, 4]));
    assert_eq!(b.read_f32(result[0].as_tensor().unwrap()).unwrap(), [0.; 8]);
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let other = if dtype == LowDtype::F16 {
            LowDtype::Bf16
        } else {
            LowDtype::F16
        };
        let mut g = b.program();
        let q = g.input_low(dtype, shape(&[0, 3])).unwrap();
        let k = g.input_low(other, shape(&[0, 3])).unwrap();
        let v = g.input_low(dtype, shape(&[0, 4])).unwrap();
        assert!(matches!(
            g.attention_low_f32(q, k, v, AttentionMask::None, options),
            Err(MlxError::Contract(
                tensor_core::TensorError::LowDtypeMismatch { .. }
            ))
        ));
        let k = g.input_low(dtype, shape(&[0, 3])).unwrap();
        let out = g
            .attention_low(q, k, v, AttentionMask::None, options)
            .unwrap();
        let program = g.compile(&[out]).unwrap();
        let q = b.upload_low(dtype, shape(&[0, 3]), &[]).unwrap();
        let wrong = b.upload_low(other, shape(&[0, 3]), &[]).unwrap();
        let k = b.upload_low(dtype, shape(&[0, 3]), &[]).unwrap();
        let v = b.upload_low(dtype, shape(&[0, 4]), &[]).unwrap();
        let out = program
            .run_typed(&[(&q).into(), (&wrong).into(), (&v).into(), (&k).into()])
            .unwrap();
        assert_eq!(out[0].shape(), &shape(&[0, 4]));
        assert!(
            b.read_low_bits(out[0].as_low().unwrap())
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn direct_low_long_masked_values_and_maximum_remain_finite() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let n = 4097;
        let mut g = b.program();
        let q = g.input_low(dtype, shape(&[1, 1])).unwrap();
        let k = g.input_low(dtype, shape(&[n, 1])).unwrap();
        let v = g.input_low(dtype, shape(&[n, 1])).unwrap();
        let m = g.input_u32(shape(&[n])).unwrap();
        let out = g
            .attention_low_f32(
                q,
                k,
                v,
                AttentionMask::Keep(&m),
                AttentionOptions::default(),
            )
            .unwrap();
        let program = g.compile(&[out]).unwrap();
        let q = b.upload_low(dtype, shape(&[1, 1]), &[0]).unwrap();
        let k = b.upload_low(dtype, shape(&[n, 1]), &vec![0; n]).unwrap();
        let max = if dtype == LowDtype::F16 {
            0x7bff
        } else {
            0x7f7f
        };
        for (run, closed) in [true, false].into_iter().enumerate() {
            let mut values = vec![max; n];
            if closed {
                values[n - 1] = encode(dtype, 1.);
            }
            let v = b.upload_low(dtype, shape(&[n, 1]), &values).unwrap();
            let mut mask = vec![u32::from(!closed); n];
            mask[n - 1] = 1;
            let m = b.upload_u32(shape(&[n]), &mask).unwrap();
            let output = program
                .run_typed(&[(&q).into(), (&k).into(), (&v).into(), (&m).into()])
                .unwrap();
            let expected = if closed {
                1.
            } else if dtype == LowDtype::F16 {
                half::f16::from_bits(max).to_f32()
            } else {
                half::bf16::from_bits(max).to_f32()
            };
            close(
                &b.read_f32(output[0].as_tensor().unwrap()).unwrap(),
                &[expected],
            );
            traces(&program, run + 1);
        }
    }
}
