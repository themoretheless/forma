use compute_core::{
    ComputeError, ComputeRuntime, GpuArray, GpuTensor, TensorComputeError,
    gpu_compute::GpuContext,
    tensor_core::{
        AttentionMask, AttentionOptions, AttentionPlan, Layout, Shape, TensorAttentionBackend,
        TensorBackend, TensorIndexBackend,
    },
};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(30);
fn context() -> Option<GpuContext> {
    let result = GpuContext::new();
    assert!(result.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    result
}
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn read(rt: &ComputeRuntime, array: &GpuArray<f32>) -> Vec<f32> {
    rt.read(array).unwrap().wait(TIMEOUT).unwrap()
}
fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            actual.is_finite()
                && (f64::from(actual) - f64::from(expected)).abs()
                    <= 4e-4 * f64::from(expected).abs().max(1.),
            "[{i}] {actual} != {expected}"
        );
    }
}

fn reference(
    q: &[f32],
    k: &[f32],
    v: &[f32],
    dims: (usize, usize, usize, usize),
    bias: &[f32],
    options: AttentionOptions,
) -> Vec<f32> {
    let (queries, keys, depth, channels) = dims;
    let scale = f64::from(options.scale.unwrap_or((depth as f32).sqrt().recip()));
    let mut result = vec![0.; queries * channels];
    for row in 0..queries {
        let scores: Vec<(usize, f64)> = (0..keys)
            .filter(|&key| {
                options
                    .causal
                    .is_none_or(|offset| key as i64 <= row as i64 + i64::from(offset))
                    && bias[row * keys + key] != f32::NEG_INFINITY
            })
            .map(|key| {
                let dot = (0..depth)
                    .map(|d| f64::from(q[row * depth + d]) * f64::from(k[key * depth + d]))
                    .sum::<f64>();
                (key, dot * scale + f64::from(bias[row * keys + key]))
            })
            .collect();
        if scores.is_empty() {
            continue;
        }
        let maximum = scores
            .iter()
            .map(|&(_, s)| s)
            .fold(f64::NEG_INFINITY, f64::max);
        let total = scores
            .iter()
            .map(|&(_, s)| (s - maximum).exp())
            .sum::<f64>();
        for channel in 0..channels {
            result[row * channels + channel] = scores
                .iter()
                .map(|&(key, s)| {
                    (s - maximum).exp() / total * f64::from(v[key * channels + channel])
                })
                .sum::<f64>() as f32;
        }
    }
    result
}

#[test]
fn shader_attention_satisfies_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_attention_backend(&rt).unwrap();
}

