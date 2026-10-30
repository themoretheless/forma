//! Page-aligned shared Metal storage with explicit host/GPU synchronization.
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::{MTLBuffer, MTLDevice, MTLResourceOptions};
use std::{
    alloc::Layout,
    ffi::c_void,
    ptr::{self, NonNull},
    sync::Arc,
};
use wgpu_native as wgpu;
#[derive(Debug)]
pub struct MetalMemoryError(pub String);
impl std::fmt::Display for MetalMemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for MetalMemoryError {}
use MetalMemoryError as Error;
pub const METAL_UMA_PAGE_BYTES: usize = 16_384;
struct UmaAllocation {
    ptr: NonNull<u8>,
    layout: Layout,
    len: usize,
}
// SAFETY: page-aligned host allocation stays pinned at a stable virtual/physical
// UMA address until all CPU and Metal references drop.
unsafe impl Send for UmaAllocation {}
unsafe impl Sync for UmaAllocation {}
impl Drop for UmaAllocation {
    fn drop(&mut self) {
        // SAFETY: allocated with std::alloc::alloc_zeroed using `self.layout`.
        unsafe {
            std::alloc::dealloc(self.ptr.as_ptr(), self.layout);
        }
    }
}

/// Zero-copy Unified Memory Architecture (UMA) buffer shared between CPU and Apple Silicon GPU.
/// Backed by 16 KiB page-aligned physical RAM wrapped via `newBufferWithBytesNoCopy`
/// (`MTLResourceStorageModeShared`) and imported into `wgpu::Buffer` without bus transfers.
#[derive(Clone)]
pub struct MetalSharedBuffer {
    alloc: Arc<UmaAllocation>,
    buffer: wgpu::Buffer,
}

impl MetalSharedBuffer {
    /// Allocate a zero-copy UMA buffer of `len` logical bytes (rounded up to a 16 KiB page
    /// multiple for Metal's `newBufferWithBytesNoCopy` contract) and bind it to `device`.
    pub fn new(
        device: &wgpu::Device,
        len: usize,
        usage: wgpu::BufferUsages,
        label: Option<&str>,
    ) -> Result<Self, Error> {
        if len == 0 {
            return Err(Error("UMA shared buffer length must be nonzero".into()));
        }
        let aligned_len = len
            .checked_next_multiple_of(METAL_UMA_PAGE_BYTES)
            .ok_or_else(|| Error("UMA shared buffer size overflow".into()))?;
        if aligned_len as u64 > device.limits().max_buffer_size {
            return Err(Error(
                "UMA shared buffer exceeds device max_buffer_size".into(),
            ));
        }
        let layout = Layout::from_size_align(aligned_len, METAL_UMA_PAGE_BYTES)
            .map_err(|_| Error("invalid UMA page alignment layout".into()))?;
        let raw_ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        let ptr = NonNull::new(raw_ptr)
            .ok_or_else(|| Error("UMA page-aligned memory allocation failed".into()))?;
        let alloc = Arc::new(UmaAllocation { ptr, layout, len });
        // SAFETY: `alloc.ptr` is 16 KiB page-aligned, `aligned_len` is a multiple of 16 KiB,
        // and the `Arc<UmaAllocation>` is kept alive until the wgpu/Metal buffer is dropped
        // or the `MetalSharedBuffer` is dropped (whichever happens last).
        let buffer = unsafe {
            let hal = device
                .as_hal::<wgpu::hal::api::Metal>()
                .ok_or_else(|| Error("UMA shared buffer requires a Metal wgpu device".into()))?;
            let raw_device = hal.raw_device();
            let nn = NonNull::new_unchecked(alloc.ptr.as_ptr().cast::<c_void>());
            let mtl_buffer: Retained<ProtocolObject<dyn MTLBuffer>> = raw_device
                .newBufferWithBytesNoCopy_length_options_deallocator(
                    nn,
                    aligned_len,
                    MTLResourceOptions::StorageModeShared,
                    None,
                )
                .ok_or_else(|| {
                    Error("Metal newBufferWithBytesNoCopy rejected UMA page allocation".into())
                })?;
            let hal_buffer =
                wgpu::hal::metal::Device::buffer_from_raw(mtl_buffer, aligned_len as u64);
            device.create_buffer_from_hal::<wgpu::hal::api::Metal>(
                hal_buffer,
                &wgpu::BufferDescriptor {
                    label,
                    size: aligned_len as u64,
                    usage,
                    mapped_at_creation: false,
                },
            )
        };
        Ok(Self { alloc, buffer })
    }

    /// Logical payload length requested by the caller.
    pub fn len(&self) -> usize {
        self.alloc.len
    }

    pub fn is_empty(&self) -> bool {
        self.alloc.len == 0
    }

    /// Page-aligned capacity visible to Metal (`multiple of 16384`).
    pub fn aligned_capacity(&self) -> usize {
        self.alloc.layout.size()
    }

    /// Underlying `wgpu::Buffer` backed by the same physical RAM pages.
    pub fn wgpu_buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// Write bytes directly into unified RAM without staging or bus transfers.
    /// Any trailing bytes up to the 4-byte word boundary are zeroed.
    pub fn write_slice(&self, src: &[u8]) -> Result<(), Error> {
        if src.len() > self.alloc.len {
            return Err(Error("source slice exceeds UMA buffer length".into()));
        }
        // SAFETY: caller serializes host writes before submitting GPU commands on the queue.
        unsafe {
            let dst = self.alloc.ptr.as_ptr();
            ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len());
            let padded_end = src.len().next_multiple_of(4).min(self.alloc.layout.size());
            if padded_end > src.len() {
                ptr::write_bytes(dst.add(src.len()), 0, padded_end - src.len());
            }
        }
        Ok(())
    }

    /// Read bytes directly from unified RAM after GPU queue synchronization (`device.poll(Wait)`).
    pub fn read_slice(&self, dst: &mut [u8]) -> Result<(), Error> {
        if dst.len() > self.alloc.len {
            return Err(Error("destination slice exceeds UMA buffer length".into()));
        }
        // SAFETY: caller polls the Metal device until submission completion before reading.
        unsafe {
            ptr::copy_nonoverlapping(self.alloc.ptr.as_ptr(), dst.as_mut_ptr(), dst.len());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a physical Metal GPU"]
    fn cloned_storage_retains_allocation_and_checks_logical_bounds() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = crate::block_on(instance.request_adapter(&Default::default())).unwrap();
        assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
        let (device, queue) = crate::block_on(adapter.request_device(&Default::default())).unwrap();
        let len = METAL_UMA_PAGE_BYTES + 4;
        let original = MetalSharedBuffer::new(
            &device,
            len,
            wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            None,
        )
        .unwrap();
        assert_eq!(original.aligned_capacity(), METAL_UMA_PAGE_BYTES * 2);
        assert!(original.write_slice(&vec![0; len + 1]).is_err());
        assert!(original.read_slice(&mut vec![0; len + 1]).is_err());
        let retained = original.clone();
        drop(original);
        let expected: Vec<u8> = (0..len).map(|i| (i.wrapping_mul(17) >> 3) as u8).collect();
        retained.write_slice(&expected).unwrap();
        let output =
            MetalSharedBuffer::new(&device, len, wgpu::BufferUsages::COPY_DST, None).unwrap();
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(
            retained.wgpu_buffer(),
            0,
            output.wgpu_buffer(),
            0,
            len as u64,
        );
        let submission = queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .unwrap();
        let mut actual = vec![0; len];
        output.read_slice(&mut actual).unwrap();
        assert_eq!(actual, expected);
    }
}
