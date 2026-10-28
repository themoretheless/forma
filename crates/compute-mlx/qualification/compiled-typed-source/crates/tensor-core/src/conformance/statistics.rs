use super::shape;
use crate::{HasShape, Shape, TensorStatsBackend};

pub(super) struct Reference {
    pub(super) mean: Vec<f32>,
    pub(super) variance: Vec<f32>,
    pub(super) softmax: Vec<f32>,
    pub(super) log_softmax: Vec<f32>,
    pub(super) logsumexp: Vec<f32>,
    pub(super) norm: Vec<f32>,
}

pub(super) fn reference(input: &Shape, values: &[f32], axes: &[usize], epsilon: f32) -> Reference {
    let output = input.reduce(axes, false).unwrap();
    let mut rows = vec![Vec::new(); output.numel()];
    for (flat, &value) in values.iter().enumerate() {
        let mut rest = flat;
        let mut row = 0;
        let mut stride = 1;
        for (axis, &dim) in input.dims().iter().enumerate().rev() {
            let coordinate = rest % dim;
            rest /= dim;
            if !axes.contains(&axis) {
                row += coordinate * stride;
                stride *= dim;
            }
        }
        rows[row].push((flat, f64::from(value)));
    }
    let mut result = Reference {
        mean: Vec::new(),
        variance: Vec::new(),
        logsumexp: Vec::new(),
        softmax: vec![0.; values.len()],
        log_softmax: vec![0.; values.len()],
        norm: vec![0.; values.len()],
    };
    for row in rows {
        assert!(!row.is_empty());
        let n = row.len() as f64;
        let origin = row[0].1;
        let delta_mean = row.iter().map(|&(_, x)| x - origin).sum::<f64>() / n;
        let variance = row
            .iter()
            .map(|&(_, x)| (x - origin - delta_mean).powi(2))
            .sum::<f64>()
            / n;
        let maximum = row
            .iter()
            .map(|&(_, x)| x)
            .fold(f64::NEG_INFINITY, f64::max);
        let exp_sum = row.iter().map(|&(_, x)| (x - maximum).exp()).sum::<f64>();
        let log_sum = exp_sum.ln();
        result.mean.push((origin + delta_mean) as f32);
        result.variance.push(variance as f32);
        result.logsumexp.push((maximum + log_sum) as f32);
        for (flat, x) in row {
            result.softmax[flat] = ((x - maximum).exp() / exp_sum) as f32;
            result.log_softmax[flat] = (x - maximum - log_sum) as f32;
            result.norm[flat] =
                ((x - origin - delta_mean) / (variance + f64::from(epsilon)).sqrt()) as f32;
        }
    }
    result
}

pub(super) fn close(label: &str, actual: &[f32], expected: &[f32], tolerance: f32) {
    assert_eq!(actual.len(), expected.len(), "{label} length");
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        if e.is_infinite() {
            assert_eq!(a, e, "{label}[{i}] infinity");
        } else {
            let scale = if matches!(label, "softmax" | "population variance") {
                // Large-group probabilities must not pass merely because each
                // individual expected value is small. Permit subnormal flush,
                // while retaining relative checks above that numerical floor.
                e.abs().max(f32::MIN_POSITIVE / tolerance)
            } else {
                e.abs().max(1.)
            };
            assert!(
                a.is_finite() && (a - e).abs() <= tolerance * scale,
                "{label}[{i}]: {a:?} != {e:?}"
            );
        }
    }
}

fn check_input<B: TensorStatsBackend>(
    b: &B,
    input: &B::Tensor,
    values: &[f32],
    axes: &[usize],
    epsilon: f32,
) -> Result<(), B::Error> {
    let expected = reference(input.shape(), values, axes, epsilon);
    let softmax = b.softmax(input, axes)?;
    let log_softmax = b.log_softmax(input, axes)?;
    let norm = b.layer_norm(input, axes, epsilon)?;
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
    for keep_dims in [false, true] {
        let output_shape = input.shape().reduce(axes, keep_dims).unwrap();
        let lse = b.logsumexp(input, axes, keep_dims)?;
        let moments = b.moments(input, axes, keep_dims)?;
        assert_eq!(lse.shape(), &output_shape);
        assert_eq!(moments.mean.shape(), &output_shape);
        assert_eq!(moments.variance.shape(), &output_shape);
        close("logsumexp", &b.read_f32(&lse)?, &expected.logsumexp, 5e-5);
        close("mean", &b.read_f32(&moments.mean)?, &expected.mean, 2e-5);
        close(
            "population variance",
            &b.read_f32(&moments.variance)?,
            &expected.variance,
            2e-4,
        );
    }
    Ok(())
}

