use compute_core::{
    ComputeError, ComputeRuntime, GpuArray, GpuTensor, TensorComputeError,
    gpu_compute::GpuContext,
    tensor_core::{Layout, Moments, Shape, TensorBackend, TensorError, TensorStatsBackend},
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
fn close(actual: f32, expected: f64, tolerance: f64) {
    assert!(
        actual.is_finite()
            && (f64::from(actual) - expected).abs() <= tolerance * expected.abs().max(1e-25),
        "{actual} != {expected}"
    );
}
fn read(rt: &ComputeRuntime, array: &GpuArray<f32>) -> Vec<f32> {
    rt.read(array).unwrap().wait(TIMEOUT).unwrap()
}

#[test]
fn shader_statistics_satisfy_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_stats_backend(&rt).unwrap();
}

#[test]
fn short_single_groups_and_grid_stride_rows_cross_the_specialization_boundaries() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for n in [2, 33, 255, 256, 257] {
        let values: Vec<f32> = (0..n).map(|i| (i % 17) as f32 / 4. - 2.).collect();
        let input = rt.upload_f32(shape(&[n]), &values).unwrap();
        let mut p = rt.program();
        let soft = p.tensor_softmax(&input, &[0]).unwrap();
        let logs = p.tensor_log_softmax(&input, &[0]).unwrap();
        let lse = p.tensor_logsumexp(&input, &[0], true).unwrap();
        let moments = p.tensor_moments(&input, &[0], true).unwrap();
        let norm = p.tensor_layer_norm(&input, &[0], 0.125).unwrap();
        p.submit();
        let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max) as f64;
        let exps: Vec<f64> = values
            .iter()
            .map(|&x| (f64::from(x) - maximum).exp())
            .collect();
        let exp_sum = exps.iter().sum::<f64>();
        let mean = values.iter().map(|&x| f64::from(x)).sum::<f64>() / n as f64;
        let variance = values
            .iter()
            .map(|&x| (f64::from(x) - mean).powi(2))
            .sum::<f64>()
            / n as f64;
        close(read(&rt, lse.values())[0], maximum + exp_sum.ln(), 4e-5);
        close(read(&rt, moments.mean.values())[0], mean, 4e-5);
        close(read(&rt, moments.variance.values())[0], variance, 3e-4);
        let soft = read(&rt, soft.values());
        let logs = read(&rt, logs.values());
        let norm = read(&rt, norm.values());
        for i in 0..n {
            close(soft[i], exps[i] / exp_sum, 4e-5);
            close(logs[i], f64::from(values[i]) - maximum - exp_sum.ln(), 5e-5);
            assert!(
                (f64::from(norm[i]) - (f64::from(values[i]) - mean) / (variance + 0.125).sqrt())
                    .abs()
                    <= 2e-5
            );
        }
    }
    // More rows than a single portable dispatch dimension can represent.
    // Distinct means and alternating spreads also detect accidental row reuse.
    let rows = 65_537;
    let raw = rt.zeros::<f32>(rows * 2).unwrap();
    let input = GpuTensor::from_array(raw.clone(), shape(&[rows, 2])).unwrap();
    let mut p = rt.program();
    let moments = p.tensor_moments(&input, &[1], false).unwrap();
    let soft = p.tensor_softmax(&input, &[1]).unwrap();
    let logs = p.tensor_log_softmax(&input, &[1]).unwrap();
    let lse = p.tensor_logsumexp(&input, &[1], false).unwrap();
    let norm = p.tensor_layer_norm(&input, &[1], 0.125).unwrap();
    for iteration in 0..2 {
        let values: Vec<f32> = (0..rows)
            .flat_map(|r| {
                let center = (r % 37) as f32;
                let spread = ((r + iteration) % 3) as f32;
                [center - spread, center + spread]
            })
            .collect();
        rt.write(&raw, 0, &values).unwrap();
        p.submit();
        let means = read(&rt, moments.mean.values());
        let variances = read(&rt, moments.variance.values());
        let soft = read(&rt, soft.values());
        let logs = read(&rt, logs.values());
        let lse = read(&rt, lse.values());
        let norm = read(&rt, norm.values());
        for row in 0..rows {
            let center = (row % 37) as f64;
            let spread = ((row + iteration) % 3) as f64;
            close(means[row], center, 2e-6);
            close(variances[row], spread * spread, 2e-6);
            let sum = 1. + (-2. * spread).exp();
            close(lse[row], center + spread + sum.ln(), 2e-6);
            for col in 0..2 {
                let shifted = if col == 0 { -2. * spread } else { 0. };
                close(soft[2 * row + col], shifted.exp() / sum, 2e-6);
                close(logs[2 * row + col], shifted - sum.ln(), 2e-6);
                let expected =
                    (f64::from(values[2 * row + col]) - center) / (spread * spread + 0.125).sqrt();
                assert!((f64::from(norm[2 * row + col]) - expected).abs() < 2e-6);
            }
        }
    }
}

