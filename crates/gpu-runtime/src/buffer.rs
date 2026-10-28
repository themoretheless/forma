//! GPU Buffer, UMA Direct Slices (`uma_slice`, `uma_slice_mut`), `BufferSlice`, `BufferView`, and `WriteOnly` mapping views.

use crate::metal::{MetalDevice, ObjCObject};
use crate::{BindingResource, BufferAddress, BufferBinding, BufferUsages, Label};
use std::ops::{Bound, Deref, DerefMut, Range, RangeBounds};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct WriteOnly<'a, T: ?Sized> {
    slice: &'a mut T,
}

impl<'a> WriteOnly<'a, [u8]> {
    #[inline]
    pub fn new(slice: &'a mut [u8]) -> Self {
        Self { slice }
    }

    #[inline]
    pub fn copy_from_slice(&mut self, src: &[u8]) {
        self.slice.copy_from_slice(src);
    }

    #[inline]
    pub fn fill(&mut self, value: u8) {
        self.slice.fill(value);
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.slice.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.slice.is_empty()
    }

    #[inline]
    pub fn subslice(&mut self, range: Range<usize>) -> WriteOnly<'_, [u8]> {
        WriteOnly {
            slice: &mut self.slice[range],
        }
    }

    #[inline]
    pub fn slice<S: RangeBounds<usize>>(&mut self, bounds: S) -> WriteOnly<'_, [u8]> {
        let start = match bounds.start_bound() {
            Bound::Included(&v) => v,
            Bound::Excluded(&v) => v + 1,
            Bound::Unbounded => 0,
        };
        let end = match bounds.end_bound() {
            Bound::Included(&v) => v + 1,
            Bound::Excluded(&v) => v,
            Bound::Unbounded => self.slice.len(),
        };
        WriteOnly {
            slice: &mut self.slice[start..end],
        }
    }

    #[inline]
    pub fn into_slice<S: RangeBounds<usize>>(self, bounds: S) -> WriteOnly<'a, [u8]> {
        let start = match bounds.start_bound() {
            Bound::Included(&v) => v,
            Bound::Excluded(&v) => v + 1,
            Bound::Unbounded => 0,
        };
        let end = match bounds.end_bound() {
            Bound::Included(&v) => v + 1,
            Bound::Excluded(&v) => v,
            Bound::Unbounded => self.slice.len(),
        };
        WriteOnly {
            slice: &mut self.slice[start..end],
        }
    }

    #[inline]
    pub fn into_chunks<const N: usize>(self) -> (WriteOnly<'a, [[u8; N]]>, WriteOnly<'a, [u8]>) {
        assert!(N > 0);
        let len = self.slice.len();
        let num_chunks = len / N;
        let split_at = num_chunks * N;
        let (head, tail) = self.slice.split_at_mut(split_at);
        let chunks =
            unsafe { std::slice::from_raw_parts_mut(head.as_mut_ptr() as *mut [u8; N], num_chunks) };
        (WriteOnly { slice: chunks }, WriteOnly { slice: tail })
    }

    #[inline]
    pub fn chunks_exact_mut(
        &mut self,
        chunk_size: usize,
    ) -> impl Iterator<Item = WriteOnly<'_, [u8]>> {
        self.slice
            .chunks_exact_mut(chunk_size)
            .map(|c| WriteOnly { slice: c })
    }
}

impl<'a, const N: usize> WriteOnly<'a, [[u8; N]]> {
    #[inline]
    pub fn write_iter<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = [u8; N]>,
    {
        for (dst, val) in self.slice.iter_mut().zip(iter) {
            *dst = val;
        }
    }
}

impl<'a> Deref for WriteOnly<'a, [u8]> {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.slice
    }
}

impl<'a> DerefMut for WriteOnly<'a, [u8]> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.slice
    }
}

#[derive(Debug, Clone)]
pub struct BufferDescriptor<'a> {
    pub label: Label<'a>,
    pub size: BufferAddress,
    pub usage: BufferUsages,
    pub mapped_at_creation: bool,
}

pub(crate) struct BufferInner {
    pub(crate) device: MetalDevice,
    pub(crate) raw: Arc<ObjCObject>,
    pub(crate) ptr: *mut u8,
    pub(crate) size: BufferAddress,
    pub(crate) usage: BufferUsages,
    pub(crate) options: u64,
    pub(crate) mapped_ready: AtomicBool,
}

unsafe impl Send for BufferInner {}
unsafe impl Sync for BufferInner {}

impl Drop for BufferInner {
    fn drop(&mut self) {
        if self.options != 0xFF {
            self.device
                .recycle_buffer(self.size as usize, self.options, self.raw.clone(), self.ptr);
        }
    }
}

#[derive(Clone)]
pub struct Buffer {
    pub(crate) inner: Arc<BufferInner>,
}

impl PartialEq for Buffer {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for Buffer {}

impl std::fmt::Debug for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Buffer")
            .field("size", &self.inner.size)
            .field("usage", &self.inner.usage)
            .finish()
    }
}

impl Buffer {
    #[inline]
    pub fn size(&self) -> BufferAddress {
        self.inner.size
    }

