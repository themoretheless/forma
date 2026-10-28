mod arithmetic;
mod attention;
mod attention_source;
mod convolution;
mod index_sources;
mod indexing;
mod kernels;
mod program;
mod reduction;
mod scan;
mod scatter;
mod scatter_source;
mod statistics;
mod statistics_sources;
use super::{GpuTensor, TensorComputeError, checked_index};
use crate::{ComputeError, ComputeRuntime, GpuArray};
pub(crate) use kernels::LowKernels;
use tensor_core::{
    HasLowDtype, HasShape, Layout, LowDtype, LowPrecisionSupport, LowStorage, Shape,
};

type Result<T> = std::result::Result<T, TensorComputeError>;

/// Two logical f16/bf16 values per u32 word. Layout strides and offsets count
/// 16-bit elements. The odd final padding lane is never part of the tensor.
/// Raw views/materialization preserve every payload bit, including NaNs.
#[derive(Clone)]
pub struct GpuLowTensor {
    words: GpuArray<u32>,
    storage_len: usize,
    layout: Layout,
    dtype: LowDtype,
}
impl GpuLowTensor {
    /// Wraps exact packed backing storage with a checked logical view. The
    /// low 16 bits hold the even element, the high 16 bits the odd element.
    pub fn from_packed(
        dtype: LowDtype,
        words: GpuArray<u32>,
        storage_len: usize,
        layout: Layout,
    ) -> Result<Self> {
        checked_index(storage_len)?;
        if words.len() != storage_len.div_ceil(2) {
            return Err(ComputeError::LengthMismatch {
                expected: storage_len.div_ceil(2),
                actual: words.len(),
            }
            .into());
        }
        layout.validate_storage_len(storage_len)?;
        checked_index(layout.shape().numel())?;
        checked_index(layout.offset())?;
        for &value in layout.shape().dims().iter().chain(layout.strides()) {
            checked_index(value)?;
        }
        Ok(Self {
            words,
            storage_len,
            layout,
            dtype,
        })
    }
    /// Physical u32 storage, suitable for recorded transfers. Materialize a
    /// strided view on GPU before interpreting these words as logical values.
    pub fn packed_words(&self) -> &GpuArray<u32> {
        &self.words
    }
    pub fn storage_len(&self) -> usize {
        self.storage_len
    }
    /// Actual allocation bytes, including four-byte transport alignment and
    /// the minimum four-byte allocation for an empty tensor.
    pub fn allocation_bytes(&self) -> u64 {
        self.words.buffer().size()
    }
    pub fn dtype(&self) -> LowDtype {
        self.dtype
    }
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
    pub fn shape(&self) -> &Shape {
        self.layout.shape()
    }
    pub fn reshape(&self, shape: Shape) -> Result<Self> {
        Self::from_packed(
            self.dtype,
            self.words.clone(),
            self.storage_len,
            self.layout.reshape(shape)?,
        )
    }
    pub fn permute(&self, axes: &[usize]) -> Result<Self> {
        Self::from_packed(
            self.dtype,
            self.words.clone(),
            self.storage_len,
            self.layout.permute(axes)?,
        )
    }
    pub fn broadcast_to(&self, shape: Shape) -> Result<Self> {
        Self::from_packed(
            self.dtype,
            self.words.clone(),
            self.storage_len,
            self.layout.broadcast_to(shape)?,
        )
    }
    pub fn narrow(&self, axis: usize, start: usize, len: usize) -> Result<Self> {
        Self::from_packed(
            self.dtype,
            self.words.clone(),
            self.storage_len,
            self.layout.narrow(axis, start, len)?,
        )
    }
}
impl HasShape for GpuLowTensor {
    fn shape(&self) -> &Shape {
        self.shape()
    }
}
impl HasLowDtype for GpuLowTensor {
    fn low_dtype(&self) -> LowDtype {
        self.dtype
    }
}

pub(super) fn check_dtype(expected: LowDtype, actual: LowDtype) -> Result<()> {
    if expected != actual {
        return Err(TensorComputeError::LowDtypeMismatch { expected, actual });
    }
    Ok(())
}
fn pack(bits: &[u16]) -> Vec<u32> {
    bits.chunks(2)
        .map(|p| u32::from(p[0]) | (u32::from(p.get(1).copied().unwrap_or(0)) << 16))
        .collect()
}

impl ComputeRuntime {
    /// Portable packed storage and direct tiled loads are supported for both
    /// formats without optional GPU features. This reports the implementation,
    /// not native f16 capability or use of tensor-matrix hardware instructions.
    pub fn low_precision_support(&self, _dtype: LowDtype) -> LowPrecisionSupport {
        LowPrecisionSupport {
            storage: LowStorage::Packed16x2,
            matmul: true,
            matmul_f32: true,
        }
    }
    pub fn zeros_low(&self, dtype: LowDtype, shape: Shape) -> Result<GpuLowTensor> {
        let len = shape.numel();
        checked_index(len)?;
        GpuLowTensor::from_packed(
            dtype,
            self.zeros(len.div_ceil(2))?,
            len,
            Layout::contiguous(shape)?,
        )
    }
    pub fn upload_low_bits(
        &self,
        dtype: LowDtype,
        shape: Shape,
        bits: &[u16],
    ) -> Result<GpuLowTensor> {
        if shape.numel() != bits.len() {
            return Err(tensor_core::TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: bits.len(),
            }
            .into());
        }
        checked_index(bits.len())?;
        GpuLowTensor::from_packed(
            dtype,
            self.upload(&pack(bits))?,
            bits.len(),
            Layout::contiguous(shape)?,
        )
    }
    /// Replaces the complete physical logical storage, keeping existing plan
    /// bindings. Bits are in backing-storage order, independently of the view.
    /// An unused final padding lane is reset to zero.
    pub fn write_low_storage_bits(&self, tensor: &GpuLowTensor, bits: &[u16]) -> Result<()> {
        self.check(tensor.packed_words())?;
        if bits.len() != tensor.storage_len {
            return Err(ComputeError::LengthMismatch {
                expected: tensor.storage_len,
                actual: bits.len(),
            }
            .into());
        }
        Ok(self.write(&tensor.words, 0, &pack(bits))?)
    }
}
