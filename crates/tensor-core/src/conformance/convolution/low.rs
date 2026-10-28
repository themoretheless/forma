use super::{Input, cases, exact, reference};
use crate::conformance::{low_ops::round, low_precision::decode, shape};
use crate::{ConvOptions, HasLowDtype, HasShape, LowDtype, TensorLowConvBackend};

fn upload<B: TensorLowConvBackend>(
    b: &B,
    dtype: LowDtype,
    input: &Input,
) -> Result<B::LowTensor, B::Error> {
    let bits: Vec<_> = input.host.values.iter().map(|&v| round(dtype, v)).collect();
    let mut tensor = b.upload_low(dtype, input.host.shape.clone(), &bits)?;
    if let Some(axes) = &input.host.permutation {
        tensor = b.permute_low(&tensor, axes)?;
    }
    if let Some(dims) = &input.broadcast {
        tensor = b.broadcast_low(&tensor, shape(dims))?;
    }
    Ok(tensor)
}
fn logical(input: &Input, dtype: LowDtype) -> (Vec<usize>, Vec<f32>) {
    let narrowed: Vec<_> = input
        .host
        .values
        .iter()
        .map(|&v| decode(dtype, round(dtype, v)))
        .collect();
    input.logical(&narrowed)
}
fn rounded(actual: &[u16], dtype: LowDtype, expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        let want = round(dtype, e as f32);
        // Arithmetic zero signs are unspecified; every nonzero result must
        // match the independent nearest-even codec exactly in these fixtures.
        assert!(
            a == want || (a & 0x7fff == 0 && want & 0x7fff == 0),
            "low conv {dtype:?}[{i}] {a:#06x} != {want:#06x} from {e:?}"
        );
    }
}

/// Direct low-input f32 output, one final low rounding, layouts, all spatial
/// ranks and empty rules. Normal products from tiny low operands are included.
pub fn check_low_conv_backend<B: TensorLowConvBackend>(b: &B) -> Result<(), B::Error> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for case in cases() {
            let input = upload(b, dtype, &case.input)?;
            let weight = upload(b, dtype, &case.weight)?;
            let (id, iv) = logical(&case.input, dtype);
            let (wd, wv) = logical(&case.weight, dtype);
            let (dims, expected) = reference((&id, &iv), (&wd, &wv), &case.options);
            let output = b.conv_low_f32(&input, &weight, &case.options)?;
            assert_eq!(output.shape().dims(), dims);
            exact(&b.read_f32(&output)?, &expected);
            let low = b.conv_low(&input, &weight, &case.options)?;
            assert_eq!(low.shape().dims(), dims);
            assert_eq!(low.low_dtype(), dtype);
            rounded(&b.read_low_bits(&low)?, dtype, &expected);
            assert_eq!(
                b.read_low_bits(&input)?,
                iv.iter().map(|&x| round(dtype, x)).collect::<Vec<_>>()
            );
            assert_eq!(
                b.read_low_bits(&weight)?,
                wv.iter().map(|&x| round(dtype, x)).collect::<Vec<_>>()
            );
        }
        final_rounding(b, dtype)?;
        tiny_products(b, dtype)?;
        invalid(b, dtype)?;
    }
    Ok(())
}

fn final_rounding<B: TensorLowConvBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let half = match dtype {
        LowDtype::F16 => 2_f32.powi(-11),
        LowDtype::Bf16 => 2_f32.powi(-8),
    };
    let input = b.upload_low(
        dtype,
        shape(&[1, 4, 1]),
        &[1., half, half, half].map(|v| round(dtype, v)),
    )?;
    let weight = b.upload_low(dtype, shape(&[1, 4, 1]), &[round(dtype, 1.); 4])?;
    let output = b.conv_low_f32(&input, &weight, &ConvOptions::new(1))?;
    let expected = 1f64 + 3. * f64::from(half);
    exact(&b.read_f32(&output)?, &[expected]);
    assert_ne!(
        f64::from(decode(dtype, round(dtype, expected as f32))),
        expected,
        "witness must expose premature low output rounding"
    );
    let low = b.conv_low(&input, &weight, &ConvOptions::new(1))?;
    rounded(&b.read_low_bits(&low)?, dtype, &[expected]);

    let large = if dtype == LowDtype::F16 { 2048. } else { 256. };
    let input = b.upload_low(
        dtype,
        shape(&[1, 1, 3]),
        &[large, 1., -large].map(|v| round(dtype, v)),
    )?;
    let weight = b.upload_low(dtype, shape(&[1, 1, 3]), &[round(dtype, 1.); 3])?;
    exact(
        &b.read_f32(&b.conv_low_f32(&input, &weight, &ConvOptions::new(1))?)?,
        &[1.],
    );
    rounded(
        &b.read_low_bits(&b.conv_low(&input, &weight, &ConvOptions::new(1))?)?,
        dtype,
        &[1.],
    );
    Ok(())
}

