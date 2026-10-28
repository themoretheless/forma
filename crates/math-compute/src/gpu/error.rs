use gpu_compute::BackendReport;

#[derive(Debug)]
pub enum GpuMathError {
    Compute(compute_core::ComputeError),
    Buffer(gpu_compute::BufferError),
    Kernel(compute_core::KernelError),
    Readback(gpu_compute::ReadbackError),
    Device(String),
    #[cfg(not(target_arch = "wasm32"))]
    Tensor(crate::tensor::TensorMathError<compute_core::TensorComputeError>),
    InvalidInput(&'static str),
}
impl std::fmt::Display for GpuMathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compute(e) => e.fmt(f),
            Self::Buffer(e) => e.fmt(f),
            Self::Kernel(e) => e.fmt(f),
            Self::Readback(e) => e.fmt(f),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tensor(e) => e.fmt(f),
            Self::Device(e) => f.write_str(e),
            Self::InvalidInput(e) => f.write_str(e),
        }
    }
}
impl std::error::Error for GpuMathError {}
impl From<compute_core::KernelError> for GpuMathError {
    fn from(e: compute_core::KernelError) -> Self {
        Self::Kernel(e)
    }
}
impl From<gpu_compute::ReadbackError> for GpuMathError {
    fn from(e: gpu_compute::ReadbackError) -> Self {
        Self::Readback(e)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuArithmetic {
    F32,
    /// Full binary64 arithmetic implemented with integer GPU shaders.
    SoftwareBinary64,
}
/// Report for actual successful execution, not requested placement. Existing
/// convenience APIs return only the value and preserve their CPU fallback path.
pub struct MathExecution<T> {
    pub value: T,
    pub backend: BackendReport,
    pub arithmetic: GpuArithmetic,
}

impl From<compute_core::ComputeError> for GpuMathError {
    fn from(e: compute_core::ComputeError) -> Self {
        Self::Compute(e)
    }
}
impl From<gpu_compute::BufferError> for GpuMathError {
    fn from(e: gpu_compute::BufferError) -> Self {
        Self::Buffer(e)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<crate::tensor::TensorMathError<compute_core::TensorComputeError>> for GpuMathError {
    fn from(e: crate::tensor::TensorMathError<compute_core::TensorComputeError>) -> Self {
        Self::Tensor(e)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl From<compute_core::TensorComputeError> for GpuMathError {
    fn from(e: compute_core::TensorComputeError) -> Self {
        Self::Tensor(crate::tensor::TensorMathError::Backend(e))
    }
}
