use super::{check, low_precision::decode, reduction::expected_f32, shape};
use crate::{BinaryOp, HasLowDtype, HasShape, LowDtype, ReduceOp, TensorLowOpsBackend, UnaryOp};

const REDUCTIONS: [ReduceOp; 4] = [
    ReduceOp::Sum,
    ReduceOp::Product,
    ReduceOp::Min,
    ReduceOp::Max,
];

fn last_finite(dtype: LowDtype) -> u16 {
    match dtype {
        LowDtype::F16 => 0x7bff,
        LowDtype::Bf16 => 0x7f7f,
    }
}

// Search mathematical representable values instead of repeating the device
// integer codec. Distances and midpoint ties are evaluated exactly in f64.
pub(super) fn round(dtype: LowDtype, value: f32) -> u16 {
    let sign = if value.is_sign_negative() { 0x8000 } else { 0 };
    let value = f64::from(value).abs();
    let max = last_finite(dtype);
    if value.is_nan() {
        return sign | (max + 2);
    }
    let last = f64::from(decode(dtype, max));
    let overflow = last + (last - f64::from(decode(dtype, max - 1))) * 0.5;
    if value >= overflow {
        return sign | (max + 1);
    }
    let (mut lo, mut hi) = (0, max);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if f64::from(decode(dtype, mid)) <= value {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == max {
        return sign | lo;
    }
    let below = value - f64::from(decode(dtype, lo));
    let above = f64::from(decode(dtype, lo + 1)) - value;
    sign | if below < above || (below == above && lo & 1 == 0) {
        lo
    } else {
        lo + 1
    }
}

fn upload<B: TensorLowOpsBackend>(
    b: &B,
    dtype: LowDtype,
    dims: &[usize],
    values: &[f32],
) -> Result<B::LowTensor, B::Error> {
    b.upload_low(
        dtype,
        shape(dims),
        &values.iter().map(|&x| round(dtype, x)).collect::<Vec<_>>(),
    )
}

fn exact_f32(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(
            a.to_bits(),
            e.to_bits(),
            "exact low reduction element {i}: {a:?} != {e:?}"
        );
    }
}

fn rounded(actual: &[u16], dtype: LowDtype, expected: &[f32], ulps: u16) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        let target = round(dtype, e);
        assert!(
            a == target || (a & 0x8000 == target & 0x8000 && a.abs_diff(target) <= ulps),
            "{dtype:?} rounded element {i}: {a:#06x} != {target:#06x} from {e}"
        );
    }
}

