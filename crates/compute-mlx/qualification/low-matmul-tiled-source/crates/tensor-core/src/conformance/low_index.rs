use super::{
    indexing::{COMPARISONS, comparison, scan_reference},
    low_ops::round,
    low_precision::decode,
    shape,
};
use crate::{CompareOp, HasLowDtype, HasShape, LowDtype, ScanOptions, TensorLowIndexBackend};

fn check_comparisons<B: TensorLowIndexBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let bits: Vec<u16> = (0..=u16::MAX).collect();
    let input = b.upload_low(dtype, shape(&[65536]), &bits)?;
    // Every payload appears in comparisons with itself, opposite sign and an
    // unrelated payload. Host IEEE comparisons form the oracle; no bit-order
    // key or device classification algorithm is repeated here.
    for variant in 0..3 {
        let other: Vec<u16> = bits
            .iter()
            .map(|&x| match variant {
                0 => x,
                1 => x ^ 0x8000,
                _ => x.wrapping_mul(73).wrapping_add(137),
            })
            .collect();
        let rhs = b.upload_low(dtype, shape(&[65536]), &other)?;
        for op in COMPARISONS {
            let expected: Vec<u32> = bits
                .iter()
                .zip(&other)
                .map(|(&a, &c)| comparison(op, decode(dtype, a), decode(dtype, c)))
                .collect();
            let result = b.compare_low(op, &input, &rhs)?;
            assert_eq!(result.shape(), &shape(&[65536]));
            assert_eq!(
                b.read_u32(&result)?,
                expected,
                "{dtype:?} {op:?} pattern {variant}"
            );
        }
    }
    let inf = if dtype == LowDtype::F16 {
        0x7c00
    } else {
        0x7f80
    };
    let edges = [
        0,
        0x8000,
        1,
        0x8001,
        2,
        0x8002,
        inf - 1,
        (inf - 1) | 0x8000,
        inf,
        inf | 0x8000,
        inf + 1,
        (inf + 1) | 0x8000,
        0x7fff,
        0xffff,
    ];
    let left = b.upload_low(dtype, shape(&[1, edges.len()]), &edges)?;
    let left = b.permute_low(&left, &[1, 0])?;
    let right = b.upload_low(dtype, shape(&[edges.len()]), &edges)?;
    for op in COMPARISONS {
        let expected: Vec<_> = edges
            .iter()
            .flat_map(|&a| {
                edges
                    .iter()
                    .map(move |&c| comparison(op, decode(dtype, a), decode(dtype, c)))
            })
            .collect();
        assert_eq!(b.read_u32(&b.compare_low(op, &left, &right)?)?, expected);
    }
    assert_eq!(b.read_low_bits(&input)?, bits);
    Ok(())
}

