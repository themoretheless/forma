//! GPU Buffer, UMA Direct Slices (`uma_slice`, `uma_slice_mut`), `BufferSlice`, `BufferView`, and `WriteOnly` mapping views.

use crate::metal::{MetalDevice, ObjCObject};
use crate::{BindingResource, BufferAddress, BufferBinding, BufferUsages, Label};
use std::ops::{Bound, Deref, DerefMut, Range, RangeBounds};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// The byte ranges of one buffer that are currently handed out as views.
///
/// A buffer is a shared handle (`Buffer: Clone`), so the borrow checker cannot see two views of
/// the same bytes. This table refuses a view that overlaps a writable one, and a writable view
/// that overlaps any other, which is what keeps the `&mut [u8]` inside [`WriteOnly`] unique.
#[derive(Default)]
pub(crate) struct MappedRanges(Mutex<Vec<MappedRange>>);

struct MappedRange {
    start: usize,
    end: usize,
    write: bool,
    views: u32,
}

impl MappedRanges {
    /// Registers `start..end`, or returns `None` when it overlaps a range that excludes it.
    /// An empty range never conflicts and holds nothing.
    fn acquire(&self, start: usize, end: usize, write: bool) -> Option<Option<Lease<'_>>> {
        if start >= end {
            return Some(None);
        }
        let mut ranges = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if ranges
            .iter()
            .any(|held| (write || held.write) && start < held.end && held.start < end)
        {
            return None;
        }
        match ranges
            .iter_mut()
            .find(|held| !write && held.start == start && held.end == end)
        {
            Some(held) => held.views += 1,
            None => ranges.push(MappedRange {
                start,
                end,
                write,
                views: 1,
            }),
        }
        Some(Some(Lease {
            ranges: self,
            start,
            end,
        }))
    }
}

/// Keeps one range registered in [`MappedRanges`] for as long as a view of it is alive.
struct Lease<'a> {
    ranges: &'a MappedRanges,
    start: usize,
    end: usize,
}

impl Clone for Lease<'_> {
    fn clone(&self) -> Self {
        let mut ranges = self.ranges.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(held) = ranges
            .iter_mut()
            .find(|held| held.start == self.start && held.end == self.end)
        {
            held.views += 1;
        }
        Self {
            ranges: self.ranges,
            start: self.start,
            end: self.end,
        }
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let mut ranges = self.ranges.0.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(index) = ranges
            .iter()
            .position(|held| held.start == self.start && held.end == self.end)
        else {
            return;
        };
        let Some(held) = ranges.get_mut(index) else {
            return;
        };
        held.views -= 1;
        if held.views == 0 {
            ranges.swap_remove(index);
        }
    }
}

/// The address and length of a registered range, with the lease that keeps it registered.
type LeasedBytes<'a> = (*mut u8, usize, Option<Lease<'a>>);

pub struct WriteOnly<'a, T: ?Sized> {
    slice: &'a mut T,
    lease: Option<Lease<'a>>,
}

impl<'a> WriteOnly<'a, [u8]> {
    #[inline]
    pub fn new(slice: &'a mut [u8]) -> Self {
        Self { slice, lease: None }
    }

    #[inline]
    pub fn expect(self, _msg: &str) -> Self {
        self
    }

    #[inline]
    pub fn unwrap(self) -> Self {
        self
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
            lease: None,
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
            lease: None,
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
            lease: self.lease,
        }
    }

    #[inline]
    pub fn into_chunks<const N: usize>(self) -> (WriteOnly<'a, [[u8; N]]>, WriteOnly<'a, [u8]>) {
        assert!(N > 0);
        let len = self.slice.len();
        let num_chunks = len / N;
        let split_at = num_chunks * N;
        let (head, tail) = self.slice.split_at_mut(split_at);
        let chunks = unsafe {
            std::slice::from_raw_parts_mut(head.as_mut_ptr() as *mut [u8; N], num_chunks)
        };
        (
            WriteOnly {
                slice: chunks,
                lease: self.lease.clone(),
            },
            WriteOnly {
                slice: tail,
                lease: self.lease,
            },
        )
    }

    #[inline]
    pub fn chunks_exact_mut(
        &mut self,
        chunk_size: usize,
    ) -> impl Iterator<Item = WriteOnly<'_, [u8]>> {
        self.slice.chunks_exact_mut(chunk_size).map(|c| WriteOnly {
            slice: c,
            lease: None,
        })
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
    pub(crate) mapped_ranges: MappedRanges,
}

unsafe impl Send for BufferInner {}
unsafe impl Sync for BufferInner {}

impl Drop for BufferInner {
    fn drop(&mut self) {
        if self.options != 0xFF {
            self.device.recycle_buffer(
                self.size as usize,
                self.options,
                self.raw.clone(),
                self.ptr,
            );
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

    /// Zero-copy read view of the whole buffer in unified memory, without `map_async`.
    ///
    /// Panics when part of the buffer is handed out as a writable view.
    #[inline]
    pub fn uma_slice(&self) -> BufferView<'_> {
        self.slice(..).get_mapped_range()
    }

    /// Zero-copy writable view of the whole buffer in unified memory.
    ///
    /// Panics when part of the buffer is already handed out as a view.
    #[inline]
    pub fn uma_slice_mut(&mut self) -> WriteOnly<'_, [u8]> {
        self.slice(..).get_mapped_range_mut()
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
        self.slice(bounds).try_mapped_range()
    }

