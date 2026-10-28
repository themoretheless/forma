use compute_core::{
    ComputeRuntime, GpuLowTensor, GpuTensor,
    gpu_compute::{GpuBuffer, GpuContext},
    tensor_core::{
        AttentionMask, AttentionOptions, Layout, LowDtype, Shape, TensorBackend, TensorLowBackend,
    },
};
fn context() -> Option<GpuContext> {
    let c = GpuContext::new();
    assert!(c.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    c
}
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
// The private fixture uses exactly representable binary fractions.
fn bits(dtype: LowDtype, x: f32) -> u16 {
    let b = x.to_bits();
    if dtype == LowDtype::Bf16 {
        return (b >> 16) as u16;
    }
    if x == 0. {
        return ((b >> 16) & 0x8000) as u16;
    }
    (((b >> 16) & 0x8000) | ((((b >> 23) & 255) - 112) << 10) | ((b & 0x7fffff) >> 13)) as u16
}
fn input(
    rt: &ComputeRuntime,
    dtype: LowDtype,
    rows: usize,
    cols: usize,
) -> (GpuLowTensor, GpuLowTensor) {
    let back = rt.zeros_low(dtype, shape(&[rows * cols + 4])).unwrap();
    let view = GpuLowTensor::from_packed(
        dtype,
        back.packed_words().clone(),
        back.storage_len(),
        Layout::new(shape(&[rows, cols]), vec![1, rows], 1).unwrap(),
    )
    .unwrap();
    (back, view)
}
fn write(
    rt: &ComputeRuntime,
    dtype: LowDtype,
    back: &GpuLowTensor,
    rows: usize,
    cols: usize,
    values: &[f32],
) {
    let mut raw = vec![0x5aa5; back.storage_len()];
    for r in 0..rows {
        for c in 0..cols {
            raw[1 + r + c * rows] = bits(dtype, values[r * cols + c]);
        }
    }
    rt.write_low_storage_bits(back, &raw).unwrap();
}
fn low_output(rt: &ComputeRuntime, dtype: LowDtype, s: Shape) -> (GpuLowTensor, GpuLowTensor) {
    let back = rt
        .upload_low_bits(dtype, shape(&[s.numel() + 4]), &vec![0x5aa5; s.numel() + 4])
        .unwrap();
    let output = back.narrow(0, 1, s.numel()).unwrap().reshape(s).unwrap();
    (back, output)
}
fn float_output(rt: &ComputeRuntime, s: Shape) -> (GpuTensor, GpuTensor) {
    let back = rt
        .upload_f32(shape(&[s.numel() + 4]), &vec![99.; s.numel() + 4])
        .unwrap();
    let output = back.narrow(0, 1, s.numel()).unwrap().reshape(s).unwrap();
    (back, output)
}
fn close(actual: &[f32], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert!(
            a.is_finite() && (f64::from(a) - e).abs() <= 3e-4 * e.abs().max(1.),
            "{i}: {a} != {e}"
        );
    }
}
#[allow(clippy::too_many_arguments)]
fn reference(
    q: &[f32],
    k: &[f32],
    v: &[f32],
    queries: usize,
    keys: usize,
    depth: usize,
    channels: usize,
    bias: &[f32],
    options: AttentionOptions,
) -> Vec<f64> {
    let scale = f64::from(options.scale.unwrap_or((depth as f32).sqrt().recip()));
    let mut out = vec![0.; queries * channels];
    for row in 0..queries {
        let scores: Vec<_> = (0..keys)
            .filter(|&j| {
                options
                    .causal
                    .is_none_or(|o| j as i64 <= row as i64 + i64::from(o))
                    && bias[j] != f32::NEG_INFINITY
            })
            .map(|j| {
                (
                    j,
                    (0..depth)
                        .map(|d| f64::from(q[row * depth + d]) * f64::from(k[j * depth + d]))
                        .sum::<f64>()
                        * scale
                        + f64::from(bias[j]),
                )
            })
            .collect();
        let max = scores.iter().map(|x| x.1).fold(f64::NEG_INFINITY, f64::max);
        let sum = scores.iter().map(|x| (x.1 - max).exp()).sum::<f64>();
        if sum > 0. {
            for c in 0..channels {
                out[row * channels + c] = scores
                    .iter()
                    .map(|&(j, x)| (x - max).exp() / sum * f64::from(v[j * channels + c]))
                    .sum();
            }
        }
    }
    out
}
#[test]
fn direct_low_attention_satisfies_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_low_attention_backend(&rt).unwrap();
}
#[test]
fn strided_packed_tiles_split_keys_and_odd_outputs_replay_changing_inputs_and_masks() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for (keys, channels) in [(0, 65), (1, 1), (33, 65), (512, 63), (513, 129), (4097, 65)] {
            let (queries, depth) = (3, 5);
            let (qb, q) = input(&rt, dtype, queries, depth);
            let (kb, k) = input(&rt, dtype, keys, depth);
            let (vb, v) = input(&rt, dtype, keys, channels);
            let raw_keep = rt.zeros::<u32>(keys * 2 + 3).unwrap();
            let keep = GpuTensor::from_layout(
                raw_keep.clone(),
                Layout::new(shape(&[1, keys]), vec![0, 2], 1).unwrap(),
            )
            .unwrap();
            let bias = rt.upload_f32(shape(&[1, keys]), &vec![0.; keys]).unwrap();
            let floats: Vec<_> = (0..2)
                .map(|_| float_output(&rt, shape(&[queries, channels])))
                .collect();
            let lows: Vec<_> = (0..2)
                .map(|_| low_output(&rt, dtype, shape(&[queries, channels])))
                .collect();
            let options = AttentionOptions {
                scale: Some(-0.75),
                causal: Some(keys as i32 / 2 - 1),
            };
            let mut p = rt.program();
            p.tensor_attention_low_f32_into(
                &q,
                &k,
                &v,
                AttentionMask::Keep(&keep),
                options,
                &floats[0].1,
            )
            .unwrap();
            p.tensor_attention_low_f32_into(
                &q,
                &k,
                &v,
                AttentionMask::Additive(&bias),
                options,
                &floats[1].1,
            )
            .unwrap();
            p.tensor_attention_low_into(
                &q,
                &k,
                &v,
                AttentionMask::Keep(&keep),
                options,
                &lows[0].1,
            )
            .unwrap();
            p.tensor_attention_low_into(
                &q,
                &k,
                &v,
                AttentionMask::Additive(&bias),
                options,
                &lows[1].1,
            )
            .unwrap();
            let total = p.tensor_sum(&floats[0].1, &[0, 1], false).unwrap();
            for iteration in 0..3 {
                let qv: Vec<f32> = (0..queries * depth)
                    .map(|i| ((i + iteration) % 9) as f32 / 8. - 0.5)
                    .collect();
                let kv: Vec<f32> = (0..keys * depth)
                    .map(|i| ((i * 3 + iteration) % 11) as f32 / 8. - 0.625)
                    .collect();
                let vv: Vec<f32> = (0..keys * channels)
                    .map(|i| ((i * 5 + iteration) % 17) as f32 / 8. - 1.)
                    .collect();
                write(&rt, dtype, &qb, queries, depth, &qv);
                write(&rt, dtype, &kb, keys, depth, &kv);
                write(&rt, dtype, &vb, keys, channels, &vv);
                let allowed: Vec<_> = (0..keys)
                    .map(|j| {
                        iteration < 2 && j % 5 != iteration && (iteration == 1 || j >= keys / 4)
                    })
                    .collect();
                let mut mask = vec![0u32; keys * 2 + 3];
                for j in 0..keys {
                    mask[1 + j * 2] = if allowed[j] { u32::MAX } else { 0 };
                }
                rt.write(&raw_keep, 0, &mask).unwrap();
                let biases: Vec<_> = (0..keys)
                    .map(|j| {
                        if allowed[j] {
                            (j % 7) as f32 * 0.001
                        } else {
                            f32::NEG_INFINITY
                        }
                    })
                    .collect();
                rt.write(bias.values(), 0, &biases).unwrap();
                p.submit();
                let keep_bias: Vec<_> = allowed
                    .iter()
                    .map(|&yes| if yes { 0. } else { f32::NEG_INFINITY })
                    .collect();
                let expected = [
                    reference(
                        &qv, &kv, &vv, queries, keys, depth, channels, &keep_bias, options,
                    ),
                    reference(
                        &qv, &kv, &vv, queries, keys, depth, channels, &biases, options,
                    ),
                ];
                for i in 0..2 {
                    let actual = rt.read_f32(&floats[i].0).unwrap();
                    assert_eq!(actual[0], 99.);
                    assert_eq!(&actual[actual.len() - 3..], &[99.; 3]);
                    close(&actual[1..actual.len() - 3], &expected[i]);
                    let raw = rt.read_low_bits(&lows[i].0).unwrap();
                    assert_eq!(raw[0], 0x5aa5);
                    assert_eq!(&raw[raw.len() - 3..], &[0x5aa5; 3]);
                    let cast = rt.cast_to_low(&floats[i].1, dtype).unwrap();
                    assert_eq!(&raw[1..raw.len() - 3], rt.read_low_bits(&cast).unwrap());
                }
                close(&rt.read_f32(&total).unwrap(), &[expected[0].iter().sum()]);
            }
        }
    }
}
#[test]
fn low_attention_validates_dtype_owner_alias_and_empty_masks_before_recording() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let foreign_rt = ComputeRuntime::new(&ctx).unwrap();
    let q = rt.zeros_low(LowDtype::Bf16, shape(&[2, 2])).unwrap();
    let k = q.clone();
    let v = q.clone();
    let foreign = foreign_rt
        .zeros_low(LowDtype::Bf16, shape(&[2, 2]))
        .unwrap();
    let foreign_mask = foreign_rt.zeros::<u32>(4).unwrap();
    let foreign_mask = GpuTensor::from_array(foreign_mask, shape(&[2, 2])).unwrap();
    let (back, output) = low_output(&rt, LowDtype::Bf16, shape(&[2, 2]));
    let (fback, foutput) = float_output(&rt, shape(&[2, 2]));
    let wrong_dtype = rt.zeros_low(LowDtype::F16, shape(&[2, 2])).unwrap();
    let alias_mask = GpuTensor::from_array(back.packed_words().clone(), shape(&[2, 2])).unwrap();
    let allocation = GpuBuffer::new(
        &ctx,
        16,
        compute_core::wgpu::BufferUsages::STORAGE
            | compute_core::wgpu::BufferUsages::COPY_SRC
            | compute_core::wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    let low_alias = GpuLowTensor::from_packed(
        LowDtype::Bf16,
        rt.import_buffer::<u32>(allocation.clone(), 4).unwrap(),
        8,
        Layout::contiguous(shape(&[2, 2])).unwrap(),
    )
    .unwrap();
    let float_alias = GpuTensor::from_array(
        rt.import_buffer::<f32>(allocation, 4).unwrap(),
        shape(&[2, 2]),
    )
    .unwrap();
    let options = AttentionOptions::default();
    let mut p = rt.program();
    assert!(
        p.tensor_attention_low_f32_into(
            &low_alias,
            &k,
            &v,
            AttentionMask::None,
            options,
            &float_alias
        )
        .is_err()
    );
    assert!(
        p.tensor_attention_low_into(
            &q,
            &k,
            &v,
            AttentionMask::Keep(&alias_mask),
            options,
            &output
        )
        .is_err()
    );
    assert!(
        p.tensor_attention_low_f32_into(
            &q,
            &k,
            &v,
            AttentionMask::Additive(&foutput),
            options,
            &foutput
        )
        .is_err()
    );
    assert!(
        p.tensor_attention_low_into(&q, &k, &v, AttentionMask::None, options, &wrong_dtype)
            .is_err()
    );
    assert!(
        p.tensor_attention_low_into(&q, &k, &v, AttentionMask::None, options, &q)
            .is_err()
    );
    assert!(
        p.tensor_attention_low_into(&q, &foreign, &v, AttentionMask::None, options, &output)
            .is_err()
    );
    assert!(
        p.tensor_attention_low_into(&q, &k, &foreign, AttentionMask::None, options, &output)
            .is_err()
    );
    assert!(
        p.tensor_attention_low_into(
            &q,
            &k,
            &v,
            AttentionMask::Keep(&foreign_mask),
            options,
            &output
        )
        .is_err()
    );
    assert!(
        p.tensor_attention_low_into(
            &q,
            &k,
            &v,
            AttentionMask::None,
            options,
            &output.permute(&[1, 0]).unwrap()
        )
        .is_err()
    );
    assert!(
        p.tensor_attention_low_f32_into(
            &q,
            &k,
            &v,
            AttentionMask::None,
            options,
            &foutput.permute(&[1, 0]).unwrap()
        )
        .is_err()
    );
    for scale in [f32::INFINITY, f32::NAN] {
        assert!(
            p.tensor_attention_low_into(
                &q,
                &k,
                &v,
                AttentionMask::None,
                AttentionOptions {
                    scale: Some(scale),
                    causal: None
                },
                &output
            )
            .is_err()
        );
    }
    let empty = rt.zeros_low(LowDtype::Bf16, shape(&[0, 2])).unwrap();
    let other = rt.zeros_low(LowDtype::F16, shape(&[0, 2])).unwrap();
    assert!(
        p.tensor_attention_low_f32(&empty, &other, &empty, AttentionMask::None, options)
            .is_err()
    );
    assert!(
        p.tensor_attention_low(&empty, &empty, &other, AttentionMask::None, options)
            .is_err()
    );
    let scalar_foreign =
        GpuTensor::from_array(foreign_rt.zeros::<u32>(1).unwrap(), shape(&[])).unwrap();
    assert!(
        p.tensor_attention_low(
            &empty,
            &empty,
            &empty,
            AttentionMask::Keep(&scalar_foreign),
            options
        )
        .is_err()
    );
    p.submit();
    assert_eq!(rt.read_low_bits(&back).unwrap(), [0x5aa5; 8]);
    assert_eq!(rt.read_f32(&fback).unwrap(), [99.; 8]);
}