fn check_raw_routing<B: TensorLowIndexBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let bits: Vec<u16> = (0..=u16::MAX).collect();
    let input = b.upload_low(dtype, shape(&[256, 256]), &bits)?;
    let transposed = b.permute_low(&input, &[1, 0])?;
    let logical: Vec<u16> = (0..256)
        .flat_map(|row| (0..256).map(move |col| (col * 256 + row) as u16))
        .collect();
    let other_bits: Vec<_> = bits.iter().rev().copied().collect();
    let other = b.upload_low(dtype, shape(&[256, 256]), &other_bits)?;
    let mask_bits: Vec<_> = (0..65536)
        .map(|i| [0, 1, 0x80000000, u32::MAX, 0, 16_777_217][i % 6])
        .collect();
    let mask = b.upload_u32(shape(&[256, 256]), &mask_bits)?;
    let mask = b.permute_u32(&mask, &[1, 0])?;
    let expected: Vec<_> = logical
        .iter()
        .enumerate()
        .map(|(i, &x)| {
            if mask_bits[(i % 256) * 256 + i / 256] != 0 {
                x
            } else {
                other_bits[i]
            }
        })
        .collect();
    let output = b.select_low(&mask, &transposed, &other)?;
    assert_eq!(output.shape(), &shape(&[256, 256]));
    assert_eq!(output.low_dtype(), dtype);
    assert_eq!(b.read_low_bits(&output)?, expected);

    // Both select branches and stable compaction must preserve ALL payloads,
    // including signaling NaNs that would change under a float conversion.
    for selected in [false, true] {
        let mask = b.upload_u32(shape(&[]), &[if selected { u32::MAX } else { 0 }])?;
        let output = b.select_low(&mask, &transposed, &other)?;
        assert_eq!(
            b.read_low_bits(&output)?,
            if selected { &logical } else { &other_bits }.clone()
        );
        let compacted = b.compact_low(&transposed, &mask)?;
        assert_eq!(compacted.values.shape(), &shape(&[65536]));
        assert_eq!(compacted.count.shape(), &shape(&[]));
        assert_eq!(
            b.read_u32(&compacted.count)?,
            [if selected { 65536 } else { 0 }]
        );
        assert_eq!(
            b.read_low_bits(&compacted.values)?,
            if selected {
                logical.clone()
            } else {
                vec![0; 65536]
            }
        );
    }

    // All raw patterns pass through a strided source and a strided 2D index
    // tensor. Invalid indices are counted twice, rather than once per row.
    let order: Vec<u32> = (0..256).rev().chain([256, u32::MAX]).collect();
    let indices = b.upload_u32(shape(&[2, 129]), &order)?;
    let indices = b.permute_u32(&indices, &[1, 0])?;
    let index_order: Vec<_> = (0..129).flat_map(|i| [order[i], order[129 + i]]).collect();
    let gathered = b.gather_low(&transposed, &indices, 1)?;
    assert_eq!(gathered.values.shape(), &shape(&[256, 129, 2]));
    let expected: Vec<_> = (0..256)
        .flat_map(|row| {
            index_order.iter().map(move |&index| {
                if index < 256 {
                    (index * 256 + row) as u16
                } else {
                    0
                }
            })
        })
        .collect();
    assert_eq!(b.read_low_bits(&gathered.values)?, expected);
    assert_eq!(b.read_u32(&gathered.invalid_count)?, [2]);
    let scalar_index = b.upload_u32(shape(&[]), &[255])?;
    let scalar_gather = b.gather_low(&transposed, &scalar_index, 0)?;
    assert_eq!(scalar_gather.values.shape(), &shape(&[256]));
    assert_eq!(
        b.read_low_bits(&scalar_gather.values)?,
        (0..256).map(|i| (i * 256 + 255) as u16).collect::<Vec<_>>()
    );
    assert_eq!(b.read_u32(&scalar_gather.invalid_count)?, [0]);
    assert_eq!(b.read_low_bits(&input)?, bits);
    assert_eq!(b.read_low_bits(&other)?, other_bits);

    // An odd-length sparse selection crosses the recursive u32 scan boundary.
    let n = 131_077;
    let bits: Vec<_> = (0..n).map(|i| (i as u16).wrapping_mul(31)).collect();
    let mask_bits: Vec<_> = (0..n)
        .map(|i| {
            if i % 7 == 0 || i % 11 == 0 {
                0
            } else {
                u32::MAX - i as u32
            }
        })
        .collect();
    let input = b.upload_low(dtype, shape(&[n]), &bits)?;
    let mask = b.upload_u32(shape(&[n]), &mask_bits)?;
    let compacted = b.compact_low(&input, &mask)?;
    let mut expected: Vec<_> = bits
        .iter()
        .zip(&mask_bits)
        .filter_map(|(&x, &m)| (m != 0).then_some(x))
        .collect();
    let count = expected.len();
    expected.resize(n, 0);
    assert_eq!(b.read_low_bits(&compacted.values)?, expected);
    assert_eq!(b.read_u32(&compacted.count)?, [count as u32]);

    let input = b.upload_low(
        dtype,
        shape(&[2, 3]),
        &[0x8000, 1, 0xffff, 0x8001, 0x7fff, 0],
    )?;
    let input = b.permute_low(&input, &[1, 0])?;
    let mask = b.upload_u32(shape(&[3, 1]), &[1, 0, 0x80000000])?;
    let compacted = b.compact_low(&input, &mask)?;
    assert_eq!(
        b.read_low_bits(&compacted.values)?,
        [0x8000, 0x8001, 0xffff, 0, 0, 0]
    );
    assert_eq!(b.read_u32(&compacted.count)?, [4]);
    Ok(())
}

