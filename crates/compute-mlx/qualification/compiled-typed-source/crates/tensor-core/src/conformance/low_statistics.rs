use super::{
    low_ops::round,
    low_precision::decode,
    shape,
    statistics::{close, reference},
};
use crate::{HasLowDtype, HasShape, LowDtype, TensorLowStatsBackend};

fn values(dtype: LowDtype, raw: &[u16]) -> Vec<f32> {
    raw.iter().map(|&x| decode(dtype, x)).collect()
}

fn encodings(dtype: LowDtype, input: &[f32]) -> Vec<u16> {
    input.iter().map(|&x| round(dtype, x)).collect()
}

fn rounded<B: TensorLowStatsBackend>(
    b: &B,
    output: &B::LowTensor,
    wide: &B::Tensor,
    label: &str,
    dtype: LowDtype,
) -> Result<(), B::Error> {
    assert_eq!(output.low_dtype(), dtype);
    assert_eq!(output.shape(), wide.shape());
    let expected = encodings(dtype, &b.read_f32(wide)?);
    let actual = b.read_low_bits(output)?;
    assert_eq!(actual, expected, "{dtype:?} {label}: final rounding");
    Ok(())
}

fn check_input<B: TensorLowStatsBackend>(
    b: &B,
    input: &B::LowTensor,
    raw: &[u16],
    axes: &[usize],
    epsilon: f32,
) -> Result<(), B::Error> {
    let dtype = input.low_dtype();
    let expected = reference(input.shape(), &values(dtype, raw), axes, epsilon);
    let softmax = b.softmax_low_f32(input, axes)?;
    let log_softmax = b.log_softmax_low_f32(input, axes)?;
    let norm = b.layer_norm_low_f32(input, axes, epsilon)?;
    assert_eq!(softmax.shape(), input.shape());
    assert_eq!(log_softmax.shape(), input.shape());
    assert_eq!(norm.shape(), input.shape());
    close("softmax", &b.read_f32(&softmax)?, &expected.softmax, 4e-5);
    close(
        "log_softmax",
        &b.read_f32(&log_softmax)?,
        &expected.log_softmax,
        5e-5,
    );
    close("layer_norm", &b.read_f32(&norm)?, &expected.norm, 2e-4);
    rounded(b, &b.softmax_low(input, axes)?, &softmax, "softmax", dtype)?;
    rounded(
        b,
        &b.log_softmax_low(input, axes)?,
        &log_softmax,
        "log_softmax",
        dtype,
    )?;
    rounded(
        b,
        &b.layer_norm_low(input, axes, epsilon)?,
        &norm,
        "layer_norm",
        dtype,
    )?;
    for keep in [false, true] {
        let expected_shape = input.shape().reduce(axes, keep).unwrap();
        let lse = b.logsumexp_low_f32(input, axes, keep)?;
        let moments = b.moments_low_f32(input, axes, keep)?;
        assert_eq!(lse.shape(), &expected_shape);
        assert_eq!(moments.mean.shape(), &expected_shape);
        assert_eq!(moments.variance.shape(), &expected_shape);
        close("logsumexp", &b.read_f32(&lse)?, &expected.logsumexp, 5e-5);
        close("mean", &b.read_f32(&moments.mean)?, &expected.mean, 2e-5);
        close(
            "population variance",
            &b.read_f32(&moments.variance)?,
            &expected.variance,
            2e-4,
        );
        rounded(
            b,
            &b.logsumexp_low(input, axes, keep)?,
            &lse,
            "logsumexp",
            dtype,
        )?;
        let low_moments = b.moments_low(input, axes, keep)?;
        rounded(b, &low_moments.mean, &moments.mean, "mean", dtype)?;
        rounded(
            b,
            &low_moments.variance,
            &moments.variance,
            "variance",
            dtype,
        )?;
    }
    assert_eq!(b.read_low_bits(input)?, raw);
    Ok(())
}

