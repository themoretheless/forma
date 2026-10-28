use super::{
    attention::{Host, Mask, data, reference},
    low_ops::round,
    low_precision::decode,
    shape,
};
use crate::{
    AttentionMask, AttentionOptions, HasLowDtype, HasShape, LowDtype, TensorLowAttentionBackend,
};

fn encode(dtype: LowDtype, host: Host<f32>) -> Host<u16> {
    Host {
        shape: host.shape,
        values: host.values.iter().map(|&x| round(dtype, x)).collect(),
        permutation: host.permutation,
    }
}
fn low_data(dtype: LowDtype, dims: &[usize], seed: usize) -> Host<u16> {
    encode(dtype, data(dims, seed))
}
fn raw(dims: &[usize], values: Vec<u16>) -> Host<u16> {
    let shape = shape(dims);
    assert_eq!(shape.numel(), values.len());
    Host {
        shape,
        values,
        permutation: None,
    }
}
fn decoded(dtype: LowDtype, values: &[u16]) -> Vec<f32> {
    values.iter().map(|&x| decode(dtype, x)).collect()
}
fn upload<B: TensorLowAttentionBackend>(
    b: &B,
    dtype: LowDtype,
    host: &Host<u16>,
) -> Result<B::LowTensor, B::Error> {
    let input = b.upload_low(dtype, host.shape.clone(), &host.values)?;
    match &host.permutation {
        Some(axes) => b.permute_low(&input, axes),
        None => Ok(input),
    }
}
fn close(actual: &[f32], expected: &[f64], relative: bool) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        let floor = if relative {
            f64::from(f32::MIN_POSITIVE)
        } else {
            1.0
        };
        assert!(
            a.is_finite() && (f64::from(a) - e).abs() <= 3e-4 * e.abs().max(floor),
            "low attention [{i}]: {a} != {e} (relative={relative})"
        );
    }
}

// The reference consumes independently decoded, logically reordered inputs.
// It uses dense f64 weights, not the executor's online recurrence or planner.
#[allow(clippy::too_many_arguments)]
fn check<B: TensorLowAttentionBackend>(
    b: &B,
    dtype: LowDtype,
    q: Host<u16>,
    k: Host<u16>,
    v: Host<u16>,
    mask: Mask,
    options: AttentionOptions,
    relative: bool,
) -> Result<Vec<f32>, B::Error> {
    let (q_shape, q_raw) = q.logical();
    let (k_shape, k_raw) = k.logical();
    let (v_shape, v_raw) = v.logical();
    let query = upload(b, dtype, &q)?;
    let key = upload(b, dtype, &k)?;
    let value = upload(b, dtype, &v)?;
    let (bias_shape, bias_values, keep, additive) = match &mask {
        Mask::None => (None, Vec::new(), None, None),
        Mask::Additive(host) => {
            let input = b.upload_f32(host.shape.clone(), &host.values)?;
            let input = match &host.permutation {
                Some(axes) => b.permute(&input, axes)?,
                None => input,
            };
            let (shape, values) = host.logical();
            (Some(shape), values, None, Some(input))
        }
        Mask::Keep(host) => {
            let input = b.upload_u32(host.shape.clone(), &host.values)?;
            let input = match &host.permutation {
                Some(axes) => b.permute_u32(&input, axes)?,
                None => input,
            };
            let (shape, values) = host.logical();
            (
                Some(shape),
                values
                    .iter()
                    .map(|&x| if x == 0 { f32::NEG_INFINITY } else { 0.0 })
                    .collect(),
                Some(input),
                None,
            )
        }
    };
    let make_mask = || match (&keep, &additive) {
        (Some(input), _) => AttentionMask::Keep(input),
        (_, Some(input)) => AttentionMask::Additive(input),
        _ => AttentionMask::None,
    };
    let wide = b.attention_low_f32(&query, &key, &value, make_mask(), options)?;
    let narrow = b.attention_low(&query, &key, &value, make_mask(), options)?;
    let (expected_shape, expected) = reference(
        (&q_shape, &decoded(dtype, &q_raw)),
        (&k_shape, &decoded(dtype, &k_raw)),
        (&v_shape, &decoded(dtype, &v_raw)),
        bias_shape.as_ref().map(|s| (s, bias_values.as_slice())),
        options,
    );
    assert_eq!(wide.shape(), &expected_shape);
    assert_eq!(narrow.shape(), &expected_shape);
    assert_eq!(narrow.low_dtype(), dtype);
    let actual = b.read_f32(&wide)?;
    close(&actual, &expected, relative);
    let expected_bits: Vec<u16> = actual.iter().map(|&x| round(dtype, x)).collect();
    assert_eq!(
        b.read_low_bits(&narrow)?,
        expected_bits,
        "{dtype:?} final attention rounding"
    );
    assert_eq!(b.read_low_bits(&query)?, q_raw);
    assert_eq!(b.read_low_bits(&key)?, k_raw);
    assert_eq!(b.read_low_bits(&value)?, v_raw);
    match (&mask, &keep, &additive) {
        (Mask::Keep(host), Some(input), _) => assert_eq!(b.read_u32(input)?, host.logical().1),
        (Mask::Additive(host), _, Some(input)) => assert_eq!(
            b.read_f32(input)?
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>(),
            host.logical()
                .1
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>()
        ),
        _ => {}
    }
    Ok(actual)
}