fn check_scan<B: TensorLowIndexBackend>(
    b: &B,
    dtype: LowDtype,
    input: &B::LowTensor,
    logical: &[f32],
    axis: usize,
) -> Result<(), B::Error> {
    let values: Vec<f64> = logical.iter().map(|&x| f64::from(x)).collect();
    for inclusive in [false, true] {
        for reverse in [false, true] {
            let options = ScanOptions { inclusive, reverse };
            let expected: Vec<f32> =
                scan_reference(&values, input.shape().dims(), axis, options, 0., |a, c| {
                    a + c
                })
                .into_iter()
                .map(|x| x as f32)
                .collect();
            let scanned = b.scan_low_f32(input, axis, options)?;
            assert_eq!(scanned.shape(), input.shape());
            // Chosen dyadic inputs keep every prefix exactly representable in
            // f32, independent of legal parallel grouping. Zero sign is free
            // for arithmetic sums, unlike raw routing or extrema.
            assert_eq!(
                b.read_f32(&scanned)?,
                expected,
                "{dtype:?} axis{axis} {options:?}"
            );
            let low = b.scan_low(input, axis, options)?;
            assert_eq!(low.shape(), input.shape());
            assert_eq!(low.low_dtype(), dtype);
            let actual = b.read_low_bits(&low)?;
            for (i, (&a, &value)) in actual.iter().zip(&expected).enumerate() {
                let e = round(dtype, value);
                assert!(
                    a == e || (a & 0x7fff == 0 && e & 0x7fff == 0),
                    "{dtype:?} scan {options:?} index{i}: {a:#06x} != {e:#06x}"
                );
            }
            assert_eq!(actual.len(), expected.len());
        }
    }
    Ok(())
}

fn check_scans<B: TensorLowIndexBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    let values: Vec<_> = (0..30).map(|i| (i as f32 - 13.) / 8.).collect();
    let bits: Vec<_> = values.iter().map(|&x| round(dtype, x)).collect();
    let input = b.upload_low(dtype, shape(&[2, 3, 5]), &bits)?;
    let input = b.permute_low(&input, &[2, 0, 1])?;
    let logical: Vec<_> = (0..5)
        .flat_map(|z| (0..2).flat_map(move |x| (0..3).map(move |y| x * 15 + y * 5 + z)))
        .map(|i| values[i])
        .collect();
    for axis in 0..3 {
        check_scan(b, dtype, &input, &logical, axis)?;
    }
    let values: Vec<_> = (0..2 * 513 * 3)
        .map(|i| ((i % 17) as f32 - 8.) / 8.)
        .collect();
    let input = b.upload_low(
        dtype,
        shape(&[2, 513, 3]),
        &values.iter().map(|&x| round(dtype, x)).collect::<Vec<_>>(),
    )?;
    check_scan(b, dtype, &input, &values, 1)?;
    let n = 131_077;
    let input = b.upload_low(dtype, shape(&[n]), &vec![round(dtype, 1.); n])?;
    check_scan(b, dtype, &input, &vec![1.; n], 0)?;
    // Low accumulation would lose the unit terms before the cancellation.
    let big = if dtype == LowDtype::F16 { 2048. } else { 256. };
    let mut values = vec![1.; 519];
    values[0] = big;
    values[518] = -big;
    let input = b.upload_low(
        dtype,
        shape(&[519]),
        &values.iter().map(|&x| round(dtype, x)).collect::<Vec<_>>(),
    )?;
    check_scan(b, dtype, &input, &values, 0)?;
    let scalar = b.upload_low(dtype, shape(&[]), &[round(dtype, 0.5)])?;
    let repeated = b.broadcast_low(&scalar, shape(&[2, 257, 3]))?;
    check_scan(b, dtype, &repeated, &vec![0.5; 2 * 257 * 3], 1)?;
    Ok(())
}