    #[inline]
    pub fn get_mapped_range_mut<S: RangeBounds<BufferAddress>>(
        &self,
        bounds: S,
    ) -> Result<WriteOnly<'_, [u8]>, BufferAccessError> {
        self.slice(bounds).try_mapped_range_mut()
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
        self.buffer
            .inner
            .mapped_ready
            .store(true, Ordering::Release);
        callback(Ok(()));
    }

    /// Panics when the range overlaps a writable view that is still alive.
    #[inline]
    pub fn get_mapped_range(&self) -> BufferView<'a> {
        match self.try_mapped_range() {
            Ok(view) => view,
            Err(err) => panic!("{err}"),
        }
    }

    /// Panics when the range overlaps a view that is still alive.
    #[inline]
    pub fn get_mapped_range_mut(&self) -> WriteOnly<'a, [u8]> {
        match self.try_mapped_range_mut() {
            Ok(view) => view,
            Err(err) => panic!("{err}"),
        }
    }

    fn try_mapped_range(&self) -> Result<BufferView<'a>, BufferAccessError> {
        let Some((ptr, len, lease)) = self.lease(false)? else {
            return Ok(BufferView {
                slice: &[],
                _lease: None,
            });
        };
        // SAFETY: `lease` bounds `ptr..ptr + len` inside the allocation and no writable view of
        // those bytes exists while the lease is held.
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        Ok(BufferView {
            slice,
            _lease: lease,
        })
    }

    fn try_mapped_range_mut(&self) -> Result<WriteOnly<'a, [u8]>, BufferAccessError> {
        let Some((ptr, len, lease)) = self.lease(true)? else {
            return Ok(WriteOnly::new(&mut []));
        };
        // SAFETY: `lease` bounds `ptr..ptr + len` inside the allocation and no other view of
        // those bytes exists while the lease is held.
        let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
        Ok(WriteOnly { slice, lease })
    }

    /// Waits for the GPU, then registers the range. `Ok(None)` is a range with no bytes behind
    /// it: an inert buffer or bounds outside the allocation.
    fn lease(&self, write: bool) -> Result<Option<LeasedBytes<'a>>, BufferAccessError> {
        let inner = &self.buffer.inner;
        if inner.ptr.is_null() {
            return Ok(None);
        }
        if !inner.device.is_last_cmd_completed() {
            inner.device.wait_idle();
        }
        inner.leased_range(self.offset, self.size, write)
    }
}

impl BufferInner {
    /// A writable view of `offset..offset + size`, or `None` when the range is outside the buffer
    /// or overlaps a view that is still alive.
    pub(crate) fn write_view(
        &self,
        offset: BufferAddress,
        size: BufferAddress,
    ) -> Option<WriteOnly<'_, [u8]>> {
        let (ptr, len, lease) = self.leased_range(offset, size, true).ok()??;
        // SAFETY: `lease` bounds `ptr..ptr + len` inside the allocation and no other view of
        // those bytes exists while the lease is held.
        let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
        Some(WriteOnly { slice, lease })
    }

    /// Registers `offset..offset + size` as a view and returns its address.
    fn leased_range(
        &self,
        offset: BufferAddress,
        size: BufferAddress,
        write: bool,
    ) -> Result<Option<LeasedBytes<'_>>, BufferAccessError> {
        let (Some(end), Ok(start), Ok(len)) = (
            offset.checked_add(size).filter(|&end| end <= self.size),
            usize::try_from(offset),
            usize::try_from(size),
        ) else {
            return Ok(None);
        };
        let _ = end;
        let Some(lease) = self.mapped_ranges.acquire(start, start + len, write) else {
            return Err(BufferAccessError(format!(
                "buffer range {start}..{} is already mapped",
                start + len
            )));
        };
        // SAFETY: `start + len <= self.size`, the length of the allocation behind `ptr`.
        Ok(Some((unsafe { self.ptr.add(start) }, len, lease)))
    }
}

pub struct BufferView<'a> {
    slice: &'a [u8],
    _lease: Option<Lease<'a>>,
}

impl<'a> BufferView<'a> {
    #[inline]
    pub fn expect(self, _msg: &str) -> Self {
        self
    }

    #[inline]
    pub fn unwrap(self) -> Self {
        self
    }

    /// Mapping cannot fail here, so this mirrors `Result::map_err` for callers written against
    /// runtimes whose `get_mapped_range` returns a `Result`.
    #[inline]
    pub fn map_err<E, F: FnOnce(BufferAccessError) -> E>(self, _op: F) -> Result<Self, E> {
        Ok(self)
    }
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