fn layouts_and_masks<B: TensorLowAttentionBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let options = AttentionOptions::default();
    for (m, n, d, dv) in [
        (3, 5, 7, 9),
        (33, 65, 17, 67),
        (2, 257, 65, 3),
        (1, 4097, 3, 5),
    ] {
        check(
            b,
            dtype,
            low_data(dtype, &[m, d], 1),
            low_data(dtype, &[n, d], 2),
            low_data(dtype, &[n, dv], 3),
            Mask::None,
            options,
            false,
        )?;
    }
    check(
        b,
        dtype,
        low_data(dtype, &[2, 1, 6, 3, 5], 1),
        low_data(dtype, &[1, 3, 2, 7, 5], 2),
        low_data(dtype, &[3, 2, 7, 9], 3),
        Mask::None,
        options,
        false,
    )?;
    let mut q = low_data(dtype, &[2, 5, 4, 3], 1);
    q.permutation = Some(vec![0, 2, 3, 1]);
    let mut k = low_data(dtype, &[1, 7, 2, 5], 2);
    k.permutation = Some(vec![0, 2, 1, 3]);
    let mut v = low_data(dtype, &[2, 9, 7], 3);
    v.permutation = Some(vec![0, 2, 1]);
    let keep = Host {
        shape: shape(&[7, 3]),
        values: (0..21)
            .map(|i| {
                if i % 3 == 0 || i % 5 == 0 {
                    0
                } else {
                    u32::MAX
                }
            })
            .collect(),
        permutation: Some(vec![1, 0]),
    };
    check(b, dtype, q, k, v, Mask::Keep(keep), options, false)?;
    for causal in [
        None,
        Some(0),
        Some(4),
        Some(-2),
        Some(i32::MIN),
        Some(i32::MAX),
    ] {
        let actual = check(
            b,
            dtype,
            low_data(dtype, &[2, 4, 3, 5], 1),
            low_data(dtype, &[1, 2, 7, 5], 2),
            low_data(dtype, &[1, 2, 7, 4], 3),
            Mask::None,
            AttentionOptions {
                causal,
                scale: None,
            },
            false,
        )?;
        if causal == Some(i32::MIN) {
            assert!(actual.iter().all(|&x| x == 0.0));
        }
    }
    for mode in 0..3 {
        let mask = match mode {
            0 => Mask::Keep(Host {
                shape: shape(&[]),
                values: vec![0],
                permutation: None,
            }),
            1 => Mask::Additive(Host {
                shape: shape(&[7, 3]),
                values: (0..21)
                    .map(|i| {
                        if i % 3 == 1 || i % 4 == 0 {
                            f32::NEG_INFINITY
                        } else {
                            (i % 5) as f32 - 2.0
                        }
                    })
                    .collect(),
                permutation: Some(vec![1, 0]),
            }),
            _ => Mask::Keep(Host {
                shape: shape(&[7]),
                values: vec![0, 1, u32::MAX, 0, 7, 0, 1],
                permutation: None,
            }),
        };
        let actual = check(
            b,
            dtype,
            low_data(dtype, &[4, 3, 5], 1),
            low_data(dtype, &[2, 7, 5], 2),
            low_data(dtype, &[2, 7, 3], 3),
            mask,
            AttentionOptions {
                scale: Some(-0.5),
                causal: Some(1),
            },
            false,
        )?;
        if mode == 0 {
            assert!(actual.iter().all(|&x| x == 0.0));
        }
    }
    // Bias must remain f32: these distinct finite values merge or overflow if
    // cast to the input dtype. A -Inf key is excluded before arithmetic.
    check(
        b,
        dtype,
        raw(&[2, 1], vec![0; 2]),
        raw(&[4, 1], vec![0; 4]),
        encode(
            dtype,
            Host {
                shape: shape(&[4, 1]),
                values: vec![1., -1., 100., 2.],
                permutation: None,
            },
        ),
        Mask::Additive(Host {
            shape: shape(&[4]),
            values: vec![65536., 65537., f32::NEG_INFINITY, 65538.],
            permutation: None,
        }),
        options,
        false,
    )?;
    // Zero strides in Q/K/V and mask are actual resident broadcast views.
    let q = b.upload_low(dtype, shape(&[1, 1]), &[round(dtype, 1.)])?;
    let k = b.broadcast_low(&q, shape(&[2, 7, 1]))?;
    let q = b.broadcast_low(&q, shape(&[4, 3, 1]))?;
    let v = b.upload_low(
        dtype,
        shape(&[2, 1, 1]),
        &[round(dtype, 2.), round(dtype, -3.)],
    )?;
    let v = b.broadcast_low(&v, shape(&[2, 7, 5]))?;
    let mask = b.upload_u32(shape(&[]), &[u32::MAX])?;
    let mask = b.broadcast_u32(&mask, shape(&[4, 3, 7]))?;
    let output = b.attention_low_f32(&q, &k, &v, AttentionMask::Keep(&mask), options)?;
    assert_eq!(output.shape(), &shape(&[4, 3, 5]));
    let expected: Vec<f64> = (0..60).map(|i| if i / 15 < 2 { 2. } else { -3. }).collect();
    close(&b.read_f32(&output)?, &expected, true);
    Ok(())
}

