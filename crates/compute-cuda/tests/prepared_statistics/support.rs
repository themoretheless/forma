use super::*;
/// Independent dyadic decode (no backend codec or layout/planner invocation).
pub fn decode(dtype: LowDtype, bits: u16) -> f64 {
    let (fraction_bits, bias) = if dtype == LowDtype::F16 {
        (10, 15)
    } else {
        (7, 127)
    };
    let fraction = u32::from(bits) & ((1 << fraction_bits) - 1);
    let exponent = (u32::from(bits) & 0x7fff) >> fraction_bits;
    let sign = if bits & 0x8000 == 0 { 1. } else { -1. };
    if exponent == 0 {
        sign * f64::from(fraction) * 2f64.powi(1 - bias - fraction_bits)
    } else {
        sign * (1. + f64::from(fraction) / 2f64.powi(fraction_bits))
            * 2f64.powi(exponent as i32 - bias)
    }
}
/// Nearest finite low value, ties to an even low significand. Fixtures do not
/// request overflowing/NaN encodings; binary search uses exact f64 dyadics.
pub fn encode(dtype: LowDtype, value: f32) -> u16 {
    assert!(value.is_finite());
    let sign = if value.is_sign_negative() { 0x8000 } else { 0 };
    let x = f64::from(value.abs());
    let max = if dtype == LowDtype::F16 {
        0x7bff
    } else {
        0x7f7f
    };
    assert!(x <= decode(dtype, max));
    let (mut lo, mut hi) = (0u16, max);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if decode(dtype, mid) < x {
            lo = mid + 1
        } else {
            hi = mid
        }
    }
    if lo == 0 {
        return sign;
    }
    let down = lo - 1;
    let dl = x - decode(dtype, down);
    let du = decode(dtype, lo) - x;
    sign | if dl < du || (dl == du && down & 1 == 0) {
        down
    } else {
        lo
    }
}
pub fn close(actual: &[f32], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        if e.abs() > f64::from(f32::MAX) {
            assert_eq!(a, e as f32);
            continue;
        }
        assert!(a.is_finite(), "nonfinite at {i}: {a} vs {e}");
        let error = (f64::from(a) - e).abs();
        let bound = 4e-5 * e.abs().max(1e-30) + 2e-7;
        assert!(
            error <= bound,
            "at {i}: {a} vs {e}, error {error} bound {bound}"
        );
    }
}
pub fn check_low_round(
    rt: &CudaRuntime,
    dtype: LowDtype,
    wide: &CudaProgramOutput,
    low: &CudaProgramOutput,
) -> Result {
    let actual = rt.read_f32(wide.as_f32()?)?;
    let low = rt.read_low_bits(low.as_low()?)?;
    assert_eq!(
        low,
        actual.iter().map(|&v| encode(dtype, v)).collect::<Vec<_>>()
    );
    Ok(())
}
pub struct Reference {
    pub soft: Vec<f64>,
    pub log: Vec<f64>,
    pub lse: f64,
    pub mean: f64,
    pub variance: f64,
    pub norm: Vec<f64>,
}
pub fn stats(x: &[f64], epsilon: f64) -> Reference {
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    let variance = x.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / x.len() as f64;
    let max = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let weights = x.iter().map(|&v| (v - max).exp()).collect::<Vec<_>>();
    let sum = weights.iter().sum::<f64>();
    Reference {
        soft: weights.iter().map(|&w| w / sum).collect(),
        log: x.iter().map(|&v| (v - max) - sum.ln()).collect(),
        lse: max + sum.ln(),
        mean,
        variance,
        norm: x
            .iter()
            .map(|&v| (v - mean) / (variance + epsilon).sqrt())
            .collect(),
    }
}

#[test]
fn independent_low_codec_known_values_and_even_rounding() {
    for (dtype, one, half_ulp, min) in [
        (LowDtype::F16, 0x3c00u16, 2f32.powi(-11), 2f64.powi(-24)),
        (LowDtype::Bf16, 0x3f80u16, 2f32.powi(-8), 2f64.powi(-133)),
    ] {
        assert_eq!(decode(dtype, one), 1.);
        assert_eq!(decode(dtype, 1), min);
        assert_eq!(encode(dtype, 0.), 0);
        assert_eq!(encode(dtype, -0.), 0x8000);
        assert_eq!(encode(dtype, 1. + half_ulp), one);
        assert_eq!(encode(dtype, 1. + 3. * half_ulp), one + 2);
        assert_eq!(encode(dtype, -1. - half_ulp), one | 0x8000);
        assert_eq!(encode(dtype, -1. - 3. * half_ulp), (one + 2) | 0x8000);
        assert_eq!(encode(dtype, (min / 2.) as f32), 0);
        assert_eq!(encode(dtype, (1.5 * min) as f32), 2);
    }
}