fn check_singletons<B: TensorLowStatsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let last = if dtype == LowDtype::F16 {
        0x7bff
    } else {
        0x7f7f
    };
    let raw: Vec<u16> = (0..=last).chain((0..=last).map(|x| x | 0x8000)).collect();
    let tensor = b.upload_low(dtype, shape(&[2, raw.len() / 2, 1]), &raw)?;
    let tensor = b.permute_low(&tensor, &[1, 2, 0])?;
    let logical: Vec<u16> = (0..=last).flat_map(|x| [x, x | 0x8000]).collect();
    for axes in [vec![], vec![1]] {
        check_input(b, &tensor, &logical, &axes, f32::from_bits(1))?;
        for keep in [false, true] {
            let lse = b.logsumexp_low_f32(&tensor, &axes, keep)?;
            let mean = b.moments_low_f32(&tensor, &axes, keep)?.mean;
            for actual in [b.read_f32(&lse)?, b.read_f32(&mean)?] {
                for (&a, &raw) in actual.iter().zip(&logical) {
                    assert_eq!(
                        a.to_bits(),
                        decode(dtype, raw).to_bits(),
                        "{dtype:?} singleton {raw:#06x}"
                    );
                }
            }
            assert_eq!(
                b.read_low_bits(&b.logsumexp_low(&tensor, &axes, keep)?)?,
                logical
            );
            assert_eq!(
                b.read_low_bits(&b.moments_low(&tensor, &axes, keep)?.mean)?,
                logical
            );
        }
    }
    for raw in [0u16, 0x8000, 1, 0x8001, last, last | 0x8000] {
        let scalar = b.upload_low(dtype, shape(&[]), &[raw])?;
        check_input(b, &scalar, &[raw], &[], 1e-5)?;
    }
    Ok(())
}

fn check_layouts<B: TensorLowStatsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let raw = encodings(
        dtype,
        &(0..30)
            .map(|i| (i % 13) as f32 / 4. - 1.5)
            .collect::<Vec<_>>(),
    );
    let input = b.upload_low(dtype, shape(&[2, 3, 5]), &raw)?;
    for axes in [vec![], vec![0], vec![1], vec![2], vec![2, 0], vec![0, 1, 2]] {
        check_input(b, &input, &raw, &axes, 1e-5)?;
    }
    let transposed = b.permute_low(&input, &[2, 0, 1])?;
    let logical: Vec<u16> = (0..5)
        .flat_map(|x| (0..2).flat_map(move |y| (0..3).map(move |z| y * 15 + z * 5 + x)))
        .map(|i| raw[i])
        .collect();
    check_input(b, &transposed, &logical, &[2, 0], 0.125)?;
    let pair = encodings(dtype, &[2., -3.]);
    let source = b.upload_low(dtype, shape(&[2, 1]), &pair)?;
    let broadcast = b.broadcast_low(&source, shape(&[4, 2, 9]))?;
    let logical: Vec<u16> = (0..72).map(|i| pair[i / 9 % 2]).collect();
    check_input(b, &broadcast, &logical, &[0, 2], 1e-5)?;
    check_input(b, &broadcast, &logical, &[1], 1e-5)?;
    Ok(())
}

fn check_extremes<B: TensorLowStatsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let last = if dtype == LowDtype::F16 {
        0x7bff
    } else {
        0x7f7f
    };
    for raw in [
        vec![last; 257],
        vec![last | 0x8000; 257],
        vec![last, last - 1, last - 2, last - 4, last - 8],
        vec![last | 0x8000, last, 0],
        vec![
            last | 0x8000,
            last,
            0,
            (last - 0x100) | 0x8000,
            last - 0x100,
        ],
        encodings(dtype, &[10000., 10001., 9998., -10000.]),
    ] {
        let input = b.upload_low(dtype, shape(&[raw.len()]), &raw)?;
        check_input(b, &input, &raw, &[0], 1e-5)?;
    }
    // Small normal inputs need centered units even when their squared
    // variance underflows; epsilon must remain meaningful down to min f32.
    let tiny = if dtype == LowDtype::F16 {
        1. / 16384.
    } else {
        1e-20
    };
    let raw = encodings(dtype, &[-tiny, tiny, -2. * tiny, 2. * tiny]);
    let input = b.upload_low(dtype, shape(&[4]), &raw)?;
    for epsilon in [f32::from_bits(1), 1e-40, 1e-5, f32::MAX] {
        check_input(b, &input, &raw, &[0], epsilon)?;
    }
    // Uniform probability in f32 must retain bits beyond low precision.
    let raw = encodings(dtype, &[1., 1., 1.]);
    let input = b.upload_low(dtype, shape(&[3]), &raw)?;
    let probability = b.read_f32(&b.softmax_low_f32(&input, &[0])?)?;
    assert!((probability[0] - 1. / 3.).abs() < 1e-6);
    assert_ne!(probability[0], decode(dtype, round(dtype, probability[0])));
    Ok(())
}