fn check_chain_and_errors<B: TensorLowIndexBackend>(
    b: &B,
    dtype: LowDtype,
) -> Result<(), B::Error> {
    let values = [-1., 2., 3., 4., -5., 6.];
    let bits: Vec<_> = values.iter().map(|&x| round(dtype, x)).collect();
    let input = b.upload_low(dtype, shape(&[2, 3]), &bits)?;
    let zero = b.upload_low(dtype, shape(&[]), &[0])?;
    let mask = b.compare_low(CompareOp::Greater, &input, &zero)?;
    let compacted = b.compact_low(&input, &mask)?;
    let scanned = b.scan_low(&compacted.values, 0, ScanOptions::default())?;
    let offsets = b.scan_u32(
        &mask,
        1,
        ScanOptions {
            inclusive: false,
            reverse: false,
        },
    )?;
    let gathered = b.gather_low(&scanned, &offsets, 0)?;
    let selected = b.select_low(&mask, &gathered.values, &zero)?;
    // Only final tensors are read. No CPU reconstruction between operations.
    assert_eq!(
        b.read_low_bits(&selected)?,
        [0., 2., 5., 2., 0., 5.].map(|x| round(dtype, x))
    );
    assert_eq!(b.read_u32(&compacted.count)?, [4]);
    assert_eq!(b.read_u32(&gathered.invalid_count)?, [0]);

    let mask = b.upload_u32(shape(&[]), &[u32::MAX])?;
    let compacted = b.compact_low(&zero, &mask)?;
    assert_eq!(compacted.values.shape(), &shape(&[1]));
    assert_eq!(b.read_low_bits(&compacted.values)?, [0]);
    assert_eq!(b.read_u32(&compacted.count)?, [1]);
    let indices = b.upload_u32(shape(&[4]), &[0, 2, 3, u32::MAX])?;
    for dims in [[2, 0, 3], [0, 3, 2]] {
        let input = b.upload_low(dtype, shape(&dims), &[])?;
        let result = b.gather_low(&input, &indices, 1)?;
        assert_eq!(result.values.shape(), &shape(&[dims[0], 4, dims[2]]));
        assert_eq!(
            b.read_low_bits(&result.values)?,
            vec![0; dims[0] * 4 * dims[2]]
        );
        assert_eq!(
            b.read_u32(&result.invalid_count)?,
            [if dims[1] == 0 { 4 } else { 2 }]
        );
        let result = b.compact_low(&input, &mask)?;
        assert!(b.read_low_bits(&result.values)?.is_empty());
        assert_eq!(b.read_u32(&result.count)?, [0]);
        assert!(
            b.read_low_bits(&b.select_low(&mask, &input, &zero)?)?
                .is_empty()
        );
        assert!(
            b.read_u32(&b.compare_low(CompareOp::Equal, &input, &zero)?)?
                .is_empty()
        );
        for axis in 0..3 {
            assert!(
                b.read_f32(&b.scan_low_f32(&input, axis, ScanOptions::default())?)?
                    .is_empty()
            );
            assert!(
                b.read_low_bits(&b.scan_low(&input, axis, ScanOptions::default())?)?
                    .is_empty()
            );
        }
        assert!(b.scan_low_f32(&input, 3, ScanOptions::default()).is_err());
        let mixed = b.upload_low(
            if dtype == LowDtype::F16 {
                LowDtype::Bf16
            } else {
                LowDtype::F16
            },
            shape(&dims),
            &[],
        )?;
        assert!(b.compare_low(CompareOp::Equal, &input, &mixed).is_err());
        assert!(b.select_low(&mask, &input, &mixed).is_err());
    }
    let empty_indices = b.upload_u32(shape(&[0, 2]), &[])?;
    let result = b.gather_low(&input, &empty_indices, 1)?;
    assert_eq!(result.values.shape(), &shape(&[2, 0, 2]));
    assert!(b.read_low_bits(&result.values)?.is_empty());
    assert_eq!(b.read_u32(&result.invalid_count)?, [0]);
    assert!(b.scan_low(&zero, 0, ScanOptions::default()).is_err());
    assert!(b.gather_low(&zero, &indices, 0).is_err());
    assert!(b.gather_low(&input, &indices, 2).is_err());
    let bad_mask = b.upload_u32(shape(&[4]), &[1; 4])?;
    assert!(b.select_low(&bad_mask, &input, &zero).is_err());
    assert!(b.compact_low(&input, &bad_mask).is_err());
    let expanding_mask = b.upload_u32(shape(&[2, 1]), &[1; 2])?;
    assert!(b.compact_low(&zero, &expanding_mask).is_err());
    assert_eq!(b.read_low_bits(&input)?, bits);
    Ok(())
}

/// Shared direct-low indexing qualification. Routing preserves all 65536 raw
/// patterns, comparison uses IEEE host semantics, and prefix sums are checked
/// against an independent f64 coordinate oracle before one low rounding.
/// The caller must enforce physical backend availability.
pub fn check_low_index_backend<B: TensorLowIndexBackend>(b: &B) -> Result<(), B::Error> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        check_comparisons(b, dtype)?;
        check_raw_routing(b, dtype)?;
        check_scans(b, dtype)?;
        check_chain_and_errors(b, dtype)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_comparison_reference_distinguishes_ieee_and_total_order() {
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            assert_eq!(
                comparison(CompareOp::Equal, decode(dtype, 0), decode(dtype, 0x8000)),
                1
            );
            assert_eq!(
                comparison(
                    CompareOp::Less,
                    decode(dtype, 0x8001),
                    decode(dtype, 0x8000)
                ),
                1
            );
            assert_eq!(
                comparison(CompareOp::Greater, decode(dtype, 1), decode(dtype, 0)),
                1
            );
            for op in COMPARISONS {
                assert_eq!(
                    comparison(op, decode(dtype, 0xffff), decode(dtype, 0xffff)),
                    u32::from(op == CompareOp::NotEqual)
                );
            }
        }
    }
}
