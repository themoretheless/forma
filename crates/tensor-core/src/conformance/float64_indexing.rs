use super::{
    float64::bits,
    indexing::{COMPARISONS, comparison},
    shape,
};
use crate::{
    CompareOp, HasShape, ScanOptions, ScatterOp, TensorF64IndexBackend, TensorF64ScatterBackend,
};

/// Independent binary64 indexing fixtures. Raw-value operations preserve bits;
/// scan fixtures use dyadic values with exactly representable partial sums.
pub fn check_f64_index_backend<B: TensorF64IndexBackend>(b: &B) -> Result<(), B::Error> {
    let left_values = [100_000_000., 100_000_001.];
    let right_values = [100_000_000., 100_000_001., 100_000_002.];
    let left = b.upload_f64(shape(&[1, 2]), &left_values)?;
    let left = b.permute_f64(&left, &[1, 0])?;
    let right = b.upload_f64(shape(&[3]), &right_values)?;
    for op in COMPARISONS {
        let mask = b.compare_f64(op, &left, &right)?;
        assert_eq!(mask.shape(), &shape(&[2, 3]));
        let expected: Vec<_> = left_values
            .iter()
            .flat_map(|&x| right_values.map(|y| comparison(op, x, y)))
            .collect();
        assert_eq!(b.read_u32(&mask)?, expected);
    }
    let mask = b.upload_u32(shape(&[2, 1]), &[0, u32::MAX])?;
    let yes_values = [
        f64::from_bits(0x7ff8_1234_5678_9abc),
        -0.,
        f64::from_bits(1),
    ];
    let yes = b.upload_f64(shape(&[3]), &yes_values)?;
    let no = b.upload_f64(shape(&[]), &[1e200])?;
    bits(
        &b.read_f64(&b.select_f64(&mask, &yes, &no)?)?,
        &[
            1e200,
            1e200,
            1e200,
            yes_values[0],
            yes_values[1],
            yes_values[2],
        ],
    );
    // Noncontiguous f64 copy must retain NaN payload and signed/subnormal bits.
    let broadcast = b.broadcast_f64(&yes, shape(&[2, 3]))?;
    let view = b.permute_f64(&broadcast, &[1, 0])?;
    bits(
        &b.read_f64(&b.materialize_f64(&view)?)?,
        &[
            yes_values[0],
            yes_values[0],
            yes_values[1],
            yes_values[1],
            yes_values[2],
            yes_values[2],
        ],
    );
    let values = b.upload_f64(
        shape(&[2, 3]),
        &[99., 1e200, -0., 99., 1e-200, 100_000_001.],
    )?;
    let sliced = b.narrow_f64(&values, 1, 1, 2)?;
    let source = b.permute_f64(&sliced, &[1, 0])?; // [[1e200,1e-200],[-0,100000001]]
    let indices = b.permute_u32(&b.upload_u32(shape(&[2, 2]), &[1, 0, 99, 1])?, &[1, 0])?;
    let gathered = b.gather_f64(&source, &indices, 1)?;
    assert_eq!(gathered.values.shape(), &shape(&[2, 2, 2]));
    bits(
        &b.read_f64(&gathered.values)?,
        &[
            1e-200,
            0.,
            1e200,
            1e-200,
            100_000_001.,
            0.,
            -0.,
            100_000_001.,
        ],
    );
    assert_eq!(b.read_u32(&gathered.invalid_count)?, [1]);
    let scalar_index = b.upload_u32(shape(&[]), &[0])?;
    bits(
        &b.read_f64(&b.gather_f64(&source, &scalar_index, 1)?.values)?,
        &[1e200, -0.],
    );
    let empty = b.upload_f64(shape(&[0, 2]), &[])?;
    let empty_gather = b.gather_f64(&empty, &indices, 1)?;
    assert!(b.read_f64(&empty_gather.values)?.is_empty());
    assert_eq!(b.read_u32(&empty_gather.invalid_count)?, [1]);
    let no_axis = b.upload_f64(shape(&[2, 0]), &[])?;
    let invalid = b.gather_f64(&no_axis, &indices, 1)?;
    bits(&b.read_f64(&invalid.values)?, &[0.; 8]);
    assert_eq!(b.read_u32(&invalid.invalid_count)?, [4]);
    assert!(b.gather_f64(&source, &indices, 2).is_err());
    let all = b.upload_u32(shape(&[]), &[17])?;
    let selected = b.compact_f64(&source, &all)?;
    bits(
        &b.read_f64(&selected.values)?,
        &[1e200, 1e-200, -0., 100_000_001.],
    );
    assert_eq!(b.read_u32(&selected.count)?, [4]);
    // Resident compare -> mask -> compact -> gather chain, no mask readback.
    let cutoff = b.upload_f64(shape(&[]), &[100_000_000.])?;
    let selected = b.compact_f64(
        &source,
        &b.compare_f64(CompareOp::Greater, &source, &cutoff)?,
    )?;
    bits(
        &b.read_f64(&selected.values)?,
        &[1e200, 100_000_001., 0., 0.],
    );
    assert_eq!(b.read_u32(&selected.count)?, [2]);
    let ordered = b.upload_u32(shape(&[2]), &[1, 0])?;
    bits(
        &b.read_f64(&b.gather_f64(&selected.values, &ordered, 0)?.values)?,
        &[100_000_001., 1e200],
    );
    let none = b.upload_u32(shape(&[]), &[0])?;
    let selected = b.compact_f64(&source, &none)?;
    bits(&b.read_f64(&selected.values)?, &[0.; 4]);
    assert_eq!(b.read_u32(&selected.count)?, [0]);
    let selected = b.compact_f64(&empty, &all)?;
    assert!(b.read_f64(&selected.values)?.is_empty());
    assert_eq!(b.read_u32(&selected.count)?, [0]);
    let scalar = b.compact_f64(&yes, &all)?;
    bits(&b.read_f64(&scalar.values)?, &yes_values);
    // 257 chunks force a recursive carry scan. Both rows have offset, nonunit
    // axis stride and fractional values which would collapse to integers in f32.
    let n = 65_537;
    let unit = 1. + 2_f64.powi(-35);
    let values: Vec<_> = (0..n + 2).flat_map(|_| [unit, 2. * unit]).collect();
    let scan_input = b.upload_f64(shape(&[n + 2, 2]), &values)?;
    let scan_input = b.narrow_f64(&scan_input, 0, 1, n)?;
    let scan_input = b.permute_f64(&scan_input, &[1, 0])?;
    for inclusive in [false, true] {
        for reverse in [false, true] {
            let options = ScanOptions { inclusive, reverse };
            let result = b.scan_f64(&scan_input, 1, options)?;
            let expected: Vec<_> = [unit, 2. * unit]
                .iter()
                .flat_map(|&value| {
                    (0..n).map(move |i| {
                        let before = if reverse { n - 1 - i } else { i };
                        (before + usize::from(inclusive)) as f64 * value
                    })
                })
                .collect();
            bits(&b.read_f64(&result)?, &expected);
        }
    }
    let scan = b.scan_f64(&empty, 1, ScanOptions::default())?;
    assert!(b.read_f64(&scan)?.is_empty());
    assert!(b.scan_f64(&no, 0, ScanOptions::default()).is_err());
    Ok(())
}