fn check_exact_bits<B: TensorLowOpsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let max = last_finite(dtype);
    let bits: Vec<u16> = (0..=max).chain((0..=max).map(|x| x | 0x8000)).collect();
    let input = b.upload_low(dtype, shape(&[bits.len()]), &bits)?;
    for (op, sign_mask) in [(UnaryOp::Negate, 0x8000), (UnaryOp::Abs, 0)] {
        let expected: Vec<_> = bits
            .iter()
            .map(|&x| {
                if sign_mask == 0 {
                    x & 0x7fff
                } else {
                    x ^ sign_mask
                }
            })
            .collect();
        assert_eq!(
            b.read_low_bits(&b.unary_low(op, &input)?)?,
            expected,
            "{dtype:?} {op:?}"
        );
    }
    let opposite: Vec<_> = bits.iter().map(|&x| x ^ 0x8000).collect();
    let other = b.upload_low(dtype, shape(&[bits.len()]), &opposite)?;
    for (op, sign) in [(BinaryOp::Min, 0x8000), (BinaryOp::Max, 0)] {
        let expected: Vec<_> = bits.iter().map(|&x| (x & 0x7fff) | sign).collect();
        assert_eq!(
            b.read_low_bits(&b.binary_low(op, &input, &other)?)?,
            expected,
            "{dtype:?} {op:?}"
        );
    }
    assert_eq!(b.read_low_bits(&input)?, bits);

    // Noncontiguous and broadcast input; compare exact f32 bits, since an
    // absolute float tolerance would hide a flushed BF16 subnormal entirely.
    let tiny = b.upload_low(dtype, shape(&[2, 3]), &[1, 0x8002, 0, 0x8001, 2, 0x8000])?;
    let tiny = b.permute_low(&tiny, &[1, 0])?;
    for (op, expected) in [
        (ReduceOp::Min, vec![0x8001, 0x8002, 0x8000]),
        (ReduceOp::Max, vec![1, 2, 0]),
    ] {
        exact_f32(
            &b.read_f32(&b.reduce_low_f32(op, &tiny, &[1], false)?)?,
            &expected
                .iter()
                .map(|&x| decode(dtype, x))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            b.read_low_bits(&b.reduce_low(op, &tiny, &[1], false)?)?,
            expected
        );
    }
    let scalar = b.upload_low(dtype, shape(&[]), &[0x8001])?;
    let expanded = b.broadcast_low(&scalar, shape(&[3, 7]))?;
    for op in [ReduceOp::Min, ReduceOp::Max] {
        exact_f32(
            &b.read_f32(&b.reduce_low_f32(op, &expanded, &[1], true)?)?,
            &[decode(dtype, 0x8001); 3],
        );
    }
    for op in REDUCTIONS {
        let same = b.reduce_low(op, &tiny, &[], false)?;
        assert_eq!(b.read_low_bits(&same)?, [1, 0x8001, 0x8002, 2, 0, 0x8000]);
    }
    // Force partial reductions of tiny values, then extrema over f32 partials.
    let n = 131_077;
    let mut data = vec![1; n];
    data[17] = 0x8000;
    data[n - 2] = 0x8003;
    data[n - 1] = 4;
    let large = b.upload_low(dtype, shape(&[n]), &data)?;
    for (op, expected) in [(ReduceOp::Min, 0x8003), (ReduceOp::Max, 4)] {
        exact_f32(
            &b.read_f32(&b.reduce_low_f32(op, &large, &[0], false)?)?,
            &[decode(dtype, expected)],
        );
        assert_eq!(
            b.read_low_bits(&b.reduce_low(op, &large, &[0], false)?)?,
            [expected]
        );
    }
    let mut zeroes = vec![0; n];
    zeroes[n - 1] = 0x8000;
    let zeroes = b.upload_low(dtype, shape(&[n]), &zeroes)?;
    for (op, expected) in [(ReduceOp::Min, 0x8000), (ReduceOp::Max, 0)] {
        exact_f32(
            &b.read_f32(&b.reduce_low_f32(op, &zeroes, &[0], false)?)?,
            &[decode(dtype, expected)],
        );
    }
    Ok(())
}

fn check_elementwise<B: TensorLowOpsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let positive = [0.25, 0.5, 1., 2., 4., 0.125];
    let input = upload(b, dtype, &[2, 3], &positive)?;
    let transposed = b.permute_low(&input, &[1, 0])?;
    for op in [
        UnaryOp::Negate,
        UnaryOp::Abs,
        UnaryOp::Square,
        UnaryOp::Sqrt,
        UnaryOp::Reciprocal,
        UnaryOp::Exp,
        UnaryOp::Log,
        UnaryOp::Sin,
        UnaryOp::Cos,
    ] {
        let expected: Vec<_> = [0, 3, 1, 4, 2, 5]
            .iter()
            .map(|&i| {
                let x = f64::from(positive[i]);
                (match op {
                    UnaryOp::Negate => -x,
                    UnaryOp::Abs => x.abs(),
                    UnaryOp::Square => x * x,
                    UnaryOp::Sqrt => x.sqrt(),
                    UnaryOp::Reciprocal => x.recip(),
                    UnaryOp::Exp => x.exp(),
                    UnaryOp::Log => x.ln(),
                    UnaryOp::Sin => x.sin(),
                    UnaryOp::Cos => x.cos(),
                }) as f32
            })
            .collect();
        let output = b.unary_low(op, &transposed)?;
        assert_eq!(output.shape(), &shape(&[3, 2]));
        assert_eq!(output.low_dtype(), dtype);
        rounded(
            &b.read_low_bits(&output)?,
            dtype,
            &expected,
            u16::from(matches!(
                op,
                UnaryOp::Sqrt
                    | UnaryOp::Reciprocal
                    | UnaryOp::Exp
                    | UnaryOp::Log
                    | UnaryOp::Sin
                    | UnaryOp::Cos
            )),
        );
    }
    let right = upload(b, dtype, &[1, 2], &[0.5, -2.])?;
    for op in [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Min,
        BinaryOp::Max,
    ] {
        let expected: Vec<_> = [0, 3, 1, 4, 2, 5]
            .iter()
            .enumerate()
            .map(|(i, &source)| {
                let (a, c) = (positive[source], [0.5, -2.][i % 2]);
                match op {
                    BinaryOp::Add => a + c,
                    BinaryOp::Subtract => a - c,
                    BinaryOp::Multiply => a * c,
                    BinaryOp::Divide => a / c,
                    BinaryOp::Min => a.min(c),
                    BinaryOp::Max => a.max(c),
                }
            })
            .collect();
        let output = b.binary_low(op, &transposed, &right)?;
        assert_eq!(output.shape(), &shape(&[3, 2]));
        assert_eq!(output.low_dtype(), dtype);
        rounded(&b.read_low_bits(&output)?, dtype, &expected, 0);
    }
    // Multiplication at an odd-lower midpoint must round upward to even.
    let (a, c, increment) = match dtype {
        LowDtype::F16 => (0x3c01, 0x3e00, 16.),
        LowDtype::Bf16 => (0x3f81, 0x3fc0, 2_f32.powi(119)),
    };
    let left = b.upload_low(dtype, shape(&[]), &[a])?;
    let right = b.upload_low(dtype, shape(&[]), &[c])?;
    rounded(
        &b.read_low_bits(&b.binary_low(BinaryOp::Multiply, &left, &right)?)?,
        dtype,
        &[decode(dtype, a) * decode(dtype, c)],
        0,
    );
    let largest = b.upload_low(dtype, shape(&[]), &[last_finite(dtype)])?;
    let increment = upload(b, dtype, &[], &[increment])?;
    assert_eq!(
        b.read_low_bits(&b.binary_low(BinaryOp::Add, &largest, &increment)?)?,
        [last_finite(dtype) + 1]
    );
    Ok(())
}

