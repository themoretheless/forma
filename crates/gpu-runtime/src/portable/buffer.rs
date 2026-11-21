//! Portable buffer allocation, shadow synchronization, and mapped range flushing.

#[cfg(not(target_arch = "wasm32"))]
use crate::BufferAccessError;
use crate::portable::PortableDevice;
use crate::portable::convert;
use crate::{
    Buffer, BufferAddress, BufferAsyncError, BufferDescriptor, BufferInner, BufferUsages,
    CommandErrorSink, Error, Features, MapMode, validation_error,
};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) struct PortableBuffer {
    pub(crate) raw: upstream_wgpu::Buffer,
    pub(crate) device: Arc<PortableDevice>,
    pub(crate) mapped_at_creation: AtomicBool,
    pub(crate) mapped_write_range: Mutex<Option<Range<u64>>>,
    backing_ptr: *mut u8,
    backing_len: usize,
}

// SAFETY: `backing_ptr` points to a unique heap allocation controlled by `MappedRanges` and `Mutex`.
unsafe impl Send for PortableBuffer {}
// SAFETY: `backing_ptr` accesses are serialized per byte range via `MappedRanges` and `PortableBuffer` state.
unsafe impl Sync for PortableBuffer {}

impl Drop for PortableBuffer {
    fn drop(&mut self) {
        if self.backing_len > 0 && !self.backing_ptr.is_null() {
            let slice_ptr = std::ptr::slice_from_raw_parts_mut(self.backing_ptr, self.backing_len);
            // SAFETY: Allocated with `Box::into_raw` at exact length `self.backing_len`; no live views remain.
            unsafe {
                drop(Box::from_raw(slice_ptr));
            }
            self.backing_ptr = std::ptr::null_mut();
            self.backing_len = 0;
        }
    }
}

impl PortableBuffer {
    fn alloc_shadow(len: usize) -> (*mut u8, usize) {
        if len == 0 {
            return (std::ptr::NonNull::<u8>::dangling().as_ptr(), 0);
        }
        let boxed = vec![0u8; len].into_boxed_slice();
        let actual_len = boxed.len();
        let ptr = Box::into_raw(boxed) as *mut u8;
        (ptr, actual_len)
    }

