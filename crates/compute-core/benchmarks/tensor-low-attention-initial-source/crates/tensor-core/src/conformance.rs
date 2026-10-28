//! The same numerical and shape fixtures run through every native backend.
use crate::{BinaryOp, HasShape, MatmulPrecision, Shape, TensorBackend, UnaryOp};

mod attention;
mod indexing;
mod low_attention;
mod low_index;
mod low_ops;
mod low_precision;
mod low_scatter;
mod low_statistics;
mod reduction;
mod scatter;
mod statistics;
mod vectors;
pub use attention::check_attention_backend;
pub use indexing::check_index_backend;
pub use low_attention::check_low_attention_backend;
pub use low_index::check_low_index_backend;
pub use low_ops::check_low_ops_backend;
pub use low_precision::check_low_backend;
pub use low_scatter::check_low_scatter_backend;
pub use low_statistics::check_low_stats_backend;
pub use reduction::check_reduce_backend;
pub use scatter::check_scatter_backend;
pub use statistics::check_stats_backend;
pub use vectors::check_matmul_backend;

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn check(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &b)) in actual.iter().zip(expected).enumerate() {
        assert!(
            a.is_finite() && (a - b).abs() <= 3e-5 * b.abs().max(1.0),
            "element {i}: {a} != {b}"
        );
    }
}

/// Runs common fixtures without relying on a backend's private implementation.
/// The caller must separately require the intended physical device to exist.
pub fn check_backend<B: TensorBackend>(backend: &B) -> Result<(), B::Error> {
    let values: Vec<f32> = (0..24).map(|i| i as f32 / 4.0 - 2.0).collect();
    let input = backend.upload_f32(shape(&[2, 3, 4]), &values)?;
    let transposed = backend.permute(&input, &[1, 0, 2])?;
    let logical: Vec<f32> = (0..3)
        .flat_map(|j| {
            (0..2).flat_map(move |i| (0..4).map(move |k| (i * 12 + j * 4 + k) as f32 / 4.0 - 2.0))
        })
        .collect();
    check(&backend.read_f32(&transposed)?, &logical);
    let flattened = backend.reshape(&transposed, shape(&[24]))?;
    check(&backend.read_f32(&flattened)?, &logical);
    let bias = backend.upload_f32(shape(&[1, 2, 1]), &[-0.5, 1.5])?;
    let added = backend.binary(BinaryOp::Add, &transposed, &bias)?;
    assert_eq!(added.shape(), &shape(&[3, 2, 4]));
    let expected: Vec<f32> = logical
        .iter()
        .enumerate()
        .map(|(i, &x)| x + [-0.5, 1.5][i / 4 % 2])
        .collect();
    check(&backend.read_f32(&added)?, &expected);
    let reduced = backend.sum_axes(&added, &[0, 2], false)?;
    let mut totals = [0.0_f64; 2];
    for (i, &x) in expected.iter().enumerate() {
        totals[i / 4 % 2] += f64::from(x);
    }
    assert_eq!(reduced.shape(), &shape(&[2]));
    check(&backend.read_f32(&reduced)?, &totals.map(|x| x as f32));
    let kept = backend.sum_axes(&added, &[2, 0], true)?;
    assert_eq!(kept.shape(), &shape(&[1, 2, 1]));
    check(&backend.read_f32(&kept)?, &totals.map(|x| x as f32));
    assert!(backend.sum_axes(&input, &[0, 0], false).is_err());
    assert!(backend.permute(&input, &[0, 0, 2]).is_err());

    let positive = [0.25_f32, 0.5, 1.0, 2.0];
    let tensor = backend.upload_f32(shape(&[4]), &positive)?;
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
        let expected: Vec<f32> = positive
            .iter()
            .map(|&x| {
                let x = f64::from(x);
                (match op {
                    UnaryOp::Negate => -x,
                    UnaryOp::Abs => x.abs(),
                    UnaryOp::Square => x * x,
                    UnaryOp::Sqrt => x.sqrt(),
                    UnaryOp::Reciprocal => x.recip(),
                    UnaryOp::Exp => x.exp(),
                    UnaryOp::Log => x.ln(),
                    UnaryOp::Sin => x.sin(),
                    UnaryOp::Cos => x.cos(),
                }) as f32
            })
            .collect();
        check(&backend.read_f32(&backend.unary(op, &tensor)?)?, &expected);
    }
    let left = backend.upload_f32(shape(&[2, 1]), &[1.0, 4.0])?;
    let right = backend.upload_f32(shape(&[1, 3]), &[2.0, 3.0, 5.0])?;
    for op in [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Min,
        BinaryOp::Max,
    ] {
        let mut expected = Vec::new();
        for a in [1.0_f32, 4.0] {
            for b in [2.0_f32, 3.0, 5.0] {
                expected.push(match op {
                    BinaryOp::Add => a + b,
                    BinaryOp::Subtract => a - b,
                    BinaryOp::Multiply => a * b,
                    BinaryOp::Divide => a / b,
                    BinaryOp::Min => a.min(b),
                    BinaryOp::Max => a.max(b),
                });
            }
        }
        check(
            &backend.read_f32(&backend.binary(op, &left, &right)?)?,
            &expected,
        );
    }

    let empty = backend.upload_f32(shape(&[2, 0, 3]), &[])?;
    let zero_sum = backend.sum_axes(&empty, &[1], false)?;
    assert_eq!(zero_sum.shape(), &shape(&[2, 3]));
    check(&backend.read_f32(&zero_sum)?, &[0.0; 6]);
    let scalar = backend.upload_f32(shape(&[]), &[2.0])?;
    assert_eq!(scalar.shape().numel(), 1);
    let broadcast = backend.broadcast_to(&scalar, shape(&[2, 3]))?;
    check(&backend.read_f32(&broadcast)?, &[2.0; 6]);
    let empty_added = backend.binary(BinaryOp::Add, &scalar, &empty)?;
    assert_eq!(empty_added.shape(), &shape(&[2, 0, 3]));
    assert!(backend.read_f32(&empty_added)?.is_empty());

    let a_values: Vec<f32> = (0..12).map(|i| i as f32 - 3.0).collect();
    let a = backend.upload_f32(shape(&[2, 2, 3]), &a_values)?;
    let b = backend.upload_f32(shape(&[1, 2, 3]), &[1., 2., 3., 4., 5., 6.])?;
    let b = backend.permute(&b, &[0, 2, 1])?;
    let product = backend.matmul(&a, &b, MatmulPrecision::F32)?;
    assert_eq!(product.shape(), &shape(&[2, 2, 2]));
    let mut expected = Vec::new();
    for batch in 0..2 {
        for row in 0..2 {
            for column in 0..2 {
                expected.push(
                    (0..3)
                        .map(|k| {
                            f64::from(a_values[batch * 6 + row * 3 + k])
                                * (column * 3 + k + 1) as f64
                        })
                        .sum::<f64>() as f32,
                );
            }
        }
    }
    check(&backend.read_f32(&product)?, &expected);
    let az = backend.upload_f32(shape(&[2, 3, 0]), &[])?;
    let bz = backend.upload_f32(shape(&[1, 0, 4]), &[])?;
    let zero_product = backend.matmul(&az, &bz, MatmulPrecision::F32)?;
    assert_eq!(zero_product.shape(), &shape(&[2, 3, 4]));
    check(&backend.read_f32(&zero_product)?, &[0.0; 24]);
    check_matmul_backend(backend)
}