fn check_tiny_norm<B: TensorLowStatsBackend>(b: &B) -> Result<(), B::Error> {
    let dtype = LowDtype::Bf16;
    // Includes true BF16 subnormals, tiny normal groups, values around the
    // 2^-64 lift threshold, a mixed-scale row, and a constant subnormal row.
    let rows = [
        [1u16, 0x8001, 2, 0x8002],
        [0x007f, 0x807f, 0x0080, 0x8080],
        [0x1f7f, 0x9f7f, 0x1f80, 0x9f80],
        [1, 0x8001, 0x3f80, 0xbf80],
        [1, 1, 1, 1],
    ];
    let physical = rows.concat();
    let input = b.upload_low(dtype, shape(&[rows.len(), 4]), &physical)?;
    let input = b.permute_low(&input, &[1, 0])?;
    let logical = (0..4)
        .flat_map(|col| rows.iter().map(move |row| row[col]))
        .collect::<Vec<_>>();
    for epsilon in [f32::from_bits(1), 1e-40, 1e-5] {
        let expected = reference(input.shape(), &values(dtype, &logical), &[0], epsilon).norm;
        let output = b.layer_norm_low_f32(&input, &[0], epsilon)?;
        assert_eq!(output.shape(), input.shape());
        let actual = b.read_f32(&output)?;
        assert_eq!(actual.len(), expected.len());
        for (i, (&a, &e)) in actual.iter().zip(&expected).enumerate() {
            let tolerance = 3e-4 * e.abs().max(f32::MIN_POSITIVE / 3e-4);
            assert!(
                a.is_finite() && (a - e).abs() <= tolerance,
                "tiny norm epsilon={epsilon} [{i}]: {a} != {e}"
            );
        }
    }
    // Hierarchical row summaries must carry the same scale choice as the
    // small path, across a non-power-of-two tail and many reduction parts.
    let raw = (0..131_077)
        .map(|i| [1, 0x8001, 2, 0x8002][i % 4])
        .collect::<Vec<_>>();
    let input = b.upload_low(dtype, shape(&[raw.len()]), &raw)?;
    let epsilon = f32::from_bits(1);
    let expected = reference(input.shape(), &values(dtype, &raw), &[0], epsilon).norm;
    let output = b.layer_norm_low_f32(&input, &[0], epsilon)?;
    assert_eq!(output.shape(), input.shape());
    let actual = b.read_f32(&output)?;
    assert_eq!(actual.len(), expected.len());
    for (&a, &e) in actual.iter().zip(&expected) {
        assert!(
            a.is_finite() && (a - e).abs() <= e.abs() * 3e-4,
            "large tiny norm: {a} != {e}"
        );
    }
    Ok(())
}

fn check_large<B: TensorLowStatsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    for count in [255, 256, 257, 513, 131_077] {
        let raw = encodings(
            dtype,
            &(0..count)
                .map(|i| ((i * 17) % 127) as f32 / 8. - 7.)
                .collect::<Vec<_>>(),
        );
        let input = b.upload_low(dtype, shape(&[count]), &raw)?;
        check_input(b, &input, &raw, &[0], 0.01)?;
    }
    // A uniform long group yields low subnormal probabilities in f16, and
    // would be all zeros if results first used low summation/reciprocals.
    let raw = vec![round(dtype, 1.); 131_077];
    let input = b.upload_low(dtype, shape(&[raw.len()]), &raw)?;
    let wide = b.softmax_low_f32(&input, &[0])?;
    let expected = vec![1. / raw.len() as f32; raw.len()];
    close("softmax", &b.read_f32(&wide)?, &expected, 4e-5);
    rounded(
        b,
        &b.softmax_low(&input, &[0])?,
        &wide,
        "long softmax",
        dtype,
    )?;
    Ok(())
}

