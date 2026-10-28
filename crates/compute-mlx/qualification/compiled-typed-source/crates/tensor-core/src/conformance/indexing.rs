use super::{check, shape};
use crate::{CompareOp, HasShape, ScanOptions, TensorIndexBackend};

pub(super) const COMPARISONS: [CompareOp; 6] = [
    CompareOp::Equal,
    CompareOp::NotEqual,
    CompareOp::Less,
    CompareOp::LessEqual,
    CompareOp::Greater,
    CompareOp::GreaterEqual,
];

pub(super) fn comparison<T: PartialOrd + PartialEq>(op: CompareOp, a: T, b: T) -> u32 {
    u32::from(match op {
        CompareOp::Equal => a == b,
        CompareOp::NotEqual => a != b,
        CompareOp::Less => a < b,
        CompareOp::LessEqual => a <= b,
        CompareOp::Greater => a > b,
        CompareOp::GreaterEqual => a >= b,
    })
}

/// Independent row-major scan oracle; it never uses a backend's layout helper.
pub(super) fn scan_reference<T: Copy>(
    values: &[T],
    dims: &[usize],
    axis: usize,
    options: ScanOptions,
    zero: T,
    add: impl Fn(T, T) -> T,
) -> Vec<T> {
    let outer: usize = dims[..axis].iter().product();
    let inner: usize = dims[axis + 1..].iter().product();
    let length = dims[axis];
    let mut result = vec![zero; values.len()];
    for row in 0..outer {
        for column in 0..inner {
            let mut sum = zero;
            for step in 0..length {
                let position = if options.reverse {
                    length - 1 - step
                } else {
                    step
                };
                let index = (row * length + position) * inner + column;
                if options.inclusive {
                    sum = add(sum, values[index]);
                }
                result[index] = sum;
                if !options.inclusive {
                    sum = add(sum, values[index]);
                }
            }
        }
    }
    result
}

/// Executes the same typed indexing contract on every backend. This fixture
/// includes resident chains: comparison/scan/selection/compaction/gather inputs
/// are passed as device tensors rather than reconstructed from readback.
pub fn check_index_backend<B: TensorIndexBackend>(backend: &B) -> Result<(), B::Error> {
    check_views(backend)?;
    check_comparisons_and_selection(backend)?;
    check_scans(backend)?;
    check_gather(backend)?;
    check_compaction(backend)?;
    Ok(())
}

fn check_views<B: TensorIndexBackend>(b: &B) -> Result<(), B::Error> {
    let values = [0, 16_777_217, u32::MAX, 11, 44, 99];
    let source = b.upload_u32(shape(&[2, 3]), &values)?;
    let view = b.permute_u32(&source, &[1, 0])?;
    let expected = [0, 11, 16_777_217, 44, u32::MAX, 99];
    assert_eq!(b.read_u32(&view)?, expected);
    assert_eq!(b.read_u32(&b.materialize_u32(&view)?)?, expected);
    assert_eq!(b.read_u32(&b.reshape_u32(&view, shape(&[6]))?)?, expected);
    let scalar = b.upload_u32(shape(&[]), &[u32::MAX])?;
    assert_eq!(
        b.read_u32(&b.broadcast_u32(&scalar, shape(&[2, 3]))?)?,
        [u32::MAX; 6]
    );
    assert!(
        b.read_u32(&b.broadcast_u32(&scalar, shape(&[0, 3]))?)?
            .is_empty()
    );
    assert!(b.reshape_u32(&source, shape(&[5])).is_err());
    assert!(b.permute_u32(&source, &[0, 0]).is_err());
    assert!(b.upload_u32(shape(&[2]), &[1]).is_err());
    Ok(())
}

