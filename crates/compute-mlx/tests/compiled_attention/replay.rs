use super::*;

#[test]
fn replay_gqa_broadcast_masks_signed_causal_and_post_dot_scale() {
    let Some(b) = backend() else { return };
    for (mask_mode, scale, causal) in [
        (0, None, None),
        (1, Some(0.), Some(0)),
        (2, Some(-0.75), Some(-2)),
        (2, Some(1.25), Some(2)),
        (0, None, Some(i32::MIN)),
        (0, None, Some(i32::MAX)),
    ] {
        let options = AttentionOptions { scale, causal };
        let mut graph = b.program();
        let qv = graph.input(shape(&[1, 4, 3, 3])).unwrap();
        let kv = graph.input(shape(&[2, 2, 5, 3])).unwrap();
        let vv = graph.input(shape(&[2, 2, 5, 3])).unwrap();
        let mv = if mask_mode == 1 {
            graph.input_u32(shape(&[3, 5])).unwrap()
        } else {
            graph.input(shape(&[3, 5])).unwrap()
        };
        let mask = match mask_mode {
            0 => AttentionMask::None,
            1 => AttentionMask::Keep(&mv),
            _ => AttentionMask::Additive(&mv),
        };
        let output = graph.attention(qv, kv, vv, mask, options).unwrap();
        let program = graph.compile(&[output, output]).unwrap();
        for version in 0..2 {
            let q: Vec<_> = (0..36)
                .map(|i| ((i + version) % 7) as f32 * 0.125)
                .collect();
            let k: Vec<_> = (0..60)
                .map(|i| ((i + 2 * version) % 11) as f32 * 0.125)
                .collect();
            let v: Vec<_> = (0..60)
                .map(|i| (1 + (i + 3 * version) % 13) as f32 * 0.25)
                .collect();
            let bias: Vec<_> = (0..15)
                .map(|i| {
                    if mask_mode == 0 {
                        0.
                    } else if i < 5 || i % 4 == version {
                        f32::NEG_INFINITY
                    } else if mask_mode == 1 {
                        0.
                    } else {
                        (i % 3) as f32 * 0.0625
                    }
                })
                .collect();
            let qt = upload_view(&b, &[1, 4, 3, 3], &q, version == 1);
            let kt = upload_view(&b, &[2, 2, 5, 3], &k, version == 1);
            let vt = upload_view(&b, &[2, 2, 5, 3], &v, version == 1);
            let mt = if mask_mode == 1 {
                b.upload_u32(
                    shape(&[3, 5]),
                    &bias
                        .iter()
                        .map(|x| u32::from(x.is_finite()) * 7)
                        .collect::<Vec<_>>(),
                )
                .unwrap()
            } else {
                upload_view(&b, &[3, 5], &bias, version == 1)
            };
            let output = program
                .run_typed(&[(&qt).into(), (&kt).into(), (&vt).into(), (&mt).into()])
                .unwrap();
            let expected = reference(&q, &k, &v, &bias, options);
            assert_eq!(output[0].shape(), &shape(&[2, 4, 3, 3]));
            close(
                &b.read_f32(output[0].as_tensor().unwrap()).unwrap(),
                &expected,
            );
            close(
                &b.read_f32(output[1].as_tensor().unwrap()).unwrap(),
                &expected,
            );
            traces(&program, version + 1);
        }
    }
}

#[test]
fn direct_low_replay_retains_f32_result_and_one_final_rounding() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let q = graph.input_low(dtype, shape(&[1, 4, 3, 3])).unwrap();
        let k = graph.input_low(dtype, shape(&[2, 2, 5, 3])).unwrap();
        let v = graph.input_low(dtype, shape(&[2, 2, 5, 3])).unwrap();
        let m = graph.input(shape(&[3, 5])).unwrap();
        let options = AttentionOptions {
            scale: Some(1.25),
            causal: Some(1),
        };
        let f = graph
            .attention_low_f32(q, k, v, AttentionMask::Additive(&m), options)
            .unwrap();
        let low = graph
            .attention_low(q, k, v, AttentionMask::Additive(&m), options)
            .unwrap();
        let program = graph.compile(&[f, low]).unwrap();
        for version in 0..2 {
            let qs: Vec<_> = (0..36)
                .map(|i| ((i + version) % 7) as f32 * 0.125)
                .collect();
            let ks: Vec<_> = (0..60)
                .map(|i| ((i + version) % 9) as f32 * 0.125)
                .collect();
            let vs: Vec<_> = (0..60)
                .map(|i| (1 + (i + version) % 13) as f32 * 0.25)
                .collect();
            let bias: Vec<_> = (0..15)
                .map(|i| {
                    if i < 5 {
                        f32::NEG_INFINITY
                    } else {
                        (i % 3) as f32 * 0.0003
                    }
                })
                .collect();
            let qt = upload_low_view(&b, dtype, &[1, 4, 3, 3], &qs, version == 1);
            let kt = upload_low_view(&b, dtype, &[2, 2, 5, 3], &ks, version == 1);
            let vt = upload_low_view(&b, dtype, &[2, 2, 5, 3], &vs, version == 1);
            let mt = upload_view(&b, &[3, 5], &bias, version == 1);
            let outputs = program
                .run_typed(&[(&qt).into(), (&kt).into(), (&vt).into(), (&mt).into()])
                .unwrap();
            let actual = b.read_f32(outputs[0].as_tensor().unwrap()).unwrap();
            close(&actual, &reference(&qs, &ks, &vs, &bias, options));
            assert_eq!(
                b.read_low_bits(outputs[1].as_low().unwrap()).unwrap(),
                actual.iter().map(|&x| encode(dtype, x)).collect::<Vec<_>>()
            );
            traces(&program, version + 1);
        }
    }
}
