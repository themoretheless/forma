use super::{check, shape};
use crate::{HasShape, ReduceOp, TensorReduceBackend};

const OPS: [ReduceOp; 4] = [
    ReduceOp::Sum,
    ReduceOp::Product,
    ReduceOp::Min,
    ReduceOp::Max,
];

// Coordinate oracle: assign each source element to the unreduced output axes.
// Does not use the backend planner or the common reduction-shape helper.
fn output_index(mut flat: usize, dims: &[usize], axes: &[usize]) -> usize {
    let mut coordinates = vec![0; dims.len()];
    for axis in (0..dims.len()).rev() {
        coordinates[axis] = flat % dims[axis];
        flat /= dims[axis];
    }
    (0..dims.len())
        .filter(|axis| !axes.contains(axis))
        .fold(0, |index, axis| index * dims[axis] + coordinates[axis])
}

pub(super) fn expected_f32(
    values: &[f32],
    dims: &[usize],
    axes: &[usize],
    op: ReduceOp,
) -> Vec<f32> {
    let size: usize = dims
        .iter()
        .enumerate()
        .filter(|(axis, _)| !axes.contains(axis))
        .map(|(_, &dim)| dim)
        .product();
    let init = match op {
        ReduceOp::Sum => 0.,
        ReduceOp::Product => 1.,
        ReduceOp::Min => f64::INFINITY,
        ReduceOp::Max => f64::NEG_INFINITY,
    };
    let mut result = vec![init; size];
    for (i, &value) in values.iter().enumerate() {
        let entry = &mut result[output_index(i, dims, axes)];
        let value = f64::from(value);
        *entry = match op {
            ReduceOp::Sum => *entry + value,
            ReduceOp::Product => *entry * value,
            ReduceOp::Min => entry.min(value),
            ReduceOp::Max => entry.max(value),
        };
    }
    result.into_iter().map(|value| value as f32).collect()
}

fn expected_u32(values: &[u32], dims: &[usize], axes: &[usize], op: ReduceOp) -> Vec<u32> {
    let size: usize = dims
        .iter()
        .enumerate()
        .filter(|(axis, _)| !axes.contains(axis))
        .map(|(_, &dim)| dim)
        .product();
    let init = match op {
        ReduceOp::Sum | ReduceOp::Max => 0,
        ReduceOp::Product => 1,
        ReduceOp::Min => u32::MAX,
    };
    let mut result = vec![init; size];
    for (i, &value) in values.iter().enumerate() {
        let entry = &mut result[output_index(i, dims, axes)];
        *entry = match op {
            ReduceOp::Sum => entry.wrapping_add(value),
            ReduceOp::Product => entry.wrapping_mul(value),
            ReduceOp::Min => (*entry).min(value),
            ReduceOp::Max => (*entry).max(value),
        };
    }
    result
}

