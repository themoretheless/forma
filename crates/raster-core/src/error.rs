//! Fallible resource and readback errors returned by [`crate::rasterizer::Rasterizer`].

/// Errors raised when allocating/initializing GPU raster buffers or reading
/// back rendered pixels.
#[derive(Debug)]
pub enum RasterError {
    Buffer(gpu_compute::BufferError),
    Readback(gpu_compute::ReadbackError),
}

impl std::fmt::Display for RasterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buffer(error) => error.fmt(f),
            Self::Readback(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for RasterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Buffer(error) => Some(error),
            Self::Readback(error) => Some(error),
        }
    }
}

impl From<gpu_compute::BufferError> for RasterError {
    fn from(error: gpu_compute::BufferError) -> Self {
        Self::Buffer(error)
    }
}

impl From<gpu_compute::ReadbackError> for RasterError {
    fn from(error: gpu_compute::ReadbackError) -> Self {
        Self::Readback(error)
    }
}
