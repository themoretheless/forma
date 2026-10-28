#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use tensor_core::{
    HasShape, LowDtype, Shape, TensorError, TensorLowBackend, TensorLowStatsBackend,
};

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
    assert!(
        (f64::from(actual) - expected).abs() <= 8e-5 * expected.abs().max(1e-30),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn common_low_statistics_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_low_stats_backend(&b).unwrap();
}

#[test]
fn direct_long_strided_statistics_preserve_partial_rows_and_lazy_inputs() {
    let Some(b) = backend() else { return };
    let count = 131077;
    let values: Vec<_> = (0..count)
        .flat_map(|i| (0..3).map(move |row| ((i % 7) as f32 - 3.) * 0.25 + row as f32))
        .collect();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let source = b.upload_f32(shape(&[count, 3]), &values).unwrap();
        let low = b.cast_to_low(&source, dtype).unwrap();
        let input = b.permute_low(&low, &[1, 0]).unwrap();
        let moments = b.moments_low_f32(&input, &[1], true).unwrap();
        let softmax = b.softmax_low_f32(&input, &[1]).unwrap();
        let norm = b.layer_norm_low_f32(&input, &[1], 1e-5).unwrap();
        drop((input, low, source));
        assert_eq!(moments.mean.shape(), &shape(&[3, 1]));
        let means = b.read_f32(&moments.mean).unwrap();
        let variances = b.read_f32(&moments.variance).unwrap();
        let probabilities = b.read_f32(&softmax).unwrap();
        let normalized = b.read_f32(&norm).unwrap();
        for row in 0..3 {
            let data: Vec<_> = (0..count).map(|i| f64::from(values[i * 3 + row])).collect();
            let mean = data.iter().sum::<f64>() / count as f64;
            let variance = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / count as f64;
            let max = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let divisor = data.iter().map(|x| (x - max).exp()).sum::<f64>();
            // Centered f32 accumulation is assessed relative to the original
            // coordinate scale, not a near-zero mean after cancellation.
            let coordinate_scale = data.iter().map(|x| x.abs()).fold(1.0, f64::max);
            assert!(
                (f64::from(means[row]) - mean).abs()
                    <= 4. * f64::from(f32::EPSILON) * coordinate_scale
            );
            close(variances[row], variance);
            for i in [0, 255, 4095, 4096, count - 1] {
                close(
                    probabilities[row * count + i],
                    (data[i] - max).exp() / divisor,
                );
                // The row-zero near-zero mean still uses an absolute f32
                // accuracy floor in normalized coordinates.
                let expected = (data[i] - mean) / (variance + 1e-5).sqrt();
                assert!((f64::from(normalized[row * count + i]) - expected).abs() < 1e-5);
            }
        }
    }
}

#[test]
fn bounded_group_and_element_grids_visit_all_statistic_rows() {
    let Some(b) = backend() else { return };
    let rows = 65537;
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let positive = if dtype == LowDtype::F16 {
            0x3c00
        } else {
            0x3f80
        };
        let bits: Vec<_> = (0..rows)
            .flat_map(|_| [positive | 0x8000, positive])
            .collect();
        let input = b.upload_low(dtype, shape(&[rows, 2]), &bits).unwrap();
        let result = b.moments_low_f32(&input, &[1], false).unwrap();
        assert_eq!(b.read_f32(&result.mean).unwrap(), vec![0.; rows]);
        assert_eq!(b.read_f32(&result.variance).unwrap(), vec![1.; rows]);
        let normalized = b
            .layer_norm_low_f32(&input, &[1], f32::from_bits(1))
            .unwrap();
        let values = b.read_f32(&normalized).unwrap();
        for row in 0..rows {
            assert_eq!(&values[row * 2..row * 2 + 2], &[-1., 1.]);
        }
        assert_eq!(b.read_low_bits(&input).unwrap(), bits);
    }
}

#[test]
fn validation_precedes_empty_shortcuts_and_foreign_execution() {
    let Some(b) = backend() else { return };
    let input = b.upload_low(LowDtype::Bf16, shape(&[0, 2]), &[]).unwrap();
    assert!(matches!(
        b.softmax_low_f32(&input, &[2]),
        Err(MlxError::Contract(TensorError::AxisOutOfBounds { .. }))
    ));
    for epsilon in [0., -1., f32::INFINITY, f32::NAN] {
        assert!(matches!(
            b.layer_norm_low_f32(&input, &[], epsilon),
            Err(MlxError::Contract(TensorError::InvalidEpsilon))
        ));
    }
    assert!(matches!(
        b.moments_low_f32(&input, &[0], false),
        Err(MlxError::Contract(TensorError::EmptyReduction))
    ));
    let other = MlxBackend::new_gpu().unwrap();
    assert!(matches!(
        other.logsumexp_low_f32(&input, &[], true),
        Err(MlxError::ForeignContext)
    ));
}