#[test]
fn depth_boundaries_zero_strides_and_grid_stride_reuse_preserve_attention() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for depth in [255, 256, 257] {
            for broadcast_depth in [false, true] {
                let (queries, keys, channels) = (2, 33, 65);
                let (qb, _) = input(&rt, dtype, queries, depth);
                let q = GpuLowTensor::from_packed(
                    dtype,
                    qb.packed_words().clone(),
                    qb.storage_len(),
                    Layout::new(
                        shape(&[queries, depth]),
                        vec![1, if broadcast_depth { 0 } else { queries }],
                        1,
                    )
                    .unwrap(),
                )
                .unwrap();
                let (kb, k) = input(&rt, dtype, keys, depth);
                let (vb, v) = input(&rt, dtype, keys, channels);
                let qv: Vec<_> = (0..queries * depth)
                    .map(|i| {
                        let logical = if broadcast_depth {
                            i / depth * depth
                        } else {
                            i
                        };
                        (logical % 7) as f32 / 8. - 0.375
                    })
                    .collect();
                let kv: Vec<_> = (0..keys * depth)
                    .map(|i| (i % 11) as f32 / 8. - 0.625)
                    .collect();
                let vv: Vec<_> = (0..keys * channels)
                    .map(|i| (i % 17) as f32 / 8. - 1.)
                    .collect();
                write(&rt, dtype, &qb, queries, depth, &qv);
                write(&rt, dtype, &kb, keys, depth, &kv);
                write(&rt, dtype, &vb, keys, channels, &vv);
                let mut p = rt.program();
                let wide = p
                    .tensor_attention_low_f32(
                        &q,
                        &k,
                        &v,
                        AttentionMask::None,
                        AttentionOptions::default(),
                    )
                    .unwrap();
                let narrow = p
                    .tensor_attention_low(
                        &q,
                        &k,
                        &v,
                        AttentionMask::None,
                        AttentionOptions::default(),
                    )
                    .unwrap();
                p.submit();
                close(
                    &rt.read_f32(&wide).unwrap(),
                    &reference(
                        &qv,
                        &kv,
                        &vv,
                        queries,
                        keys,
                        depth,
                        channels,
                        &[0.; 33],
                        AttentionOptions::default(),
                    ),
                );
                assert_eq!(
                    rt.read_low_bits(&narrow).unwrap(),
                    rt.read_low_bits(&rt.cast_to_low(&wide, dtype).unwrap())
                        .unwrap()
                );
            }
        }
    }
    // The same workgroup processes another logical query after the X-grid cap.
    // Distinct query values expose state that is not refreshed inside the loop.
    let rows = 65_537;
    let qbits: Vec<u16> = (0..rows)
        .map(|i| bits(LowDtype::Bf16, (i % 7) as f32 / 4. - 0.75))
        .collect();
    let q = rt
        .upload_low_bits(LowDtype::Bf16, shape(&[rows, 1]), &qbits)
        .unwrap();
    let k = rt
        .upload_low_bits(LowDtype::Bf16, shape(&[2, 1]), &[0xbf80, 0x3f80])
        .unwrap();
    let v = k.clone();
    let mut p = rt.program();
    let out = p
        .tensor_attention_low_f32(&q, &k, &v, AttentionMask::None, AttentionOptions::default())
        .unwrap();
    p.submit();
    let expected: Vec<_> = (0..rows)
        .map(|i| ((i % 7) as f64 / 4. - 0.75).tanh())
        .collect();
    close(&rt.read_f32(&out).unwrap(), &expected);
}
