use super::{check, shape};
use crate::{HasShape, ScatterOp, TensorScatterBackend};

pub(super) const OPS: [ScatterOp; 5] = [
    ScatterOp::Replace,
    ScatterOp::Add,
    ScatterOp::Multiply,
    ScatterOp::Min,
    ScatterOp::Max,
];

// Independent row-major address oracle. Each source slice is visited in logical
// index order; this establishes the deterministic last-index Replace reference.
pub(super) fn destinations(dims: &[usize], indices: &[u32], axis: usize) -> Vec<Option<usize>> {
    let prefix: usize = dims[..axis].iter().product();
    let suffix: usize = dims[axis + 1..].iter().product();
    let extent = dims[axis];
    (0..prefix)
        .flat_map(|p| {
            indices.iter().flat_map(move |&index| {
                (0..suffix).map(move |s| {
                    if (index as usize) < extent {
                        Some((p * extent + index as usize) * suffix + s)
                    } else {
                        None
                    }
                })
            })
        })
        .collect()
}

pub(super) fn fold_f32(
    base: &[f32],
    updates: &[f32],
    destinations: &[Option<usize>],
    op: ScatterOp,
) -> Vec<f32> {
    assert_eq!(updates.len(), destinations.len());
    let mut out: Vec<f64> = base.iter().map(|&v| f64::from(v)).collect();
    for (&destination, &update) in destinations.iter().zip(updates) {
        if let Some(i) = destination {
            let update = f64::from(update);
            out[i] = match op {
                ScatterOp::Replace => update,
                ScatterOp::Add => out[i] + update,
                ScatterOp::Multiply => out[i] * update,
                ScatterOp::Min => out[i].min(update),
                ScatterOp::Max => out[i].max(update),
            };
        }
    }
    out.into_iter().map(|v| v as f32).collect()
}

fn fold_u32(
    base: &[u32],
    updates: &[u32],
    destinations: &[Option<usize>],
    op: ScatterOp,
) -> Vec<u32> {
    assert_eq!(updates.len(), destinations.len());
    let mut out = base.to_vec();
    for (&destination, &update) in destinations.iter().zip(updates) {
        if let Some(i) = destination {
            out[i] = match op {
                ScatterOp::Replace => update,
                ScatterOp::Add => out[i].wrapping_add(update),
                ScatterOp::Multiply => out[i].wrapping_mul(update),
                ScatterOp::Min => out[i].min(update),
                ScatterOp::Max => out[i].max(update),
            };
        }
    }
    out
}

struct Case<'a, B: TensorScatterBackend> {
    floats: &'a B::Tensor,
    uints: &'a B::UIntTensor,
    base_f32: &'a [f32],
    base_u32: &'a [u32],
    indices: &'a B::UIntTensor,
    index_values: &'a [u32],
    updates_f32: &'a B::Tensor,
    updates_u32: &'a B::UIntTensor,
    expanded_f32: &'a [f32],
    expanded_u32: &'a [u32],
    axis: usize,
}

fn check_case<B: TensorScatterBackend>(b: &B, case: Case<'_, B>) -> Result<(), B::Error> {
    let dims = case.floats.shape().dims();
    assert_eq!(case.floats.shape(), case.uints.shape());
    let destinations = destinations(dims, case.index_values, case.axis);
    let invalid = case
        .index_values
        .iter()
        .filter(|&&index| index as usize >= dims[case.axis])
        .count() as u32;
    for op in OPS {
        let f = b.scatter_f32(op, case.floats, case.indices, case.updates_f32, case.axis)?;
        let u = b.scatter_u32(op, case.uints, case.indices, case.updates_u32, case.axis)?;
        assert_eq!(f.values.shape(), case.floats.shape());
        assert_eq!(u.values.shape(), case.uints.shape());
        assert_eq!(f.invalid_count.shape(), &shape(&[]));
        assert_eq!(u.invalid_count.shape(), &shape(&[]));
        assert_eq!(b.read_u32(&f.invalid_count)?, [invalid]);
        assert_eq!(b.read_u32(&u.invalid_count)?, [invalid]);
        check(
            &b.read_f32(&f.values)?,
            &fold_f32(case.base_f32, case.expanded_f32, &destinations, op),
        );
        assert_eq!(
            b.read_u32(&u.values)?,
            fold_u32(case.base_u32, case.expanded_u32, &destinations, op),
            "scatter {op:?}"
        );
    }
    // Scatter returns new values; shared input views remain valid and unchanged.
    check(&b.read_f32(case.floats)?, case.base_f32);
    assert_eq!(b.read_u32(case.uints)?, case.base_u32);
    Ok(())
}

