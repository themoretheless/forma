use super::{
    low_ops::round,
    low_precision::decode,
    scatter::{OPS, destinations, fold_f32},
    shape,
};
use crate::{HasLowDtype, HasShape, LowDtype, ScanOptions, ScatterOp, TensorLowScatterBackend};

fn bits(dtype: LowDtype, values: &[f32]) -> Vec<u16> {
    values.iter().map(|&x| round(dtype, x)).collect()
}

// Raw replacement and extrema use mathematical values with an explicit zero
// tie, independently of the device's ordered-bit key or atomic writer.
fn reference(
    dtype: LowDtype,
    base: &[u16],
    updates: &[u16],
    destinations: &[Option<usize>],
    op: ScatterOp,
) -> (Vec<u16>, Vec<f32>) {
    assert_eq!(updates.len(), destinations.len());
    if matches!(op, ScatterOp::Add | ScatterOp::Multiply) {
        let base: Vec<_> = base.iter().map(|&x| decode(dtype, x)).collect();
        let updates: Vec<_> = updates.iter().map(|&x| decode(dtype, x)).collect();
        let wide = fold_f32(&base, &updates, destinations, op);
        return (bits(dtype, &wide), wide);
    }
    let mut result = base.to_vec();
    for (&destination, &update) in destinations.iter().zip(updates) {
        if let Some(index) = destination {
            let old = decode(dtype, result[index]);
            let value = decode(dtype, update);
            let take = match op {
                ScatterOp::Replace => true,
                ScatterOp::Min => {
                    value < old || (value == 0. && old == 0. && value.is_sign_negative())
                }
                ScatterOp::Max => {
                    value > old || (value == 0. && old == 0. && !value.is_sign_negative())
                }
                _ => unreachable!(),
            };
            if take {
                result[index] = update;
            }
        }
    }
    let wide = result.iter().map(|&x| decode(dtype, x)).collect();
    (result, wide)
}

struct Case<'a, B: TensorLowScatterBackend> {
    input: &'a B::LowTensor,
    base: &'a [u16],
    indices: &'a B::UIntTensor,
    index_values: &'a [u32],
    updates: &'a B::LowTensor,
    expanded: &'a [u16],
    axis: usize,
}

fn check_case<B: TensorLowScatterBackend>(
    b: &B,
    case: &Case<'_, B>,
    ops: &[ScatterOp],
) -> Result<(), B::Error> {
    let dtype = case.input.low_dtype();
    let destinations = destinations(case.input.shape().dims(), case.index_values, case.axis);
    let invalid = case
        .index_values
        .iter()
        .filter(|&&i| i as usize >= case.input.shape().dims()[case.axis])
        .count() as u32;
    for &op in ops {
        let (expected_low, expected_wide) =
            reference(dtype, case.base, case.expanded, &destinations, op);
        let low = b.scatter_low(op, case.input, case.indices, case.updates, case.axis)?;
        let wide = b.scatter_low_f32(op, case.input, case.indices, case.updates, case.axis)?;
        assert_eq!(low.values.shape(), case.input.shape());
        assert_eq!(wide.values.shape(), case.input.shape());
        assert_eq!(low.values.low_dtype(), dtype);
        assert_eq!(low.invalid_count.shape(), &shape(&[]));
        assert_eq!(wide.invalid_count.shape(), &shape(&[]));
        assert_eq!(b.read_u32(&low.invalid_count)?, [invalid]);
        assert_eq!(b.read_u32(&wide.invalid_count)?, [invalid]);
        let actual = b.read_low_bits(&low.values)?;
        assert_eq!(actual.len(), expected_low.len());
        let arithmetic = matches!(op, ScatterOp::Add | ScatterOp::Multiply);
        for (i, (&a, &e)) in actual.iter().zip(&expected_low).enumerate() {
            assert!(
                a == e || (arithmetic && a & 0x7fff == 0 && e & 0x7fff == 0),
                "{dtype:?} {op:?} low[{i}]: {a:#06x} != {e:#06x}"
            );
        }
        let actual = b.read_f32(&wide.values)?;
        assert_eq!(actual.len(), expected_wide.len());
        for (i, (&a, &e)) in actual.iter().zip(&expected_wide).enumerate() {
            // Fixtures use dyadic arithmetic whose result remains exact in
            // f32 for any accumulation order. Raw extrema require exact bits.
            if e.is_nan() {
                assert!(a.is_nan(), "{dtype:?} {op:?} wide[{i}] lost NaN");
            } else if arithmetic && a == 0. && e == 0. {
            } else {
                assert_eq!(
                    a.to_bits(),
                    e.to_bits(),
                    "{dtype:?} {op:?} wide[{i}]: {a:?} != {e:?}"
                );
            }
        }
    }
    assert_eq!(b.read_low_bits(case.input)?, case.base);
    Ok(())
}

