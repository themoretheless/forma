#![cfg(all(
    not(target_arch = "wasm32"),
    any(feature = "gpu", feature = "tensor-mlx", feature = "tensor-cuda")
))]

#[path = "tensor_math/geometry.rs"]
mod geometry;
#[path = "tensor_math/neighbors.rs"]
mod neighbors;
use math_compute::tensor::{NeighborOptions, TensorMath};
use tensor_core::{
    Shape, TensorEvalBackend, TensorReduceBackend, TensorScatterBackend, TensorStatsBackend,
};
type Result = std::result::Result<(), Box<dyn std::error::Error>>;

fn exercise<B>(backend: &B, other: &B) -> Result
where
    B: TensorReduceBackend + TensorScatterBackend + TensorStatsBackend + TensorEvalBackend,
    B::Error: 'static,
{
    geometry::run(backend)?;
    neighbors::run(backend)?;
    let math = TensorMath::new(backend);
    for points in [[[f32::NAN, 0., 0.]], [[f32::INFINITY, 0., 0.]]] {
        assert!(math.upload_points(&points).is_err());
    }
    assert!(math.upload_points_f64(&[[f64::MAX, 0., 0.]]).is_err());
    let foreign = other.upload_f32(Shape::new(vec![1, 3])?, &[0., 0., 0.])?;
    let local = math.upload_points(&[[1., 2., 3.]])?;
    assert!(math.squared_distance_pairs(&local, &foreign).is_err());
    assert!(math.bounds(&foreign).is_err());
    assert!(math.read_points(&foreign).is_err());
    assert!(
        math.nearest(&local, &foreign, 1, NeighborOptions::default())
            .is_err()
    );
    let empty = math.upload_points(&[])?;
    assert!(
        math.nearest(&empty, &foreign, 1, NeighborOptions::default())
            .is_err()
    );
    let rounded = math.upload_points_f64(&[[0.125, -0.25, 0.5]])?;
    assert_eq!(math.read_points(&rounded)?, [[0.125, -0.25, 0.5]]);
    Ok(())
}

#[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
#[test]
fn wgsl_resident_geometry_contract() -> Result {
    let Some(context) = gpu_compute::GpuContext::new() else {
        assert_ne!(
            std::env::var("COMPUTE_REQUIRE_GPU").as_deref(),
            Ok("1"),
            "required WGSL device unavailable"
        );
        eprintln!("SKIP tensor geometry WGSL: device unavailable");
        return Ok(());
    };
    let session = math_compute::gpu::MathGpuSession::new(&context);
    let math = session.tensor()?;
    let other = compute_core::ComputeRuntime::new(&context)?;
    eprintln!("tensor geometry WGSL: {:?}", context.backend_report());
    exercise(math.backend(), &other)
}

#[cfg(all(feature = "tensor-mlx", not(target_arch = "wasm32")))]
#[test]
fn mlx_resident_geometry_contract() -> Result {
    let backend = match compute_mlx::MlxBackend::new_gpu() {
        Ok(backend) => backend,
        Err(compute_mlx::MlxError::Unavailable(message)) => {
            assert_ne!(
                std::env::var("COMPUTE_REQUIRE_MLX").as_deref(),
                Ok("1"),
                "required MLX unavailable: {message}"
            );
            eprintln!("SKIP tensor geometry MLX: {message}");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let other = compute_mlx::MlxBackend::new_gpu()?;
    eprintln!("tensor geometry MLX: GPU runtime {}", backend.version());
    exercise(&backend, &other)
}

#[cfg(all(feature = "tensor-cuda", not(target_arch = "wasm32")))]
#[test]
fn cuda_resident_geometry_contract() -> Result {
    let backend = match compute_cuda::CudaRuntime::new() {
        Ok(backend) => backend,
        Err(error @ compute_cuda::CudaError::Unavailable(_)) => {
            assert!(
                !["COMPUTE_REQUIRE_CUDA", "CUDA_REQUIRED"]
                    .iter()
                    .any(|name| std::env::var(name).as_deref() == Ok("1")),
                "required CUDA tensor geometry unavailable: {error}"
            );
            eprintln!("SKIP tensor geometry CUDA: {error}");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let other = compute_cuda::CudaRuntime::new()?;
    eprintln!("tensor geometry CUDA: {:?}", backend.capabilities());
    exercise(&backend, &other)
}
