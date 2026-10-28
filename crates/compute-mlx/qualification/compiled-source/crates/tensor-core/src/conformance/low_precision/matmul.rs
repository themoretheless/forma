//! Independent low matmul fixtures; no production layout or matmul planner.
use super::{
    super::{
        attention::{Host, data},
        low_ops::round,
        shape,
    },
    decode,
};
use crate::{HasLowDtype, HasShape, LowDtype, TensorLowBackend};

mod accumulation;

fn coordinates(mut index: usize, dims: &[usize]) -> Vec<usize> {
    let mut result = vec![0; dims.len()];
    for (axis, &extent) in dims.iter().enumerate().rev() {
        result[axis] = index % extent;
        index /= extent;
    }
    result
}
fn address(dims: &[usize], coordinates: &[usize]) -> usize {
    let start = coordinates.len() - dims.len();
    dims.iter().enumerate().fold(0, |offset, (axis, &extent)| {
        offset * extent
            + if extent == 1 {
                0
            } else {
                coordinates[start + axis]
            }
    })
}

// Reuse the shared fixture's storage descriptor. Reorder independently here:
// Host::logical uses the production Layout, which this oracle must not reuse.
fn logical(host: &Host<f32>, dtype: LowDtype) -> (Vec<usize>, Vec<f64>) {
    let original = host.shape.dims();
    let axes = host
        .permutation
        .clone()
        .unwrap_or_else(|| (0..original.len()).collect());
    let dims: Vec<_> = axes.iter().map(|&axis| original[axis]).collect();
    let values = (0..host.values.len())
        .map(|index| {
            let target = coordinates(index, &dims);
            let mut source = vec![0; original.len()];
            for (axis, &original_axis) in axes.iter().enumerate() {
                source[original_axis] = target[axis];
            }
            f64::from(decode(
                dtype,
                round(dtype, host.values[address(original, &source)]),
            ))
        })
        .collect();
    (dims, values)
}

struct Input<B: TensorLowBackend> {
    tensor: B::LowTensor,
    dims: Vec<usize>,
    values: Vec<f64>,
}
fn prepare<B: TensorLowBackend>(
    b: &B,
    dtype: LowDtype,
    host: Host<f32>,
    broadcast: Option<&[usize]>,
) -> Result<Input<B>, B::Error> {
    let bits: Vec<_> = host
        .values
        .iter()
        .map(|&value| round(dtype, value))
        .collect();
    let mut tensor = b.upload_low(dtype, host.shape.clone(), &bits)?;
    if let Some(axes) = &host.permutation {
        tensor = b.permute_low(&tensor, axes)?;
    }
    let (mut dims, mut values) = logical(&host, dtype);
    if let Some(target) = broadcast {
        tensor = b.broadcast_low(&tensor, shape(target))?;
        values = (0..target.iter().product())
            .map(|index| values[address(&dims, &coordinates(index, target))])
            .collect();
        dims = target.to_vec();
    }
    Ok(Input {
        tensor,
        dims,
        values,
    })
}

