use compute_core::{ComputeError, ComputeRuntime, TensorComputeError, gpu_compute::GpuContext};
use tensor_core::{MatmulPrecision, Shape, TensorBackend};

fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    assert!(
        context.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "shader GPU required"
    );
    context
}

#[test]
fn shader_backend_satisfies_shared_tensor_conformance() {
    let Some(context) = context() else { return };
    eprintln!(
        "WGSL tensor conformance backend: {:?}",
        context.backend_report()
    );
    let runtime = ComputeRuntime::new(&context).unwrap();
    tensor_core::conformance::check_backend(&runtime).unwrap();
}

#[test]
fn shader_tensor_backend_rejects_foreign_views_and_reduced_precision() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let other = ComputeRuntime::new(&context).unwrap();
    let shape = Shape::new(vec![1, 1]).unwrap();
    let foreign = other.upload_f32(shape.clone(), &[2.0]).unwrap();
    assert!(matches!(
        runtime.read_f32(&foreign),
        Err(TensorComputeError::Compute(ComputeError::ForeignArray))
    ));
    assert!(runtime.reshape(&foreign, shape.clone()).is_err());
    assert!(runtime.permute(&foreign, &[1, 0]).is_err());
    assert!(runtime.broadcast_to(&foreign, shape.clone()).is_err());
    let local = runtime.upload_f32(shape.clone(), &[3.0]).unwrap();
    for precision in [
        MatmulPrecision::AllowTf32,
        MatmulPrecision::AllowF16,
        MatmulPrecision::AllowBf16,
    ] {
        assert!(
            matches!(runtime.matmul(&local, &local, precision), Err(TensorComputeError::UnsupportedPrecision(mode)) if mode == precision)
        );
    }
    assert!(runtime.upload_f32(shape, &[1.0, 2.0]).is_err());
    assert_eq!(runtime.read_f32(&local).unwrap(), [3.0]);
}