/// Stable distribution/moment/normalization fixtures. The independent f64
/// reference uses an input anchor rather than any backend's scaling algorithm.
/// The caller separately requires the intended physical runtime to exist.
pub fn check_stats_backend<B: TensorStatsBackend>(b: &B) -> Result<(), B::Error> {
    let values: Vec<f32> = (0..30).map(|i| (i % 13) as f32 / 4. - 1.5).collect();
    let input = b.upload_f32(shape(&[2, 3, 5]), &values)?;
    for axes in [vec![], vec![0], vec![2], vec![2, 0], vec![0, 1, 2]] {
        check_input(b, &input, &values, &axes, 1e-5)?;
    }
    let transposed = b.permute(&input, &[2, 0, 1])?;
    let mut logical = Vec::new();
    for col in 0..5 {
        for batch in 0..2 {
            for row in 0..3 {
                logical.push(values[batch * 15 + row * 5 + col]);
            }
        }
    }
    check_input(b, &transposed, &logical, &[2, 0], 0.125)?;
    let broadcast = b.broadcast_to(
        &b.upload_f32(shape(&[2, 1]), &[2., -3.])?,
        shape(&[4, 2, 9]),
    )?;
    let logical: Vec<f32> = (0..72).map(|i| [2., -3.][i / 9 % 2]).collect();
    check_input(b, &broadcast, &logical, &[0, 2], 1e-5)?;
    let scalar = b.upload_f32(shape(&[]), &[42.])?;
    check_input(b, &scalar, &[42.], &[], 1e-5)?;

    // Huge offsets exercise overflow-safe centering and shifted log-softmax.
    for values in [
        vec![1e30; 257],
        vec![1e8, 1e8 + 8., 1e8 + 16., 1e8 + 32., 1e8 + 64.],
        vec![10000., 10001., 9998., -10000.],
        vec![-1e30, 1e30, 0., 5e29, -5e29],
        vec![-f32::MAX, f32::MAX, 0.],
        // Midpoint construction must not become (lo+hi)/2 under shader
        // reassociation: that overflows even though the midpoint is finite.
        [0u32, 1, 2, 4, 8]
            .map(|step| f32::from_bits(f32::MAX.to_bits() - step * 65536))
            .to_vec(),
        [0u32, 1, 2, 4, 8]
            .map(|step| -f32::from_bits(f32::MAX.to_bits() - step * 65536))
            .to_vec(),
    ] {
        let input = b.upload_f32(shape(&[values.len()]), &values)?;
        check_input(b, &input, &values, &[0], 1e-5)?;
    }
    // A tiny epsilon must survive parameter handling even when its own bits
    // are subnormal. Normalization must not first materialize tiny variance.
    for epsilon in [f32::from_bits(1), 1e-40, 1e-5, f32::MAX] {
        let values = [-1e-20, 1e-20, -2e-20, 2e-20];
        let input = b.upload_f32(shape(&[4]), &values)?;
        check_input(b, &input, &values, &[0], epsilon)?;
    }

    // Multi-workgroup contractions and a prime tail exercise partial states.
    let values: Vec<f32> = (0..131_077)
        .map(|i| ((i * 17) % 127) as f32 / 8. - 7.)
        .collect();
    let large = b.upload_f32(shape(&[values.len()]), &values)?;
    check_input(b, &large, &values, &[0], 0.01)?;

    let empty = b.upload_f32(shape(&[2, 0, 3]), &[])?;
    for axes in [vec![], vec![0], vec![1], vec![0, 1, 2]] {
        for result in [
            b.softmax(&empty, &axes)?,
            b.log_softmax(&empty, &axes)?,
            b.layer_norm(&empty, &axes, 1e-5)?,
        ] {
            assert_eq!(result.shape(), empty.shape());
            assert!(b.read_f32(&result)?.is_empty());
        }
    }
    for keep_dims in [false, true] {
        for axes in [vec![], vec![0]] {
            let lse = b.logsumexp(&empty, &axes, keep_dims)?;
            let moments = b.moments(&empty, &axes, keep_dims)?;
            let expected_shape = empty.shape().reduce(&axes, keep_dims).unwrap();
            assert_eq!(lse.shape(), &expected_shape);
            assert_eq!(moments.mean.shape(), &expected_shape);
            assert_eq!(moments.variance.shape(), &expected_shape);
            assert!(b.read_f32(&lse)?.is_empty());
            assert!(b.read_f32(&moments.mean)?.is_empty());
            assert!(b.read_f32(&moments.variance)?.is_empty());
        }
        assert!(b.logsumexp(&empty, &[1], keep_dims).is_err());
        assert!(b.moments(&empty, &[1], keep_dims).is_err());
    }
    for axes in [vec![3], vec![1, 1]] {
        assert!(b.softmax(&input, &axes).is_err());
        assert!(b.log_softmax(&input, &axes).is_err());
        assert!(b.logsumexp(&input, &axes, false).is_err());
        assert!(b.moments(&input, &axes, false).is_err());
        assert!(b.layer_norm(&input, &axes, 1e-5).is_err());
    }
    for epsilon in [0., -0., -1., f32::INFINITY, f32::NAN] {
        assert!(b.layer_norm(&input, &[1], epsilon).is_err());
        assert!(b.layer_norm(&empty, &[1], epsilon).is_err());
    }
    Ok(())
}