/// Runs identical resident scatter scenarios on every backend. References use
/// explicit coordinates and include deterministic duplicates, wraparound and
/// invalid-count semantics independent of output shape.
pub fn check_scatter_backend<B: TensorScatterBackend>(b: &B) -> Result<(), B::Error> {
    let floats: Vec<f32> = (0..24).map(|i| (i % 9) as f32 / 2. - 2.).collect();
    let uints: Vec<u32> = (0..24).map(|i| 0xfffffff0u32.wrapping_add(i)).collect();
    let input = b.upload_f32(shape(&[2, 3, 4]), &floats)?;
    let input = b.permute(&input, &[2, 0, 1])?;
    let uint = b.upload_u32(shape(&[2, 3, 4]), &uints)?;
    let uint = b.permute_u32(&uint, &[2, 0, 1])?;
    let order: Vec<usize> = (0..4)
        .flat_map(|x| (0..2).flat_map(move |y| (0..3).map(move |z| y * 12 + z * 4 + x)))
        .collect();
    let logical_f32: Vec<f32> = order.iter().map(|&i| floats[i]).collect();
    let logical_u32: Vec<u32> = order.iter().map(|&i| uints[i]).collect();
    let indices = b.upload_u32(shape(&[3, 2]), &[1, 2, 0, u32::MAX, 1, 0])?;
    let indices = b.permute_u32(&indices, &[1, 0])?;
    let index_values = [1, 0, 1, 2, u32::MAX, 0];
    let updates_f32: Vec<f32> = (0..9).map(|i| (i % 5) as f32 / 2. - 1.).collect();
    let updates_u32: Vec<u32> = (0..9).map(|i| 0x80000001u32.wrapping_add(i * 2)).collect();
    let f = b.upload_f32(shape(&[3, 3]), &updates_f32)?;
    let f = b.permute(&f, &[1, 0])?;
    let u = b.upload_u32(shape(&[3, 3]), &updates_u32)?;
    let u = b.permute_u32(&u, &[1, 0])?;
    let order: Vec<usize> = (0..4)
        .flat_map(|_| (0..6).flat_map(|j| (0..3).map(move |k| k * 3 + j % 3)))
        .collect();
    let expanded_f32: Vec<f32> = order.iter().map(|&i| updates_f32[i]).collect();
    let expanded_u32: Vec<u32> = order.iter().map(|&i| updates_u32[i]).collect();
    check_case(
        b,
        Case {
            floats: &input,
            uints: &uint,
            base_f32: &logical_f32,
            base_u32: &logical_u32,
            indices: &indices,
            index_values: &index_values,
            updates_f32: &f,
            updates_u32: &u,
            expanded_f32: &expanded_f32,
            expanded_u32: &expanded_u32,
            axis: 1,
        },
    )?;

    // Each axis of a 3D base, including leading and trailing scatter slices.
    let base_f32 = [2.; 12];
    let base_u32 = [u32::MAX; 12];
    let input = b.upload_f32(shape(&[2, 3, 2]), &base_f32)?;
    let uint = b.upload_u32(shape(&[2, 3, 2]), &base_u32)?;
    let f = b.upload_f32(shape(&[]), &[0.5])?;
    let u = b.upload_u32(shape(&[]), &[3])?;
    let idx = b.upload_u32(shape(&[4]), &[1, 0, 1, u32::MAX])?;
    for axis in 0..3 {
        let n = 12 / [2, 3, 2][axis] * 4;
        check_case(
            b,
            Case {
                floats: &input,
                uints: &uint,
                base_f32: &base_f32,
                base_u32: &base_u32,
                indices: &idx,
                index_values: &[1, 0, 1, u32::MAX],
                updates_f32: &f,
                updates_u32: &u,
                expanded_f32: &vec![0.5; n],
                expanded_u32: &vec![3; n],
                axis,
            },
        )?;
    }

    // Scalar indices remove the indexed dimension from the required updates.
    for index in [0, 2, u32::MAX] {
        let idx = b.upload_u32(shape(&[]), &[index])?;
        check_case(
            b,
            Case {
                floats: &input,
                uints: &uint,
                base_f32: &base_f32,
                base_u32: &base_u32,
                indices: &idx,
                index_values: &[index],
                updates_f32: &f,
                updates_u32: &u,
                expanded_f32: &[0.5; 6],
                expanded_u32: &[3; 6],
                axis: 0,
            },
        )?;
    }

    // Empty base, zero target extent and empty indices have distinct counts.
    for (dims, index_values) in [
        (vec![0, 3, 2], vec![0, 3, u32::MAX]),
        (vec![2, 0, 3], vec![0, 1, u32::MAX]),
        (vec![2, 3, 0], vec![2, 3, u32::MAX]),
        (vec![2, 3, 2], vec![]),
    ] {
        let count = dims.iter().product();
        let input = b.upload_f32(shape(&dims), &vec![7.; count])?;
        let uint = b.upload_u32(shape(&dims), &vec![0x80000007; count])?;
        let indices = b.upload_u32(shape(&[index_values.len()]), &index_values)?;
        let updates = dims[0] * index_values.len() * dims[2];
        check_case(
            b,
            Case {
                floats: &input,
                uints: &uint,
                base_f32: &vec![7.; count],
                base_u32: &vec![0x80000007; count],
                indices: &indices,
                index_values: &index_values,
                updates_f32: &f,
                updates_u32: &u,
                expanded_f32: &vec![0.5; updates],
                expanded_u32: &vec![3; updates],
                axis: 1,
            },
        )?;
    }

    // A long repeated index spans many workgroups. Last logical position must
    // win Replace; all updates must contribute to every associative operation.
    let n = 65_539;
    let indices = b.upload_u32(shape(&[1]), &[2])?;
    let indices = b.broadcast_u32(&indices, shape(&[n]))?;
    let mut fvalues = vec![1.; n];
    fvalues[n - 1] = 3.;
    let mut uvalues = vec![1; n];
    uvalues[0] = u32::MAX;
    uvalues[n - 1] = 7;
    let f = b.upload_f32(shape(&[n]), &fvalues)?;
    let u = b.upload_u32(shape(&[n]), &uvalues)?;
    let input = b.upload_f32(shape(&[4]), &[8., 9., 2., 11.])?;
    let uint = b.upload_u32(shape(&[4]), &[8, 9, 2, 11])?;
    check_case(
        b,
        Case {
            floats: &input,
            uints: &uint,
            base_f32: &[8., 9., 2., 11.],
            base_u32: &[8, 9, 2, 11],
            indices: &indices,
            index_values: &vec![2; n],
            updates_f32: &f,
            updates_u32: &u,
            expanded_f32: &fvalues,
            expanded_u32: &uvalues,
            axis: 0,
        },
    )?;

    // Result/count compose into gather and scan without a host synchronization.
    let idx = b.upload_u32(shape(&[5]), &[2, 0, 2, 99, 1])?;
    let update = b.upload_f32(shape(&[5]), &[1., 2., 3., 99., 4.])?;
    let scattered = b.scatter_f32(ScatterOp::Add, &input, &idx, &update, 0)?;
    let selected = b.gather_f32(&scattered.values, &idx, 0)?;
    let prefix = b.scan_f32(&selected.values, 0, crate::ScanOptions::default())?;
    check(&b.read_f32(&prefix)?, &[6., 16., 22., 22., 35.]);
    assert_eq!(b.read_u32(&scattered.invalid_count)?, [1]);
    assert_eq!(b.read_u32(&selected.invalid_count)?, [1]);
    let bad = b.upload_f32(shape(&[2, 5]), &[1.; 10])?;
    assert!(
        b.scatter_f32(ScatterOp::Add, &input, &idx, &bad, 0)
            .is_err()
    );
    assert!(
        b.scatter_f32(ScatterOp::Add, &input, &idx, &update, 1)
            .is_err()
    );
    Ok(())
}