fn check_empty_errors<B: TensorLowStatsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let empty = b.upload_low(dtype, shape(&[2, 0, 3]), &[])?;
    for axes in [vec![], vec![0], vec![1], vec![0, 1, 2]] {
        for out in [
            b.softmax_low_f32(&empty, &axes)?,
            b.log_softmax_low_f32(&empty, &axes)?,
            b.layer_norm_low_f32(&empty, &axes, 1e-5)?,
        ] {
            assert_eq!(out.shape(), empty.shape());
            assert!(b.read_f32(&out)?.is_empty());
        }
        for out in [
            b.softmax_low(&empty, &axes)?,
            b.log_softmax_low(&empty, &axes)?,
            b.layer_norm_low(&empty, &axes, 1e-5)?,
        ] {
            assert_eq!(out.shape(), empty.shape());
            assert_eq!(out.low_dtype(), dtype);
            assert!(b.read_low_bits(&out)?.is_empty());
        }
    }
    for keep in [false, true] {
        for axes in [vec![], vec![0]] {
            let output = empty.shape().reduce(&axes, keep).unwrap();
            let m = b.moments_low_f32(&empty, &axes, keep)?;
            for out in [
                m.mean,
                m.variance,
                b.logsumexp_low_f32(&empty, &axes, keep)?,
            ] {
                assert_eq!(out.shape(), &output);
                assert!(b.read_f32(&out)?.is_empty());
            }
            let m = b.moments_low(&empty, &axes, keep)?;
            for out in [m.mean, m.variance, b.logsumexp_low(&empty, &axes, keep)?] {
                assert_eq!(out.shape(), &output);
                assert_eq!(out.low_dtype(), dtype);
                assert!(b.read_low_bits(&out)?.is_empty());
            }
        }
        assert!(b.logsumexp_low_f32(&empty, &[1], keep).is_err());
        assert!(b.moments_low_f32(&empty, &[1], keep).is_err());
        assert!(b.logsumexp_low(&empty, &[1], keep).is_err());
        assert!(b.moments_low(&empty, &[1], keep).is_err());
    }
    let normal = b.upload_low(dtype, shape(&[2, 3]), &[round(dtype, 1.); 6])?;
    for input in [&normal, &empty] {
        for axes in [vec![3], vec![0, 0]] {
            assert!(b.softmax_low_f32(input, &axes).is_err());
            assert!(b.log_softmax_low_f32(input, &axes).is_err());
            assert!(b.logsumexp_low_f32(input, &axes, false).is_err());
            assert!(b.moments_low_f32(input, &axes, false).is_err());
            assert!(b.layer_norm_low_f32(input, &axes, 1e-5).is_err());
            assert!(b.softmax_low(input, &axes).is_err());
            assert!(b.log_softmax_low(input, &axes).is_err());
            assert!(b.logsumexp_low(input, &axes, false).is_err());
            assert!(b.moments_low(input, &axes, false).is_err());
            assert!(b.layer_norm_low(input, &axes, 1e-5).is_err());
        }
        for epsilon in [0., -0., -1., f32::INFINITY, f32::NAN] {
            assert!(b.layer_norm_low_f32(input, &[1], epsilon).is_err());
            assert!(b.layer_norm_low(input, &[1], epsilon).is_err());
        }
    }
    Ok(())
}

fn check_chain<B: TensorLowStatsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let raw = encodings(dtype, &[1., 2., 3., -2., 0., 1.]);
    let input = b.upload_low(dtype, shape(&[2, 3]), &raw)?;
    let probabilities = b.softmax_low(&input, &[1])?;
    let normalized = b.layer_norm_low_f32(&probabilities, &[1], 0.125)?;
    let moments = b.moments_low_f32(&probabilities, &[1], false)?;
    // Intermediate probabilities remain resident. The independent reference
    // includes their public low-output rounding before the following stage.
    let probabilities = reference(input.shape(), &values(dtype, &raw), &[1], 0.125).softmax;
    let probabilities = values(dtype, &encodings(dtype, &probabilities));
    let expected = reference(input.shape(), &probabilities, &[1], 0.125);
    close(
        "layer_norm",
        &b.read_f32(&normalized)?,
        &expected.norm,
        2e-4,
    );
    close("mean", &b.read_f32(&moments.mean)?, &expected.mean, 2e-5);
    close(
        "population variance",
        &b.read_f32(&moments.variance)?,
        &expected.variance,
        2e-4,
    );
    Ok(())
}

/// Direct low-input statistics against the shared independent f64 oracle,
/// plus exact final cast checks, every finite singleton pattern and strict
/// relative tiny-BF16 normalization. Physical runtime availability is required
/// separately by the calling backend's qualification gate.
pub fn check_low_stats_backend<B: TensorLowStatsBackend>(b: &B) -> Result<(), B::Error> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        check_singletons(b, dtype)?;
        check_layouts(b, dtype)?;
        check_extremes(b, dtype)?;
        check_large(b, dtype)?;
        check_empty_errors(b, dtype)?;
        check_chain(b, dtype)?;
    }
    check_tiny_norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiny_low_norm_reference_requires_nonzero_normal_f32_outputs() {
        let raw = [1, 0x8001, 2, 0x8002];
        let expected = reference(
            &shape(&[4]),
            &values(LowDtype::Bf16, &raw),
            &[0],
            f32::from_bits(1),
        );
        assert_eq!(expected.mean, [0.]);
        assert_eq!(expected.variance, [0.]);
        assert!(expected.norm.iter().all(|x| x.is_normal()));
        assert_eq!(expected.norm[0], -expected.norm[1]);
        assert_eq!(expected.norm[2], -expected.norm[3]);
        assert_eq!(expected.norm[2], 2. * expected.norm[0]);
    }
}