fn check_layouts<B: TensorLowScatterBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let original = bits(
        dtype,
        &(0..24)
            .map(|i| (i % 9) as f32 / 2. - 2.)
            .collect::<Vec<_>>(),
    );
    let input = b.upload_low(dtype, shape(&[2, 3, 4]), &original)?;
    let input = b.permute_low(&input, &[2, 0, 1])?;
    let logical: Vec<_> = (0..4)
        .flat_map(|x| (0..2).flat_map(move |y| (0..3).map(move |z| y * 12 + z * 4 + x)))
        .map(|i| original[i])
        .collect();
    let indices = b.upload_u32(shape(&[3, 2]), &[1, 2, 0, u32::MAX, 1, 0])?;
    let indices = b.permute_u32(&indices, &[1, 0])?;
    let index_values = [1, 0, 1, 2, u32::MAX, 0];
    let original_updates = bits(
        dtype,
        &(0..9).map(|i| (i % 5) as f32 / 2. - 1.).collect::<Vec<_>>(),
    );
    let updates = b.upload_low(dtype, shape(&[3, 3]), &original_updates)?;
    let updates = b.permute_low(&updates, &[1, 0])?;
    let expanded: Vec<_> = (0..4)
        .flat_map(|_| (0..6).flat_map(|j| (0..3).map(move |k| k * 3 + j % 3)))
        .map(|i| original_updates[i])
        .collect();
    check_case(
        b,
        &Case {
            input: &input,
            base: &logical,
            indices: &indices,
            index_values: &index_values,
            updates: &updates,
            expanded: &expanded,
            axis: 1,
        },
        &OPS,
    )?;
    assert_eq!(
        b.read_low_bits(&updates)?,
        (0..3)
            .flat_map(|x| (0..3).map(move |y| y * 3 + x))
            .map(|i| original_updates[i])
            .collect::<Vec<_>>()
    );

    let base = bits(dtype, &[2.; 12]);
    let input = b.upload_low(dtype, shape(&[2, 3, 2]), &base)?;
    let updates = b.upload_low(dtype, shape(&[]), &bits(dtype, &[0.5]))?;
    let indices = b.upload_u32(shape(&[4]), &[1, 0, 1, u32::MAX])?;
    for axis in 0..3 {
        let n = 12 / [2, 3, 2][axis] * 4;
        check_case(
            b,
            &Case {
                input: &input,
                base: &base,
                indices: &indices,
                index_values: &[1, 0, 1, u32::MAX],
                updates: &updates,
                expanded: &bits(dtype, &vec![0.5; n]),
                axis,
            },
            &OPS,
        )?;
    }
    for index in [0, 2, u32::MAX] {
        let indices = b.upload_u32(shape(&[]), &[index])?;
        check_case(
            b,
            &Case {
                input: &input,
                base: &base,
                indices: &indices,
                index_values: &[index],
                updates: &updates,
                expanded: &bits(dtype, &[0.5; 6]),
                axis: 0,
            },
            &OPS,
        )?;
    }
    // An aliased input/update source must stay unchanged while the result moves.
    let base = bits(dtype, &[1., 2., 4.]);
    let input = b.upload_low(dtype, shape(&[3]), &base)?;
    let indices = b.upload_u32(shape(&[3]), &[2, 1, 0])?;
    check_case(
        b,
        &Case {
            input: &input,
            base: &base,
            indices: &indices,
            index_values: &[2, 1, 0],
            updates: &input,
            expanded: &base,
            axis: 0,
        },
        &OPS,
    )?;
    Ok(())
}