#[test]
fn strided_hierarchies_preserve_sentinels_and_reuse_resident_normalization_chains() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for n in [1, 255, 256, 257, 4096, 4097, 131077] {
        let raw = rt.zeros::<f32>(2 * n + 4).unwrap();
        let input = GpuTensor::from_layout(
            raw.clone(),
            Layout::new(shape(&[2, n]), vec![1, 2], 1).unwrap(),
        )
        .unwrap();
        let mean_raw = rt.upload(&[-999.0_f32; 6]).unwrap();
        let variance_raw = rt.upload(&[-888.0_f32; 7]).unwrap();
        let moments = Moments {
            mean: GpuTensor::from_layout(
                mean_raw.clone(),
                Layout::new(shape(&[2, 1]), vec![1, 1], 2).unwrap(),
            )
            .unwrap(),
            variance: GpuTensor::from_layout(
                variance_raw.clone(),
                Layout::new(shape(&[2, 1]), vec![1, 1], 3).unwrap(),
            )
            .unwrap(),
        };
        let norm_raw = rt.upload(&vec![-777.0_f32; 2 * n + 6]).unwrap();
        let normalized = GpuTensor::from_layout(
            norm_raw.clone(),
            Layout::new(shape(&[2, n]), vec![n, 1], 3).unwrap(),
        )
        .unwrap();
        let soft_raw = rt.upload(&vec![-666.0_f32; 2 * n + 4]).unwrap();
        let soft = GpuTensor::from_layout(
            soft_raw.clone(),
            Layout::new(shape(&[2, n]), vec![n, 1], 1).unwrap(),
        )
        .unwrap();
        let epsilon = f32::from_bits(1);
        let mut p = rt.program();
        p.tensor_moments_into(&input, &[1], true, &moments).unwrap();
        p.tensor_layer_norm_into(&input, &[1], epsilon, &normalized)
            .unwrap();
        p.tensor_softmax_into(&normalized, &[1], &soft).unwrap();
        let sums = p.tensor_sum(&soft, &[1], false).unwrap();
        let logs = p.tensor_log_softmax(&input, &[1]).unwrap();
        let lse = p.tensor_logsumexp(&input, &[1], false).unwrap();
        for iteration in 0..3 {
            let values: Vec<f32> = (0..2 * n)
                .map(|i| match iteration {
                    0 => 1e8 + (((i * 7) % 13) as f32 - 6.) * 8.,
                    1 => ((i * 11) % 19) as f32 / 4. - 2.,
                    _ => 1e30,
                })
                .collect();
            rt.write(&raw, 1, &values).unwrap();
            p.submit();
            let mean = read(&rt, &mean_raw);
            let variance = read(&rt, &variance_raw);
            let norm = read(&rt, &norm_raw);
            let soft = read(&rt, &soft_raw);
            let sums = read(&rt, sums.values());
            let logs = read(&rt, logs.values());
            let lse = read(&rt, lse.values());
            assert_eq!(&mean[..2], &[-999.; 2]);
            assert_eq!(&mean[4..], &[-999.; 2]);
            assert_eq!(&variance[..3], &[-888.; 3]);
            assert_eq!(&variance[5..], &[-888.; 2]);
            assert_eq!(&norm[..3], &[-777.; 3]);
            assert_eq!(&norm[2 * n + 3..], &[-777.; 3]);
            assert_eq!(soft[0], -666.);
            assert_eq!(&soft[2 * n + 1..], &[-666.; 3]);
            for row in 0..2 {
                let row_values: Vec<f64> = (0..n).map(|c| f64::from(values[2 * c + row])).collect();
                let anchor = row_values[0];
                let delta_mean = row_values.iter().map(|x| x - anchor).sum::<f64>() / n as f64;
                let expected_var = row_values
                    .iter()
                    .map(|x| (x - anchor - delta_mean).powi(2))
                    .sum::<f64>()
                    / n as f64;
                let maximum = row_values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let log_sum = row_values
                    .iter()
                    .map(|x| (x - maximum).exp())
                    .sum::<f64>()
                    .ln();
                close(mean[row + 2], anchor + delta_mean, 3e-5);
                close(variance[row + 3], expected_var, 3e-4);
                close(sums[row], 1., 3e-5);
                close(lse[row], maximum + log_sum, 4e-5);
                for (col, &x) in row_values.iter().enumerate() {
                    let expected_norm =
                        (x - anchor - delta_mean) / (expected_var + f64::from(epsilon)).sqrt();
                    assert!(
                        (f64::from(norm[3 + row * n + col]) - expected_norm).abs() <= 3e-4,
                        "norm n={n}, iteration={iteration}, row={row}, col={col}"
                    );
                    let expected_log = x - maximum - log_sum;
                    assert!(
                        (f64::from(logs[row * n + col]) - expected_log).abs()
                            <= 5e-5 * expected_log.abs().max(1.)
                    );
                }
            }
        }
    }
}

