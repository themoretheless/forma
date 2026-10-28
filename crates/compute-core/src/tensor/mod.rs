//! Resident f32 and u32 tensors with validated, backend-neutral shape and layout metadata.
//! View transformations only change metadata; materialization and arithmetic
//! record shader work into the same reusable `ComputeProgram` as array kernels.

mod attention;
mod convolution;
pub(crate) use attention::AttentionKernels;
mod index_kernels;
mod indexing;
mod kernels;
mod low;
mod matmul;
mod normalization;
pub(crate) use index_kernels::TensorIndexKernels;
pub use low::GpuLowTensor;
pub(crate) use low::LowKernels;
pub(crate) use normalization::StatsKernels;
mod program;
mod reduction;
mod scatter_kernels;
use crate::{ComputeError, GpuArray, GpuElement};
pub(crate) use kernels::TensorKernels;
pub(crate) use scatter_kernels::TensorScatterKernels;
use tensor_core::{Layout, Shape};

#[derive(Debug)]
pub enum TensorComputeError {
    Tensor(tensor_core::TensorError),
    Compute(ComputeError),
    ShapeMismatch {
        expected: Vec<usize>,
        actual: Vec<usize>,
    },
    OutputNotContiguous,
    IndexTooLarge,
    UnsupportedPrecision(tensor_core::MatmulPrecision),
    Evaluation(String),
    LowDtypeMismatch {
        expected: tensor_core::LowDtype,
        actual: tensor_core::LowDtype,
    },
}
impl std::fmt::Display for TensorComputeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tensor(error) => error.fmt(f),
            Self::Compute(error) => error.fmt(f),
            Self::ShapeMismatch { expected, actual } => {
                write!(f, "tensor shape {actual:?}; expected {expected:?}")
            }
            Self::OutputNotContiguous => write!(f, "tensor output must have a contiguous layout"),
            Self::IndexTooLarge => write!(f, "tensor metadata exceeds WGSL's u32 index range"),
            Self::UnsupportedPrecision(precision) => write!(
                f,
                "WGSL tensors do not support {precision:?} multiplication precision"
            ),
            Self::Evaluation(error) => write!(f, "GPU evaluation failed: {error}"),
            Self::LowDtypeMismatch { expected, actual } => {
                write!(f, "low tensor dtype {actual:?}; expected {expected:?}")
            }
        }
    }
}
impl std::error::Error for TensorComputeError {}
impl From<tensor_core::TensorError> for TensorComputeError {
    fn from(error: tensor_core::TensorError) -> Self {
        Self::Tensor(error)
    }
}
impl From<ComputeError> for TensorComputeError {
    fn from(error: ComputeError) -> Self {
        Self::Compute(error)
    }
}
impl From<crate::KernelError> for TensorComputeError {
    fn from(error: crate::KernelError) -> Self {
        Self::Compute(error.into())
    }
}

/// Shared `f32` (default) or `u32` storage plus a validated logical layout.
/// Integer operations preserve `u32` values without conversion to floating point.
/// `reshape`, `permute`,
/// and `broadcast_to` are allocation-free GPU views: no dispatch or readback.
/// Strides and offsets are measured in elements. Overlapping read views are
/// supported; recorded operations require a distinct contiguous output.
#[derive(Clone)]
pub struct GpuTensor<T: GpuElement = f32> {
    values: GpuArray<T>,
    layout: Layout,
}
impl<T: GpuElement> GpuTensor<T> {
    /// Creates an exact row-major tensor over the complete array.
    pub fn from_array(values: GpuArray<T>, shape: Shape) -> Result<Self, TensorComputeError> {
        if values.len() != shape.numel() {
            return Err(ComputeError::LengthMismatch {
                expected: shape.numel(),
                actual: values.len(),
            }
            .into());
        }
        Self::from_layout(values, Layout::contiguous(shape)?)
    }
    /// Creates a view after proving every addressed element fits the array.
    pub fn from_layout(values: GpuArray<T>, layout: Layout) -> Result<Self, TensorComputeError> {
        layout.validate_storage_len(values.len())?;
        checked_index(layout.offset())?;
        checked_index(layout.shape().numel())?;
        for &value in layout.shape().dims().iter().chain(layout.strides()) {
            checked_index(value)?;
        }
        Ok(Self { values, layout })
    }
    /// Physical backing storage. For logical row-major values of a strided
    /// view, record `ComputeProgram::tensor_materialize` for `f32` or
    /// `ComputeProgram::tensor_materialize_u32` for `u32` before reading.
    pub fn values(&self) -> &GpuArray<T> {
        &self.values
    }
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
    pub fn shape(&self) -> &Shape {
        self.layout.shape()
    }
    /// Reshape is a view operation and requires a contiguous input layout.
    pub fn reshape(&self, shape: Shape) -> Result<Self, TensorComputeError> {
        Self::from_layout(self.values.clone(), self.layout.reshape(shape)?)
    }
    pub fn permute(&self, axes: &[usize]) -> Result<Self, TensorComputeError> {
        Self::from_layout(self.values.clone(), self.layout.permute(axes)?)
    }
    pub fn broadcast_to(&self, shape: Shape) -> Result<Self, TensorComputeError> {
        Self::from_layout(self.values.clone(), self.layout.broadcast_to(shape)?)
    }
    pub fn narrow(
        &self,
        axis: usize,
        start: usize,
        len: usize,
    ) -> Result<Self, TensorComputeError> {
        Self::from_layout(self.values.clone(), self.layout.narrow(axis, start, len)?)
    }
}
impl<T: GpuElement> tensor_core::HasShape for GpuTensor<T> {
    fn shape(&self) -> &Shape {
        self.layout.shape()
    }
}
fn checked_index(value: usize) -> Result<u32, TensorComputeError> {
    u32::try_from(value).map_err(|_| TensorComputeError::IndexTooLarge)
}
