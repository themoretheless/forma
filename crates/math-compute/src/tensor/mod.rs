//! Resident geometry expressed once through WGSL, CUDA and MLX tensor contracts.
//!
//! `TensorMathF64` preserves the existing f64 coordinate domain on backends
//! implementing `TensorF64Backend`. `TensorMath` uses f32 point tensors with shape `[N, 3]`. It does not choose a
//! device, fall back to CPU, or change the existing f64 geometry API. Upload and
//! read methods are explicit host boundaries; intermediate tensors remain on
//! the backend. Arithmetic and reduction order follow that backend's f32
//! contract. Inputs and intermediate results must stay in the documented
//! numerical domain; converting f64 points can lose small coordinate differences.
//!
//! Use `TensorMath::new(&runtime)` with `compute_core::ComputeRuntime`,
//! `compute_cuda::CudaRuntime` or `compute_mlx::MlxBackend`. Recipes may submit
//! separate commands or build a lazy graph; this API does not promise fusion
//! or recorded replay. Neighbor evaluation fences prevent a lazy graph from
//! growing across every tile, while backend allocator caches remain independent.

use tensor_core::{BackendKind, HasShape, Shape, TensorBackend, TensorError};
mod float64;
pub use float64::{TensorCovariance, TensorMathF64};
mod geometry;
mod neighbors;
mod statistics;
/// Optional native adapters for callers that select them through math features.
#[cfg(all(feature = "tensor-cuda", not(target_arch = "wasm32")))]
pub use compute_cuda as cuda;
#[cfg(all(feature = "tensor-mlx", not(target_arch = "wasm32")))]
pub use compute_mlx as mlx;
pub use geometry::TensorBounds;
pub use neighbors::{NeighborOptions, TensorDirectedChamfer, TensorNeighbors};
pub use statistics::{TensorPointCloudStats, TensorPointMoments};

#[derive(Debug)]
pub enum TensorMathError<E> {
    Backend(E),
    Contract(TensorError),
    InvalidInput(&'static str),
}
impl<E: std::fmt::Display> std::fmt::Display for TensorMathError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(error) => write!(f, "tensor geometry backend: {error}"),
            Self::Contract(error) => error.fmt(f),
            Self::InvalidInput(message) => f.write_str(message),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for TensorMathError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(error) => Some(error),
            Self::Contract(error) => Some(error),
            Self::InvalidInput(_) => None,
        }
    }
}
impl<E> From<TensorError> for TensorMathError<E> {
    fn from(error: TensorError) -> Self {
        Self::Contract(error)
    }
}
pub type Result<T, E> = std::result::Result<T, TensorMathError<E>>;
pub(super) fn shape<E>(dims: &[usize]) -> Result<Shape, E> {
    Ok(Shape::new(dims.to_vec())?)
}

/// Domain recipes borrowing a caller-owned tensor backend. Results keep their
/// native ownership and can feed either another recipe or backend operations.
pub struct TensorMath<'a, B: TensorBackend> {
    pub(super) backend: &'a B,
}
impl<'a, B: TensorBackend> TensorMath<'a, B> {
    pub fn new(backend: &'a B) -> Self {
        Self { backend }
    }
    pub fn backend(&self) -> &'a B {
        self.backend
    }
    pub fn kind(&self) -> BackendKind {
        self.backend.kind()
    }
    pub(super) fn check_points(&self, points: &B::Tensor) -> Result<usize, B::Error> {
        match points.shape().dims() {
            &[count, 3] => Ok(count),
            _ => Err(TensorMathError::InvalidInput(
                "point tensor must have shape [N, 3]",
            )),
        }
    }
    /// Upload finite f32 coordinates. An empty cloud has shape `[0, 3]`.
    pub fn upload_points(&self, points: &[[f32; 3]]) -> Result<B::Tensor, B::Error> {
        if points.iter().flatten().any(|value| !value.is_finite()) {
            return Err(TensorMathError::InvalidInput(
                "point coordinates must be finite",
            ));
        }
        self.backend
            .upload_f32(shape(&[points.len(), 3])?, points.as_flattened())
            .map_err(TensorMathError::Backend)
    }
    /// Explicitly round finite f64 coordinates to f32. Rejects overflow, but
    /// cannot preserve differences below f32 resolution or all subnormal values.
    /// Use [`TensorMathF64::upload_points`] to retain binary64 coordinates.
    pub fn upload_points_f64(&self, points: &[crate::V3]) -> Result<B::Tensor, B::Error> {
        let mut values = Vec::with_capacity(points.len());
        for point in points {
            let narrowed = point.map(|value| value as f32);
            if narrowed.iter().any(|value| !value.is_finite()) {
                return Err(TensorMathError::InvalidInput(
                    "point coordinates must be finite and representable as f32",
                ));
            }
            values.push(narrowed);
        }
        self.upload_points(&values)
    }
    /// Complete the tensor's producers and copy point values to host memory.
    pub fn read_points(&self, points: &B::Tensor) -> Result<Vec<[f32; 3]>, B::Error> {
        self.check_points(points)?;
        let values = self
            .backend
            .read_f32(points)
            .map_err(TensorMathError::Backend)?;
        Ok(values.as_chunks::<3>().0.to_vec())
    }
}
