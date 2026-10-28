use compute_cuda::{CudaError, CudaPrepareOptions, CudaRuntime};
use tensor_core::{
    BinaryOp, CompareOp, LowDtype, ReduceOp, Shape, TensorBackend, TensorIndexBackend,
    TensorLowBackend, UnaryOp,
};
#[path = "prepared_typed/low.rs"]
mod low;
#[path = "prepared_typed/unsigned.rs"]
mod unsigned;
#[path = "prepared_typed/validation.rs"]
mod validation;
type Result<T = ()> = std::result::Result<T, CudaError>;
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

/// Native execution is mandatory under CUDA_REQUIRED. A host-only green result
/// accompanied by SKIP is compilation evidence, never numerical qualification.
#[test]
fn native_cuda_prepared_typed_contract() -> Result {
    let rt = match CudaRuntime::new() {
        Ok(rt) => rt,
        Err(error @ CudaError::Unavailable(_)) => {
            assert!(
                !["CUDA_REQUIRED", "COMPUTE_REQUIRE_CUDA"]
                    .iter()
                    .any(|name| std::env::var(name).as_deref() == Ok("1")),
                "required native typed CUDA execution unavailable: {error}"
            );
            eprintln!("SKIP native typed prepared CUDA qualification: {error}");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    eprintln!("Typed prepared CUDA hardware: {:?}", rt.capabilities());
    unsigned::arithmetic_views_and_replay(&rt)?;
    unsigned::reductions(&rt)?;
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        low::raw_bits_casts_and_views(&rt, dtype)?;
        low::arithmetic_rounding_and_reductions(&rt, dtype)?;
        low::matrix_products(&rt, dtype)?;
    }
    validation::mixed_bindings_and_recovery(&rt)?;
    rt.synchronize()
}