    #[inline]
    pub fn usage(&self) -> BufferUsages {
        self.inner.usage
    }

    /// Zero-copy UMA direct slice view into unified memory without map_async overhead
    #[inline]
    pub fn uma_slice(&self) -> &[u8] {
        if !self.inner.device.is_last_cmd_completed() {
            self.inner.device.wait_idle();
        }
        unsafe { std::slice::from_raw_parts(self.inner.ptr, self.inner.size as usize) }
    }

    /// Zero-copy UMA direct mutable slice into unified memory
    #[inline]
    pub fn uma_slice_mut(&mut self) -> &mut [u8] {
        if !self.inner.device.is_last_cmd_completed() {
            self.inner.device.wait_idle();
        }
        unsafe { std::slice::from_raw_parts_mut(self.inner.ptr, self.inner.size as usize) }
    }

    #[inline]
    pub fn slice<S: RangeBounds<BufferAddress>>(&self, bounds: S) -> BufferSlice<'_> {
        let start = match bounds.start_bound() {
            Bound::Included(&v) => v,
            Bound::Excluded(&v) => v + 1,
            Bound::Unbounded => 0,
        };
        let end = match bounds.end_bound() {
            Bound::Included(&v) => v + 1,
            Bound::Excluded(&v) => v,
            Bound::Unbounded => self.inner.size,
        };
        BufferSlice {
            buffer: self,
            offset: start,
            size: end.saturating_sub(start),
        }
    }

    #[inline]
    pub fn map_async<S, F>(&self, mode: MapMode, bounds: S, callback: F)
    where
        S: RangeBounds<BufferAddress>,
        F: FnOnce(Result<(), BufferAsyncError>) + Send + 'static,
    {
        self.slice(bounds).map_async(mode, callback);
    }

    #[inline]
    pub fn get_mapped_range<S: RangeBounds<BufferAddress>>(
        &self,
        bounds: S,
    ) -> Result<BufferView<'_>, BufferAccessError> {
        Ok(self.slice(bounds).get_mapped_range())
    }

    #[inline]
    pub fn get_mapped_range_mut<S: RangeBounds<BufferAddress>>(
        &self,
        bounds: S,
    ) -> Result<WriteOnly<'_, [u8]>, BufferAccessError> {
        Ok(self.slice(bounds).get_mapped_range_mut())
    }

    #[inline]
    pub fn as_entire_binding(&self) -> BindingResource<'_> {
        BindingResource::Buffer(BufferBinding {
            buffer: self,
            offset: 0,
            size: None,
        })
    }

    #[inline]
    pub fn as_entire_buffer_binding(&self) -> BufferBinding<'_> {
        BufferBinding {
            buffer: self,
            offset: 0,
            size: None,
        }
    }

    #[inline]
    pub fn unmap(&self) {
        self.inner.mapped_ready.store(false, Ordering::Release);
    }

    #[inline]
    pub fn destroy(&self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapMode {
    Read,
    Write,
}

#[derive(Debug, Clone)]
pub struct BufferAsyncError;
impl std::fmt::Display for BufferAsyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BufferAsyncError")
    }
}
impl std::error::Error for BufferAsyncError {}

#[derive(Debug, Clone)]
pub struct BufferAccessError(pub String);
impl std::fmt::Display for BufferAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for BufferAccessError {}

#[derive(Clone, Copy)]
pub struct BufferSlice<'a> {
    pub buffer: &'a Buffer,
    pub offset: BufferAddress,
    pub size: BufferAddress,
}

impl<'a> BufferSlice<'a> {
    #[inline]
    pub fn map_async<F>(&self, _mode: MapMode, callback: F)
    where
        F: FnOnce(Result<(), BufferAsyncError>) + Send + 'static,
    {
        // On Apple Silicon UMA, shared memory is already mapped; ensure GPU work is complete before reading
        if !self.buffer.inner.device.is_last_cmd_completed() {
            self.buffer.inner.device.wait_idle();
        }
        self.buffer.inner.mapped_ready.store(true, Ordering::Release);
        callback(Ok(()));
    }

    #[inline]
    pub fn get_mapped_range(&self) -> BufferView<'a> {
        if !self.buffer.inner.device.is_last_cmd_completed() {
            self.buffer.inner.device.wait_idle();
        }
        let slice = unsafe {
            std::slice::from_raw_parts(
                self.buffer.inner.ptr.add(self.offset as usize),
                self.size as usize,
            )
        };
        BufferView { slice }
    }

    #[inline]
    pub fn get_mapped_range_mut(&self) -> WriteOnly<'a, [u8]> {
        if !self.buffer.inner.device.is_last_cmd_completed() {
            self.buffer.inner.device.wait_idle();
        }
        let slice = unsafe {
            std::slice::from_raw_parts_mut(
                self.buffer.inner.ptr.add(self.offset as usize),
                self.size as usize,
            )
        };
        WriteOnly::new(slice)
    }
}

pub struct BufferView<'a> {
    slice: &'a [u8],
}

impl<'a> Deref for BufferView<'a> {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.slice
    }
}

impl<'a> AsRef<[u8]> for BufferView<'a> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.slice
    }
}