/// Repeated-index folds retain f64 values and include the original base. Exact
/// dyadic inputs keep unordered arithmetic fixtures independent of fold order.
pub fn check_f64_scatter_backend<B: TensorF64ScatterBackend>(b: &B) -> Result<(), B::Error> {
    let base_values = [100_000_000., 1., 3., 4., 5., 6.];
    let base = b.upload_f64(shape(&[2, 3]), &base_values)?;
    let raw = b.upload_u32(shape(&[2, 2]), &[1, 0, 1, 99])?;
    let indices = b.permute_u32(&raw, &[1, 0])?; // 1,1,0,99; last logical update wins
    let unit = 1. + 2_f64.powi(-35);
    let updates = [unit, 2., 3., 123., 4., unit, 6., 123.];
    let changes = b.upload_f64(shape(&[2, 2, 2]), &updates)?;
    for op in [
        ScatterOp::Replace,
        ScatterOp::Add,
        ScatterOp::Multiply,
        ScatterOp::Min,
        ScatterOp::Max,
    ] {
        let result = b.scatter_f64(op, &base, &indices, &changes, 1)?;
        let mut expected = base_values;
        for row in 0..2 {
            for (j, index) in [1, 1, 0, 99].into_iter().enumerate() {
                if index >= 3 {
                    continue;
                }
                let old = &mut expected[row * 3 + index];
                let value = updates[row * 4 + j];
                *old = match op {
                    ScatterOp::Replace => value,
                    ScatterOp::Add => *old + value,
                    ScatterOp::Multiply => *old * value,
                    ScatterOp::Min => old.min(value),
                    ScatterOp::Max => old.max(value),
                };
            }
        }
        bits(&b.read_f64(&result.values)?, &expected);
        assert_eq!(b.read_u32(&result.invalid_count)?, [1]);
        bits(&b.read_f64(&base)?, &base_values);
    }
    let zero = b.upload_f64(shape(&[1]), &[0.])?;
    let repeat = b.upload_u32(shape(&[4097]), &vec![0; 4097])?;
    let update = b.upload_f64(shape(&[]), &[unit])?;
    let folded = b.scatter_f64(ScatterOp::Add, &zero, &repeat, &update, 0)?;
    bits(&b.read_f64(&folded.values)?, &[4097. * unit]);
    let idx = b.upload_u32(shape(&[2]), &[0, 0])?;
    let zeros = b.upload_f64(shape(&[2]), &[-0., 0.])?;
    for (op, value) in [(ScatterOp::Min, -0.), (ScatterOp::Max, 0.)] {
        bits(
            &b.read_f64(&b.scatter_f64(op, &zero, &idx, &zeros, 0)?.values)?,
            &[value],
        );
    }
    let raw_values = [f64::from_bits(1), f64::from_bits(0x7ff8_1234_5678_9abc)];
    let raw = b.upload_f64(shape(&[2]), &raw_values)?;
    let replaced = b.scatter_f64(ScatterOp::Replace, &zero, &idx, &raw, 0)?;
    bits(&b.read_f64(&replaced.values)?, &[raw_values[1]]);
    let empty = b.upload_f64(shape(&[0, 3]), &[])?;
    let empty_updates = b.upload_f64(shape(&[0, 2, 2]), &[])?;
    let result = b.scatter_f64(ScatterOp::Add, &empty, &indices, &empty_updates, 1)?;
    assert!(b.read_f64(&result.values)?.is_empty());
    assert_eq!(b.read_u32(&result.invalid_count)?, [1]);
    assert!(
        b.scatter_f64(ScatterOp::Add, &base, &indices, &zero, 2)
            .is_err()
    );
    Ok(())
}