    pub(crate) fn copy_into_backing(&self, offset: usize, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let Some(end) = offset.checked_add(bytes.len()) else {
            return;
        };
        if end > self.backing_len || self.backing_ptr.is_null() {
            return;
        }
        // SAFETY: `offset + bytes.len() <= self.backing_len` and `self.backing_ptr` is uniquely owned here.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.backing_ptr.add(offset),
                bytes.len(),
            );
        }
    }

    pub(crate) fn backing_slice(&self, offset: usize, len: usize) -> Option<&[u8]> {
        let end = offset.checked_add(len)?;
        if end > self.backing_len || self.backing_ptr.is_null() {
            return None;
        }
        // SAFETY: `offset + len <= self.backing_len`; callers hold exclusive `MappedRanges` or immutable lock.
        Some(unsafe { std::slice::from_raw_parts(self.backing_ptr.add(offset), len) })
    }

    /// Synchronizes a write lease that was closed back into the underlying GPU buffer.
    pub(crate) fn flush_written_range(&self, start: usize, end: usize) {
        let Some(len) = end.checked_sub(start).filter(|&l| l > 0) else {
            return;
        };
        let Some(bytes) = self.backing_slice(start, len) else {
            return;
        };
        let mapped_write = self
            .mapped_write_range
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some();
        if self.mapped_at_creation.load(Ordering::Acquire) || mapped_write {
            if let Ok(mut dst) = self
                .raw
                .slice(start as u64..end as u64)
                .get_mapped_range_mut()
            {
                dst.copy_from_slice(bytes);
            }
        } else {
            self.device
                .queue
                .write_buffer(&self.raw, start as u64, bytes);
        }
    }

    pub(crate) fn unmap(&self) {
        let was_mapped_at_creation = self.mapped_at_creation.swap(false, Ordering::AcqRel);
        let mapped_write = self
            .mapped_write_range
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if was_mapped_at_creation {
            if let Some(bytes) = self.backing_slice(0, self.backing_len) {
                if let Ok(mut dst) = self.raw.slice(..).get_mapped_range_mut() {
                    dst.copy_from_slice(bytes);
                }
            }
            self.raw.unmap();
        } else if let Some(range) = mapped_write {
            let len = (range.end - range.start) as usize;
            if let Some(bytes) = self.backing_slice(range.start as usize, len) {
                if let Ok(mut dst) = self.raw.slice(range).get_mapped_range_mut() {
                    dst.copy_from_slice(bytes);
                }
            }
            self.raw.unmap();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn sync_read_range(&self, start: u64, size: u64) -> Result<(), BufferAccessError> {
        if size == 0 {
            return Ok(());
        }
        let Some(end) = start.checked_add(size) else {
            return Err(BufferAccessError("buffer readback range overflows".into()));
        };
        let status = Arc::new(Mutex::new(None));
        let done = status.clone();
        self.raw
            .slice(start..end)
            .map_async(upstream_wgpu::MapMode::Read, move |res| {
                *done.lock().unwrap_or_else(|e| e.into_inner()) = Some(res);
            });
        let _ = self
            .device
            .device
            .poll(upstream_wgpu::PollType::wait_indefinitely());
        let result = status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .unwrap_or(Err(upstream_wgpu::BufferAsyncError));
        if result.is_err() {
            return Err(BufferAccessError("GPU buffer readback failed".into()));
        }
        {
            let mapped = self
                .raw
                .slice(start..end)
                .get_mapped_range()
                .map_err(|err| BufferAccessError(err.to_string()))?;
            self.copy_into_backing(start as usize, &mapped);
        }
        self.raw.unmap();
        Ok(())
    }
}

pub(crate) fn create_buffer(
    device: &Arc<PortableDevice>,
    features: Features,
    limits: &crate::Limits,
    record_error: &dyn Fn(Error),
    desc: &BufferDescriptor<'_>,
) -> Buffer {
    let missing_ray_query = desc
        .usage
        .intersects(BufferUsages::BLAS_INPUT | BufferUsages::TLAS_INPUT)
        && !features.contains(Features::EXPERIMENTAL_RAY_QUERY);
    let missing_mappable_primary = !features.contains(Features::MAPPABLE_PRIMARY_BUFFERS)
        && ((desc.usage.contains(BufferUsages::MAP_READ)
            && !(desc.usage - BufferUsages::MAP_READ - BufferUsages::COPY_DST).is_empty())
            || (desc.usage.contains(BufferUsages::MAP_WRITE)
                && !(desc.usage - BufferUsages::MAP_WRITE - BufferUsages::COPY_SRC).is_empty()));
    let invalid_descriptor = desc.usage.is_empty()
        || desc.size > limits.max_buffer_size
        || missing_ray_query
        || missing_mappable_primary;

    if invalid_descriptor {
        let msg = format!(
            "Invalid buffer descriptor: size={}, usage={:?}, enabled_features={:?}",
            desc.size, desc.usage, features
        );
        record_error(validation_error(msg));
        return Buffer::inert(desc.size, desc.usage);
    }

    let Ok(len) = usize::try_from(desc.size) else {
        record_error(Error::OutOfMemory {
            source: Arc::new(std::io::Error::new(
                std::io::ErrorKind::OutOfMemory,
                "buffer size exceeds host address space",
            )),
        });
        return Buffer::inert(desc.size, desc.usage);
    };

    let raw = device
        .device
        .create_buffer(&upstream_wgpu::BufferDescriptor {
            label: desc.label,
            size: desc.size,
            usage: convert::to_wgpu_buffer_usages(desc.usage),
            mapped_at_creation: desc.mapped_at_creation,
        });
    let (backing_ptr, backing_len) = PortableBuffer::alloc_shadow(len);
    let portable = Arc::new(PortableBuffer {
        raw,
        device: device.clone(),
        mapped_at_creation: AtomicBool::new(desc.mapped_at_creation),
        mapped_write_range: Mutex::new(None),
        backing_ptr,
        backing_len,
    });
    Buffer::from_portable(
        portable,
        backing_ptr,
        desc.size,
        desc.usage,
        desc.mapped_at_creation,
    )
}

pub(crate) fn create_buffer_init(
    device: &Arc<PortableDevice>,
    limits: &crate::Limits,
    record_error: &dyn Fn(Error),
    label: crate::Label<'_>,
    contents: &[u8],
    usage: BufferUsages,
) -> Buffer {
    let size = (contents.len() as u64).max(4);
    if usage.is_empty() || size > limits.max_buffer_size {
        let msg = format!("Invalid buffer_init descriptor: size={size}, usage={usage:?}");
        record_error(validation_error(msg));
        return Buffer::inert(size, usage);
    }
    let raw = device
        .device
        .create_buffer(&upstream_wgpu::BufferDescriptor {
            label,
            size,
            usage: convert::to_wgpu_buffer_usages(usage),
            mapped_at_creation: true,
        });
    {
        if let Ok(mut mapped) = raw.slice(..contents.len() as u64).get_mapped_range_mut() {
            mapped.copy_from_slice(contents);
        }
    }
    raw.unmap();

    let (backing_ptr, backing_len) = PortableBuffer::alloc_shadow(size as usize);
    let portable = Arc::new(PortableBuffer {
        raw,
        device: device.clone(),
        mapped_at_creation: AtomicBool::new(false),
        mapped_write_range: Mutex::new(None),
        backing_ptr,
        backing_len,
    });
    portable.copy_into_backing(0, contents);
    Buffer::from_portable(portable, backing_ptr, size, usage, false)
}

pub(crate) fn write_buffer(
    device: &PortableDevice,
    portable: &PortableBuffer,
    buffer_size: BufferAddress,
    offset: BufferAddress,
    data: &[u8],
    sink: Option<CommandErrorSink>,
) {
    let fits = offset
        .checked_add(data.len() as u64)
        .is_some_and(|end| end <= buffer_size);
    if !fits {
        if let Some(sink) = sink {
            sink.command_failure(validation_error(format!(
                "write_buffer: {} bytes at offset {offset} exceed buffer of {buffer_size} bytes",
                data.len()
            )));
        }
        return;
    }
    portable.copy_into_backing(offset as usize, data);
    device.queue.write_buffer(&portable.raw, offset, data);
}

fn finish_map_callback(
    buf_inner: Arc<BufferInner>,
    port: Arc<PortableBuffer>,
    offset: u64,
    end: u64,
    mode: MapMode,
    callback: impl FnOnce(Result<(), BufferAsyncError>) + Send + 'static,
) -> impl FnOnce(Result<(), upstream_wgpu::BufferAsyncError>) + Send + 'static {
    move |res| match res {
        Ok(()) => {
            match mode {
                MapMode::Read => {
                    let ok = {
                        if let Ok(view) = port.raw.slice(offset..end).get_mapped_range() {
                            port.copy_into_backing(offset as usize, &view);
                            true
                        } else {
                            false
                        }
                    };
                    port.raw.unmap();
                    if !ok {
                        callback(Err(BufferAsyncError));
                        return;
                    }
                    buf_inner.mapped_ready.store(true, Ordering::Release);
                }
                MapMode::Write => {
                    *port
                        .mapped_write_range
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(offset..end);
                    buf_inner.mapped_ready.store(true, Ordering::Release);
                }
            }
            callback(Ok(()));
        }
        Err(_) => callback(Err(BufferAsyncError)),
    }
}