fn reference(left: (&[usize], &[f64]), right: (&[usize], &[f64])) -> (Vec<usize>, Vec<f64>) {
    let left_vector = left.0.len() == 1;
    let right_vector = right.0.len() == 1;
    let a = if left_vector {
        vec![1, left.0[0]]
    } else {
        left.0.to_vec()
    };
    let b = if right_vector {
        vec![right.0[0], 1]
    } else {
        right.0.to_vec()
    };
    let m = a[a.len() - 2];
    let k = a[a.len() - 1];
    let n = b[b.len() - 1];
    assert_eq!(k, b[b.len() - 2]);
    let batch_rank = a.len().max(b.len()) - 2;
    let mut batch = vec![1; batch_rank];
    for dims in [&a, &b] {
        let prefix = &dims[..dims.len() - 2];
        for (axis, &extent) in prefix.iter().enumerate() {
            let destination = batch_rank - prefix.len() + axis;
            assert!(batch[destination] == 1 || extent == 1 || batch[destination] == extent);
            if extent != 1 {
                batch[destination] = extent;
            }
        }
    }
    let mut output = batch.clone();
    if !left_vector {
        output.push(m);
    }
    if !right_vector {
        output.push(n);
    }
    let mut result = Vec::new();
    for item in 0..batch.iter().product() {
        let prefix = coordinates(item, &batch);
        for row in 0..m {
            for col in 0..n {
                let mut ac = prefix.clone();
                ac.extend([row, 0]);
                let mut bc = prefix.clone();
                bc.extend([0, col]);
                let mut total = 0.0;
                for inner in 0..k {
                    *ac.last_mut().unwrap() = inner;
                    bc[batch_rank] = inner;
                    total += left.1[address(&a, &ac)] * right.1[address(&b, &bc)];
                }
                result.push(total);
            }
        }
    }
    (output, result)
}
fn check_case<B: TensorLowBackend>(
    b: &B,
    dtype: LowDtype,
    left: Input<B>,
    right: Input<B>,
    distinguish_low: bool,
) -> Result<(), B::Error> {
    let (dims, expected) = reference((&left.dims, &left.values), (&right.dims, &right.values));
    let expected: Vec<f32> = expected.iter().map(|&x| x as f32).collect();
    let rounded: Vec<_> = expected.iter().map(|&x| round(dtype, x)).collect();
    if distinguish_low {
        assert!(
            expected.iter().zip(&rounded).any(|(&wide, &bits)| {
                (wide - decode(dtype, bits)).abs() > 3e-5 * wide.abs().max(1.0)
            }),
            "fixture must reject an already-low-rounded f32 result"
        );
    }
    let support = b.low_precision_support(dtype);
    if support.matmul_f32 {
        let product = b.matmul_low_f32(&left.tensor, &right.tensor)?;
        assert_eq!(product.shape(), &shape(&dims));
        super::super::check(&b.read_f32(&product)?, &expected);
    } else {
        assert!(b.matmul_low_f32(&left.tensor, &right.tensor).is_err());
    }
    if support.matmul {
        let product = b.matmul_low(&left.tensor, &right.tensor)?;
        assert_eq!(product.shape(), &shape(&dims));
        assert_eq!(product.low_dtype(), dtype);
        let actual = b.read_low_bits(&product)?;
        assert_eq!(actual.len(), rounded.len());
        for (index, (&got, &want)) in actual.iter().zip(&rounded).enumerate() {
            // Summation order may select either sign of an exact zero.
            assert!(
                got == want || (got & 0x7fff == 0 && want & 0x7fff == 0),
                "{dtype:?} final matmul rounding [{index}]: {got:#06x} != {want:#06x}"
            );
        }
    } else {
        assert!(b.matmul_low(&left.tensor, &right.tensor).is_err());
    }
    for input in [&left, &right] {
        let actual = b.read_low_bits(&input.tensor)?;
        let expected: Vec<_> = input
            .values
            .iter()
            .map(|&x| round(dtype, x as f32))
            .collect();
        assert_eq!(actual, expected, "matmul mutated an input view");
    }
    Ok(())
}
fn transposed(dims: &[usize], axes: &[usize], seed: usize) -> Host<f32> {
    let mut host = data(dims, seed);
    // Positive dyadics make larger dots expose low-output rounding in both formats.
    for value in &mut host.values {
        *value += 1.5;
    }
    host.permutation = Some(axes.to_vec());
    host
}

