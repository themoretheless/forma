//! Inexact f32 accumulation, with an order-independent forward-error interval.
//!
//! Unlike the exact dyadic fixtures, these use every low mantissa bit and varied
//! normal exponents. The portable assumption is single-precision accumulation,
//! not a fixed order or round-to-nearest at every addition. NVIDIA's PTX 12.8
//! MMA specification leaves accumulation order, rounding and subnormal handling
//! unspecified. Use u=2^-23 (also covering directed rounding), and keep the
//! BF16-subnormal times large-value probes in native backend qualification:
//! https://docs.nvidia.com/cuda/archive/12.8.0/parallel-thread-execution/index.html#warp-level-matrix-instructions-mma
use super::*;

fn input(dtype: LowDtype, dims: &[usize], seed: usize) -> Host<f32> {
    let (fraction, bias) = match dtype {
        LowDtype::F16 => (10, 15),
        LowDtype::Bf16 => (7, 127),
    };
    let shape = shape(dims);
    Host {
        values: (0..shape.numel())
            .map(|i| {
                let exponent = ((i * 7 + i / 11 + seed * 3) % 13) as i32 - 10;
                let mantissa = (i * 73 + i / 3 + seed * 29) & ((1 << fraction) - 1);
                let sign = if (i * 13 + i / 5 + seed).is_multiple_of(3) {
                    0x8000
                } else {
                    0
                };
                decode(
                    dtype,
                    sign | (((exponent + bias) as u16) << fraction) | mantissa as u16,
                )
            })
            .collect(),
        shape,
        permutation: Some(vec![1, 0]),
    }
}

fn gamma(steps: usize, unit: f64) -> f64 {
    let error = steps as f64 * unit;
    error / (1.0 - error)
}

fn interval(reference: f64, absolute_sum: f64, k: usize) -> (f64, f64) {
    // Low inputs have at most 11 significant bits and unbiased exponents
    // [-10,2]. Every product is exact and normal in f32; all possible partial
    // sums remain finite. A sum of K terms, in any parenthesization, has at
    // most K-1 rounded additions on a contributing term's path.
    let g32 = gamma(k - 1, 2_f64.powi(-23));
    let g64 = gamma(k - 1, 2_f64.powi(-53));
    // The f64 dot and absolute sum are independent approximations. Account for
    // both, and leave room for the bound/endpoint arithmetic itself. This is
    // insignificant compared with g32, but avoids treating f64 as exact.
    let error = (g32 + 4.0 * g64) * absolute_sum / (1.0 - g64);
    (reference - error, reference + error)
}

fn low_interval(dtype: LowDtype, bounds: (f64, f64)) -> (f64, f64) {
    let mut lower = bounds.0 as f32;
    let mut upper = bounds.1 as f32;
    // Outward conversion prevents an intermediate f64->f32 rounding from
    // excluding a valid final low value at a nearest-even midpoint.
    if f64::from(lower) > bounds.0 {
        lower = lower.next_down();
    }
    if f64::from(upper) < bounds.1 {
        upper = upper.next_up();
    }
    (
        f64::from(decode(dtype, round(dtype, lower))),
        f64::from(decode(dtype, round(dtype, upper))),
    )
}

fn expected(left: (&[usize], &[f64]), right: (&[usize], &[f64])) -> (Vec<f64>, Vec<f64>) {
    let (_, values) = reference(left, right);
    let a: Vec<_> = left.1.iter().map(|x| x.abs()).collect();
    let b: Vec<_> = right.1.iter().map(|x| x.abs()).collect();
    let (_, absolute) = reference((left.0, &a), (right.0, &b));
    (values, absolute)
}

pub(super) fn check<B: TensorLowBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    for k in [65, 257] {
        let left = prepare(b, dtype, input(dtype, &[k, 17], 17), None)?;
        let right = prepare(b, dtype, input(dtype, &[19, k], 29), None)?;
        let (values, absolute) = expected((&left.dims, &left.values), (&right.dims, &right.values));
        let bounds: Vec<_> = values
            .iter()
            .zip(&absolute)
            .map(|(&value, &absolute)| interval(value, absolute, k))
            .collect();
        let support = b.low_precision_support(dtype);
        if support.matmul_f32 {
            let output = b.matmul_low_f32(&left.tensor, &right.tensor)?;
            assert_eq!(output.shape(), &shape(&[17, 19]));
            let actual = b.read_f32(&output)?;
            assert_eq!(actual.len(), values.len());
            for (i, (&actual, &(lo, hi))) in actual.iter().zip(&bounds).enumerate() {
                assert!(
                    actual.is_finite() && f64::from(actual) >= lo && f64::from(actual) <= hi,
                    "{dtype:?} K={k} f32 accumulation [{i}]: {actual} outside [{lo}, {hi}]"
                );
            }
        }
        if support.matmul {
            let output = b.matmul_low(&left.tensor, &right.tensor)?;
            assert_eq!(output.shape(), &shape(&[17, 19]));
            assert_eq!(output.low_dtype(), dtype);
            let actual = b.read_low_bits(&output)?;
            assert_eq!(actual.len(), values.len());
            for (i, (&bits, &bounds)) in actual.iter().zip(&bounds).enumerate() {
                let actual = f64::from(decode(dtype, bits));
                let (lo, hi) = low_interval(dtype, bounds);
                assert!(
                    actual.is_finite() && actual >= lo && actual <= hi,
                    "{dtype:?} K={k} rounded accumulation [{i}]: {actual} outside [{lo}, {hi}]"
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_accepts_f32_orders_but_rejects_widened_low_results() {
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            for k in [65, 257] {
                let (ad, av) = logical(&input(dtype, &[k, 17], 17), dtype);
                let (bd, bv) = logical(&input(dtype, &[19, k], 29), dtype);
                let (values, absolute) = expected((&ad, &av), (&bd, &bv));
                let mut inexact = 0;
                let mut low_rejected = 0;
                for row in 0..17 {
                    for column in 0..19 {
                        let index = row * 19 + column;
                        let products: Vec<_> = (0..k)
                            .map(|inner| av[row * k + inner] * bv[inner * 19 + column])
                            .collect();
                        assert!(products.iter().all(|&p| f64::from(p as f32) == p));
                        let forward = products.iter().fold(0f32, |sum, &p| sum + p as f32);
                        let reverse = products.iter().rev().fold(0f32, |sum, &p| sum + p as f32);
                        let mut paired: Vec<_> = products.iter().map(|&p| p as f32).collect();
                        while paired.len() > 1 {
                            paired = paired
                                .chunks(2)
                                .map(|pair| pair.iter().copied().sum())
                                .collect();
                        }
                        let (lo, hi) = interval(values[index], absolute[index], k);
                        for result in [forward, reverse, paired[0]] {
                            assert!(f64::from(result) >= lo && f64::from(result) <= hi);
                            let result = f64::from(decode(dtype, round(dtype, result)));
                            let (lo, hi) = low_interval(dtype, (lo, hi));
                            assert!(result >= lo && result <= hi);
                        }
                        inexact += usize::from(f64::from(forward) != values[index]);
                        let low = f64::from(decode(dtype, round(dtype, values[index] as f32)));
                        low_rejected += usize::from(low < lo || low > hi);
                    }
                }
                assert!(inexact > 0, "fixture must exercise rounded f32 additions");
                assert!(low_rejected > 0, "fixture must reject widened low results");
            }
        }
    }
}