fn check_comparisons_and_selection<B: TensorIndexBackend>(b: &B) -> Result<(), B::Error> {
    let fa = [-1.0_f32, 5.0];
    let fb = [-1.0_f32, 4.0, 7.0];
    let ua = [16_777_217, u32::MAX];
    let ub = [16_777_216, 16_777_217, u32::MAX];
    let left = b.upload_f32(shape(&[2, 1]), &fa)?;
    let right = b.upload_f32(shape(&[3]), &fb)?;
    let left_u = b.upload_u32(shape(&[2, 1]), &ua)?;
    let right_u = b.upload_u32(shape(&[3]), &ub)?;
    for op in COMPARISONS {
        let expected: Vec<u32> = fa
            .into_iter()
            .flat_map(|a| fb.map(|v| comparison(op, a, v)))
            .collect();
        let mask = b.compare(op, &left, &right)?;
        assert_eq!(mask.shape(), &shape(&[2, 3]));
        assert_eq!(b.read_u32(&mask)?, expected, "f32 {op:?}");
        let expected: Vec<u32> = ua
            .into_iter()
            .flat_map(|a| ub.map(|v| comparison(op, a, v)))
            .collect();
        assert_eq!(
            b.read_u32(&b.compare_u32(op, &left_u, &right_u)?)?,
            expected,
            "u32 {op:?}"
        );
    }
    let mask = b.upload_u32(shape(&[2, 1]), &[0, u32::MAX])?;
    let yes = b.upload_f32(shape(&[1, 3]), &[-2.5, 0.0, 3.25])?;
    let no = b.upload_f32(shape(&[]), &[8.0])?;
    check(
        &b.read_f32(&b.select_f32(&mask, &yes, &no)?)?,
        &[8., 8., 8., -2.5, 0., 3.25],
    );
    let yes_u = b.upload_u32(shape(&[3]), &ub)?;
    let no_u = b.upload_u32(shape(&[]), &[19])?;
    assert_eq!(
        b.read_u32(&b.select_u32(&mask, &yes_u, &no_u)?)?,
        [19, 19, 19, ub[0], ub[1], ub[2]]
    );
    let empty = b.upload_u32(shape(&[0, 1]), &[])?;
    let selected = b.select_u32(&empty, &yes_u, &no_u)?;
    assert_eq!(selected.shape(), &shape(&[0, 3]));
    assert!(b.read_u32(&selected)?.is_empty());
    Ok(())
}

fn check_scans<B: TensorIndexBackend>(b: &B) -> Result<(), B::Error> {
    // Permuted shape is [5,2,3]; each axis has a distinct physical stride.
    let original: Vec<u32> = (0..30)
        .map(|i| if i % 7 == 0 { u32::MAX } else { i })
        .collect();
    let original_f: Vec<f32> = (0..30).map(|i| (i as f32 - 13.) / 8.).collect();
    let input = b.permute_u32(&b.upload_u32(shape(&[2, 3, 5]), &original)?, &[2, 0, 1])?;
    let input_f = b.permute(&b.upload_f32(shape(&[2, 3, 5]), &original_f)?, &[2, 0, 1])?;
    let mut logical = Vec::new();
    let mut logical_f = Vec::new();
    for z in 0..5 {
        for x in 0..2 {
            for y in 0..3 {
                logical.push(original[x * 15 + y * 5 + z]);
                logical_f.push(f64::from(original_f[x * 15 + y * 5 + z]));
            }
        }
    }
    for axis in 0..3 {
        for inclusive in [false, true] {
            for reverse in [false, true] {
                let options = ScanOptions { inclusive, reverse };
                let expected =
                    scan_reference(&logical, &[5, 2, 3], axis, options, 0u32, u32::wrapping_add);
                assert_eq!(b.read_u32(&b.scan_u32(&input, axis, options)?)?, expected);
                let expected: Vec<f32> =
                    scan_reference(&logical_f, &[5, 2, 3], axis, options, 0., |a, c| a + c)
                        .into_iter()
                        .map(|x| x as f32)
                        .collect();
                check(
                    &b.read_f32(&b.scan_f32(&input_f, axis, options)?)?,
                    &expected,
                );
            }
        }
    }
    // Multiple rows/chunks with a trailing dimension, including unsigned carry.
    let dims = [2, 513, 3];
    let values: Vec<u32> = (0..3078)
        .map(|i| {
            if i % 131 == 0 {
                u32::MAX
            } else {
                (i % 5) as u32
            }
        })
        .collect();
    let values_f: Vec<f32> = (0..3078).map(|i| ((i % 17) as f32 - 8.) / 8.).collect();
    let input = b.upload_u32(shape(&dims), &values)?;
    let input_f = b.upload_f32(shape(&dims), &values_f)?;
    for inclusive in [false, true] {
        for reverse in [false, true] {
            let options = ScanOptions { inclusive, reverse };
            assert_eq!(
                b.read_u32(&b.scan_u32(&input, 1, options)?)?,
                scan_reference(&values, &dims, 1, options, 0u32, u32::wrapping_add)
            );
            let values_f64: Vec<f64> = values_f.iter().map(|&x| f64::from(x)).collect();
            let expected: Vec<f32> =
                scan_reference(&values_f64, &dims, 1, options, 0., |a, c| a + c)
                    .into_iter()
                    .map(|x| x as f32)
                    .collect();
            check(&b.read_f32(&b.scan_f32(&input_f, 1, options)?)?, &expected);
        }
    }
    // A scan deeper than one level of block sums.
    let long: Vec<u32> = (0..131_075).map(|i| i % 5).collect();
    let options = ScanOptions {
        inclusive: false,
        reverse: false,
    };
    let scanned = b.scan_u32(&b.upload_u32(shape(&[long.len()]), &long)?, 0, options)?;
    assert_eq!(
        b.read_u32(&scanned)?,
        scan_reference(&long, &[long.len()], 0, options, 0u32, u32::wrapping_add)
    );
    let empty = b.upload_u32(shape(&[2, 0, 3]), &[])?;
    assert!(
        b.read_u32(&b.scan_u32(&empty, 1, ScanOptions::default())?)?
            .is_empty()
    );
    assert!(b.scan_u32(&empty, 3, ScanOptions::default()).is_err());
    let scalar = b.upload_f32(shape(&[]), &[1.])?;
    assert!(b.scan_f32(&scalar, 0, ScanOptions::default()).is_err());
    Ok(())
}

