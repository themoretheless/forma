//! **ASD** (Accelerated Storage Direct / Apple Silicon Direct / GPUDirect Storage `cuFile` / DirectStorage):
//! Zero-copy page-aligned memory-mapped storage and direct I/O streaming into GPU buffers.

use std::ptr::NonNull;

/// Page-aligned allocation for **ASD** (`newBufferWithBytesNoCopy:` on Apple Silicon / page-locked DMA on Linux/Windows).
pub struct AsdPageBuffer {
    ptr: NonNull<u8>,
    aligned_capacity: usize,
    logical_len: usize,
    page_size: usize,
}

unsafe impl Send for AsdPageBuffer {}
unsafe impl Sync for AsdPageBuffer {}

impl AsdPageBuffer {
    pub const APPLE_SILICON_PAGE_SIZE: usize = 16384;
    pub const X86_64_PAGE_SIZE: usize = 4096;

    /// Universal page alignment (`16 KiB`) that satisfies both Apple Silicon (`16 KiB` pages)
    /// and x86_64 / ARM64 Linux/Windows/Android (`4 KiB` pages, since `16384 = 4 * 4096`).
    pub const UNIVERSAL_PAGE_ALIGNMENT: usize = 16384;

    /// Returns the OS/hardware physical VM page size (`16 KiB` on Apple Silicon, `4 KiB` on x86_64/other).
    #[inline]
    pub const fn host_os_page_size() -> usize {
        crate::platform::TargetPlatform::current().default_page_size()
    }

    /// Returns the allocation alignment used by [`AsdPageBuffer`] (`16 KiB`, a multiple of both `4 KiB` and `16 KiB`).
    #[inline]
    pub const fn native_page_size() -> usize {
        Self::UNIVERSAL_PAGE_ALIGNMENT
    }

    pub fn new_zeroed(byte_len: usize) -> Self {
        let page_size = Self::native_page_size();
        let aligned_capacity = ((byte_len.max(1) + page_size - 1) / page_size) * page_size;
        let layout = std::alloc::Layout::from_size_align(aligned_capacity, page_size)
            .expect("valid page-aligned layout");
        let raw = unsafe { std::alloc::alloc_zeroed(layout) };
        let ptr = NonNull::new(raw).unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
        Self {
            ptr,
            aligned_capacity,
            logical_len: byte_len,
            page_size,
        }
    }

    pub fn from_slice(data: &[u8]) -> Self {
        let mut buf = Self::new_zeroed(data.len());
        buf.as_mut_slice().copy_from_slice(data);
        buf
    }

    #[inline]
    pub fn page_size(&self) -> usize {
        self.page_size
    }

    #[inline]
    pub fn aligned_capacity(&self) -> usize {
        self.aligned_capacity
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.logical_len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.logical_len == 0
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.logical_len) }
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.logical_len) }
    }

    /// Binds this page-aligned ASD memory region directly to a GPU [`crate::Buffer`] with zero copies (`newBufferWithBytesNoCopy:`).
    pub fn bind_to_gpu(&self, device: &crate::Device, usage: crate::BufferUsages) -> crate::Buffer {
        unsafe {
            device
                .create_buffer_no_copy(
                    self.ptr.as_ptr(),
                    self.aligned_capacity,
                    self.logical_len as u64,
                    usage,
                )
                .unwrap_or_else(|| {
                    use crate::util::DeviceExt;
                    device.create_buffer_init(&crate::util::BufferInitDescriptor {
                        label: Some("asd-fallback-buffer"),
                        contents: self.as_slice(),
                        usage,
                    })
                })
        }
    }
}

impl Drop for AsdPageBuffer {
    fn drop(&mut self) {
        if let Ok(layout) =
            std::alloc::Layout::from_size_align(self.aligned_capacity, self.page_size)
        {
            unsafe {
                std::alloc::dealloc(self.ptr.as_ptr(), layout);
            }
        }
    }
}
