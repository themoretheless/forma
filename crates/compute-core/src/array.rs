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

/// Copy the sealed scalar wire format into an owned, correctly aligned Vec.
/// An explicit byte copy avoids context-dependent scalar-loop codegen on LE.
pub(crate) fn decode_elements<T: GpuElement>(bytes: &[u8]) -> Vec<T> {
    #[cfg(target_endian = "little")]
    {
        const { assert!(std::mem::size_of::<T>() == 4) };
        let len = bytes.len() / 4;
        let mut out = Vec::<T>::with_capacity(len);
        // SAFETY: the sealed types are f32/u32, four bytes with every bit
        // pattern valid. The fresh allocation has T alignment and capacity;
        // a byte copy needs no source alignment. The initialized source cannot
        // overlap this new allocation. Expose elements only after initializing
        // all their bytes; ignore incomplete trailing words as the scalar path did.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.as_mut_ptr().cast::<u8>(), len * 4);
            out.set_len(len);
        }
        out
    }
    #[cfg(not(target_endian = "little"))]
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| T::from_bytes(*v))
        .collect()
}

#[cfg(test)]
mod decode_tests {
    use super::decode_elements;

    #[test]
    fn bulk_decode_owns_aligned_storage_and_preserves_unaligned_source_bits() {
        let mut bits = vec![
            0,
            0x8000_0000,
            1,
            u32::MAX,
            0x7f80_0000,
            0x7fa0_1234,
            0x7fc0_1234,
        ];
        let mut state = 0xa390_31d5u32;
        for _ in 0..1021 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            bits.push(state);
        }
        let mut raw = vec![0x55];
        raw.extend(bits.iter().flat_map(|v| v.to_le_bytes()));
        raw.extend([0x11, 0x22, 0x33]);
        let floats = decode_elements::<f32>(&raw[1..]);
        let words = decode_elements::<u32>(&raw[1..]);
        raw.fill(0);
        assert_eq!(words, bits);
        assert_eq!(floats.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), bits);
        assert!(decode_elements::<f32>(&[]).is_empty());
        assert!(decode_elements::<u32>(&[1, 2, 3]).is_empty());
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
