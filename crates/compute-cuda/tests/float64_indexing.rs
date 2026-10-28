use compute_cuda::{CudaError, CudaRuntime};
use tensor_core::{
    CompareOp, ScanOptions, ScatterOp, Shape, TensorF64Backend, TensorF64IndexBackend,
    TensorF64ScatterBackend, TensorIndexBackend,
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
                "required native CUDA f64 indexing unavailable: {error}"
            );
            eprintln!("SKIP native CUDA f64 indexing: {error}");
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
#[test]
fn native_f64_indexing_and_scatter_contracts() -> Result<(), CudaError> {
    let Some(b) = runtime()? else {
        return Ok(());
    };
    eprintln!("CUDA f64 indexing device: {:?}", b.capabilities());
    tensor_core::conformance::check_f64_index_backend(&b)?;
    tensor_core::conformance::check_f64_scatter_backend(&b)
}
#[test]
fn native_f64_indexing_rejects_foreign_inputs_even_for_empty_results() -> Result<(), CudaError> {
    let Some(b) = runtime()? else {
        return Ok(());
    };
    let other = CudaRuntime::new()?;
    let local = b.upload_f64(shape(&[0]), &[])?;
    let foreign = other.upload_f64(shape(&[0]), &[])?;
    let mask = b.upload_u32(shape(&[0]), &[])?;
    let foreign_mask = other.upload_u32(shape(&[0]), &[])?;
    assert!(matches!(
        b.compare_f64(CompareOp::Less, &local, &foreign),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.select_f64(&foreign_mask, &local, &local),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.select_f64(&mask, &foreign, &local),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.select_f64(&mask, &local, &foreign),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.scan_f64(&foreign, 0, ScanOptions::default()),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.gather_f64(&local, &foreign_mask, 0),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.gather_f64(&foreign, &mask, 0),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.compact_f64(&local, &foreign_mask),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.compact_f64(&foreign, &mask),
        Err(CudaError::ForeignRuntime)
    ));
    for op in [
        ScatterOp::Replace,
        ScatterOp::Add,
        ScatterOp::Multiply,
        ScatterOp::Min,
        ScatterOp::Max,
    ] {
        assert!(matches!(
            b.scatter_f64(op, &foreign, &mask, &local, 0),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            b.scatter_f64(op, &local, &foreign_mask, &local, 0),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            b.scatter_f64(op, &local, &mask, &foreign, 0),
            Err(CudaError::ForeignRuntime)
        ));
    }
    Ok(())
}
#[test]
fn native_f64_indices_offsets_and_repeated_calls_keep_values_independent() -> Result<(), CudaError>
{
    let Some(b) = runtime()? else {
        return Ok(());
    };
    let input = b.narrow(
        &b.upload_f64(shape(&[4]), &[99., 1e200, 100_000_001., 99.])?,
        0,
        1,
        2,
    )?;
    let indices = b.narrow(&b.upload_u32(shape(&[5]), &[99, 1, 0, 1, 99])?, 0, 1, 3)?;
    let updates = b.narrow(
        &b.upload_f64(shape(&[5]), &[99., 1e-200, 3., 100_000_003., 99.])?,
        0,
        1,
        3,
    )?;
    let saved = b.scatter_f64(ScatterOp::Replace, &input, &indices, &updates, 0)?;
    assert_eq!(b.read_f64(&saved.values)?, [3., 100_000_003.]);
    let other = b.upload_f64(shape(&[]), &[7.])?;
    let next = b.scatter_f64(ScatterOp::Replace, &input, &indices, &other, 0)?;
    assert_eq!(b.read_f64(&next.values)?, [7., 7.]);
    assert_eq!(b.read_f64(&saved.values)?, [3., 100_000_003.]);
    assert_eq!(b.read_f64(&input)?, [1e200, 100_000_001.]);
    assert_eq!(b.read_u32(&saved.invalid_count)?, [0]);
    assert_eq!(
        b.read_f64(&b.gather_f64(&input, &indices, 0)?.values)?,
        [100_000_001., 1e200, 100_000_001.]
    );
    Ok(())
}
