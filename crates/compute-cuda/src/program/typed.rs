//! Checked typed transport; low allocations remain behind CudaLowTensor.
use super::{
    plan::CudaDtype,
    storage::{StorageMut, StorageRef},
    validation::Binding,
};
use crate::{CudaError, CudaLowTensor, CudaRuntime, CudaTensor};
use std::sync::Arc;
use tensor_core::{HasShape, Layout, Shape};

#[derive(Clone, Copy)]
pub enum CudaProgramInput<'a> {
    F32(&'a CudaTensor),
    U32(&'a CudaTensor<u32>),
    Low(&'a CudaLowTensor),
}
#[derive(Clone)]
pub enum CudaProgramOutput {
    F32(CudaTensor),
    U32(CudaTensor<u32>),
    Low(CudaLowTensor),
}
pub enum CudaProgramOutputMut<'a> {
    F32(&'a mut CudaTensor),
    U32(&'a mut CudaTensor<u32>),
    Low(&'a mut CudaLowTensor),
}
macro_rules! transport {
    ($variant:ident,$tensor:ty,$dtype:expr,$as:ident,$into:ident) => {
        impl<'a> From<&'a $tensor> for CudaProgramInput<'a> {
            fn from(t: &'a $tensor) -> Self {
                Self::$variant(t)
            }
        }
        impl<'a> From<&'a mut $tensor> for CudaProgramOutputMut<'a> {
            fn from(t: &'a mut $tensor) -> Self {
                Self::$variant(t)
            }
        }
        impl CudaProgramOutput {
            pub fn $as(&self) -> Result<&$tensor, CudaError> {
                match self {
                    Self::$variant(t) => Ok(t),
                    _ => Err(CudaError::Dtype),
                }
            }
            pub fn $into(self) -> Result<$tensor, CudaError> {
                match self {
                    Self::$variant(t) => Ok(t),
                    _ => Err(CudaError::Dtype),
                }
            }
        }
    };
}
transport!(F32, CudaTensor, CudaDtype::F32, as_f32, into_f32);
transport!(U32, CudaTensor<u32>, CudaDtype::U32, as_u32, into_u32);
transport!(Low, CudaLowTensor, CudaDtype::F16, as_low, into_low);
impl CudaProgramInput<'_> {
    pub fn dtype(&self) -> CudaDtype {
        match self {
            Self::F32(_) => CudaDtype::F32,
            Self::U32(_) => CudaDtype::U32,
            Self::Low(t) => t.dtype.into(),
        }
    }
    pub fn layout(&self) -> &Layout {
        match self {
            Self::F32(t) => t.layout(),
            Self::U32(t) => t.layout(),
            Self::Low(t) => t.layout(),
        }
    }
    pub(super) fn check(&self, rt: &CudaRuntime) -> Result<(), CudaError> {
        match self {
            Self::F32(t) => rt.check(t),
            Self::U32(t) => rt.check(t),
            Self::Low(t) => rt.check(&t.tensor),
        }
    }
    pub(super) fn binding(&self) -> Binding<'_> {
        match self {
            Self::F32(t) => binding(t, self.dtype()),
            Self::U32(t) => binding(t, self.dtype()),
            Self::Low(t) => binding(&t.tensor, self.dtype()),
        }
    }
    pub(super) fn storage(&self) -> StorageRef<'_> {
        match self {
            Self::F32(t) => StorageRef::F32(&t.storage),
            Self::U32(t) => StorageRef::U32(&t.storage),
            Self::Low(t) => StorageRef::Low(&t.tensor.storage, t.dtype),
        }
    }
}
impl HasShape for CudaProgramInput<'_> {
    fn shape(&self) -> &Shape {
        self.layout().shape()
    }
}
impl CudaProgramOutput {
    pub fn dtype(&self) -> CudaDtype {
        match self {
            Self::F32(_) => CudaDtype::F32,
            Self::U32(_) => CudaDtype::U32,
            Self::Low(t) => t.dtype.into(),
        }
    }
    pub fn layout(&self) -> &Layout {
        match self {
            Self::F32(t) => t.layout(),
            Self::U32(t) => t.layout(),
            Self::Low(t) => t.layout(),
        }
    }
    pub fn as_input(&self) -> CudaProgramInput<'_> {
        match self {
            Self::F32(t) => t.into(),
            Self::U32(t) => t.into(),
            Self::Low(t) => t.into(),
        }
    }
    pub fn as_output_mut(&mut self) -> CudaProgramOutputMut<'_> {
        match self {
            Self::F32(t) => t.into(),
            Self::U32(t) => t.into(),
            Self::Low(t) => t.into(),
        }
    }
    pub(super) fn zeros(
        rt: &CudaRuntime,
        shape: Shape,
        dtype: CudaDtype,
    ) -> Result<Self, CudaError> {
        Ok(match dtype {
            CudaDtype::F32 => Self::F32(rt.zeros(shape)?),
            CudaDtype::U32 => Self::U32(rt.zeros_typed(shape)?),
            CudaDtype::F16 | CudaDtype::Bf16 => Self::Low(CudaLowTensor {
                tensor: rt.zeros_typed(shape)?,
                dtype: dtype.low_dtype().unwrap(),
            }),
        })
    }
}
impl HasShape for CudaProgramOutput {
    fn shape(&self) -> &Shape {
        self.layout().shape()
    }
}
impl CudaProgramOutputMut<'_> {
    pub fn dtype(&self) -> CudaDtype {
        match self {
            Self::F32(_) => CudaDtype::F32,
            Self::U32(_) => CudaDtype::U32,
            Self::Low(t) => t.dtype.into(),
        }
    }
    pub fn layout(&self) -> &Layout {
        match self {
            Self::F32(t) => t.layout(),
            Self::U32(t) => t.layout(),
            Self::Low(t) => t.layout(),
        }
    }
    pub(super) fn check(&self, rt: &CudaRuntime) -> Result<(), CudaError> {
        match self {
            Self::F32(t) => rt.check(t),
            Self::U32(t) => rt.check(t),
            Self::Low(t) => rt.check(&t.tensor),
        }
    }
    pub(super) fn binding(&self) -> Binding<'_> {
        match self {
            Self::F32(t) => binding(t, self.dtype()),
            Self::U32(t) => binding(t, self.dtype()),
            Self::Low(t) => binding(&t.tensor, self.dtype()),
        }
    }
    pub(super) fn storage_mut(&mut self) -> Result<StorageMut<'_>, CudaError> {
        Ok(match self {
            Self::F32(t) => {
                StorageMut::F32(Arc::get_mut(&mut t.storage).ok_or(CudaError::SharedOutput)?)
            }
            Self::U32(t) => {
                StorageMut::U32(Arc::get_mut(&mut t.storage).ok_or(CudaError::SharedOutput)?)
            }
            Self::Low(t) => StorageMut::Low(
                Arc::get_mut(&mut t.tensor.storage).ok_or(CudaError::SharedOutput)?,
                t.dtype,
            ),
        })
    }
}
impl HasShape for CudaProgramOutputMut<'_> {
    fn shape(&self) -> &Shape {
        self.layout().shape()
    }
}
fn binding<T>(tensor: &CudaTensor<T>, dtype: CudaDtype) -> Binding<'_> {
    Binding {
        layout: &tensor.layout,
        dtype,
        storage_len: tensor.storage.len(),
        allocation: Arc::as_ptr(&tensor.storage) as usize,
        unique: Arc::strong_count(&tensor.storage) == 1 && Arc::weak_count(&tensor.storage) == 0,
    }
}