fn extremes<B: TensorLowAttentionBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let options = AttentionOptions::default();
    let last = if dtype == LowDtype::F16 {
        0x7bff
    } else {
        0x7f7f
    };
    for (m, n, dv) in [(1, 65, 3), (17, 65, 67), (1, 4097, 3)] {
        check(
            b,
            dtype,
            raw(&[m, 1], vec![0; m]),
            raw(&[n, 1], vec![0; n]),
            raw(
                &[n, dv],
                (0..n * dv)
                    .map(|i| if i % dv == 1 { last | 0x8000 } else { last })
                    .collect(),
            ),
            Mask::None,
            options,
            true,
        )?;
    }
    // A discarded early large-valued tile must not erase later ordinary V.
    for count in [65, 4097] {
        let k = (0..count)
            .map(|i| round(dtype, if i < 32 { 0. } else { 200. }))
            .collect();
        let v = (0..count)
            .map(|i| if i < 32 { last } else { round(dtype, 1.) })
            .collect();
        check(
            b,
            dtype,
            raw(&[1, 1], vec![round(dtype, 1.)]),
            raw(&[count, 1], k),
            raw(&[count, 1], v),
            Mask::None,
            options,
            true,
        )?;
    }
    for scale in [Some(0.), Some(-2.), None] {
        check(
            b,
            dtype,
            encode(
                dtype,
                Host {
                    shape: shape(&[3, 1]),
                    values: vec![1., -1., 0.],
                    permutation: None,
                },
            ),
            raw(
                &[65, 1],
                (0..65)
                    .map(|i| round(dtype, 10000. + (i % 7) as f32 * 32.))
                    .collect(),
            ),
            low_data(dtype, &[65, 5], 3),
            Mask::None,
            AttentionOptions {
                scale,
                causal: None,
            },
            false,
        )?;
    }
    // Low output is a final cast of a true f32 result, not a widened native
    // low attention result. Uniform thirds retain additional f32 bits.
    let actual = check(
        b,
        dtype,
        raw(&[2, 1], vec![0; 2]),
        raw(&[3, 1], vec![0; 3]),
        raw(&[3, 1], vec![round(dtype, 1.), 0, 0]),
        Mask::None,
        options,
        true,
    )?;
    assert!((actual[0] - 1. / 3.).abs() < 1e-6);
    assert_ne!(actual[0], decode(dtype, round(dtype, actual[0])));
    // A small but normal positive weighted result needs a relative bound.
    let tiny = if dtype == LowDtype::F16 { 1u16 } else { 0x0080 };
    check(
        b,
        dtype,
        raw(&[1, 1], vec![0]),
        raw(&[65, 1], vec![0; 65]),
        raw(&[65, 1], vec![tiny; 65]),
        Mask::None,
        options,
        true,
    )?;
    Ok(())
}