fn tiny_products<B: TensorLowConvBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let (last_subnormal, large, one) = match dtype {
        LowDtype::F16 => (0x03ff, 0x7bff, 0x3c00),
        LowDtype::Bf16 => (0x007f, 0x7f7f, 0x3f80),
    };
    let tiny = [1, last_subnormal, 0x8001, last_subnormal | 0x8000, 0, one];
    let expected: Vec<_> = tiny
        .iter()
        .map(|&v| f64::from(decode(dtype, v)) * f64::from(decode(dtype, large)))
        .collect();
    assert!(
        expected[..4]
            .iter()
            .all(|&v| v.abs() >= f64::from(f32::MIN_POSITIVE) && v.abs() <= f64::from(f32::MAX))
    );
    // Tiny operand in input, then in weight: every critical product is normal
    // f32, so flushing raw BF16 subnormals would be an observable defect.
    let input = b.upload_low(dtype, shape(&[1, 1, 6]), &tiny)?;
    let weight = b.upload_low(dtype, shape(&[1, 1, 1]), &[large])?;
    exact(
        &b.read_f32(&b.conv_low_f32(&input, &weight, &ConvOptions::new(1))?)?,
        &expected,
    );
    rounded(
        &b.read_low_bits(&b.conv_low(&input, &weight, &ConvOptions::new(1))?)?,
        dtype,
        &expected,
    );
    let input = b.upload_low(dtype, shape(&[1, 1, 1]), &[large])?;
    let weight = b.upload_low(dtype, shape(&[6, 1, 1]), &tiny)?;
    exact(
        &b.read_f32(&b.conv_low_f32(&input, &weight, &ConvOptions::new(1))?)?,
        &expected,
    );
    rounded(
        &b.read_low_bits(&b.conv_low(&input, &weight, &ConvOptions::new(1))?)?,
        dtype,
        &expected,
    );
    Ok(())
}

fn invalid<B: TensorLowConvBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let other = if dtype == LowDtype::F16 {
        LowDtype::Bf16
    } else {
        LowDtype::F16
    };
    for n in [0, 1] {
        let input = b.upload_low(dtype, shape(&[n, 1, 3]), &vec![0; n * 3])?;
        let weight = b.upload_low(other, shape(&[1, 1, 1]), &[0])?;
        assert!(
            b.conv_low_f32(&input, &weight, &ConvOptions::new(1))
                .is_err()
        );
        assert!(b.conv_low(&input, &weight, &ConvOptions::new(1)).is_err());
    }
    let input = b.upload_low(dtype, shape(&[0, 1, 3]), &[])?;
    let weight = b.upload_low(dtype, shape(&[1, 1, 1]), &[0])?;
    let bad = ConvOptions {
        groups: 0,
        ..ConvOptions::new(1)
    };
    assert!(b.conv_low_f32(&input, &weight, &bad).is_err());
    assert!(b.conv_low(&input, &weight, &bad).is_err());
    let bad = ConvOptions {
        padding_before: vec![usize::MAX],
        ..ConvOptions::new(1)
    };
    assert!(b.conv_low_f32(&input, &weight, &bad).is_err());
    assert!(b.conv_low(&input, &weight, &bad).is_err());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn witness_really_requires_f32_accumulation_and_final_rounding() {
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            let half = if dtype == LowDtype::F16 {
                2_f32.powi(-11)
            } else {
                2_f32.powi(-8)
            };
            let total = 1. + 3. * half;
            assert_ne!(decode(dtype, round(dtype, total)), total);
            assert_eq!(decode(dtype, round(dtype, total)), 1. + 4. * half);
            let product =
                f64::from(decode(LowDtype::Bf16, 1)) * f64::from(decode(LowDtype::Bf16, 0x7f7f));
            assert_eq!(product, 0.0311279296875);
        }
    }
}