fn bf16_product_cases() -> Vec<(u16, u16)> {
    let mut cases = Vec::new();
    // Both extreme BF16 subnormals, every sign combination, and both operand
    // positions. These products are normal f32, so input flushing is invalid.
    for tiny in [0x0001, 0x007f] {
        for left_sign in [0, 0x8000] {
            for right_sign in [0, 0x8000] {
                let pair = (tiny | left_sign, 0x7f7f | right_sign);
                cases.extend([pair, (pair.1, pair.0)]);
            }
        }
    }
    // Zero and ordinary normal controls exercise both sides of the guard.
    for (left, right) in [(0, 0x7f7f), (0x3fc0, 0x4000)] {
        for left_sign in [0, 0x8000] {
            for right_sign in [0, 0x8000] {
                let pair = (left | left_sign, right | right_sign);
                cases.extend([pair, (pair.1, pair.0)]);
            }
        }
    }
    cases
}

fn check_bf16_normal_products<B: TensorLowOpsBackend>(b: &B) -> Result<(), B::Error> {
    let dtype = LowDtype::Bf16;
    let cases = bf16_product_cases();
    assert_eq!(cases.len(), 32);
    let left = b.upload_low(
        dtype,
        shape(&[8, 4]),
        &cases.iter().map(|p| p.0).collect::<Vec<_>>(),
    )?;
    let right = b.upload_low(
        dtype,
        shape(&[8, 4]),
        &cases.iter().map(|p| p.1).collect::<Vec<_>>(),
    )?;
    let left = b.permute_low(&left, &[1, 0])?;
    let right = b.permute_low(&right, &[1, 0])?;
    let output = b.binary_low(BinaryOp::Multiply, &left, &right)?;
    assert_eq!(output.shape(), &shape(&[4, 8]));
    assert_eq!(output.low_dtype(), dtype);
    let actual = b.read_low_bits(&output)?;
    assert_eq!(actual.len(), cases.len());
    for (i, &got) in actual.iter().enumerate() {
        let source = (i % 8) * 4 + i / 8;
        let (a, c) = cases[source];
        let product = f64::from(decode(dtype, a)) * f64::from(decode(dtype, c));
        let f32_product = product as f32;
        assert_eq!(
            f64::from(f32_product),
            product,
            "BF16 product is exactly representable in f32"
        );
        if source < 16 {
            assert!(f32_product.is_normal());
        }
        if product == 0. {
            // Ordinary arithmetic does not promise a particular zero sign.
            assert_eq!(got & 0x7fff, 0, "zero BF16 product {a:04x}*{c:04x}");
        } else {
            assert_eq!(
                got,
                round(dtype, f32_product),
                "normal BF16 product {a:04x}*{c:04x}: {product}"
            );
        }
    }
    Ok(())
}

