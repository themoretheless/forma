use compute_core::{
    ComputeError, ComputeRuntime, GpuLowTensor, GpuTensor, TensorComputeError,
    gpu_compute::{GpuBuffer, GpuContext},
    tensor_core::{Layout, LowDtype, Moments, Shape, TensorBackend, TensorLowBackend},
};
fn context() -> Option<GpuContext> {
    let c = GpuContext::new();
    assert!(c.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    c
}
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn bf(x: f32) -> u16 {
    ((x.to_bits() + 0x7fff + ((x.to_bits() >> 16) & 1)) >> 16) as u16
}
fn low_output(rt: &ComputeRuntime, s: Shape) -> (GpuLowTensor, GpuLowTensor) {
    let n = s.numel();
    let back = rt
        .upload_low_bits(LowDtype::Bf16, shape(&[n + 4]), &vec![0x5aa5; n + 4])
        .unwrap();
    let out = back.narrow(0, 1, n).unwrap().reshape(s).unwrap();
    (back, out)
}
fn float_output(rt: &ComputeRuntime, s: Shape) -> (GpuTensor, GpuTensor) {
    let n = s.numel();
    let back = rt.upload_f32(shape(&[n + 4]), &vec![77.; n + 4]).unwrap();
    let out = back.narrow(0, 1, n).unwrap().reshape(s).unwrap();
    (back, out)
}
fn close(a: f32, b: f64) {
    let expected = b as f32;
    if expected.is_infinite() {
        assert_eq!(a.to_bits(), expected.to_bits());
        return;
    }
    assert!(
        a.is_finite()
            && (f64::from(a) - b).abs() <= (3e-4 * b.abs()).max(f64::from(f32::MIN_POSITIVE)),
        "{a:?} != {b:?}"
    );
}
#[test]
fn packed_statistics_satisfy_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_low_stats_backend(&rt).unwrap();
}
#[test]
fn all_recorded_outputs_replay_strided_inputs_and_preserve_odd_neighbor_words() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    const N: usize = 257;
    let backing = rt.zeros_low(LowDtype::Bf16, shape(&[3 * N + 2])).unwrap();
    let input = backing
        .narrow(0, 1, 3 * N)
        .unwrap()
        .reshape(shape(&[N, 3]))
        .unwrap()
        .permute(&[1, 0])
        .unwrap();
    let shapes = [
        shape(&[3, N]),
        shape(&[3, N]),
        shape(&[3, N]),
        shape(&[3]),
        shape(&[3]),
        shape(&[3]),
    ];
    let floats: Vec<_> = shapes
        .iter()
        .map(|s| float_output(&rt, s.clone()))
        .collect();
    let lows: Vec<_> = shapes.iter().map(|s| low_output(&rt, s.clone())).collect();
    let epsilon = f32::from_bits(1);
    let mut p = rt.program();
    p.tensor_softmax_low_f32_into(&input, &[1], &floats[0].1)
        .unwrap();
    p.tensor_log_softmax_low_f32_into(&input, &[1], &floats[1].1)
        .unwrap();
    p.tensor_layer_norm_low_f32_into(&input, &[1], epsilon, &floats[2].1)
        .unwrap();
    p.tensor_logsumexp_low_f32_into(&input, &[1], false, &floats[3].1)
        .unwrap();
    p.tensor_moments_low_f32_into(
        &input,
        &[1],
        false,
        &Moments {
            mean: floats[4].1.clone(),
            variance: floats[5].1.clone(),
        },
    )
    .unwrap();
    p.tensor_softmax_low_into(&input, &[1], &lows[0].1).unwrap();
    p.tensor_log_softmax_low_into(&input, &[1], &lows[1].1)
        .unwrap();
    p.tensor_layer_norm_low_into(&input, &[1], epsilon, &lows[2].1)
        .unwrap();
    p.tensor_logsumexp_low_into(&input, &[1], false, &lows[3].1)
        .unwrap();
    p.tensor_moments_low_into(
        &input,
        &[1],
        false,
        &Moments {
            mean: lows[4].1.clone(),
            variance: lows[5].1.clone(),
        },
    )
    .unwrap();
    for amplitude in [1e-40f32, 1e30, 0.5] {
        let mut bits = vec![0u16; 3 * N + 2];
        for i in 0..N {
            for row in 0..3 {
                bits[1 + i * 3 + row] = bf(amplitude * ((i % 7) as f32 - 3.) * row as f32);
            }
        }
        rt.write_low_storage_bits(&backing, &bits).unwrap();
        p.submit();
        let values: Vec<_> = floats
            .iter()
            .map(|(b, _)| {
                let v = rt.read_f32(b).unwrap();
                assert_eq!(v[0], 77.);
                assert_eq!(&v[v.len() - 3..], &[77.; 3]);
                v[1..v.len() - 3].to_vec()
            })
            .collect();
        for row in 0..3 {
            let x: Vec<f64> = (0..N)
                .map(|i| f64::from(f32::from_bits(u32::from(bits[1 + i * 3 + row]) << 16)))
                .collect();
            let max = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let sum: f64 = x.iter().map(|v| (v - max).exp()).sum();
            let mean = x.iter().sum::<f64>() / N as f64;
            let variance = x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / N as f64;
            close(values[3][row], max + sum.ln());
            close(values[4][row], mean);
            close(values[5][row], variance);
            for i in 0..N {
                close(values[0][row * N + i], (x[i] - max).exp() / sum);
                close(values[1][row * N + i], x[i] - max - sum.ln());
                close(
                    values[2][row * N + i],
                    (x[i] - mean) / (variance + f64::from(epsilon)).sqrt(),
                );
            }
        }
        for (i, (back, _)) in lows.iter().enumerate() {
            let raw = rt.read_low_bits(back).unwrap();
            assert_eq!(raw[0], 0x5aa5);
            assert_eq!(&raw[raw.len() - 3..], &[0x5aa5; 3]);
            let expected = rt.cast_to_low(&floats[i].1, LowDtype::Bf16).unwrap();
            assert_eq!(&raw[1..raw.len() - 3], rt.read_low_bits(&expected).unwrap());
        }
    }
}
#[test]
fn tiny_bf16_rows_cross_workgroup_grid_limit_without_losing_normal_results() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let rows = 65_537;
    let bits: Vec<u16> = (0..rows)
        .flat_map(|r| {
            let b = (r % 127 + 1) as u16;
            [b | 0x8000, b]
        })
        .collect();
    let input = rt
        .upload_low_bits(LowDtype::Bf16, shape(&[rows, 2]), &bits)
        .unwrap();
    let mut p = rt.program();
    let norm = p
        .tensor_layer_norm_low_f32(&input, &[1], f32::from_bits(1))
        .unwrap();
    let moments = p.tensor_moments_low_f32(&input, &[1], false).unwrap();
    p.submit();
    let actual = rt.read_f32(&norm).unwrap();
    assert_eq!(rt.read_f32(&moments.mean).unwrap(), vec![0.; rows]);
    assert_eq!(rt.read_f32(&moments.variance).unwrap(), vec![0.; rows]);
    for (i, &a) in actual.iter().enumerate() {
        let x = f64::from(f32::from_bits(u32::from(bits[i]) << 16));
        close(a, x / (x * x + f64::from(f32::from_bits(1))).sqrt());
    }
}
#[test]
fn packed_stats_validation_is_transactional_across_types_and_moments_outputs() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let foreign_rt = ComputeRuntime::new(&ctx).unwrap();
    let input = rt.zeros_low(LowDtype::Bf16, shape(&[2, 2])).unwrap();
    let foreign = foreign_rt
        .zeros_low(LowDtype::Bf16, shape(&[2, 2]))
        .unwrap();
    let (back, output) = low_output(&rt, shape(&[2, 2]));
    let (mean_back, mean) = low_output(&rt, shape(&[2]));
    let wrong = rt.zeros_low(LowDtype::F16, shape(&[2])).unwrap();
    let (float_back, float) = float_output(&rt, shape(&[2, 2]));
    let (float_mean_back, float_mean) = float_output(&rt, shape(&[2]));
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
    let mut p = rt.program();
    assert!(matches!(
        p.tensor_softmax_low_f32_into(&low_alias, &[1], &float_alias),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(p.tensor_softmax_low_into(&input, &[1], &input).is_err());
    assert!(p.tensor_softmax_low_into(&foreign, &[1], &output).is_err());
    assert!(
        p.tensor_log_softmax_low_into(&input, &[1], &output.permute(&[1, 0]).unwrap())
            .is_err()
    );
    assert!(p.tensor_softmax_low_f32_into(&input, &[2], &float).is_err());
    assert!(
        p.tensor_logsumexp_low_into(&input, &[1, 1], false, &mean)
            .is_err()
    );
    assert!(
        p.tensor_moments_low_into(
            &input,
            &[1],
            false,
            &Moments {
                mean: mean.clone(),
                variance: wrong
            }
        )
        .is_err()
    );
    assert!(
        p.tensor_moments_low_into(
            &input,
            &[1],
            false,
            &Moments {
                mean: mean.clone(),
                variance: mean.clone()
            }
        )
        .is_err()
    );
    assert!(
        p.tensor_moments_low_f32_into(
            &input,
            &[1],
            false,
            &Moments {
                mean: float_mean.clone(),
                variance: float_mean.clone()
            }
        )
        .is_err()
    );
    for e in [0., -1., f32::INFINITY, f32::NAN] {
        assert!(
            p.tensor_layer_norm_low_into(&input, &[1], e, &output)
                .is_err()
        );
        assert!(
            p.tensor_layer_norm_low_f32_into(&input, &[1], e, &float)
                .is_err()
        );
    }
    p.submit();
    assert_eq!(rt.read_low_bits(&back).unwrap(), [0x5aa5; 8]);
    assert_eq!(rt.read_low_bits(&mean_back).unwrap(), [0x5aa5; 6]);
    assert_eq!(rt.read_f32(&float_back).unwrap(), [77.; 8]);
    assert_eq!(rt.read_f32(&float_mean_back).unwrap(), [77.; 6]);
}