/// Shared typed reduction tests, including strided multi-axis inputs, empty
/// contractions, u32 wrapping, hierarchy boundaries and resident composition.
pub fn check_reduce_backend<B: TensorReduceBackend>(b: &B) -> Result<(), B::Error> {
    let floats: Vec<f32> = (0..30)
        .map(|i| {
            if i % 5 == 0 {
                -1.
            } else {
                1. + (i % 3) as f32 / 32.
            }
        })
        .collect();
    let ints: Vec<u32> = (0..30).map(|i| u32::MAX.wrapping_sub(i * 17)).collect();
    let input = b.upload_f32(shape(&[2, 3, 5]), &floats)?;
    let input = b.permute(&input, &[1, 0, 2])?;
    let uint = b.upload_u32(shape(&[2, 3, 5]), &ints)?;
    let uint = b.permute_u32(&uint, &[1, 0, 2])?;
    let order: Vec<usize> = (0..3)
        .flat_map(|j| (0..2).flat_map(move |i| (0..5).map(move |k| i * 15 + j * 5 + k)))
        .collect();
    let float_logical: Vec<f32> = order.iter().map(|&i| floats[i]).collect();
    let uint_logical: Vec<u32> = order.iter().map(|&i| ints[i]).collect();
    for mask in 0..8 {
        let axes: Vec<usize> = (0..3)
            .rev()
            .filter(|axis| mask & (1 << axis) != 0)
            .collect();
        for keep in [false, true] {
            let output_dims: Vec<usize> = [3, 2, 5]
                .into_iter()
                .enumerate()
                .filter_map(|(axis, dim)| {
                    if axes.contains(&axis) {
                        keep.then_some(1)
                    } else {
                        Some(dim)
                    }
                })
                .collect();
            for op in OPS {
                let output = b.reduce_f32(op, &input, &axes, keep)?;
                assert_eq!(output.shape(), &shape(&output_dims));
                check(
                    &b.read_f32(&output)?,
                    &expected_f32(&float_logical, &[3, 2, 5], &axes, op),
                );
                let output = b.reduce_u32(op, &uint, &axes, keep)?;
                assert_eq!(output.shape(), &shape(&output_dims));
                assert_eq!(
                    b.read_u32(&output)?,
                    expected_u32(&uint_logical, &[3, 2, 5], &axes, op)
                );
            }
            let count: usize = axes.iter().map(|&axis| [3, 2, 5][axis]).product();
            let expected: Vec<f32> = expected_f32(&float_logical, &[3, 2, 5], &axes, ReduceOp::Sum)
                .iter()
                .map(|v| v / count as f32)
                .collect();
            let mean = b.mean_axes(&input, &axes, keep)?;
            assert_eq!(mean.shape(), &shape(&output_dims));
            check(&b.read_f32(&mean)?, &expected);
        }
    }

    // Signed extrema force padding lanes to use a real reduction identity.
    for (op, values, expected) in [
        (ReduceOp::Min, [f32::MAX, f32::MAX / 2.], f32::MAX / 2.),
        (ReduceOp::Max, [-f32::MAX, -f32::MAX / 2.], -f32::MAX / 2.),
    ] {
        let v = b.upload_f32(shape(&[2]), &values)?;
        check(
            &b.read_f32(&b.reduce_f32(op, &v, &[0], false)?)?,
            &[expected],
        );
    }

    for dims in [vec![2, 0, 3], vec![0, 0, 3]] {
        let f = b.upload_f32(shape(&dims), &[])?;
        let u = b.upload_u32(shape(&dims), &[])?;
        for keep in [false, true] {
            for op in OPS {
                if dims[0] != 0 && matches!(op, ReduceOp::Min | ReduceOp::Max) {
                    assert!(b.reduce_f32(op, &f, &[1], keep).is_err());
                    assert!(b.reduce_u32(op, &u, &[1], keep).is_err());
                } else {
                    let f = b.reduce_f32(op, &f, &[1], keep)?;
                    let u = b.reduce_u32(op, &u, &[1], keep)?;
                    let out = if keep {
                        vec![dims[0], 1, 3]
                    } else {
                        vec![dims[0], 3]
                    };
                    assert_eq!(f.shape(), &shape(&out));
                    assert_eq!(u.shape(), &shape(&out));
                    let identity = u32::from(op == ReduceOp::Product);
                    check(&b.read_f32(&f)?, &vec![identity as f32; dims[0] * 3]);
                    assert_eq!(b.read_u32(&u)?, vec![identity; dims[0] * 3]);
                }
            }
            if dims[0] != 0 {
                assert!(b.mean_axes(&f, &[1], keep).is_err());
            } else {
                assert!(b.read_f32(&b.mean_axes(&f, &[1], keep)?)?.is_empty());
            }
        }
        for op in OPS {
            assert!(b.read_f32(&b.reduce_f32(op, &f, &[], false)?)?.is_empty());
            assert!(b.read_u32(&b.reduce_u32(op, &u, &[], false)?)?.is_empty());
        }
    }

    let scalar = b.upload_f32(shape(&[]), &[1.25])?;
    let uint_scalar = b.upload_u32(shape(&[]), &[u32::MAX])?;
    for op in OPS {
        let f = b.reduce_f32(op, &scalar, &[], false)?;
        assert_eq!(f.shape(), &shape(&[]));
        check(&b.read_f32(&f)?, &[1.25]);
        assert_eq!(
            b.read_u32(&b.reduce_u32(op, &uint_scalar, &[], true)?)?,
            [u32::MAX]
        );
    }
    let expanded = b.broadcast_to(&scalar, shape(&[3, 2, 5]))?;
    check(
        &b.read_f32(&b.mean_axes(&expanded, &[0, 2], true)?)?,
        &[1.25; 2],
    );
    check(
        &b.read_f32(&b.reduce_f32(ReduceOp::Product, &expanded, &[1], false)?)?,
        &[1.5625; 15],
    );
    let expanded = b.broadcast_u32(&uint_scalar, shape(&[2, 3]))?;
    assert_eq!(
        b.read_u32(&b.reduce_u32(ReduceOp::Sum, &expanded, &[1], false)?)?,
        [u32::MAX - 2; 2]
    );

    // Cross more than one partial-reduction level; all references stay exact.
    let n = 131_077;
    let mut values = vec![1.; n];
    values[17] = -1.;
    values[n - 1] = 2.;
    let f = b.upload_f32(shape(&[n]), &values)?;
    let mut uints = vec![1; n];
    uints[0] = u32::MAX;
    uints[n - 1] = 3;
    let u = b.upload_u32(shape(&[n]), &uints)?;
    for op in OPS {
        check(
            &b.read_f32(&b.reduce_f32(op, &f, &[0], false)?)?,
            &expected_f32(&values, &[n], &[0], op),
        );
        assert_eq!(
            b.read_u32(&b.reduce_u32(op, &u, &[0], false)?)?,
            expected_u32(&uints, &[n], &[0], op)
        );
    }
    check(
        &b.read_f32(&b.mean_axes(&f, &[0], false)?)?,
        &[(n - 1) as f32 / n as f32],
    );
    // Resident reduction composition: reduce axes independently then finalize.
    let first = b.reduce_f32(ReduceOp::Product, &input, &[2], false)?;
    let last = b.mean_axes(&first, &[1, 0], false)?;
    let products = expected_f32(&float_logical, &[3, 2, 5], &[2], ReduceOp::Product);
    check(
        &b.read_f32(&last)?,
        &[products.iter().map(|&v| f64::from(v)).sum::<f64>() as f32 / 6.],
    );
    assert!(b.reduce_f32(ReduceOp::Sum, &input, &[0, 0], false).is_err());
    assert!(b.reduce_u32(ReduceOp::Min, &uint, &[3], true).is_err());
    assert!(b.mean_axes(&input, &[0, 0], true).is_err());
    Ok(())
}
