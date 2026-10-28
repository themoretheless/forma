use super::{check, shape};
use crate::{HasLowDtype, HasShape, LowDtype, TensorLowBackend};

mod matmul;

// Mathematical reference, deliberately independent of the device bit codecs.
pub(super) fn decode(dtype: LowDtype, bits: u16) -> f32 {
    let (fraction_bits, exponent_bits, bias) = match dtype {
        LowDtype::F16 => (10, 5, 15),
        LowDtype::Bf16 => (7, 8, 127),
    };
    let fraction = bits & ((1 << fraction_bits) - 1);
    let exponent = (bits >> fraction_bits) & ((1 << exponent_bits) - 1);
    let value = if exponent == (1 << exponent_bits) - 1 {
        if fraction == 0 {
            f64::INFINITY
        } else {
            f64::NAN
        }
    } else if exponent == 0 {
        f64::from(fraction) * 2_f64.powi(1 - bias - fraction_bits)
    } else {
        (1.0 + f64::from(fraction) / 2_f64.powi(fraction_bits))
            * 2_f64.powi(i32::from(exponent) - bias)
    };
    (if bits & 0x8000 != 0 { -value } else { value }) as f32
}

fn assert_cast(actual: &[f32], expected: &[f32], dtype: LowDtype) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        if e.is_nan() {
            assert!(a.is_nan(), "{dtype:?} decode {i:#06x} lost NaN: {a:?}");
        } else {
            assert_eq!(a.to_bits(), e.to_bits(), "{dtype:?} decode {i:#06x}");
        }
    }
}

fn check_casts<B: TensorLowBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let bits: Vec<u16> = (0..=u16::MAX).collect();
    let expected: Vec<f32> = bits.iter().map(|&v| decode(dtype, v)).collect();
    let tensor = b.upload_low(dtype, shape(&[256, 256]), &bits)?;
    assert_eq!(tensor.low_dtype(), dtype);
    assert_eq!(b.read_low_bits(&tensor)?, bits);

    // All payloads survive a real strided materialization and reshape.
    let transposed = b.permute_low(&tensor, &[1, 0])?;
    let copied = b.materialize_low(&transposed)?;
    let flattened = b.reshape_low(&copied, shape(&[65536]))?;
    let transposed_bits: Vec<u16> = (0..256)
        .flat_map(|x| (0..256).map(move |y| (y * 256 + x) as u16))
        .collect();
    assert_eq!(b.read_low_bits(&flattened)?, transposed_bits);
    let restored = b.permute_low(&copied, &[1, 0])?;
    assert_cast(&b.read_f32(&b.cast_to_f32(&restored)?)?, &expected, dtype);

    // Every finite representable value must round-trip, including subnormals
    // and negative zero. Casts need only retain the classification of NaNs.
    let f32_tensor = b.upload_f32(shape(&[65536]), &expected)?;
    let encoded = b.read_low_bits(&b.cast_to_low(&f32_tensor, dtype)?)?;
    for (i, (&actual, &value)) in encoded.iter().zip(&expected).enumerate() {
        if value.is_nan() {
            assert!(
                decode(dtype, actual).is_nan(),
                "{dtype:?} encode NaN {i:#x}"
            );
        } else {
            assert_eq!(actual, i as u16, "{dtype:?} exact encode {i:#06x}");
        }
    }

    // Exercise every positive rounding boundary and its negative mirror. The
    // exact midpoint between adjacent low values is representable in f32.
    let last_finite = match dtype {
        LowDtype::F16 => 0x7bff,
        LowDtype::Bf16 => 0x7f7f,
    };
    let infinity = last_finite + 1;
    let mut values = Vec::new();
    let mut rounded = Vec::new();
    for lower in 0..=last_finite {
        let lo = f64::from(decode(dtype, lower));
        let hi = if lower == last_finite {
            // A hypothetical next value at the next exponent supplies the
            // overflow midpoint. Infinity itself has no finite midpoint.
            lo + (lo - f64::from(decode(dtype, lower - 1)))
        } else {
            f64::from(decode(dtype, lower + 1))
        };
        let midpoint = ((lo + hi) * 0.5) as f32;
        let upper = (lower + 1).min(infinity);
        let tie = if lower & 1 == 0 { lower } else { upper };
        for (value, result) in [
            (f32::from_bits(midpoint.to_bits() - 1), lower),
            (midpoint, tie),
            (f32::from_bits(midpoint.to_bits() + 1), upper),
        ] {
            values.extend([value, -value]);
            rounded.extend([result, result | 0x8000]);
        }
    }
    // f32 subnormals below both formats' first midpoint must become signed zero.
    values.extend([f32::from_bits(1), -f32::from_bits(1)]);
    rounded.extend([0, 0x8000]);
    let source = b.upload_f32(shape(&[values.len()]), &values)?;
    let actual = b.read_low_bits(&b.cast_to_low(&source, dtype)?)?;
    assert_eq!(actual.len(), rounded.len());
    for (i, (&a, &e)) in actual.iter().zip(&rounded).enumerate() {
        assert_eq!(a, e, "{dtype:?} rounding value {:?} at {i}", values[i]);
    }

    let source = b.upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])?;
    let strided = b.permute(&source, &[1, 0])?;
    let converted = b.cast_to_low(&strided, dtype)?;
    assert_eq!(converted.shape(), &shape(&[3, 2]));
    assert_eq!(
        b.read_f32(&b.cast_to_f32(&converted)?)?,
        [1., 4., 2., 5., 3., 6.]
    );

    let odd_bits = [0x8000, 0x0001, 0x7fff, 0xffff, 0x1234];
    let odd = b.upload_low(dtype, shape(&[5]), &odd_bits)?;
    assert_eq!(b.read_low_bits(&b.materialize_low(&odd)?)?, odd_bits);
    let scalar = b.upload_low(dtype, shape(&[]), &[0x8001])?;
    let broadcast = b.broadcast_low(&scalar, shape(&[3, 5]))?;
    assert_eq!(
        b.read_low_bits(&b.materialize_low(&broadcast)?)?,
        vec![0x8001; 15]
    );
    let empty = b.broadcast_low(&scalar, shape(&[0, 5]))?;
    assert!(b.read_low_bits(&empty)?.is_empty());
    assert!(b.read_f32(&b.cast_to_f32(&empty)?)?.is_empty());
    let empty_f32 = b.upload_f32(shape(&[2, 0]), &[])?;
    assert!(
        b.read_low_bits(&b.cast_to_low(&empty_f32, dtype)?)?
            .is_empty()
    );
    assert!(b.upload_low(dtype, shape(&[2]), &[0]).is_err());
    assert!(b.reshape_low(&odd, shape(&[2, 3])).is_err());
    assert!(b.permute_low(&odd, &[1]).is_err());
    assert!(b.broadcast_low(&odd, shape(&[6])).is_err());
    Ok(())
}

