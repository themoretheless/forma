//! Buffer allocation, pooling and writes.

use super::*;

impl MetalDevice {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn alloc_buffer(&self, size: usize, options: u64) -> Option<(Arc<ObjCObject>, *mut u8)> {
        let aligned_size = size.max(16).checked_add(255).map(|n| (n & !255).max(256))?;
        if let Ok(mut pool) = self.buffer_pool.lock() {
            if let Some(pos) = pool
                .iter()
                .position(|entry| entry.aligned_size == aligned_size && entry.options == options)
            {
                let entry = pool.swap_remove(pos);
                if !entry.ptr.is_null() {
                    unsafe {
                        ptr::write_bytes(entry.ptr, 0, size);
                    }
                }
                return Some((entry.raw, entry.ptr));
            }
        }
        unsafe {
            let s = selectors();
            let f_buf: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, u64) -> *mut c_void =
                msg_send();
            let f_contents: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut u8 = msg_send();
            let raw_buf = f_buf(
                self.raw.as_ptr(),
                s.new_buffer_with_length,
                aligned_size,
                options,
            );
            let raw = ObjCObject::from_retained(raw_buf)?;
            let ptr = if (options & 0xF0) == (2 << 4) {
                // MTLStorageModePrivate has no CPU contents pointer
                ptr::null_mut()
            } else {
                f_contents(raw.as_ptr(), s.contents)
            };
            Some((Arc::new(raw), ptr))
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn alloc_buffer_with_bytes(
        &self,
        bytes: &[u8],
        options: u64,
    ) -> Option<(Arc<ObjCObject>, *mut u8)> {
        if bytes.is_empty() {
            return self.alloc_buffer(16, options);
        }
        let aligned_size = bytes.len().checked_add(255).map(|n| (n & !255).max(256))?;
        if let Ok(mut pool) = self.buffer_pool.lock() {
            if let Some(pos) = pool.iter().position(|entry| {
                entry.aligned_size == aligned_size
                    && entry.options == options
                    && !entry.ptr.is_null()
            }) {
                let entry = pool.swap_remove(pos);
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), entry.ptr, bytes.len());
                }
                return Some((entry.raw, entry.ptr));
            }
        }
        unsafe {
            let s = selectors();
            let f_buf: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, u64) -> *mut c_void =
                msg_send();
            let f_contents: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut u8 = msg_send();
            let raw_buf = f_buf(
                self.raw.as_ptr(),
                s.new_buffer_with_length,
                aligned_size,
                options,
            );
            let raw = ObjCObject::from_retained(raw_buf)?;
            let ptr = f_contents(raw.as_ptr(), s.contents);
            if !ptr.is_null() {
                ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            }
            Some((Arc::new(raw), ptr))
        }
    }

    /// ASD (Apple Silicon Direct / Accelerated Storage Direct) zero-copy page-aligned buffer creation.
    ///
    /// # Safety
    ///
    /// `page_aligned_ptr` must be valid, page-aligned for Metal `newBufferWithBytesNoCopy`,
    /// and remain live for `page_aligned_len` bytes until the returned `MTLBuffer` is dropped.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub unsafe fn alloc_buffer_no_copy(
        &self,
        page_aligned_ptr: *mut c_void,
        page_aligned_len: usize,
        options: u64,
    ) -> Option<(Arc<ObjCObject>, *mut u8)> {
        let s = selectors();
        let f_nocopy: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
            usize,
            u64,
            *mut c_void,
        ) -> *mut c_void = msg_send();
        // SAFETY: Caller guarantees `page_aligned_ptr` is valid and page-aligned for
        // `page_aligned_len` bytes, and outlives the `MTLBuffer` wrapper.
        let raw_buf = unsafe {
            f_nocopy(
                self.raw.as_ptr(),
                s.new_buffer_with_bytes_no_copy,
                page_aligned_ptr,
                page_aligned_len,
                options,
                ptr::null_mut(),
            )
        };
        let raw = ObjCObject::from_retained(raw_buf)?;
        Some((Arc::new(raw), page_aligned_ptr as *mut u8))
    }

    pub fn recycle_buffer(&self, size: usize, options: u64, buf: Arc<ObjCObject>, ptr: *mut u8) {
        if Arc::strong_count(&buf) == 1 && !buf.as_ptr().is_null() && !ptr.is_null() {
            if let Some(aligned_size) = size.max(16).checked_add(255).map(|n| (n & !255).max(256)) {
                if let Ok(mut pool) = self.buffer_pool.lock() {
                    if pool.len() < 256 {
                        pool.push(PooledMetalBuffer {
                            aligned_size,
                            options,
                            raw: buf,
                            ptr,
                        });
                    }
                }
            }
        }
    }

    pub fn write_buffer(&self, buffer: &crate::Buffer, offset: crate::BufferAddress, data: &[u8]) {
        self.write_buffer_with_error_sink(buffer, offset, data, None);
    }

    pub(crate) fn write_buffer_with_error_sink(
        &self,
        buffer: &crate::Buffer,
        offset: crate::BufferAddress,
        data: &[u8],
        error_sink: Option<crate::CommandErrorSink>,
    ) {
        if data.is_empty() {
            return;
        }
        let Some(end) = offset.checked_add(data.len() as u64) else {
            if let Some(sink) = error_sink {
                let message = format!(
                    "write_buffer offset overflow: offset={offset}, data_len={}",
                    data.len()
                );
                sink.command_failure(crate::Error::Validation {
                    source: Arc::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        message.clone(),
                    )),
                    description: message,
                });
            }
            return;
        };
        if end > buffer.inner.size || buffer.inner.raw.as_ptr().is_null() {
            if let Some(sink) = error_sink {
                let message = format!(
                    "write_buffer out of bounds or invalid buffer: offset={offset}, data_len={}, buffer_size={}",
                    data.len(),
                    buffer.inner.size
                );
                sink.command_failure(crate::Error::Validation {
                    source: Arc::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        message.clone(),
                    )),
                    description: message,
                });
            }
            return;
        }
        let Ok(offset_usize) = usize::try_from(offset) else {
            return;
        };
        self.flush_pending_compute();
        if !self.is_last_cmd_completed() {
            let _ = self.wait_for_submission(None, None);
        }
        if buffer.inner.ptr.is_null() {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            unsafe {
                let Some((stg_raw, _)) = self.alloc_buffer_with_bytes(data, 0) else {
                    if let Some(sink) = error_sink {
                        sink.command_failure(crate::Error::OutOfMemory {
                            source: Arc::new(std::io::Error::new(
                                std::io::ErrorKind::OutOfMemory,
                                "Metal staging buffer allocation failed",
                            )),
                        });
                    }
                    return;
                };
                let s = selectors();
                let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                    msg_send();
                let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
                let f_copy: unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    usize,
                    *mut c_void,
                    usize,
                    usize,
                ) = msg_send();
                let cmd_buf = f_obj(self.queue.as_ptr(), s.command_buffer);
                let blit = f_obj(cmd_buf, s.blit_command_encoder);
                f_copy(
                    blit,
                    s.copy_from_buffer,
                    stg_raw.as_ptr(),
                    0,
                    buffer.inner.raw.as_ptr(),
                    offset_usize,
                    data.len(),
                );
                f_void(blit, s.end_encoding);
                let signal = error_sink
                    .as_ref()
                    .is_some_and(crate::CommandErrorSink::has_scoped_work);
                self.install_completion_signal(cmd_buf);
                f_void(cmd_buf, s.commit);
                f_void(cmd_buf, s.wait_until_completed);
                if let Some(command) = ObjCObject::from_unretained(cmd_buf) {
                    self.pending_error_checks
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(PendingCommand {
                            command: CompletionCommand::Native(command),
                            sinks: error_sink.into_iter().collect(),
                            submission_index: 0,
                        });
                    if signal {
                        self.scope_completion_service.notify();
                    }
                }
            }
            return;
        }
        unsafe {
            let dst = buffer.inner.ptr.add(offset_usize);
            ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        }
    }
}