#[test]
fn strided_attention_reuses_masks_inputs_and_output_offsets_across_key_and_value_tiles() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for (keys, channels) in [(0, 65), (1, 1), (31, 63), (32, 64), (33, 65), (257, 129)] {
        let (queries, depth) = (5, 7);
        let qraw = rt.zeros::<f32>(queries * depth + 4).unwrap();
        let kraw = rt.zeros::<f32>(keys * depth + 4).unwrap();
        let vraw = rt.zeros::<f32>(keys * channels + 4).unwrap();
        let query = GpuTensor::from_layout(
            qraw.clone(),
            Layout::new(shape(&[queries, depth]), vec![1, queries], 1).unwrap(),
        )
        .unwrap();
        let key = GpuTensor::from_layout(
            kraw.clone(),
            Layout::new(shape(&[keys, depth]), vec![1, keys], 2).unwrap(),
        )
        .unwrap();
        let value = GpuTensor::from_layout(
            vraw.clone(),
            Layout::new(shape(&[keys, channels]), vec![1, keys], 3).unwrap(),
        )
        .unwrap();
        let mraw = rt.zeros::<u32>(keys * 3 + 4).unwrap();
        let keep = GpuTensor::from_layout(
            mraw.clone(),
            Layout::new(shape(&[1, keys]), vec![0, 3], 2).unwrap(),
        )
        .unwrap();
        let bias = rt
            .upload_f32(shape(&[queries, keys]), &vec![0.; queries * keys])
            .unwrap();
        let output_raw = rt
            .upload(&vec![-999.0_f32; queries * channels + 6])
            .unwrap();
        let output = GpuTensor::from_layout(
            output_raw.clone(),
            Layout::new(shape(&[queries, channels]), vec![channels, 1], 3).unwrap(),
        )
        .unwrap();
        let options = AttentionOptions {
            scale: Some(-0.75),
            causal: Some(3),
        };
        let mut p = rt.program();
        p.tensor_attention_into(
            &query,
            &key,
            &value,
            AttentionMask::Keep(&keep),
            options,
            &output,
        )
        .unwrap();
        let additive = p
            .tensor_attention(
                &query,
                &key,
                &value,
                AttentionMask::Additive(&bias),
                options,
            )
            .unwrap();
        let sum = p.tensor_sum(&output, &[0, 1], false).unwrap();
        for iteration in 0..3 {
            let qdata: Vec<f32> = (0..queries * depth)
                .map(|i| ((i * 3 + iteration) % 17) as f32 / 8. - 1.)
                .collect();
            let kdata: Vec<f32> = (0..keys * depth)
                .map(|i| ((i * 7 + iteration) % 13) as f32 / 4. - 1.5)
                .collect();
            let vdata: Vec<f32> = (0..keys * channels)
                .map(|i| ((i * 11 + iteration) % 19) as f32 / 4. - 2.)
                .collect();
            rt.write(&qraw, 1, &qdata).unwrap();
            rt.write(&kraw, 2, &kdata).unwrap();
            rt.write(&vraw, 3, &vdata).unwrap();
            let flags: Vec<u32> = (0..keys)
                .map(|i| {
                    if iteration == 1 || i % 5 == 0 {
                        0
                    } else {
                        u32::MAX
                    }
                })
                .collect();
            let mut mask_storage = vec![0u32; keys * 3];
            for (i, &flag) in flags.iter().enumerate() {
                mask_storage[i * 3] = flag;
            }
            rt.write(&mraw, 2, &mask_storage).unwrap();
            let biases: Vec<f32> = (0..queries * keys)
                .map(|i| {
                    if iteration == 1 || i % 7 == 0 {
                        f32::NEG_INFINITY
                    } else {
                        (i % 3) as f32 / 8.
                    }
                })
                .collect();
            rt.write(bias.values(), 0, &biases).unwrap();
            let q: Vec<f32> = (0..queries * depth)
                .map(|i| qdata[(i % depth) * queries + i / depth])
                .collect();
            let k: Vec<f32> = (0..keys * depth)
                .map(|i| kdata[(i % depth) * keys + i / depth])
                .collect();
            let v: Vec<f32> = (0..keys * channels)
                .map(|i| vdata[(i % channels) * keys + i / channels])
                .collect();
            let keep_bias: Vec<f32> = (0..queries * keys)
                .map(|i| {
                    if flags[i % keys] == 0 {
                        f32::NEG_INFINITY
                    } else {
                        0.
                    }
                })
                .collect();
            let expected = reference(
                &q,
                &k,
                &v,
                (queries, keys, depth, channels),
                &keep_bias,
                options,
            );
            let expected_add = reference(
                &q,
                &k,
                &v,
                (queries, keys, depth, channels),
                &biases,
                options,
            );
            p.submit();
            let actual = read(&rt, &output_raw);
            assert_eq!(&actual[..3], &[-999.; 3]);
            assert_eq!(&actual[queries * channels + 3..], &[-999.; 3]);
            close(&actual[3..queries * channels + 3], &expected);
            close(&read(&rt, additive.values()), &expected_add);
            close(&read(&rt, sum.values()), &[expected.iter().sum()]);
        }
    }
}