fn check_raw_and_extrema<B: TensorLowScatterBackend>(
    b: &B,
    dtype: LowDtype,
) -> Result<(), B::Error> {
    let bits: Vec<u16> = (0..=u16::MAX).collect();
    let mut base = bits.clone();
    base.push(0x7fff); // untouched odd tail, including a raw NaN payload
    let input = b.upload_low(dtype, shape(&[65537]), &base)?;
    let updates = b.upload_low(dtype, shape(&[256, 256]), &bits)?;
    let updates = b.permute_low(&updates, &[1, 0])?;
    let indices = b.upload_u32(shape(&[256, 256]), &(0..65536).rev().collect::<Vec<_>>())?;
    let indices = b.permute_u32(&indices, &[1, 0])?;
    let order: Vec<_> = (0..256)
        .flat_map(|x| (0..256).map(move |y| y * 256 + x))
        .collect();
    let expanded: Vec<_> = order.iter().map(|&i| bits[i]).collect();
    let index_values: Vec<_> = order.iter().map(|&i| 65535 - i as u32).collect();
    check_case(
        b,
        &Case {
            input: &input,
            base: &base,
            indices: &indices,
            index_values: &index_values,
            updates: &updates,
            expanded: &expanded,
            axis: 0,
        },
        &[ScatterOp::Replace],
    )?;

    let max = if dtype == LowDtype::F16 {
        0x7bff
    } else {
        0x7f7f
    };
    let base: Vec<_> = (0..=max).chain((0..=max).map(|x| x | 0x8000)).collect();
    let update_bits: Vec<_> = base.iter().map(|&x| x ^ 0x8000).collect();
    let index_values: Vec<_> = (0..base.len() as u32).collect();
    let input = b.upload_low(dtype, shape(&[base.len()]), &base)?;
    let updates = b.upload_low(dtype, shape(&[base.len()]), &update_bits)?;
    let indices = b.upload_u32(shape(&[base.len()]), &index_values)?;
    check_case(
        b,
        &Case {
            input: &input,
            base: &base,
            indices: &indices,
            index_values: &index_values,
            updates: &updates,
            expanded: &update_bits,
            axis: 0,
        },
        &[ScatterOp::Min, ScatterOp::Max],
    )?;

    // Contended tiny extrema and signed-zero ties must survive atomic retries
    // or linked-list traversal without ever entering floating arithmetic.
    let n = 65539;
    let indices = b.upload_u32(shape(&[]), &[1])?;
    let indices = b.broadcast_u32(&indices, shape(&[n]))?;
    for (base, mut raw) in [
        ([1, 0, 0x8000], vec![0x8000; n]),
        ([0x8001, 0x8000, 2], vec![0; n]),
        ([1, 2, 0x8002], vec![1; n]),
    ] {
        if raw[0] == 1 {
            raw[17] = 0x8003;
            raw[n - 1] = 4;
        }
        let input = b.upload_low(dtype, shape(&[3]), &base)?;
        let updates = b.upload_low(dtype, shape(&[n]), &raw)?;
        check_case(
            b,
            &Case {
                input: &input,
                base: &base,
                indices: &indices,
                index_values: &vec![1; n],
                updates: &updates,
                expanded: &raw,
                axis: 0,
            },
            &[ScatterOp::Min, ScatterOp::Max],
        )?;
    }
    // Last logical index wins even when its payload is a signaling NaN. Run
    // repeatedly to expose scheduling-dependent replacement implementations.
    let base = [0x8000, 1, 0xffff];
    let input = b.upload_low(dtype, shape(&[3]), &base)?;
    let mut raw = vec![1; n];
    raw[n - 1] = max + 2;
    let updates = b.upload_low(dtype, shape(&[n]), &raw)?;
    for _ in 0..3 {
        check_case(
            b,
            &Case {
                input: &input,
                base: &base,
                indices: &indices,
                index_values: &vec![1; n],
                updates: &updates,
                expanded: &raw,
                axis: 0,
            },
            &[ScatterOp::Replace],
        )?;
    }
    Ok(())
}

fn check_accumulation<B: TensorLowScatterBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let n = 65539;
    let index_values = vec![2; n];
    let indices = b.upload_u32(shape(&[]), &[2])?;
    let indices = b.broadcast_u32(&indices, shape(&[n]))?;
    let base = bits(dtype, &[8., 9., 2., 11.]);
    let input = b.upload_low(dtype, shape(&[4]), &base)?;
    let mut values = vec![1.; n];
    values[n - 1] = 3.;
    let expanded = bits(dtype, &values);
    let updates = b.upload_low(dtype, shape(&[n]), &expanded)?;
    check_case(
        b,
        &Case {
            input: &input,
            base: &base,
            indices: &indices,
            index_values: &index_values,
            updates: &updates,
            expanded: &expanded,
            axis: 0,
        },
        &OPS,
    )?;

    let (big, increment, mul_base, mul_update, largest, overflow_increment) = match dtype {
        LowDtype::F16 => (2048., 1. / 2048., 0x3c01, 0x3e00, 0x7bff, 16.),
        LowDtype::Bf16 => (256., 1. / 256., 0x3f81, 0x3fc0, 0x7f7f, 2_f32.powi(119)),
    };
    let mut cancellation = vec![1.; 258];
    cancellation[257] = -big;
    for (op, base, updates) in [
        (
            ScatterOp::Add,
            bits(dtype, &[big]),
            bits(dtype, &cancellation),
        ),
        (
            ScatterOp::Add,
            bits(dtype, &[1.]),
            bits(dtype, &[increment]),
        ),
        (ScatterOp::Multiply, vec![mul_base], vec![mul_update]),
        (
            ScatterOp::Add,
            vec![largest],
            bits(dtype, &[overflow_increment]),
        ),
    ] {
        let input = b.upload_low(dtype, shape(&[1]), &base)?;
        let update = b.upload_low(dtype, shape(&[updates.len()]), &updates)?;
        let index_values = vec![0; updates.len()];
        let indices = b.upload_u32(shape(&[updates.len()]), &index_values)?;
        check_case(
            b,
            &Case {
                input: &input,
                base: &base,
                indices: &indices,
                index_values: &index_values,
                updates: &update,
                expanded: &updates,
                axis: 0,
            },
            &[op],
        )?;
    }
    Ok(())
}