fn check_reductions<B: TensorLowOpsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let values: Vec<_> = (0..24).map(|i| [-1., 1., 2., 0.5][i % 4]).collect();
    let input = upload(b, dtype, &[2, 3, 4], &values)?;
    let input = b.permute_low(&input, &[1, 0, 2])?;
    let logical: Vec<_> = (0..3)
        .flat_map(|j| (0..2).flat_map(move |i| (0..4).map(move |k| i * 12 + j * 4 + k)))
        .map(|i| values[i])
        .collect();
    for mask in 0..8 {
        let axes: Vec<_> = (0..3)
            .rev()
            .filter(|axis| mask & (1 << axis) != 0)
            .collect();
        for keep in [false, true] {
            let expected_shape = input.shape().reduce(&axes, keep).unwrap();
            for op in REDUCTIONS {
                let expected = expected_f32(&logical, &[3, 2, 4], &axes, op);
                let result = b.reduce_low_f32(op, &input, &axes, keep)?;
                assert_eq!(result.shape(), &expected_shape);
                check(&b.read_f32(&result)?, &expected);
                let result = b.reduce_low(op, &input, &axes, keep)?;
                assert_eq!(result.shape(), &expected_shape);
                assert_eq!(result.low_dtype(), dtype);
                rounded(&b.read_low_bits(&result)?, dtype, &expected, 0);
            }
            let count: usize = axes.iter().map(|&axis| [3, 2, 4][axis]).product();
            let expected: Vec<_> = expected_f32(&logical, &[3, 2, 4], &axes, ReduceOp::Sum)
                .into_iter()
                .map(|x| x / count as f32)
                .collect();
            let result = b.mean_low_f32(&input, &axes, keep)?;
            assert_eq!(result.shape(), &expected_shape);
            check(&b.read_f32(&result)?, &expected);
            rounded(
                &b.read_low_bits(&b.mean_low(&input, &axes, keep)?)?,
                dtype,
                &expected,
                0,
            );
        }
    }
    let (big, count) = match dtype {
        LowDtype::F16 => (2048., 2053),
        LowDtype::Bf16 => (256., 257),
    };
    let input = upload(b, dtype, &[3], &[big, 1., -big])?;
    exact_f32(
        &b.read_f32(&b.reduce_low_f32(ReduceOp::Sum, &input, &[0], false)?)?,
        &[1.],
    );
    for n in [count, 131_077] {
        let input = upload(b, dtype, &[n], &vec![1.; n])?;
        exact_f32(
            &b.read_f32(&b.reduce_low_f32(ReduceOp::Sum, &input, &[0], false)?)?,
            &[n as f32],
        );
        rounded(
            &b.read_low_bits(&b.reduce_low(ReduceOp::Sum, &input, &[0], false)?)?,
            dtype,
            &[n as f32],
            0,
        );
        exact_f32(&b.read_f32(&b.mean_low_f32(&input, &[0], false)?)?, &[1.]);
        exact_f32(
            &b.read_f32(&b.reduce_low_f32(ReduceOp::Product, &input, &[0], false)?)?,
            &[1.],
        );
    }
    // A true f32 result retains a contribution below the low-format ULP.
    let increment = if dtype == LowDtype::F16 {
        1. / 2048.
    } else {
        1. / 256.
    };
    let input = upload(b, dtype, &[2], &[1., increment])?;
    exact_f32(
        &b.read_f32(&b.reduce_low_f32(ReduceOp::Sum, &input, &[0], false)?)?,
        &[1. + increment],
    );
    exact_f32(
        &b.read_f32(&b.mean_low_f32(&input, &[0], false)?)?,
        &[(1. + increment) / 2.],
    );
    assert_eq!(
        b.read_low_bits(&b.reduce_low(ReduceOp::Sum, &input, &[0], false)?)?,
        [round(dtype, 1.)]
    );
    Ok(())
}