pub(crate) fn map_async(
    inner: &Arc<BufferInner>,
    portable: &Arc<PortableBuffer>,
    offset: u64,
    size: u64,
    mode: MapMode,
    callback: impl FnOnce(Result<(), BufferAsyncError>) + Send + 'static,
) {
    let Some(end) = offset.checked_add(size).filter(|&e| e <= inner.size) else {
        callback(Err(BufferAsyncError));
        return;
    };
    portable.raw.slice(offset..end).map_async(
        convert::to_wgpu_map_mode(mode),
        finish_map_callback(inner.clone(), portable.clone(), offset, end, mode, callback),
    );
}

pub(crate) fn map_buffer_on_submit(
    encoder: &upstream_wgpu::CommandEncoder,
    inner: &Arc<BufferInner>,
    portable: &Arc<PortableBuffer>,
    offset: u64,
    size: u64,
    mode: MapMode,
    callback: impl FnOnce(Result<(), BufferAsyncError>) + Send + 'static,
) {
    let Some(end) = offset.checked_add(size).filter(|&e| e <= inner.size) else {
        callback(Err(BufferAsyncError));
        return;
    };
    encoder.map_buffer_on_submit(
        &portable.raw,
        convert::to_wgpu_map_mode(mode),
        offset..end,
        finish_map_callback(inner.clone(), portable.clone(), offset, end, mode, callback),
    );
}