fn tiny_products<B: TensorLowAttentionBackend>(b: &B) -> Result<(), B::Error> {
    let dtype = LowDtype::Bf16;
    // Normal f32 products must survive BF16 input-subnormal flushing. Both
    // operand orders are tested, with signs and the subnormal/normal boundary.
    for small in [1, 0x007f, 0x0080] {
        for reverse in [false, true] {
            let q = if reverse { 0x7f7f } else { small };
            let k = if reverse { small } else { 0x7f7f };
            check(
                b,
                dtype,
                raw(&[2, 1], vec![q, q | 0x8000]),
                raw(&[3, 1], vec![k, k | 0x8000, 0]),
                raw(&[3, 1], vec![round(dtype, 1.), round(dtype, -1.), 0]),
                Mask::None,
                AttentionOptions::default(),
                true,
            )?;
        }
    }
    Ok(())
}

fn empty_and_errors<B: TensorLowAttentionBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let options = AttentionOptions::default();
    for (m, n, dv) in [(0, 3, 5), (3, 0, 5), (3, 5, 0)] {
        let actual = check(
            b,
            dtype,
            low_data(dtype, &[m, 4], 1),
            low_data(dtype, &[n, 4], 2),
            low_data(dtype, &[n, dv], 3),
            Mask::None,
            options,
            false,
        )?;
        assert!(actual.iter().all(|&x| x == 0.));
    }
    check(
        b,
        dtype,
        low_data(dtype, &[0, 4, 3, 5], 1),
        low_data(dtype, &[1, 2, 7, 5], 2),
        low_data(dtype, &[2, 7, 3], 3),
        Mask::None,
        options,
        false,
    )?;
    let other = if dtype == LowDtype::F16 {
        LowDtype::Bf16
    } else {
        LowDtype::F16
    };
    for count in [0, 2] {
        let q = b.upload_low(
            dtype,
            shape(&[count, 3]),
            &vec![round(dtype, 1.); count * 3],
        )?;
        let k = b.upload_low(dtype, shape(&[4, 3]), &[round(dtype, 1.); 12])?;
        let v = b.upload_low(dtype, shape(&[4, 5]), &[round(dtype, 1.); 20])?;
        let wrong_k = b.upload_low(other, shape(&[4, 3]), &[round(other, 1.); 12])?;
        let wrong_v = b.upload_low(other, shape(&[4, 5]), &[round(other, 1.); 20])?;
        for (key, value) in [(&wrong_k, &v), (&k, &wrong_v)] {
            assert!(
                b.attention_low_f32(&q, key, value, AttentionMask::None, options)
                    .is_err()
            );
            assert!(
                b.attention_low(&q, key, value, AttentionMask::None, options)
                    .is_err()
            );
        }
        for scale in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let options = AttentionOptions {
                scale: Some(scale),
                causal: None,
            };
            assert!(
                b.attention_low_f32(&q, &k, &v, AttentionMask::None, options)
                    .is_err()
            );
            assert!(
                b.attention_low(&q, &k, &v, AttentionMask::None, options)
                    .is_err()
            );
        }
        let bad_mask = b.upload_u32(shape(&[2, count, 4]), &vec![1; 2 * count * 4])?;
        assert!(
            b.attention_low_f32(&q, &k, &v, AttentionMask::Keep(&bad_mask), options)
                .is_err()
        );
        assert!(
            b.attention_low(&q, &k, &v, AttentionMask::Keep(&bad_mask), options)
                .is_err()
        );
        let scalar = b.upload_low(dtype, shape(&[]), &[0])?;
        assert!(
            b.attention_low_f32(&scalar, &k, &v, AttentionMask::None, options)
                .is_err()
        );
        assert!(
            b.attention_low(&scalar, &k, &v, AttentionMask::None, options)
                .is_err()
        );
    }
    Ok(())
}

