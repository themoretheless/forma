use compute_cuda::{CudaError, CudaRuntime};
use tensor_core::{
    CompareOp, ScanOptions, Shape, TensorBackend, TensorEvalBackend, TensorIndexBackend, UnaryOp,
};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

#[test]
fn native_evaluate_fences_producers_and_validates_every_owner() -> Result<(), CudaError> {
    let backend = match CudaRuntime::new() {
        Ok(backend) => backend,
        Err(error @ CudaError::Unavailable(_)) => {
            assert!(
                !["CUDA_REQUIRED", "COMPUTE_REQUIRE_CUDA"]
                    .iter()
                    .any(|name| std::env::var(name).as_deref() == Ok("1")),
                "required CUDA evaluation unavailable: {error}"
            );
            eprintln!("SKIP native CUDA evaluation: {error}");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let input = backend.upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])?;
    let square = backend.unary(UnaryOp::Square, &input)?;
    let threshold = backend.upload_f32(shape(&[]), &[9.])?;
    let mask = backend.compare(CompareOp::Greater, &square, &threshold)?;
    let prefix = backend.scan_u32(&mask, 1, ScanOptions::default())?;
    let floats = backend.permute(&square, &[1, 0])?;
    let integers = backend.permute_u32(&prefix, &[1, 0])?;
    backend.evaluate(&[&floats, &floats], &[&integers])?;
    drop((input, square, threshold, mask, prefix));
    assert_eq!(backend.read_f32(&floats)?, [1., 16., 4., 25., 9., 36.]);
    assert_eq!(backend.read_u32(&integers)?, [0, 1, 0, 2, 0, 3]);

    let empty = backend.upload_f32(shape(&[0, 3]), &[])?;
    let empty_u32 = backend.upload_u32(shape(&[0, 3]), &[])?;
    let foreign_backend = CudaRuntime::new()?;
    let foreign = foreign_backend.upload_f32(shape(&[0, 3]), &[])?;
    let foreign_u32 = foreign_backend.upload_u32(shape(&[0, 3]), &[])?;
    assert!(matches!(
        backend.evaluate(&[&floats, &foreign], &[&integers]),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        backend.evaluate(&[&floats], &[&integers, &foreign_u32]),
        Err(CudaError::ForeignRuntime)
    ));
    backend.evaluate(&[&empty], &[&empty_u32])?;
    backend.evaluate(&[], &[])?;
    assert_eq!(backend.read_f32(&floats)?, [1., 16., 4., 25., 9., 36.]);
    Ok(())
}
