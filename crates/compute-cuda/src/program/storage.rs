use super::plan::{CudaDtype, TensorSpec};
use crate::{CudaError, CudaRuntime};
use gpu_compute::cuda::CudaSlice;
use tensor_core::LowDtype;

/// Logical scratch signature plus the physical storage reserved for kernels.
/// Raw low scatter CAS requires an even number of u16 slots; caller outputs
/// remain ordinary dense tensors because terminal copies write logical values.
pub(super) struct ScratchSpec {
    pub tensor: TensorSpec,
    pub physical_len: usize,
    // Private f64 kernel state has no public tensor/value binding. Its layout
    // supplies geometry only; the storage flag determines allocation and bytes.
    f64_accumulator: bool,
}
impl ScratchSpec {
    pub fn new(tensor: TensorSpec) -> Self {
        Self {
            physical_len: tensor.shape().numel().max(1),
            tensor,
            f64_accumulator: false,
        }
    }
    pub fn f64(shape: tensor_core::Shape) -> Result<Self, CudaError> {
        let mut spec = Self::new(TensorSpec {
            layout: tensor_core::Layout::contiguous(shape)?,
            dtype: CudaDtype::F32,
        });
        spec.f64_accumulator = true;
        spec.bytes()?;
        Ok(spec)
    }
    pub fn bytes(&self) -> Result<usize, CudaError> {
        if self.physical_len < self.tensor.shape().numel().max(1) {
            return Err(CudaError::InvalidInput("scratch storage is too small"));
        }
        self.physical_len
            .checked_mul(if self.f64_accumulator {
                8
            } else {
                self.tensor.dtype.byte_width()
            })
            .ok_or(CudaError::InvalidInput("CUDA scratch byte count overflows"))
    }
    pub fn pad_low_words(&mut self) -> Result<(), CudaError> {
        if self.f64_accumulator || self.dtype.low_dtype().is_none() {
            return Err(CudaError::Dtype);
        }
        self.physical_len = crate::low_scatter::padded_slots(self.tensor.shape().numel())?;
        self.bytes()?;
        Ok(())
    }
}
impl std::ops::Deref for ScratchSpec {
    type Target = TensorSpec;
    fn deref(&self) -> &TensorSpec {
        &self.tensor
    }
}

pub(super) enum Storage {
    F64(CudaSlice<f64>),
    F32(CudaSlice<f32>),
    U32(CudaSlice<u32>),
    Low(CudaSlice<u16>, LowDtype),
}
#[derive(Clone, Copy)]
pub(super) enum StorageRef<'a> {
    F64(&'a CudaSlice<f64>),
    F32(&'a CudaSlice<f32>),
    U32(&'a CudaSlice<u32>),
    Low(&'a CudaSlice<u16>, LowDtype),
}
pub(super) enum StorageMut<'a> {
    F64(&'a mut CudaSlice<f64>),
    F32(&'a mut CudaSlice<f32>),
    U32(&'a mut CudaSlice<u32>),
    Low(&'a mut CudaSlice<u16>, LowDtype),
}
impl Storage {
    pub fn zeros(rt: &CudaRuntime, spec: &ScratchSpec) -> Result<Self, CudaError> {
        spec.bytes()?;
        let count = spec.physical_len;
        if spec.f64_accumulator {
            return Ok(Self::F64(rt.device.stream.alloc_zeros(count)?));
        }
        Ok(match spec.dtype {
            CudaDtype::F32 => Self::F32(rt.device.stream.alloc_zeros(count)?),
            CudaDtype::U32 => Self::U32(rt.device.stream.alloc_zeros(count)?),
            CudaDtype::F16 | CudaDtype::Bf16 => Self::Low(
                rt.device.stream.alloc_zeros(count)?,
                spec.dtype.low_dtype().unwrap(),
            ),
        })
    }
    pub fn as_ref(&self) -> StorageRef<'_> {
        match self {
            Self::F64(s) => StorageRef::F64(s),
            Self::F32(s) => StorageRef::F32(s),
            Self::U32(s) => StorageRef::U32(s),
            Self::Low(s, d) => StorageRef::Low(s, *d),
        }
    }
    pub fn as_mut(&mut self) -> StorageMut<'_> {
        match self {
            Self::F64(s) => StorageMut::F64(s),
            Self::F32(s) => StorageMut::F32(s),
            Self::U32(s) => StorageMut::U32(s),
            Self::Low(s, d) => StorageMut::Low(s, *d),
        }
    }
}
impl<'a> StorageRef<'a> {
    pub fn f64(self) -> Result<&'a CudaSlice<f64>, CudaError> {
        if let Self::F64(s) = self {
            Ok(s)
        } else {
            Err(CudaError::Dtype)
        }
    }
    pub fn f32(self) -> Result<&'a CudaSlice<f32>, CudaError> {
        if let Self::F32(s) = self {
            Ok(s)
        } else {
            Err(CudaError::Dtype)
        }
    }
    pub fn u32(self) -> Result<&'a CudaSlice<u32>, CudaError> {
        if let Self::U32(s) = self {
            Ok(s)
        } else {
            Err(CudaError::Dtype)
        }
    }
}
impl<'a> StorageMut<'a> {
    pub fn f64(self) -> Result<&'a mut CudaSlice<f64>, CudaError> {
        if let Self::F64(s) = self {
            Ok(s)
        } else {
            Err(CudaError::Dtype)
        }
    }
    /// Reinitialize resident storage before each data-dependent operation.
    /// cudarc retains its ordinary write event guard for the async memset.
    pub fn clear(self, rt: &CudaRuntime) -> Result<(), CudaError> {
        match self {
            Self::F64(s) => rt.device.stream.memset_zeros(s)?,
            Self::F32(s) => rt.device.stream.memset_zeros(s)?,
            Self::U32(s) => rt.device.stream.memset_zeros(s)?,
            Self::Low(s, _) => rt.device.stream.memset_zeros(s)?,
        }
        Ok(())
    }

    pub fn f32(self) -> Result<&'a mut CudaSlice<f32>, CudaError> {
        if let Self::F32(s) = self {
            Ok(s)
        } else {
            Err(CudaError::Dtype)
        }
    }
    pub fn u32(self) -> Result<&'a mut CudaSlice<u32>, CudaError> {
        if let Self::U32(s) = self {
            Ok(s)
        } else {
            Err(CudaError::Dtype)
        }
    }
}
