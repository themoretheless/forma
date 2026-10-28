use super::shape;
use crate::{BinaryOp, Float64Support, HasShape, ReduceOp, TensorF64Backend, UnaryOp};

fn close(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (&a, &e) in actual.iter().zip(expected) {
        assert!(
            a.is_finite() && (a - e).abs() <= 2e-13 * e.abs().max(f64::MIN_POSITIVE),
            "{a:?} != {e:?}"
        );
    }
}
pub(super) fn bits(actual: &[f64], expected: &[f64]) {
    assert_eq!(
        actual.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        expected.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
    );
}

/// Native/software binary64 qualification. Includes precision/range witnesses
/// that a backend silently narrowing values or accumulators to f32 cannot pass.
/// Callers must require the actual intended device separately.
pub fn check_f64_backend<B: TensorF64Backend>(b: &B) -> Result<(), B::Error> {
    assert_ne!(b.f64_support(), Float64Support::Unsupported);
    let values = [
        100_000_000.,
        100_000_001.,
        1e200,
        1e-200,
        -0.,
        f64::from_bits(1),
    ];
    let input = b.upload_f64(shape(&[2, 3]), &values)?;
    bits(&b.read_f64(&input)?, &values);
    let transposed = b.permute_f64(&input, &[1, 0])?;
    let flat = b.reshape_f64(&transposed, shape(&[6]))?;
    bits(
        &b.read_f64(&flat)?,
        &[
            values[0], values[3], values[1], values[4], values[2], values[5],
        ],
    );
    let slice = b.narrow_f64(&flat, 0, 1, 3)?;
    bits(&b.read_f64(&slice)?, &[values[3], values[1], values[4]]);
    let repeated = b.broadcast_f64(&b.upload_f64(shape(&[1]), &[1e200])?, shape(&[3]))?;
    bits(&b.read_f64(&b.materialize_f64(&repeated)?)?, &[1e200; 3]);
    let a = b.upload_f64(shape(&[2, 1]), &[100_000_001., 100_000_003.])?;
    let c = b.upload_f64(shape(&[2]), &[100_000_000., 100_000_002.])?;
    bits(
        &b.read_f64(&b.binary_f64(BinaryOp::Subtract, &a, &c)?)?,
        &[1., -1., 3., 1.],
    );
    let a = b.upload_f64(shape(&[3]), &[1. + 2_f64.powi(-40), 1e200, 1e-200])?;
    let c = b.upload_f64(shape(&[3]), &[2., 1e-100, 1e100])?;
    let av = b.read_f64(&a)?;
    let cv = b.read_f64(&c)?;
    for op in [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Min,
        BinaryOp::Max,
    ] {
        let expected: Vec<_> = av
            .iter()
            .zip(&cv)
            .map(|(&x, &y)| match op {
                BinaryOp::Add => x + y,
                BinaryOp::Subtract => x - y,
                BinaryOp::Multiply => x * y,
                BinaryOp::Divide => x / y,
                BinaryOp::Min => x.min(y),
                BinaryOp::Max => x.max(y),
            })
            .collect();
        close(&b.read_f64(&b.binary_f64(op, &a, &c)?)?, &expected);
    }
    let numbers = [0.125, 0.5, 1. + 2_f64.powi(-30), 3.];
    let x = b.upload_f64(shape(&[4]), &numbers)?;
    for op in [
        UnaryOp::Negate,
        UnaryOp::Abs,
        UnaryOp::Square,
        UnaryOp::Sqrt,
        UnaryOp::Reciprocal,
        UnaryOp::Exp,
        UnaryOp::Log,
        UnaryOp::Sin,
        UnaryOp::Cos,
    ] {
        let expected: Vec<_> = numbers
            .iter()
            .map(|&v| match op {
                UnaryOp::Negate => -v,
                UnaryOp::Abs => v.abs(),
                UnaryOp::Square => v * v,
                UnaryOp::Sqrt => v.sqrt(),
                UnaryOp::Reciprocal => 1. / v,
                UnaryOp::Exp => v.exp(),
                UnaryOp::Log => v.ln(),
                UnaryOp::Sin => v.sin(),
                UnaryOp::Cos => v.cos(),
            })
            .collect();
        close(&b.read_f64(&b.unary_f64(op, &x)?)?, &expected);
    }
    // Large multi-pass reduction: all terms and the exact expected sum are dyadic.
    let n = 8193;
    let unit = 1. + 2_f64.powi(-35);
    let many = b.upload_f64(shape(&[n]), &vec![unit; n])?;
    bits(
        &b.read_f64(&b.reduce_f64(ReduceOp::Sum, &many, &[0], false)?)?,
        &[n as f64 * unit],
    );
    bits(&b.read_f64(&b.mean_f64(&many, &[0], false)?)?, &[unit]);
    let grid = b.upload_f64(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])?;
    let grid = b.permute_f64(&grid, &[1, 0])?;
    for (op, expected) in [
        (ReduceOp::Sum, [5., 7., 9.]),
        (ReduceOp::Product, [4., 10., 18.]),
        (ReduceOp::Min, [1., 2., 3.]),
        (ReduceOp::Max, [4., 5., 6.]),
    ] {
        let reduced = b.reduce_f64(op, &grid, &[1], true)?;
        assert_eq!(reduced.shape(), &shape(&[3, 1]));
        bits(&b.read_f64(&reduced)?, &expected);
    }
    let signed = b.upload_f64(shape(&[2]), &[-0., 0.])?;
    for (op, expected) in [(ReduceOp::Min, -0.), (ReduceOp::Max, 0.)] {
        bits(
            &b.read_f64(&b.reduce_f64(op, &signed, &[0], false)?)?,
            &[expected],
        );
    }
    let empty = b.upload_f64(shape(&[2, 0]), &[])?;
    for (op, expected) in [(ReduceOp::Sum, 0.), (ReduceOp::Product, 1.)] {
        bits(
            &b.read_f64(&b.reduce_f64(op, &empty, &[1], false)?)?,
            &[expected; 2],
        );
    }
    assert!(b.reduce_f64(ReduceOp::Min, &empty, &[1], false).is_err());
    assert!(b.mean_f64(&empty, &[1], false).is_err());
    assert!(b.reduce_f64(ReduceOp::Sum, &grid, &[1, 1], false).is_err());
    bits(
        &b.read_f64(&b.reduce_f64(ReduceOp::Sum, &input, &[], false)?)?,
        &values,
    );
    // Binary64 products, output and vector promotion. f32 loses the result 1.
    let left = b.upload_f64(shape(&[2]), &[100_000_001., 100_000_000.])?;
    let right = b.upload_f64(shape(&[2]), &[1., -1.])?;
    let dot = b.matmul_f64(&left, &right)?;
    assert_eq!(dot.shape(), &shape(&[]));
    bits(&b.read_f64(&dot)?, &[1.]);
    let small_addend = 2_f64.powi(-40);
    let precise = b.upload_f64(shape(&[1, 2]), &[1., small_addend])?;
    let ones = b.upload_f64(shape(&[2, 1]), &[1.; 2])?;
    bits(
        &b.read_f64(&b.matmul_f64(&precise, &ones)?)?,
        &[1. + small_addend],
    );
    let offset_left = b.narrow_f64(
        &b.upload_f64(shape(&[3, 2]), &[99., 99., 1., 2., 3., 4.])?,
        0,
        1,
        2,
    )?;
    let offset_right = b.narrow_f64(&b.upload_f64(shape(&[3, 1]), &[99., 2., 3.])?, 0, 1, 2)?;
    bits(
        &b.read_f64(&b.matmul_f64(&offset_left, &offset_right)?)?,
        &[8., 18.],
    );
    let wide = b.upload_f64(shape(&[1, 2]), &[1e200, 2e200])?;
    let small = b.upload_f64(shape(&[2, 1]), &[1e-100, 1e-100])?;
    close(&b.read_f64(&b.matmul_f64(&wide, &small)?)?, &[3e100]);
    let matrices = b.upload_f64(shape(&[2, 2, 2]), &[1., 2., 3., 4., 5., 6., 7., 8.])?;
    let matrices = b.permute_f64(&matrices, &[0, 2, 1])?;
    let weights = b.upload_f64(shape(&[2]), &[1., 2.])?;
    bits(
        &b.read_f64(&b.matmul_f64(&matrices, &weights)?)?,
        &[7., 10., 19., 22.],
    );
    let z = b.upload_f64(shape(&[0, 3]), &[])?;
    bits(&b.read_f64(&b.matmul_f64(&empty, &z)?)?, &[0.; 6]);
    let pending = b.unary_f64(UnaryOp::Negate, &many)?;
    b.evaluate_f64(&[&pending, &dot])?;
    bits(&b.read_f64(&pending)?, &vec![-unit; n]);
    let pending = b.binary_f64(BinaryOp::Subtract, &left, &left)?;
    b.evaluate_f64(&[])?;
    bits(&b.read_f64(&pending)?, &[0.; 2]);
    Ok(())
}