#[test]
fn split_keys_merge_masked_parts_with_offsets_and_changed_resident_inputs() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for (keys, channels) in [(511, 65), (512, 65), (513, 129), (4097, 65)] {
        let queries = 3;
        let qdata = [1., -0.5, 2.];
        let query = rt.upload_f32(shape(&[queries, 1]), &qdata).unwrap();
        let key = rt.upload_f32(shape(&[keys, 1]), &vec![0.; keys]).unwrap();
        let value = rt
            .upload_f32(shape(&[keys, channels]), &vec![0.; keys * channels])
            .unwrap();
        let flags = rt.upload_u32(shape(&[keys]), &vec![0; keys]).unwrap();
        let raw = rt
            .upload(&vec![-999.0_f32; queries * channels + 4])
            .unwrap();
        let out = GpuTensor::from_layout(
            raw.clone(),
            Layout::new(shape(&[queries, channels]), vec![channels, 1], 2).unwrap(),
        )
        .unwrap();
        let options = AttentionOptions {
            scale: Some(0.5),
            causal: None,
        };
        let causal_options = AttentionOptions {
            scale: Some(-0.25),
            causal: Some((keys / 2) as i32),
        };
        let mut p = rt.program();
        p.tensor_attention_into(
            &query,
            &key,
            &value,
            AttentionMask::Keep(&flags),
            options,
            &out,
        )
        .unwrap();
        let causal = p
            .tensor_attention(
                &query,
                &key,
                &value,
                AttentionMask::Keep(&flags),
                causal_options,
            )
            .unwrap();
        for iteration in 0..3 {
            let kdata: Vec<f32> = (0..keys)
                .map(|i| ((i * 3 + iteration * 7) % 29) as f32 / 4. - 3.5)
                .collect();
            let vdata: Vec<f32> = (0..keys * channels)
                .map(|i| ((i * 7 + iteration) % 31) as f32 / 8. - 2.)
                .collect();
            let flags_data: Vec<u32> = (0..keys)
                .map(|i| match iteration {
                    0 => u32::from(i >= keys / 2),
                    1 => 0,
                    _ => u32::from(i < keys / 2),
                })
                .collect();
            rt.write(key.values(), 0, &kdata).unwrap();
            rt.write(value.values(), 0, &vdata).unwrap();
            rt.write(flags.values(), 0, &flags_data).unwrap();
            let biases: Vec<f32> = (0..queries * keys)
                .map(|i| {
                    if flags_data[i % keys] == 0 {
                        f32::NEG_INFINITY
                    } else {
                        0.
                    }
                })
                .collect();
            p.submit();
            let actual = read(&rt, &raw);
            assert_eq!(&actual[..2], &[-999.; 2]);
            assert_eq!(&actual[queries * channels + 2..], &[-999.; 2]);
            close(
                &actual[2..queries * channels + 2],
                &reference(
                    &qdata,
                    &kdata,
                    &vdata,
                    (queries, keys, 1, channels),
                    &biases,
                    options,
                ),
            );
            close(
                &read(&rt, causal.values()),
                &reference(
                    &qdata,
                    &kdata,
                    &vdata,
                    (queries, keys, 1, channels),
                    &biases,
                    causal_options,
                ),
            );
        }
    }
    let query = rt.upload_f32(shape(&[1, 1]), &[1.]).unwrap();
    let keys = 513;
    let mut scores = vec![-256.; keys];
    scores[256..].fill(0.);
    let key = rt.upload_f32(shape(&[keys, 1]), &scores).unwrap();
    for current in [f32::MAX, 1., 1e-30, 0.] {
        let mut values = vec![f32::MAX; keys];
        values[256..].fill(current);
        let value = rt.upload_f32(shape(&[keys, 1]), &values).unwrap();
        let output = rt
            .attention(
                &query,
                &key,
                &value,
                AttentionMask::None,
                AttentionOptions::default(),
            )
            .unwrap();
        let actual = rt.read_f32(&output).unwrap()[0];
        assert!(
            actual.is_finite()
                && (f64::from(actual) - f64::from(current)).abs()
                    <= 2e-5 * f64::from(current).abs(),
            "split expired-scale {actual} != {current}"
        );
    }
}

