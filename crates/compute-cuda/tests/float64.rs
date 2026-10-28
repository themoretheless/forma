use compute_cuda::{CudaError, CudaRuntime};
use tensor_core::{
    BinaryOp, Float64Support, ReduceOp, Shape, TensorBackend, TensorF64Backend, UnaryOp,
};
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn runtime() -> Result<Option<CudaRuntime>, CudaError> {
    match CudaRuntime::new() {
        Ok(runtime) => Ok(Some(runtime)),
        Err(error @ CudaError::Unavailable(_)) => {
            assert!(
                !["CUDA_REQUIRED", "COMPUTE_REQUIRE_CUDA"]
                    .iter()
                    .any(|name| std::env::var(name).as_deref() == Ok("1")),
                "required native CUDA f64 unavailable: {error}"
            );
            eprintln!("SKIP native CUDA f64 qualification: {error}");
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
#[test]
fn native_binary64_shared_contract() -> Result<(), CudaError> {
    let Some(b) = runtime()? else {
        return Ok(());
    };
    assert_eq!(b.f64_support(), Float64Support::Native);
    eprintln!("CUDA f64 device: {:?}", b.capabilities());
    tensor_core::conformance::check_f64_backend(&b)
}
#[test]
fn native_binary64_owners_writes_and_scalar_abi() -> Result<(), CudaError> {
    let Some(b) = runtime()? else {
        return Ok(());
    };
    let other = CudaRuntime::new()?;
    let foreign = other.upload_f64(shape(&[0, 3]), &[])?;
    let local = b.upload_f64(shape(&[0, 3]), &[])?;
    let weights = b.upload_f64(shape(&[3, 1]), &[1.; 3])?;
    assert!(matches!(
        b.read_f64(&foreign),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.binary_f64(BinaryOp::Add, &local, &foreign),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.unary_f64(UnaryOp::Abs, &foreign),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.reduce_f64(ReduceOp::Sum, &foreign, &[0], false),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.matmul_f64(&foreign, &weights),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.evaluate_f64(&[&local, &foreign]),
        Err(CudaError::ForeignRuntime)
    ));
    let mut values = b.upload_f64(shape(&[3]), &[1., 2., 3.])?;
    let alias = b.narrow_f64(&values, 0, 1, 2)?;
    assert!(matches!(
        b.write_f64(&mut values, &[4., 5., 6.]),
        Err(CudaError::SharedOutput)
    ));
    drop(alias);
    b.write_f64(&mut values, &[1., 2., 3.])?;
    let result = b.affine_f64(&values, 1. + 2_f64.powi(-40), 1e-20)?;
    assert_eq!(
        b.read_f64(&result)?,
        [1., 2., 3.].map(|x| x * (1. + 2_f64.powi(-40)) + 1e-20)
    );
    Ok(())
}
