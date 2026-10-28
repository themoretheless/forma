use std::{marker::PhantomData, sync::Arc};

use crate::{ComputeError, wgpu};

mod sealed {
    pub trait Sealed {}
    impl Sealed for f32 {}
    impl Sealed for u32 {}
}

/// Scalar types with a defined four-byte WGSL storage representation.
pub trait GpuElement: sealed::Sealed + Copy + Send + Sync + 'static {
    fn to_bytes(self) -> [u8; 4];
    fn from_bytes(bytes: [u8; 4]) -> Self;
}

impl GpuElement for f32 {
    fn to_bytes(self) -> [u8; 4] {
        self.to_le_bytes()
    }
    fn from_bytes(bytes: [u8; 4]) -> Self {
        Self::from_le_bytes(bytes)
    }
}
impl GpuElement for u32 {
    fn to_bytes(self) -> [u8; 4] {
        self.to_le_bytes()
    }
    fn from_bytes(bytes: [u8; 4]) -> Self {
        Self::from_le_bytes(bytes)
    }
}

#[derive(Clone)]
pub(crate) struct Allocation {
    pub buffer: gpu_compute::GpuBuffer,
    pub owner: Arc<()>,
}

/// Fixed-length, shared GPU storage. Cloning shares the allocation and contents.
/// Updating contents preserves bindings in prepared programs. Use `prefix` to
/// prepare a smaller workload on the same allocation without reallocating.
#[derive(Clone)]
pub struct GpuArray<T: GpuElement> {
    pub(crate) allocation: Allocation,
    pub(crate) len: u32,
    marker: PhantomData<T>,
}

impl<T: GpuElement> GpuArray<T> {
    pub(crate) fn new(buffer: gpu_compute::GpuBuffer, owner: Arc<()>, len: u32) -> Self {
        Self {
            allocation: Allocation { buffer, owner },
            len,
            marker: PhantomData,
        }
    }
    pub fn len(&self) -> usize {
        self.len as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// A view starting at element zero. Existing programs keep their own length.
    pub fn prefix(&self, len: usize) -> Result<Self, ComputeError> {
        if len > self.len() {
            return Err(ComputeError::OutOfBounds);
        }
        Ok(Self {
            allocation: self.allocation.clone(),
            len: len as u32,
            marker: PhantomData,
        })
    }

    /// Validated borrowed bytes on the original device, for domain adapters.
    pub fn view(&self) -> gpu_compute::GpuBufferView<'_> {
        self.allocation
            .buffer
            .view(0..self.len() as u64 * 4)
            .expect("array length was validated")
    }

    pub(crate) fn buffer(&self) -> &wgpu::Buffer {
        self.allocation.buffer.raw()
    }
    pub(crate) fn aliases(&self, other: &Self) -> bool {
        self.buffer() == other.buffer()
    }
}