#[test]
fn streaming_attention_handles_long_single_query_and_workgroup_grid_stride() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    // A prime number of keys crosses many online softmax tiles, while Dv=65
    // requires two independent channel tiles. Uniform scores have a closed oracle.
    let keys = 131_077;
    let query = rt.upload_f32(shape(&[1, 1]), &[0.]).unwrap();
    let key = rt.upload_f32(shape(&[keys, 1]), &vec![1.; keys]).unwrap();
    let scalar = rt.upload_f32(shape(&[]), &[3.25]).unwrap();
    let value = scalar.broadcast_to(shape(&[keys, 65])).unwrap();
    let result = rt
        .attention(
            &query,
            &key,
            &value,
            AttentionMask::None,
            AttentionOptions::default(),
        )
        .unwrap();
    close(&rt.read_f32(&result).unwrap(), &[3.25; 65]);
    // 65537 workgroups crosses the portable X-dispatch limit. All query rows
    // should copy the sole value row exactly; this also checks grid-stride reset.
    let queries = 65_537;
    let query = scalar.broadcast_to(shape(&[queries, 1])).unwrap();
    let key = rt.upload_f32(shape(&[1, 1]), &[0.125]).unwrap();
    let value = rt.upload_f32(shape(&[1, 1]), &[-2.5]).unwrap();
    let result = rt
        .attention(
            &query,
            &key,
            &value,
            AttentionMask::None,
            AttentionOptions::default(),
        )
        .unwrap();
    assert_eq!(rt.read_f32(&result).unwrap(), vec![-2.5; queries]);

    // Recording depends on actual operand/output addresses, not score numel.
    // Do not execute the quadratic arithmetic of this metadata-only case.
    let key = scalar.broadcast_to(shape(&[65_537, 1])).unwrap();
    let value = scalar.broadcast_to(shape(&[65_537, 1])).unwrap();
    let plan = AttentionPlan::new(
        query.shape(),
        key.shape(),
        value.shape(),
        None,
        AttentionOptions::default(),
    )
    .unwrap();
    assert!(plan.scores.numel() > u32::MAX as usize);
    let mut p = rt.program();
    let result = p
        .tensor_attention(
            &query,
            &key,
            &value,
            AttentionMask::None,
            AttentionOptions::default(),
        )
        .unwrap();
    assert_eq!(result.shape(), &shape(&[queries, 1]));
}

#[test]
fn scaled_value_accumulator_handles_extreme_finite_values_without_raw_weighted_sums() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let query = rt.upload_f32(shape(&[1, 1]), &[1.]).unwrap();
    let keys = 65;
    let key = rt.upload_f32(shape(&[keys, 1]), &vec![0.; keys]).unwrap();
    let values: Vec<f32> = (0..keys)
        .flat_map(|i| [f32::MAX, -f32::MAX, if i % 2 == 0 { 1e30 } else { -1e30 }])
        .collect();
    let value = rt.upload_f32(shape(&[keys, 3]), &values).unwrap();
    let output = rt
        .attention(
            &query,
            &key,
            &value,
            AttentionMask::None,
            AttentionOptions::default(),
        )
        .unwrap();
    close(
        &rt.read_f32(&output).unwrap(),
        &[f32::MAX, -f32::MAX, 1e30 / keys as f32],
    );
    let key = rt
        .upload_f32(shape(&[2, 1]), &[-f32::MAX, f32::MAX])
        .unwrap();
    let value = rt.upload_f32(shape(&[2, 1]), &[-7., 11.]).unwrap();
    let output = rt
        .attention(
            &query,
            &key,
            &value,
            AttentionMask::None,
            AttentionOptions {
                scale: Some(1.),
                causal: None,
            },
        )
        .unwrap();
    assert_eq!(rt.read_f32(&output).unwrap(), [11.]);
    // A newly dominant key tile discards the old denominator contribution.
    // Its values must also replace an obsolete huge accumulator scale.
    let mut keys = vec![-256.; 32];
    keys.extend([0.; 32]);
    let key = rt.upload_f32(shape(&[64, 1]), &keys).unwrap();
    for current in [1.0_f32, 1e-30, 0.] {
        let mut values = vec![f32::MAX; 32];
        values.extend([current; 32]);
        let value = rt.upload_f32(shape(&[64, 1]), &values).unwrap();
        let output = rt
            .attention(
                &query,
                &key,
                &value,
                AttentionMask::None,
                AttentionOptions {
                    scale: Some(1.),
                    causal: None,
                },
            )
            .unwrap();
        let actual = rt.read_f32(&output).unwrap()[0];
        assert!(actual.is_finite());
        assert!(
            (actual - current).abs() <= 1e-5 * current.abs(),
            "reset {actual} != {current}"
        );
    }
    for tiny in [f32::MIN_POSITIVE, f32::from_bits(1), -f32::from_bits(1)] {
        let value = rt.upload_f32(shape(&[64, 1]), &[tiny; 64]).unwrap();
        let output = rt
            .attention(
                &query,
                &key,
                &value,
                AttentionMask::None,
                AttentionOptions::default(),
            )
            .unwrap();
        let actual = rt.read_f32(&output).unwrap()[0];
        assert!(
            actual.is_finite() && (actual - tiny).abs() <= f32::MIN_POSITIVE,
            "subnormal {actual} != {tiny}"
        );
    }
}