pub(super) fn check<B: TensorLowBackend>(b: &B, dtype: LowDtype) -> Result<(), B::Error> {
    // Dyadic /16 inputs give /256 products; for these sizes every partial
    // sum fits f32 exactly, independent of order/FMA. Exact final low rounding
    // therefore imposes no stronger accumulation order than the contract.
    for k in [65, 257] {
        check_case(
            b,
            dtype,
            prepare(b, dtype, transposed(&[k, 17], &[1, 0], 1), None)?,
            prepare(b, dtype, transposed(&[19, k], &[1, 0], 2), None)?,
            true,
        )?;
        check_case(
            b,
            dtype,
            prepare(b, dtype, data(&[k], 3), None)?,
            prepare(b, dtype, transposed(&[7, k], &[1, 0], 4), None)?,
            false,
        )?;
        check_case(
            b,
            dtype,
            prepare(b, dtype, transposed(&[2, k, 5], &[0, 2, 1], 5), None)?,
            prepare(b, dtype, data(&[k], 6), None)?,
            false,
        )?;
        let increment = if dtype == LowDtype::F16 {
            1. / 2048.
        } else {
            1. / 256.
        };
        let mut values = vec![increment; k];
        values[0] = 4096.;
        values[1] = 1.;
        values[2] = 0.;
        values[k - 1] = -4096.;
        check_case(
            b,
            dtype,
            prepare(
                b,
                dtype,
                Host {
                    shape: shape(&[k]),
                    values,
                    permutation: None,
                },
                None,
            )?,
            prepare(
                b,
                dtype,
                Host {
                    shape: shape(&[1]),
                    values: vec![1.],
                    permutation: None,
                },
                Some(&[k]),
            )?,
            true,
        )?;
    }
    check_case(
        b,
        dtype,
        prepare(
            b,
            dtype,
            transposed(&[2, 1, 65, 1, 5], &[0, 3, 1, 4, 2], 7),
            Some(&[2, 3, 1, 5, 65]),
        )?,
        prepare(
            b,
            dtype,
            transposed(&[1, 4, 7, 3, 65], &[0, 3, 1, 4, 2], 8),
            None,
        )?,
        false,
    )?;
    // Both contraction axes have actual zero strides, with distinct row/col values.
    check_case(
        b,
        dtype,
        prepare(b, dtype, data(&[3, 1], 9), Some(&[3, 257]))?,
        prepare(b, dtype, transposed(&[5, 1], &[1, 0], 10), Some(&[257, 5]))?,
        true,
    )?;
    for (a, bshape) in [
        (&[3, 0][..], &[0, 5][..]),
        (&[0, 65][..], &[65, 7][..]),
        (&[2, 1, 0, 65][..], &[1, 3, 65, 7][..]),
        (&[65, 0][..], &[0][..]),
    ] {
        check_case(
            b,
            dtype,
            prepare(b, dtype, data(a, 1), None)?,
            prepare(b, dtype, data(bshape, 2), None)?,
            false,
        )?;
    }
    let other = if dtype == LowDtype::F16 {
        LowDtype::Bf16
    } else {
        LowDtype::F16
    };
    for (a, rhs) in [
        (&[0][..], &[0][..]),
        (&[0, 3][..], &[3, 4][..]),
        (&[2, 0][..], &[0, 4][..]),
        (&[3, 2][..], &[2, 0][..]),
        (&[0, 2, 3][..], &[3, 4][..]),
    ] {
        let left = prepare(b, dtype, data(a, 1), None)?;
        let right = prepare(b, other, data(rhs, 2), None)?;
        assert!(b.matmul_low(&left.tensor, &right.tensor).is_err());
        assert!(b.matmul_low_f32(&left.tensor, &right.tensor).is_err());
    }
    // Empty result geometry still validates K, rank and batch compatibility.
    for (a, rhs) in [
        (&[0, 3][..], &[4, 0][..]),
        (&[2, 0, 3][..], &[3, 3, 0][..]),
        (&[][..], &[0, 3][..]),
    ] {
        let left = prepare(b, dtype, data(a, 1), None)?;
        let right = prepare(b, dtype, data(rhs, 2), None)?;
        assert!(b.matmul_low(&left.tensor, &right.tensor).is_err());
        assert!(b.matmul_low_f32(&left.tensor, &right.tensor).is_err());
    }
    accumulation::check(b, dtype)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oracle_handles_vectors_broadcasts_and_permutations_without_planner() {
        assert_eq!(
            reference((&[2], &[2., 3.]), (&[2], &[4., 5.])),
            (vec![], vec![23.])
        );
        assert_eq!(
            reference((&[2], &[2., 3.]), (&[2, 2], &[4., 5., 6., 7.])),
            (vec![2], vec![26., 31.])
        );
        assert_eq!(
            reference(
                (&[2, 1, 1, 2], &[1., 2., 3., 4.]),
                (&[1, 3, 2, 1], &[1., 1., 2., 2., 3., 3.])
            ),
            (vec![2, 3, 1, 1], vec![3., 6., 9., 7., 14., 21.])
        );
        let host = Host {
            shape: shape(&[2, 3]),
            values: vec![1., 2., 3., 4., 5., 6.],
            permutation: Some(vec![1, 0]),
        };
        assert_eq!(
            logical(&host, LowDtype::F16),
            (vec![3, 2], vec![1., 4., 2., 5., 3., 6.])
        );
        assert_eq!(
            reference((&[2, 0], &[]), (&[0, 3], &[])),
            (vec![2, 3], vec![0.; 6])
        );
    }
    #[test]
    fn tail_fixtures_distinguish_final_low_rounding_and_keep_f32_sums_exact() {
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            for k in [65, 257] {
                let (a, av) = logical(&transposed(&[k, 17], &[1, 0], 1), dtype);
                let (b, bv) = logical(&transposed(&[19, k], &[1, 0], 2), dtype);
                let (_, expected) = reference((&a, &av), (&b, &bv));
                assert!(expected.iter().any(|&x| {
                    (x - f64::from(decode(dtype, round(dtype, x as f32)))).abs()
                        > 3e-5 * x.abs().max(1.)
                }));
                assert!(expected.iter().all(|&x| f64::from(x as f32) == x));
            }
        }
    }
}
