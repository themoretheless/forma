//! Command submission, completion and device loss.

use super::*;

impl MetalDevice {
    /// Installs a completion handler that notifies `scope_completion_service` when Metal
    /// finishes executing `command`, avoiding blocking `waitUntilCompleted` driver calls.
    ///
    /// # Safety
    ///
    /// `command` must be a valid uncommitted `MTLCommandBuffer` pointer.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(super) unsafe fn install_completion_signal(&self, command: *mut c_void) {
        let service = Arc::downgrade(&self.scope_completion_service);
        // Metal copies this block before commit. Only a Send+Sync Weak signal
        // crosses the driver thread; user callbacks run on caller/collector threads.
        let block = block2::RcBlock::new(move |_: *mut c_void| {
            if let Some(service) = service.upgrade() {
                service.native_completed();
            }
        });
        let add: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut block2::Block<dyn Fn(*mut c_void)>,
        ) = msg_send();
        // SAFETY: `command` is an uncommitted `MTLCommandBuffer` and `block` matches Metal's
        // `MTLCommandBufferHandler` signature.
        unsafe {
            add(
                command,
                sel("addCompletedHandler:"),
                block2::RcBlock::as_ptr(&block),
            );
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn flush_pending_compute(&self) {
        let batch = if let Ok(mut v) = self.pending_compute_ops.lock() {
            if v.ops.is_empty() {
                return;
            }
            std::mem::take(&mut *v)
        } else {
            return;
        };
        let s = selectors();
        let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
        let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();

        // SAFETY: `self.queue` is a valid `MTLCommandQueue`; the encoder is ended before commit.
        let cmd_buf = unsafe {
            let cmd_buf_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
            let Some(cmd_buf) = ObjCObject::from_unretained(cmd_buf_ptr) else {
                return;
            };
            let enc = f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
            crate::encode_compute_dispatches(enc, &batch.ops);
            f_void(enc, s.end_encoding);
            self.install_completion_signal(cmd_buf.as_ptr());
            f_void(cmd_buf.as_ptr(), s.commit);
            cmd_buf
        };
        let idx = batch.last_idx;
        let _ = self.device_loss_message(); // Reap completed work even when callers do not poll.
        let scoped = batch
            .sinks
            .iter()
            .any(crate::CommandErrorSink::has_scoped_work);
        self.pending_error_checks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(PendingCommand {
                command: CompletionCommand::Native(cmd_buf.clone()),
                sinks: batch.sinks,
                submission_index: idx,
            });
        if scoped {
            self.scope_completion_service.notify();
        }
        if let Ok(mut last) = self.last_cmd_buf.lock() {
            *last = Some((cmd_buf, idx));
        }
    }

    /// Classifies an `NSError` reported by a failed `MTLCommandBuffer`.
    ///
    /// # Safety
    ///
    /// `error` must be a non-null valid `NSError` instance pointer.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(super) unsafe fn classify_command_nserror(
        &self,
        error: *mut c_void,
        failure: &mut Option<crate::Error>,
    ) {
        let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
        let f_code: unsafe extern "C" fn(*mut c_void, *mut c_void) -> isize = msg_send();
        // SAFETY: Caller guarantees `error` is a live `NSError`; static domain global is read-only.
        let (code, domain, metal_domain, description) = unsafe {
            let code = f_code(error, sel("code")) as i64;
            let domain = nsstring_to_string(f_obj(error, sel("domain")));
            let metal_domain = domain == nsstring_to_string(MTLCommandBufferErrorDomain);
            let description = nserror_description(error);
            (code, domain, metal_domain, description)
        };
        if metal_domain && command_error_is_device_loss(code) {
            self.record_backend_device_loss(format!(
                "Metal command buffer device loss (code {code}): {description}"
            ));
        } else {
            *failure = Some(command_failure_error(
                metal_domain,
                code,
                format!(
                    "Metal command buffer failure (domain {domain}, code {code}): {description}"
                ),
            ));
        }
    }

    /// # Safety
    ///
    /// `command` must point to a valid `MTLCommandBuffer`.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(super) unsafe fn inspect_command_loss(
        &self,
        command: *mut c_void,
        failure: &mut Option<crate::Error>,
    ) -> bool {
        let f_status: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize = msg_send();
        // SAFETY: Caller guarantees `command` is a valid `MTLCommandBuffer`.
        let status = unsafe { f_status(command, selectors().status) };
        if status < 4 {
            return false;
        }
        if status == 5 {
            let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            // SAFETY: Failed `MTLCommandBuffer` retains its `NSError` while `command` is live.
            let error = unsafe { f_obj(command, sel("error")) };
            if !error.is_null() {
                // SAFETY: `error` is a non-null `NSError` returned by `command`.
                unsafe { self.classify_command_nserror(error, failure) };
            } else {
                *failure = Some(command_failure_error(
                    false,
                    0,
                    "Metal command buffer failed without NSError".into(),
                ));
            }
        }
        true
    }

    #[cfg(all(
        any(test, feature = "test-hooks"),
        any(target_os = "macos", target_os = "ios")
    ))]
    pub(crate) fn inject_device_loss_for_test(&self, message: String) {
        self.record_backend_device_loss(message);
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    pub(crate) fn last_command_owned_by_for_test(&self, sink: &crate::CommandErrorSink) -> bool {
        self.pending_error_checks
            .lock()
            .unwrap()
            .last()
            .is_some_and(|entry| entry.sinks.len() == 1 && entry.sinks[0].same_device(sink))
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    pub(crate) fn inject_command_failure_for_test(
        &self,
        sink: crate::CommandErrorSink,
        message: &str,
    ) {
        self.pending_error_checks
            .lock()
            .unwrap()
            .push(PendingCommand {
                command: CompletionCommand::Failed(command_failure_error(false, 0, message.into())),
                sinks: vec![sink.capture()],
                submission_index: 0,
            });
        self.scope_completion_service.notify();
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    pub(crate) fn inject_classified_command_failure_for_test(
        &self,
        sink: crate::CommandErrorSink,
        metal_domain: bool,
        code: i64,
    ) {
        self.pending_error_checks
            .lock()
            .unwrap()
            .push(PendingCommand {
                command: CompletionCommand::Failed(command_failure_error(
                    metal_domain,
                    code,
                    format!("injected code {code}"),
                )),
                sinks: vec![sink.capture()],
                submission_index: 0,
            });
        self.scope_completion_service.notify();
    }

    /// Poll completed command buffers without blocking and retain the first
    /// device-removal/access-revocation cause for all logical device observers.
    pub(crate) fn device_loss_message(&self) -> Option<String> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            // Own the batch outside the mutex: successful completion can also
            // drop a scope lease and invoke an arbitrary Future waker.
            let mut pending = std::mem::take(
                &mut *self
                    .pending_error_checks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()),
            );
            let mut index = 0;
            while index < pending.len() {
                let mut failure = None;
                let completed = match &pending[index].command {
                    CompletionCommand::Native(command) => unsafe {
                        self.inspect_command_loss(command.as_ptr(), &mut failure)
                    },
                    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
                    CompletionCommand::Failed(error) => {
                        failure = Some(error.clone());
                        true
                    }
                };
                if completed {
                    let entry = pending.swap_remove(index);
                    for sink in entry.sinks {
                        if let Some(error) = &failure {
                            sink.command_failure(error.clone());
                        }
                    }
                } else {
                    index += 1;
                }
            }
            let mut queued = self
                .pending_error_checks
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if queued.is_empty() {
                *queued = pending;
            } else {
                queued.append(&mut pending);
            }
        }
        self.backend_device_loss
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn wait_idle(&self) {
        #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
        {
            self.blocking_wait_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.flush_pending_compute();
        let cmd_opt = self.last_cmd_buf.lock().ok().and_then(|mut g| g.take());
        if let Some((cmd_buf, _)) = cmd_opt {
            unsafe {
                let s = selectors();
                let f_status: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize = msg_send();
                let st = f_status(cmd_buf.as_ptr(), s.status);
                // MTLCommandBufferStatusCompleted = 4, Error = 5
                if st < 4 {
                    let f_wait: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
                    f_wait(cmd_buf.as_ptr(), s.wait_until_completed);
                }
            }
        }
        let _ = self.device_loss_message();
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn is_last_cmd_completed(&self) -> bool {
        if let Ok(pending) = self.pending_compute_ops.lock() {
            if !pending.ops.is_empty() {
                return false;
            }
        }
        let Ok(guard) = self.last_cmd_buf.lock() else {
            return true;
        };
        let Some((cmd_buf, _)) = guard.as_ref() else {
            return true;
        };
        unsafe {
            let s = selectors();
            let f_status: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize = msg_send();
            f_status(cmd_buf.as_ptr(), s.status) >= 4
        }
    }

    pub fn submit<I: IntoIterator<Item = crate::CommandBuffer>>(
        &self,
        command_buffers: I,
    ) -> crate::SubmissionIndex {
        self.submit_with_error_sink(command_buffers, None)
    }

    pub(crate) fn submit_with_error_sink<I: IntoIterator<Item = crate::CommandBuffer>>(
        &self,
        command_buffers: I,
        error_sink: Option<crate::CommandErrorSink>,
    ) -> crate::SubmissionIndex {
        use crate::SubmissionIndex;
        use std::sync::atomic::Ordering;

        let idx = self.next_submission_index.fetch_add(1, Ordering::Relaxed);

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            let Some(cb) = self.merged(command_buffers) else {
                return SubmissionIndex(idx);
            };
            // Each fast path hands the buffer back when it does not apply.
            let cb = match self.batch_untimed_compute(cb, &error_sink, idx) {
                Ok(()) => return SubmissionIndex(idx),
                Err(cb) => cb,
            };
            let cb = match self.run_without_commands(cb, idx) {
                Ok(()) => return SubmissionIndex(idx),
                Err(cb) => cb,
            };
            let cb = match self.copy_small_buffer_on_cpu(cb) {
                Ok(()) => return SubmissionIndex(idx),
                Err(cb) => cb,
            };
            self.encode_and_commit(cb, error_sink, idx);
        }

        SubmissionIndex(idx)
    }

    /// Joins the command buffers of one submission into the first of them.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn merged<I: IntoIterator<Item = crate::CommandBuffer>>(
        &self,
        command_buffers: I,
    ) -> Option<crate::CommandBuffer> {
        let mut iter = command_buffers.into_iter();
        let mut first_cb = iter.next()?;
        for mut extra in iter {
            let Some((extra_ops, extra_cbs)) = extra.metal_parts_mut() else {
                continue;
            };
            let mut ops = std::mem::take(extra_ops);
            let mut cbs = std::mem::take(extra_cbs);
            if let Some((first_ops, first_cbs)) = first_cb.metal_parts_mut() {
                first_ops.append(&mut ops);
                self.recycle_cmd_ops(ops);
                first_cbs.append(&mut cbs);
            }
        }
        Some(first_cb)
    }

    /// A submission of nothing but untimed compute passes joins the pending batch instead of
    /// getting a command buffer of its own.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn batch_untimed_compute(
        &self,
        mut cb: crate::CommandBuffer,
        error_sink: &Option<crate::CommandErrorSink>,
        idx: u64,
    ) -> Result<(), crate::CommandBuffer> {
        use crate::RecordedOp;
        let is_pure_untimed_compute = cb.metal_parts_mut().is_some_and(|(ops, cbs)| {
            cbs.is_empty()
                && !ops.is_empty()
                && ops.iter().all(|op| {
                    matches!(
                        op,
                        RecordedOp::ComputePass {
                            dispatches,
                            start_ts: None,
                            end_ts: None,
                        } if !dispatches.is_empty()
                    )
                })
        });
        if !is_pure_untimed_compute {
            return Err(cb);
        }
        let mut should_flush = false;
        if let Ok(mut pending) = self.pending_compute_ops.lock() {
            if let Some(sink) = error_sink.clone() {
                add_error_sink(&mut pending.sinks, sink);
            }
            if let Some((cb_ops, _)) = cb.metal_parts_mut() {
                let mut ops = std::mem::take(cb_ops);
                for op in ops.drain(..) {
                    if let RecordedOp::ComputePass { mut dispatches, .. } = op {
                        pending.ops.append(&mut dispatches);
                        self.recycle_compute_dispatches(dispatches);
                    }
                }
                self.recycle_cmd_ops(ops);
            }
            pending.last_idx = idx;
            should_flush = !pending.ops.is_empty();
        }
        if should_flush {
            self.flush_pending_compute();
        }
        Ok(())
    }

    /// A submission with no commands attaches its callbacks to any earlier work instead of
    /// entering a synchronous backend wait.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn run_without_commands(
        &self,
        mut cb: crate::CommandBuffer,
        idx: u64,
    ) -> Result<(), crate::CommandBuffer> {
        let Some((ops, on_submit_callbacks)) = cb.metal_parts_mut() else {
            return Err(cb);
        };
        if !ops.is_empty() {
            return Err(cb);
        }
        self.recycle_cmd_ops(std::mem::take(ops));
        let callbacks = std::mem::take(on_submit_callbacks);
        if callbacks.is_empty() {
            return Ok(());
        }
        let prev_idx = idx.saturating_sub(1);
        if self.is_last_cmd_completed() {
            for callback in callbacks {
                callback();
            }
        } else {
            for callback in callbacks {
                self.enqueue_work_done(prev_idx, callback);
            }
        }
        Ok(())
    }

    /// A single small copy between CPU-visible buffers is done with a memory copy once earlier
    /// GPU work has finished.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn copy_small_buffer_on_cpu(
        &self,
        mut cb: crate::CommandBuffer,
    ) -> Result<(), crate::CommandBuffer> {
        use crate::RecordedOp;
        let is_small_uma_copy = cb.metal_parts_mut().is_some_and(|(ops, _)| {
            matches!(
                ops.as_slice(),
                [RecordedOp::CopyBufferToBuffer { src, dst, size, .. }]
                    if *size <= 65536 && !src.inner.ptr.is_null() && !dst.inner.ptr.is_null()
            )
        });
        if !is_small_uma_copy {
            return Err(cb);
        }
        self.wait_idle();
        let Some((ops, on_submit_callbacks)) = cb.metal_parts_mut() else {
            return Err(cb);
        };
        if let Some(RecordedOp::CopyBufferToBuffer {
            src,
            src_offset,
            dst,
            dst_offset,
            size,
        }) = ops.pop()
        {
            if size > 0 {
                // SAFETY: `copy_buffer_to_buffer` refuses a range outside either buffer and a
                // copy within one buffer, so both ranges are in bounds and do not overlap.
                unsafe {
                    ptr::copy_nonoverlapping(
                        src.inner.ptr.add(src_offset as usize),
                        dst.inner.ptr.add(dst_offset as usize),
                        size as usize,
                    );
                }
            }
        }
        self.recycle_cmd_ops(std::mem::take(ops));
        let callbacks = std::mem::take(on_submit_callbacks);
        for callback in callbacks {
            callback();
        }
        Ok(())
    }

    /// Whether any of the sinks has an error scope waiting for this work.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn has_scoped_work(error_sinks: &[crate::CommandErrorSink]) -> bool {
        error_sinks
            .iter()
            .any(crate::CommandErrorSink::has_scoped_work)
    }

    /// Commits a command buffer that carries a timed pass, waits for it and stores the GPU times
    /// in the query sets. `allow_instant` accepts an end time equal to the start time.
    ///
    /// # Safety
    ///
    /// `sub_cmd_ptr` must be an uncommitted `MTLCommandBuffer` belonging to `self`.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe fn commit_timed(
        &self,
        sub_cmd_ptr: *mut c_void,
        error_sinks: &[crate::CommandErrorSink],
        start_ts: Option<(crate::QuerySet, u32)>,
        end_ts: Option<(crate::QuerySet, u32)>,
        allow_instant: bool,
        submission_index: u64,
    ) {
        let s = selectors();
        let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
        let f_f64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f64 = msg_send();
        let signal = Self::has_scoped_work(error_sinks);
        // SAFETY: `sub_cmd_ptr` is an uncommitted command buffer created from `self.queue`.
        let (t_start_s, t_end_s) = unsafe {
            self.install_completion_signal(sub_cmd_ptr);
            f_void(sub_cmd_ptr, s.commit);
            f_void(sub_cmd_ptr, s.wait_until_completed);
            if let Some(command) = ObjCObject::from_unretained(sub_cmd_ptr) {
                self.pending_error_checks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(PendingCommand {
                        command: CompletionCommand::Native(command),
                        sinks: error_sinks.to_vec(),
                        submission_index,
                    });
                if signal {
                    self.scope_completion_service.notify();
                }
            }
            (
                f_f64(sub_cmd_ptr, s.gpu_start_time),
                f_f64(sub_cmd_ptr, s.gpu_end_time),
            )
        };
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let ended = t_end_s > t_start_s || (allow_instant && t_end_s == t_start_s);
        let (start_ns, end_ns) = if t_start_s > 0.0 && ended {
            ((t_start_s * 1e9) as u64, (t_end_s * 1e9) as u64)
        } else {
            (now_ns, now_ns + 1000)
        };
        for (slot, value) in [(start_ts, start_ns), (end_ts, end_ns)] {
            let Some((query_set, index)) = slot else {
                continue;
            };
            let Ok(mut timestamps) = query_set.timestamps_ns.lock() else {
                continue;
            };
            if let Some(entry) = timestamps.get_mut(index as usize) {
                *entry = value;
            }
        }
    }

    /// Encodes one recorded operation onto either the shared `active_compute_enc` / `cmd_buf_ptr`
    /// or its own timed command buffer.
    ///
    /// # Safety
    ///
    /// `cmd_buf_ptr` must be an uncommitted `MTLCommandBuffer` belonging to `self`, and
    /// `active_compute_enc` (when non-null) must be an active compute encoder on `cmd_buf_ptr`.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe fn encode_recorded_op(
        &self,
        cmd_buf_ptr: *mut c_void,
        active_compute_enc: &mut *mut c_void,
        op: crate::RecordedOp,
        error_sinks: &[crate::CommandErrorSink],
        submission_index: u64,
    ) {
        use crate::{RecordedOp, encode_compute_dispatches, encode_non_compute_op};
        let s = selectors();
        let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
        let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
        let end_compute = |enc: &mut *mut c_void| {
            if !enc.is_null() {
                // SAFETY: `*enc` is an open compute encoder on `cmd_buf_ptr`.
                unsafe { f_void(*enc, s.end_encoding) };
                *enc = ptr::null_mut();
            }
        };

        match op {
            RecordedOp::ComputePass {
                dispatches,
                start_ts,
                end_ts,
            } => {
                if dispatches.is_empty() {
                    self.recycle_compute_dispatches(dispatches);
                    for (query_set, index) in [start_ts, end_ts].into_iter().flatten() {
                        // SAFETY: `cmd_buf_ptr` has no open non-compute encoder.
                        unsafe {
                            encode_non_compute_op(
                                self,
                                cmd_buf_ptr,
                                RecordedOp::WriteTimestamp { query_set, index },
                            );
                        }
                    }
                    return;
                }
                if start_ts.is_some() || end_ts.is_some() {
                    end_compute(active_compute_enc);
                    // SAFETY: Dedicated command buffer isolates timed compute pass measurements.
                    unsafe {
                        let sub_cmd_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
                        let sub_enc = f_obj(sub_cmd_ptr, s.compute_command_encoder);
                        encode_compute_dispatches(sub_enc, &dispatches);
                        self.recycle_compute_dispatches(dispatches);
                        f_void(sub_enc, s.end_encoding);
                        self.commit_timed(
                            sub_cmd_ptr,
                            error_sinks,
                            start_ts,
                            end_ts,
                            true,
                            submission_index,
                        );
                    }
                    return;
                }
                if active_compute_enc.is_null() {
                    // SAFETY: `cmd_buf_ptr` has no open encoder after previous ops ended theirs.
                    *active_compute_enc = unsafe { f_obj(cmd_buf_ptr, s.compute_command_encoder) };
                }
                // SAFETY: `active_compute_enc` is an open compute command encoder.
                unsafe { encode_compute_dispatches(*active_compute_enc, &dispatches) };
                self.recycle_compute_dispatches(dispatches);
            }
            RecordedOp::RenderPass(mut rp) => {
                end_compute(active_compute_enc);
                let start_ts = rp.start_ts.take();
                let end_ts = rp.end_ts.take();
                if start_ts.is_some() || end_ts.is_some() {
                    // SAFETY: Dedicated command buffer isolates timed render pass measurements.
                    unsafe {
                        let sub_cmd_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
                        crate::encode_render_pass(self, sub_cmd_ptr, rp);
                        self.commit_timed(
                            sub_cmd_ptr,
                            error_sinks,
                            start_ts,
                            end_ts,
                            false,
                            submission_index,
                        );
                    }
                } else {
                    // SAFETY: `cmd_buf_ptr` has no active compute encoder after `end_compute`.
                    unsafe { crate::encode_render_pass(self, cmd_buf_ptr, rp) };
                }
            }
            other => {
                end_compute(active_compute_enc);
                // SAFETY: `cmd_buf_ptr` has no active compute encoder after `end_compute`.
                unsafe { encode_non_compute_op(self, cmd_buf_ptr, other) };
            }
        }
    }

    /// Commits the finished command buffer, installs its completion notification, and records
    /// its pending error sinks and submit callbacks.
    ///
    /// # Safety
    ///
    /// `cmd_buf` must be ready to commit on `self.queue`, and `pool` (when non-null) must be a
    /// valid `NSAutoreleasePool` pushed in the current frame.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe fn finalize_committed_command(
        &self,
        cmd_buf: ObjCObject,
        pool: *mut c_void,
        error_sinks: Vec<crate::CommandErrorSink>,
        idx: u64,
        callbacks: Vec<WorkDoneCallback>,
    ) {
        let s = selectors();
        let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
        let signal = Self::has_scoped_work(&error_sinks);
        // SAFETY: `cmd_buf` is uncommitted and `pool` belongs to this thread/frame.
        unsafe {
            self.install_completion_signal(cmd_buf.as_ptr());
            f_void(cmd_buf.as_ptr(), s.commit);
            if !pool.is_null() {
                f_void(pool, s.drain);
            }
        }
        let _ = self.device_loss_message(); // Reap completed work even when callers do not poll.
        self.pending_error_checks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(PendingCommand {
                command: CompletionCommand::Native(cmd_buf.clone()),
                sinks: error_sinks,
                submission_index: idx,
            });
        if signal {
            self.scope_completion_service.notify();
        }
        if let Ok(mut last) = self.last_cmd_buf.lock() {
            *last = Some((cmd_buf, idx));
        }
        for callback in callbacks {
            self.enqueue_work_done(idx, callback);
        }
    }

    /// Encodes the pending compute batch and the commands of `cb` into one command buffer and
    /// commits it. Timed passes get command buffers of their own.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn encode_and_commit(
        &self,
        mut cb: crate::CommandBuffer,
        error_sink: Option<crate::CommandErrorSink>,
        idx: u64,
    ) {
        use crate::encode_compute_dispatches;
        let batch = std::mem::take(
            &mut *self
                .pending_compute_ops
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        );
        let mut pending = batch.ops;
        let mut error_sinks = batch.sinks;
        if let Some(sink) = error_sink {
            add_error_sink(&mut error_sinks, sink);
        }

        let s = selectors();
        let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
        let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();

        // SAFETY: Allocates an autorelease pool and command buffer from `self.queue`; ends the
        // compute encoder before committing or draining the pool.
        let Some((cb_ops, cb_callbacks)) = cb.metal_parts_mut() else {
            return;
        };
        let callbacks = std::mem::take(cb_callbacks);
        let mut ops = std::mem::take(cb_ops);
        unsafe {
            let pool = f_obj(f_obj(s.cls_ns_autorelease_pool, s.alloc), s.init);
            let cmd_buf_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
            assert!(!cmd_buf_ptr.is_null(), "Failed to create MTLCommandBuffer");
            let cmd_buf = ObjCObject::from_unretained(cmd_buf_ptr).unwrap();

            // The compute encoder stays open across consecutive untimed compute passes.
            let mut active_compute_enc: *mut c_void = ptr::null_mut();
            if !pending.is_empty() {
                active_compute_enc = f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
                encode_compute_dispatches(active_compute_enc, &pending);
                pending.clear();
            }
            for op in ops.drain(..) {
                self.encode_recorded_op(
                    cmd_buf.as_ptr(),
                    &mut active_compute_enc,
                    op,
                    &error_sinks,
                    idx,
                );
            }
            self.recycle_cmd_ops(ops);
            if !active_compute_enc.is_null() {
                f_void(active_compute_enc, s.end_encoding);
            }
            self.finalize_committed_command(cmd_buf, pool, error_sinks, idx, callbacks);
        }
    }
}