fn check_gather<B: TensorIndexBackend>(b: &B) -> Result<(), B::Error> {
    let floats: Vec<f32> = (0..12).map(|i| i as f32 + 0.5).collect();
    let ints: Vec<u32> = (0..12).map(|i| u32::MAX - i).collect();
    let source = b.permute(&b.upload_f32(shape(&[2, 3, 2]), &floats)?, &[0, 2, 1])?;
    let source_u = b.permute_u32(&b.upload_u32(shape(&[2, 3, 2]), &ints)?, &[0, 2, 1])?;
    let indices = b.permute_u32(
        &b.upload_u32(shape(&[2, 2]), &[2, 0, u32::MAX, 3])?,
        &[1, 0],
    )?;
    let gathered = b.gather_f32(&source, &indices, 2)?;
    let gathered_u = b.gather_u32(&source_u, &indices, 2)?;
    assert_eq!(gathered.values.shape(), &shape(&[2, 2, 2, 2]));
    assert_eq!(gathered.invalid_count.shape(), &shape(&[]));
    assert_eq!(b.read_u32(&gathered.invalid_count)?, [2]);
    assert_eq!(b.read_u32(&gathered_u.invalid_count)?, [2]);
    let mut expected = Vec::new();
    let mut expected_u = Vec::new();
    for batch in 0..2 {
        for feature in 0..2 {
            for index in [2usize, usize::MAX, 0, 3] {
                if index < 3 {
                    expected.push(floats[batch * 6 + index * 2 + feature]);
                    expected_u.push(ints[batch * 6 + index * 2 + feature]);
                } else {
                    expected.push(0.);
                    expected_u.push(0);
                }
            }
        }
    }
    check(&b.read_f32(&gathered.values)?, &expected);
    assert_eq!(b.read_u32(&gathered_u.values)?, expected_u);
    let scalar_index = b.upload_u32(shape(&[]), &[1])?;
    let scalar_gather = b.gather_f32(&source, &scalar_index, 2)?;
    assert_eq!(scalar_gather.values.shape(), &shape(&[2, 2]));
    check(&b.read_f32(&scalar_gather.values)?, &[2.5, 3.5, 8.5, 9.5]);
    assert_eq!(b.read_u32(&scalar_gather.invalid_count)?, [0]);

    let empty_axis = b.upload_f32(shape(&[2, 0, 3]), &[])?;
    let all_invalid = b.gather_f32(&empty_axis, &indices, 1)?;
    assert_eq!(all_invalid.values.shape(), &shape(&[2, 2, 2, 3]));
    check(&b.read_f32(&all_invalid.values)?, &[0.; 24]);
    assert_eq!(b.read_u32(&all_invalid.invalid_count)?, [4]);
    // Invalid counts remain meaningful even when other input axes are empty.
    let empty_output = b.upload_u32(shape(&[0, 3]), &[])?;
    let empty_gather = b.gather_u32(&empty_output, &indices, 1)?;
    assert!(b.read_u32(&empty_gather.values)?.is_empty());
    assert_eq!(b.read_u32(&empty_gather.invalid_count)?, [2]);
    let no_indices = b.upload_u32(shape(&[0, 2]), &[])?;
    let no_gather = b.gather_f32(&source, &no_indices, 1)?;
    assert_eq!(no_gather.values.shape(), &shape(&[2, 0, 2, 3]));
    assert!(b.read_f32(&no_gather.values)?.is_empty());
    assert_eq!(b.read_u32(&no_gather.invalid_count)?, [0]);
    assert!(b.gather_f32(&source, &indices, 3).is_err());
    Ok(())
}