fn check_empty_errors_and_chain<B: TensorLowScatterBackend>(
    b: &B,
    dtype: LowDtype,
) -> Result<(), B::Error> {
    let update_bits = bits(dtype, &[0.5]);
    let update = b.upload_low(dtype, shape(&[]), &update_bits)?;
    for (dims, index_values) in [
        (vec![0, 3, 2], vec![0, 3, u32::MAX]),
        (vec![2, 0, 3], vec![0, 1, u32::MAX]),
        (vec![2, 3, 0], vec![2, 3, u32::MAX]),
        (vec![2, 3, 2], vec![]),
        (vec![2, 3, 2], vec![3, 99, u32::MAX]),
    ] {
        let base = bits(dtype, &vec![7.; dims.iter().product()]);
        let input = b.upload_low(dtype, shape(&dims), &base)?;
        let indices = b.upload_u32(shape(&[index_values.len()]), &index_values)?;
        let expanded = vec![update_bits[0]; dims[0] * index_values.len() * dims[2]];
        check_case(
            b,
            &Case {
                input: &input,
                base: &base,
                indices: &indices,
                index_values: &index_values,
                updates: &update,
                expanded: &expanded,
                axis: 1,
            },
            &OPS,
        )?;
        let other = b.upload_low(
            if dtype == LowDtype::F16 {
                LowDtype::Bf16
            } else {
                LowDtype::F16
            },
            shape(&[]),
            &[0],
        )?;
        for op in OPS {
            assert!(b.scatter_low(op, &input, &indices, &other, 1).is_err());
            assert!(b.scatter_low_f32(op, &input, &indices, &other, 1).is_err());
        }
    }
    let input = b.upload_low(dtype, shape(&[4]), &bits(dtype, &[8., 9., 2., 11.]))?;
    let indices = b.upload_u32(shape(&[5]), &[2, 0, 2, 99, 1])?;
    let update = b.upload_low(dtype, shape(&[5]), &bits(dtype, &[1., 2., 3., 99., 4.]))?;
    let scattered = b.scatter_low(ScatterOp::Add, &input, &indices, &update, 0)?;
    let gathered = b.gather_low(&scattered.values, &indices, 0)?;
    let scanned = b.scan_low_f32(&gathered.values, 0, ScanOptions::default())?;
    assert_eq!(b.read_f32(&scanned)?, [6., 16., 22., 22., 35.]);
    assert_eq!(b.read_u32(&scattered.invalid_count)?, [1]);
    assert_eq!(b.read_u32(&gathered.invalid_count)?, [1]);
    let bad = b.upload_low(dtype, shape(&[2, 5]), &bits(dtype, &[1.; 10]))?;
    let scalar = b.upload_low(dtype, shape(&[]), &[0])?;
    for op in OPS {
        assert!(b.scatter_low(op, &input, &indices, &bad, 0).is_err());
        assert!(b.scatter_low_f32(op, &input, &indices, &bad, 0).is_err());
        assert!(b.scatter_low(op, &input, &indices, &update, 1).is_err());
        assert!(
            b.scatter_low_f32(op, &scalar, &indices, &update, 0)
                .is_err()
        );
    }
    Ok(())
}

/// Shared low scatter contracts, independent raw/f64 reference, exact extrema,
/// complete finite sign pairs, all raw Replace payloads, strided/broadcast
/// layouts, contended duplicates, final rounding and resident composition.
/// The caller must enforce physical device availability.
pub fn check_low_scatter_backend<B: TensorLowScatterBackend>(b: &B) -> Result<(), B::Error> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        check_layouts(b, dtype)?;
        check_raw_and_extrema(b, dtype)?;
        check_accumulation(b, dtype)?;
        check_empty_errors_and_chain(b, dtype)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_scatter_reference_keeps_payloads_and_zero_ties() {
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            let targets = [Some(0), Some(0), None];
            assert_eq!(
                reference(
                    dtype,
                    &[0x7fff],
                    &[1, 0xffff, 0],
                    &targets,
                    ScatterOp::Replace
                )
                .0,
                [0xffff]
            );
            assert_eq!(
                reference(dtype, &[0], &[0x8000, 0, 1], &targets, ScatterOp::Min).0,
                [0x8000]
            );
            assert_eq!(
                reference(dtype, &[0x8000], &[0, 0x8000, 1], &targets, ScatterOp::Max).0,
                [0]
            );
            assert_eq!(
                reference(dtype, &[1], &[0x8001, 2, 0], &targets, ScatterOp::Min).0,
                [0x8001]
            );
        }
    }
}