fn chain<B: TensorLowAttentionBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let q = b.upload_low(dtype, shape(&[2, 1]), &[0; 2])?;
    let k = b.upload_low(dtype, shape(&[3, 1]), &[0; 3])?;
    let v = b.upload_low(
        dtype,
        shape(&[3, 2]),
        &[round(dtype, 1.), 0, 0, round(dtype, 1.), 0, 0],
    )?;
    let intermediate =
        b.attention_low(&q, &k, &v, AttentionMask::None, AttentionOptions::default())?;
    let k = b.upload_low(
        dtype,
        shape(&[2, 2]),
        &[round(dtype, 1.), 0, 0, round(dtype, 2.)],
    )?;
    let v = b.upload_low(
        dtype,
        shape(&[2, 1]),
        &[round(dtype, 1.), round(dtype, -1.)],
    )?;
    let result = b.attention_low_f32(
        &intermediate,
        &k,
        &v,
        AttentionMask::None,
        AttentionOptions::default(),
    )?;
    assert_eq!(result.shape(), &shape(&[2, 1]));
    let third = decode(dtype, round(dtype, 1. / 3.));
    let (_, expected) = reference(
        (&shape(&[2, 2]), &[third; 4]),
        (&shape(&[2, 2]), &[1., 0., 0., 2.]),
        (&shape(&[2, 1]), &[1., -1.]),
        None,
        AttentionOptions::default(),
    );
    close(&b.read_f32(&result)?, &expected, true);
    Ok(())
}

/// Direct low Q/K/V attention against independent dense f64 evaluation, exact
/// final low rounding, numerical extremes and resident composition. The caller
/// separately requires an actual backend device to establish native execution.
pub fn check_low_attention_backend<B: TensorLowAttentionBackend>(b: &B) -> Result<(), B::Error> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        layouts_and_masks(b, dtype)?;
        extremes(b, dtype)?;
        empty_and_errors(b, dtype)?;
        chain(b, dtype)?;
    }
    tiny_products(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subnormal_operand_reference_requires_a_normal_nonzero_attention_result() {
        let q = shape(&[1, 1]);
        let k = shape(&[3, 1]);
        let tiny = decode(LowDtype::Bf16, 1);
        let large = decode(LowDtype::Bf16, 0x7f7f);
        let (_, expected) = reference(
            (&q, &[tiny]),
            (&k, &[large, -large, 0.]),
            (&k, &[1., -1., 0.]),
            None,
            AttentionOptions::default(),
        );
        assert!(expected[0] > 0.02 && expected[0] < 0.03);
        assert!((expected[0] as f32).is_normal());
    }
}