fn upload<B: TensorLowBackend>(
    b: &B,
    dtype: LowDtype,
    dims: &[usize],
    values: &[f32],
) -> Result<B::LowTensor, B::Error> {
    b.cast_to_low(&b.upload_f32(shape(dims), values)?, dtype)
}

fn check_product<B: TensorLowBackend>(
    b: &B,
    left: &B::LowTensor,
    right: &B::LowTensor,
    dims: &[usize],
    expected: &[f32],
) -> Result<(), B::Error> {
    let support = b.low_precision_support(left.low_dtype());
    if support.matmul {
        let result = b.matmul_low(left, right)?;
        assert_eq!(result.low_dtype(), left.low_dtype());
        assert_eq!(result.shape(), &shape(dims));
        check(&b.read_f32(&b.cast_to_f32(&result)?)?, expected);
    } else {
        assert!(b.matmul_low(left, right).is_err());
    }
    if support.matmul_f32 {
        let result = b.matmul_low_f32(left, right)?;
        assert_eq!(result.shape(), &shape(dims));
        check(&b.read_f32(&result)?, expected);
    } else {
        assert!(b.matmul_low_f32(left, right).is_err());
    }
    Ok(())
}

/// Exhaustive raw storage and conversion boundaries, plus portable low-input
/// matmul fixtures. Requires the caller to enforce physical device availability.
/// Matrix fixtures include exact dyadic f32 accumulation and independent final
/// low rounding, so widened low-output results cannot pass as direct f32.
pub fn check_low_backend<B: TensorLowBackend>(b: &B) -> Result<(), B::Error> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        check_casts(b, dtype)?;
        matmul::check(b, dtype)?;
        let vector = upload(b, dtype, &[3], &[2., -1., 3.])?;
        let other = upload(b, dtype, &[3], &[4., 5., -2.])?;
        check_product(b, &vector, &other, &[], &[-3.])?;
        let matrix = upload(b, dtype, &[2, 3], &[1., 2., 3., 4., 5., 6.])?;
        check_product(b, &matrix, &vector, &[2], &[9., 21.])?;
        let transposed = b.permute_low(&matrix, &[1, 0])?;
        check_product(b, &vector, &transposed, &[2], &[9., 21.])?;
        let batches = b.broadcast_low(&matrix, shape(&[2, 1, 2, 3]))?;
        let right = b.broadcast_low(&transposed, shape(&[1, 3, 3, 2]))?;
        let expected: Vec<f32> = (0..6).flat_map(|_| [14., 32., 32., 77.]).collect();
        check_product(b, &batches, &right, &[2, 3, 2, 2], &expected)?;

        // Partial tiles on all three matrix axes, with a strided left operand
        // and a broadcast right batch. Small integral results are exact in both
        // low formats, so a CPU f64 dot product is an independent reference.
        let left_values: Vec<f32> = (0..2 * 19 * 17)
            .map(|i| ((i * 7 + i / 17) % 3) as f32 - 1.)
            .collect();
        let right_values: Vec<f32> = (0..19 * 21)
            .map(|i| ((i * 11 + i / 21) % 3) as f32 - 1.)
            .collect();
        let left = upload(b, dtype, &[2, 19, 17], &left_values)?;
        let left = b.permute_low(&left, &[0, 2, 1])?;
        let right = upload(b, dtype, &[1, 19, 21], &right_values)?;
        let mut expected = Vec::new();
        for batch in 0..2 {
            for row in 0..17 {
                for col in 0..21 {
                    expected.push(
                        (0..19)
                            .map(|k| {
                                f64::from(left_values[batch * 19 * 17 + k * 17 + row])
                                    * f64::from(right_values[k * 21 + col])
                            })
                            .sum::<f64>() as f32,
                    );
                }
            }
        }
        check_product(b, &left, &right, &[2, 17, 21], &expected)?;
        let scalar = upload(b, dtype, &[], &[2.])?;
        let repeated = b.broadcast_low(&scalar, shape(&[3]))?;
        check_product(b, &repeated, &vector, &[], &[8.])?;

        // A half-precision accumulator would lose unit contributions. Every
        // product and the final result fit both formats; f32 accumulation does
        // not lose the small terms, independent of summation grouping.
        let mut values = vec![1_f32; 258];
        values[0] = 4096.;
        values[257] = -4096.;
        let left = upload(b, dtype, &[258], &values)?;
        let ones = upload(b, dtype, &[258], &vec![1.; 258])?;
        check_product(b, &left, &ones, &[], &[256.])?;

        // Direct f32 output must not be a rounded low output widened afterward.
        let increment = match dtype {
            LowDtype::F16 => 1. / 2048.,
            LowDtype::Bf16 => 1. / 256.,
        };
        let left = upload(b, dtype, &[2], &[1., increment])?;
        let right = upload(b, dtype, &[2], &[1., 1.])?;
        if b.low_precision_support(dtype).matmul {
            let result = b.matmul_low(&left, &right)?;
            assert_eq!(b.read_f32(&b.cast_to_f32(&result)?)?, [1.]);
        }
        if b.low_precision_support(dtype).matmul_f32 {
            assert_eq!(
                b.read_f32(&b.matmul_low_f32(&left, &right)?)?,
                [1. + increment]
            );
        }

        let empty = b.upload_low(dtype, shape(&[0]), &[])?;
        check_product(b, &empty, &empty, &[], &[0.])?;
        let zero_matrix = b.upload_low(dtype, shape(&[2, 0, 4]), &[])?;
        check_product(b, &empty, &zero_matrix, &[2, 4], &[0.; 8])?;
        let empty_batch = b.upload_low(dtype, shape(&[0, 2, 3]), &[])?;
        check_product(b, &empty_batch, &vector, &[0, 2], &[])?;
        let mixed_dtype = if dtype == LowDtype::F16 {
            LowDtype::Bf16
        } else {
            LowDtype::F16
        };
        let mixed = upload(b, mixed_dtype, &[3], &[1., 2., 3.])?;
        for (left, right) in [(&scalar, &vector), (&vector, &matrix), (&vector, &mixed)] {
            assert!(b.matmul_low(left, right).is_err());
            assert!(b.matmul_low_f32(left, right).is_err());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mathematical_low_reference_covers_format_boundaries() {
        assert_eq!(decode(LowDtype::F16, 0x0001), 2_f32.powi(-24));
        assert_eq!(decode(LowDtype::F16, 0x03ff), 1023. * 2_f32.powi(-24));
        assert_eq!(decode(LowDtype::F16, 0x0400), 2_f32.powi(-14));
        assert_eq!(decode(LowDtype::F16, 0x7bff), 65504.);
        assert_eq!(decode(LowDtype::Bf16, 0x0001).to_bits(), 0x00010000);
        assert_eq!(decode(LowDtype::Bf16, 0x0080).to_bits(), 0x00800000);
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            assert_eq!(decode(dtype, 0x8000).to_bits(), 0x80000000);
            assert!(decode(dtype, 0xffff).is_nan());
        }
    }
}