fn check_compaction<B: TensorIndexBackend>(b: &B) -> Result<(), B::Error> {
    let input = b.upload_f32(shape(&[2, 3]), &[-1., 2., 3., 4., -5., 6.])?;
    let zero = b.upload_f32(shape(&[]), &[0.])?;
    let mask = b.compare(CompareOp::Greater, &input, &zero)?;
    let compacted = b.compact_f32(&input, &mask)?;
    let offsets = b.scan_u32(
        &mask,
        1,
        ScanOptions {
            inclusive: false,
            reverse: false,
        },
    )?;
    let gathered = b.gather_f32(&compacted.values, &offsets, 0)?;
    let selected = b.select_f32(&mask, &gathered.values, &zero)?;
    // Only final values/counts are read, so the whole chain remains resident.
    check(&b.read_f32(&selected)?, &[0., 2., 3., 2., 0., 3.]);
    check(&b.read_f32(&compacted.values)?, &[2., 3., 4., 6., 0., 0.]);
    assert_eq!(compacted.values.shape(), &shape(&[6]));
    assert_eq!(compacted.count.shape(), &shape(&[]));
    assert_eq!(b.read_u32(&compacted.count)?, [4]);
    assert_eq!(b.read_u32(&gathered.invalid_count)?, [0]);

    let values = [16_777_217, u32::MAX, 7, 99, 23, 66];
    let source = b.permute_u32(&b.upload_u32(shape(&[2, 3]), &values)?, &[1, 0])?;
    let mask = b.upload_u32(shape(&[3, 1]), &[1, 0, 7])?;
    let compacted = b.compact_u32(&source, &mask)?;
    assert_eq!(
        b.read_u32(&compacted.values)?,
        [16_777_217, 99, 7, 66, 0, 0]
    );
    assert_eq!(b.read_u32(&compacted.count)?, [4]);
    let logical = [16_777_217, 99, u32::MAX, 23, 7, 66];
    for selected in [false, true] {
        let mask = b.upload_u32(shape(&[]), &[if selected { u32::MAX } else { 0 }])?;
        let compacted = b.compact_u32(&source, &mask)?;
        assert_eq!(
            b.read_u32(&compacted.count)?,
            [if selected { 6 } else { 0 }]
        );
        assert_eq!(
            b.read_u32(&compacted.values)?,
            if selected { logical } else { [0; 6] }
        );
    }
    let empty = b.upload_u32(shape(&[0, 2]), &[])?;
    let mask = b.upload_u32(shape(&[1, 2]), &[0, 1])?;
    let compacted = b.compact_u32(&empty, &mask)?;
    assert!(b.read_u32(&compacted.values)?.is_empty());
    assert_eq!(b.read_u32(&compacted.count)?, [0]);
    let scalar = b.upload_u32(shape(&[]), &[u32::MAX])?;
    let compacted = b.compact_u32(&scalar, &scalar)?;
    assert_eq!(compacted.values.shape(), &shape(&[1]));
    assert_eq!(b.read_u32(&compacted.values)?, [u32::MAX]);
    assert_eq!(b.read_u32(&compacted.count)?, [1]);
    assert!(b.compact_u32(&scalar, &mask).is_err());
    Ok(())
}
