#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use tensor_core::{HasShape, Shape, TensorError, TensorStatsBackend};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn backend() -> Option<MlxBackend> {
    match MlxBackend::new_gpu() {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("MLX unavailable: {e}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{e}");
            None
        }
    }
}
fn close(actual: f32, expected: f64) {
    let difference = (f64::from(actual) - expected).abs();
    assert!(
        difference <= 3e-5 * expected.abs() + 1e-30,
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn common_stats_backend_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_stats_backend(&b).unwrap();
}

#[test]
fn softmax_preserves_log_normalizers_with_huge_offsets_and_extremes() {
    let Some(b) = backend() else { return };
    let values = [
        1e30,
        1e30,
        1e30,
        1e30,
        -f32::MAX,
        f32::MAX,
        -f32::MAX,
        f32::MAX,
    ];
    let input = b.upload_f32(shape(&[2, 4]), &values).unwrap();
    let probabilities = b.read_f32(&b.softmax(&input, &[1]).unwrap()).unwrap();
    assert_eq!(probabilities, [0.25, 0.25, 0.25, 0.25, 0., 0.5, 0., 0.5]);
    let logs = b.read_f32(&b.log_softmax(&input, &[1]).unwrap()).unwrap();
    for &value in &logs[..4] {
        close(value, -(4_f64.ln()));
    }
    assert_eq!(logs[4], f32::NEG_INFINITY);
    assert_eq!(logs[6], f32::NEG_INFINITY);
    close(logs[5], -(2_f64.ln()));
    close(logs[7], -(2_f64.ln()));
    let normalizers = b
        .read_f32(&b.logsumexp(&input, &[1], false).unwrap())
        .unwrap();
    assert_eq!(normalizers, [1e30, f32::MAX]);
}

#[test]
fn scaled_moments_and_layer_norm_handle_overflowing_raw_variance() {
    let Some(b) = backend() else { return };
    let input = b
        .upload_f32(
            shape(&[3, 4]),
            &[
                1e38, 1e38, 1e38, 1e38, -1e30, 1e30, -1e30, 1e30, 1048575., 1048577., 1048575.,
                1048577.,
            ],
        )
        .unwrap();
    let moments = b.moments(&input, &[1], false).unwrap();
    assert_eq!(moments.mean.shape(), &shape(&[3]));
    assert_eq!(b.read_f32(&moments.mean).unwrap(), [1e38, 0., 1048576.]);
    assert_eq!(
        b.read_f32(&moments.variance).unwrap(),
        [0., f32::INFINITY, 1.]
    );
    for epsilon in [1e-5, f32::from_bits(1)] {
        let output = b
            .read_f32(&b.layer_norm(&input, &[1], epsilon).unwrap())
            .unwrap();
        assert_eq!(&output[..4], &[0.; 4]);
        for (i, &value) in output[4..8].iter().enumerate() {
            close(value, if i % 2 == 0 { -1. } else { 1. });
        }
        for (i, &value) in output[8..].iter().enumerate() {
            let expected = (if i % 2 == 0 { -1. } else { 1. }) / (1. + f64::from(epsilon)).sqrt();
            close(value, expected);
        }
    }
}

#[test]
fn layer_norm_uses_both_safe_scaled_branches_with_subnormal_epsilon() {
    let Some(b) = backend() else { return };
    for &(scale, epsilon) in &[
        (1e-30f32, f32::from_bits(1)),
        (1e-20, f32::from_bits(1)),
        (1., 1e20),
        (1e30, 1e-20),
        (1., 1.),
    ] {
        let input = b.upload_f32(shape(&[2]), &[-scale, scale]).unwrap();
        let result = b
            .read_f32(&b.layer_norm(&input, &[0], epsilon).unwrap())
            .unwrap();
        let expected = f64::from(scale) / (f64::from(scale).powi(2) + f64::from(epsilon)).sqrt();
        close(result[0], -expected);
        close(result[1], expected);
    }
}

#[test]
fn arbitrary_axis_statistics_match_cpu_groups_for_strided_inputs() {
    let Some(b) = backend() else { return };
    let values: Vec<f32> = (0..24).map(|i| i as f32 * 0.5 - 5.).collect();
    let input = b.upload_f32(shape(&[2, 3, 4]), &values).unwrap();
    let input = b.permute(&input, &[2, 0, 1]).unwrap();
    let moments = b.moments(&input, &[2, 0], true).unwrap();
    assert_eq!(moments.mean.shape(), &shape(&[1, 2, 1]));
    let means = b.read_f32(&moments.mean).unwrap();
    let variances = b.read_f32(&moments.variance).unwrap();
    let probabilities = b.read_f32(&b.softmax(&input, &[2, 0]).unwrap()).unwrap();
    for group in 0..2 {
        let row = &values[group * 12..group * 12 + 12];
        let mean = row.iter().map(|&x| f64::from(x)).sum::<f64>() / 12.;
        let variance = row
            .iter()
            .map(|&x| (f64::from(x) - mean).powi(2))
            .sum::<f64>()
            / 12.;
        close(means[group], mean);
        close(variances[group], variance);
        let max = f64::from(row[11]);
        let sum = row.iter().map(|&x| (f64::from(x) - max).exp()).sum::<f64>();
        for x in 0..4 {
            for y in 0..3 {
                close(
                    probabilities[x * 6 + group * 3 + y],
                    (f64::from(row[y * 4 + x]) - max).exp() / sum,
                );
            }
        }
    }
}

#[test]
fn stats_empty_singleton_owner_dtype_and_epsilon_validation() {
    let Some(b) = backend() else { return };
    let input = b.upload_f32(shape(&[2]), &[3., -0.]).unwrap();
    assert_eq!(
        b.read_f32(&b.softmax(&input, &[]).unwrap()).unwrap(),
        [1.; 2]
    );
    assert_eq!(
        b.read_f32(&b.log_softmax(&input, &[]).unwrap()).unwrap(),
        [0.; 2]
    );
    assert_eq!(
        b.read_f32(&b.layer_norm(&input, &[], 1.).unwrap()).unwrap(),
        [0.; 2]
    );
    let moments = b.moments(&input, &[], false).unwrap();
    assert_eq!(b.read_f32(&moments.variance).unwrap(), [0.; 2]);
    assert_eq!(
        b.read_f32(&moments.mean).unwrap()[1].to_bits(),
        (-0f32).to_bits()
    );
    let empty = b.upload_f32(shape(&[2, 0, 3]), &[]).unwrap();
    assert!(
        b.read_f32(&b.softmax(&empty, &[1]).unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(
        b.read_f32(&b.layer_norm(&empty, &[1], f32::from_bits(1)).unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        b.moments(&empty, &[1], false),
        Err(MlxError::Contract(TensorError::EmptyReduction))
    ));
    assert!(matches!(
        b.logsumexp(&empty, &[1], false),
        Err(MlxError::Contract(TensorError::EmptyReduction))
    ));
    assert!(
        b.moments(&empty, &[0], false)
            .unwrap()
            .mean
            .shape()
            .is_empty()
    );
    for epsilon in [0., -0., -1., f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        assert!(matches!(
            b.layer_norm(&empty, &[1], epsilon),
            Err(MlxError::Contract(TensorError::InvalidEpsilon))
        ));
    }
    assert!(b.softmax(&empty, &[1, 1]).is_err());
    assert!(b.log_softmax(&input, &[1]).is_err());
    let u = b.upload_u32(shape(&[2]), &[1, 2]).unwrap();
    assert!(matches!(b.moments(&u, &[0], false), Err(MlxError::Dtype)));
    assert!(matches!(b.softmax(&u, &[0]), Err(MlxError::Dtype)));
    let other = MlxBackend::new_gpu().unwrap();
    assert!(matches!(
        other.softmax(&input, &[0]),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        other.layer_norm(&input, &[0], 1.),
        Err(MlxError::ForeignContext)
    ));
}