#[test]
fn attention_checks_owners_masks_shapes_and_aliases_without_partial_recording() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let other = ComputeRuntime::new(&ctx).unwrap();
    let q = rt.upload_f32(shape(&[2, 2]), &[1., 0., 0., 1.]).unwrap();
    let k = q.clone();
    let v = rt.upload_f32(shape(&[2, 2]), &[2., 3., 4., 5.]).unwrap();
    let foreign = other.upload_f32(shape(&[2, 2]), &[0.; 4]).unwrap();
    let foreign_mask = other.upload_u32(shape(&[]), &[1]).unwrap();
    let output = rt.upload_f32(shape(&[2, 2]), &[-999.; 4]).unwrap();
    let wrong_mask = rt.upload_u32(shape(&[3, 2]), &[1; 6]).unwrap();
    let options = AttentionOptions::default();
    let mut p = rt.program();
    assert!(matches!(
        p.tensor_attention_into(&q, &k, &v, AttentionMask::None, options, &q),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(
        p.tensor_attention_into(
            &q,
            &k,
            &v,
            AttentionMask::Additive(&output),
            options,
            &output
        )
        .is_err()
    );
    assert!(
        p.tensor_attention(&foreign, &k, &v, AttentionMask::None, options)
            .is_err()
    );
    assert!(
        p.tensor_attention(&q, &k, &v, AttentionMask::Keep(&foreign_mask), options)
            .is_err()
    );
    assert!(
        p.tensor_attention(&q, &k, &v, AttentionMask::Keep(&wrong_mask), options)
            .is_err()
    );
    assert!(
        p.tensor_attention_into(
            &q,
            &k,
            &v,
            AttentionMask::None,
            options,
            &output.permute(&[1, 0]).unwrap()
        )
        .is_err()
    );
    for scale in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            p.tensor_attention(
                &q,
                &k,
                &v,
                AttentionMask::None,
                AttentionOptions {
                    scale: Some(scale),
                    causal: None
                }
            )
            .is_err()
        );
    }
    p.submit();
    assert_eq!(rt.read_f32(&output).unwrap(), [-999.; 4]);
    p.tensor_attention_into(&q, &k, &v, AttentionMask::None, options, &output)
        .unwrap();
    p.submit();
    close(
        &rt.read_f32(&output).unwrap(),
        &reference(
            &[1., 0., 0., 1.],
            &[1., 0., 0., 1.],
            &[2., 3., 4., 5.],
            (2, 2, 2, 2),
            &[0.; 4],
            options,
        ),
    );
}