fn check_empty_and_errors<B: TensorLowOpsBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    for dims in [[2, 0, 3], [0, 0, 3]] {
        let input = b.upload_low(dtype, shape(&dims), &[])?;
        for keep in [false, true] {
            for op in REDUCTIONS {
                if dims[0] != 0 && matches!(op, ReduceOp::Min | ReduceOp::Max) {
                    assert!(b.reduce_low_f32(op, &input, &[1], keep).is_err());
                    assert!(b.reduce_low(op, &input, &[1], keep).is_err());
                } else {
                    let expected = vec![f32::from(op == ReduceOp::Product); dims[0] * 3];
                    let result = b.reduce_low_f32(op, &input, &[1], keep)?;
                    assert_eq!(result.shape(), &input.shape().reduce(&[1], keep).unwrap());
                    exact_f32(&b.read_f32(&result)?, &expected);
                    rounded(
                        &b.read_low_bits(&b.reduce_low(op, &input, &[1], keep)?)?,
                        dtype,
                        &expected,
                        0,
                    );
                }
                assert!(
                    b.read_low_bits(&b.reduce_low(op, &input, &[], keep)?)?
                        .is_empty()
                );
            }
            if dims[0] != 0 {
                assert!(b.mean_low_f32(&input, &[1], keep).is_err());
                assert!(b.mean_low(&input, &[1], keep).is_err());
            } else {
                assert!(b.read_f32(&b.mean_low_f32(&input, &[1], keep)?)?.is_empty());
                assert!(
                    b.read_low_bits(&b.mean_low(&input, &[1], keep)?)?
                        .is_empty()
                );
            }
        }
        assert!(
            b.read_low_bits(&b.unary_low(UnaryOp::Square, &input)?)?
                .is_empty()
        );
        let scalar = upload(b, dtype, &[], &[2.])?;
        assert!(
            b.read_low_bits(&b.binary_low(BinaryOp::Add, &input, &scalar)?)?
                .is_empty()
        );
        let other_dtype = if dtype == LowDtype::F16 {
            LowDtype::Bf16
        } else {
            LowDtype::F16
        };
        let mixed = b.upload_low(other_dtype, shape(&dims), &[])?;
        assert!(b.binary_low(BinaryOp::Add, &input, &mixed).is_err());
        for op in REDUCTIONS {
            assert!(b.reduce_low_f32(op, &input, &[0, 0], false).is_err());
        }
        assert!(b.mean_low(&input, &[3], false).is_err());
    }
    let scalar = b.upload_low(dtype, shape(&[]), &[0x8000])?;
    for op in REDUCTIONS {
        exact_f32(
            &b.read_f32(&b.reduce_low_f32(op, &scalar, &[], false)?)?,
            &[-0.],
        );
        assert_eq!(
            b.read_low_bits(&b.reduce_low(op, &scalar, &[], true)?)?,
            [0x8000]
        );
    }
    exact_f32(&b.read_f32(&b.mean_low_f32(&scalar, &[], true)?)?, &[-0.]);
    Ok(())
}

/// Shared f16/bf16 arithmetic and f32-accumulating reduction fixtures. Includes
/// every finite sign/absolute/extrema payload, exact subnormal and signed-zero
/// checks, low rounding boundaries, strided/broadcast layouts, arbitrary axes,
/// empty contracts and hierarchy boundaries. Caller must require a real device.
pub fn check_low_ops_backend<B: TensorLowOpsBackend>(b: &B) -> Result<(), B::Error> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        check_exact_bits(b, dtype)?;
        check_elementwise(b, dtype)?;
        check_reductions(b, dtype)?;
        check_empty_and_errors(b, dtype)?;
    }
    check_bf16_normal_products(b)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_bf16_multiplication_oracle_has_exact_normal_f32_products() {
        let dtype = LowDtype::Bf16;
        let cases = bf16_product_cases();
        assert_eq!(cases.len(), 32);
        for &(a, b) in &cases[..16] {
            let product = f64::from(decode(dtype, a)) * f64::from(decode(dtype, b));
            assert!((product as f32).is_normal());
            assert_eq!(f64::from(product as f32), product);
            assert_ne!(round(dtype, product as f32) & 0x7fff, 0);
        }
        assert_eq!(
            f64::from(decode(dtype, 1)) * f64::from(decode(dtype, 0x7f7f)),
            0.0311279296875
        );
        assert_eq!(
            f64::from(decode(dtype, 0x007f)) * f64::from(decode(dtype, 0x7f7f)),
            3.9532470703125
        );
    }

    #[test]
    fn mathematical_rounding_reference_handles_ties_and_overflow() {
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            for bits in 0..=last_finite(dtype) {
                assert_eq!(round(dtype, decode(dtype, bits)), bits);
                assert_eq!(round(dtype, decode(dtype, bits | 0x8000)), bits | 0x8000);
            }
            let first = decode(dtype, 1);
            assert_eq!(round(dtype, first / 2.), 0);
            assert_eq!(round(dtype, -first / 2.), 0x8000);
            assert_eq!(round(dtype, first * 1.5), 2);
        }
        assert_eq!(round(LowDtype::F16, 65520.), 0x7c00);
        assert_eq!(round(LowDtype::F16, 1. + 1. / 2048.), 0x3c00);
        assert_eq!(round(LowDtype::Bf16, 1. + 1. / 256.), 0x3f80);
        assert_eq!(
            round(
                LowDtype::Bf16,
                decode(LowDtype::Bf16, 0x7f7f) + 2_f32.powi(119)
            ),
            0x7f80
        );
    }
}