#[test]
fn overflow_classification_and_tiny_epsilon_do_not_corrupt_normalized_outputs() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let root = f32::MAX.sqrt().to_bits();
    let mut magnitudes: Vec<f32> = (root - 3..=root + 3).map(f32::from_bits).collect();
    magnitudes.extend([1e30, f32::MAX * 0.5, f32::MAX, 1e-20]);
    for a in magnitudes {
        let input = rt.upload_f32(shape(&[2]), &[-a, a]).unwrap();
        let moments = rt.moments(&input, &[0], false).unwrap();
        let actual_var = rt.read_f32(&moments.variance).unwrap()[0];
        let expected_var = (f64::from(a) * f64::from(a)) as f32;
        assert_eq!(
            actual_var.is_infinite(),
            expected_var.is_infinite(),
            "a={a}"
        );
        if expected_var.is_normal() {
            close(actual_var, f64::from(expected_var), 2e-6);
        }
        assert_eq!(rt.read_f32(&moments.mean).unwrap(), [0.]);
        for epsilon in [f32::from_bits(1), 1e-40, 1e-5, f32::MAX] {
            let normalized = rt.layer_norm(&input, &[0], epsilon).unwrap();
            let normalized = rt.read_f32(&normalized).unwrap();
            let expected = f64::from(a) / (f64::from(a).powi(2) + f64::from(epsilon)).sqrt();
            for (actual, expected) in normalized.into_iter().zip([-expected, expected]) {
                assert!(actual.is_finite());
                if expected != 0.0 && expected.abs() < f64::from(f32::MIN_POSITIVE) {
                    println!(
                        "subnormal normalization: a={a:?}, epsilon={epsilon:?}, actual={actual:?} (0x{:08x}), f64_reference={expected:?}",
                        actual.to_bits()
                    );
                }
                // WGSL permits flushing subnormal arithmetic. Keep a relative
                // check for ordinary outputs and allow only that underflow floor.
                assert!(
                    (f64::from(actual) - expected).abs()
                        <= (2e-5 * expected.abs()).max(f64::from(f32::MIN_POSITIVE)),
                    "a={a}, epsilon={epsilon}: {actual} != {expected}"
                );
            }
        }
        let log = rt.log_softmax(&input, &[0]).unwrap();
        let log = rt.read_f32(&log).unwrap();
        let difference = (-2. * f64::from(a)) as f32;
        assert_eq!(log[0].is_infinite(), difference.is_infinite());
        if difference.is_infinite() {
            assert_eq!(log[0].to_bits(), f32::NEG_INFINITY.to_bits());
        }
    }
}

#[test]
fn validation_rejects_aliases_foreign_views_and_invalid_metadata_before_recording() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let foreign = ComputeRuntime::new(&ctx).unwrap();
    let input = rt
        .upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])
        .unwrap();
    let outside = foreign.upload_f32(shape(&[2, 3]), &[0.; 6]).unwrap();
    let output = rt.upload_f32(shape(&[2, 3]), &[-99.; 6]).unwrap();
    let mean = rt.upload_f32(shape(&[2]), &[-88.; 2]).unwrap();
    let wrong = rt.upload_f32(shape(&[1]), &[-77.]).unwrap();
    let mut p = rt.program();
    assert!(matches!(
        p.tensor_softmax_into(&input, &[1], &input),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(p.tensor_log_softmax_into(&input, &[1], &outside).is_err());
    assert!(p.tensor_logsumexp(&outside, &[1], false).is_err());
    assert!(p.tensor_moments(&outside, &[1], false).is_err());
    assert!(p.tensor_softmax(&input, &[2]).is_err());
    assert!(p.tensor_log_softmax(&input, &[0, 0]).is_err());
    assert!(
        p.tensor_moments_into(
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
        p.tensor_moments_into(
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
    let strided = output.permute(&[1, 0]).unwrap();
    assert!(
        p.tensor_softmax_into(&input.permute(&[1, 0]).unwrap(), &[0], &strided)
            .is_err()
    );
    for epsilon in [0., -1., f32::INFINITY, f32::NAN] {
        assert!(matches!(
            p.tensor_layer_norm_into(&input, &[1], epsilon, &output),
            Err(TensorComputeError::Tensor(TensorError::InvalidEpsilon))
        ));
    }
    p.tensor_softmax_into(&input, &[1], &output).unwrap();
    p.submit();
    assert_eq!(rt.read_f32(&mean).unwrap(), [-88.; 2]);
    let values = rt.read_f32(&output).unwrap();
    close(values[..3].iter().copied().sum(), 1., 1e-6);
    close(values[3..].iter().copied().sum(), 1., 1e-6);
}
