use super::{check, shape};
use crate::{HasShape, MatmulPrecision, TensorBackend};

/// Rank-one promotion fixtures with independent explicit dot-product references.
pub fn check_matmul_backend<B: TensorBackend>(b: &B) -> Result<(), B::Error> {
    let vector = b.upload_f32(shape(&[3]), &[2., -1., 3.])?;
    let other = b.upload_f32(shape(&[3]), &[4., 5., -2.])?;
    let dot = b.matmul(&vector, &other, MatmulPrecision::F32)?;
    assert_eq!(dot.shape(), &shape(&[]));
    check(&b.read_f32(&dot)?, &[-3.]);

    // A transposed matrix exercises vector promotion alongside strided storage.
    let matrix = b.upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])?;
    let mv = b.matmul(&matrix, &vector, MatmulPrecision::F32)?;
    assert_eq!(mv.shape(), &shape(&[2]));
    check(&b.read_f32(&mv)?, &[9., 21.]);
    let matrix_t = b.permute(&matrix, &[1, 0])?;
    let vm = b.matmul(&vector, &matrix_t, MatmulPrecision::F32)?;
    assert_eq!(vm.shape(), &shape(&[2]));
    check(&b.read_f32(&vm)?, &[9., 21.]);

    let values: Vec<f32> = (0..36).map(|i| (i % 11) as f32 - 4.).collect();
    let batches = b.upload_f32(shape(&[2, 3, 2, 3]), &values)?;
    let expected: Vec<f32> = values
        .as_chunks::<3>()
        .0
        .iter()
        .map(|v| 2. * v[0] - v[1] + 3. * v[2])
        .collect();
    let bm = b.matmul(&batches, &vector, MatmulPrecision::F32)?;
    assert_eq!(bm.shape(), &shape(&[2, 3, 2]));
    check(&b.read_f32(&bm)?, &expected);
    let bt = b.permute(&batches, &[0, 1, 3, 2])?;
    let vb = b.matmul(&vector, &bt, MatmulPrecision::F32)?;
    assert_eq!(vb.shape(), &shape(&[2, 3, 2]));
    check(&b.read_f32(&vb)?, &expected);

    // Zero-stride vectors retain their logical length after promotion.
    let scalar = b.upload_f32(shape(&[]), &[2.])?;
    let repeated = b.broadcast_to(&scalar, shape(&[3]))?;
    let repeated_dot = b.matmul(&repeated, &vector, MatmulPrecision::F32)?;
    check(&b.read_f32(&repeated_dot)?, &[8.]);

    let empty = b.upload_f32(shape(&[0]), &[])?;
    let empty_dot = b.matmul(&empty, &empty, MatmulPrecision::F32)?;
    assert_eq!(empty_dot.shape(), &shape(&[]));
    check(&b.read_f32(&empty_dot)?, &[0.]);
    let zero_matrix = b.upload_f32(shape(&[2, 0, 4]), &[])?;
    let zeros = b.matmul(&empty, &zero_matrix, MatmulPrecision::F32)?;
    assert_eq!(zeros.shape(), &shape(&[2, 4]));
    check(&b.read_f32(&zeros)?, &[0.; 8]);
    let zero_matrix_t = b.permute(&zero_matrix, &[0, 2, 1])?;
    let zeros = b.matmul(&zero_matrix_t, &empty, MatmulPrecision::F32)?;
    assert_eq!(zeros.shape(), &shape(&[2, 4]));
    check(&b.read_f32(&zeros)?, &[0.; 8]);
    let empty_batch = b.upload_f32(shape(&[0, 2, 3]), &[])?;
    let result = b.matmul(&empty_batch, &vector, MatmulPrecision::F32)?;
    assert_eq!(result.shape(), &shape(&[0, 2]));
    assert!(b.read_f32(&result)?.is_empty());
    let empty_batch_t = b.permute(&empty_batch, &[0, 2, 1])?;
    let result = b.matmul(&vector, &empty_batch_t, MatmulPrecision::F32)?;
    assert_eq!(result.shape(), &shape(&[0, 2]));
    assert!(b.read_f32(&result)?.is_empty());
    assert!(b.matmul(&scalar, &vector, MatmulPrecision::F32).is_err());
    assert!(b.matmul(&vector, &scalar, MatmulPrecision::F32).is_err());
    assert!(b.matmul(&vector, &matrix, MatmulPrecision::F32).is_err());
    Ok(())
}
