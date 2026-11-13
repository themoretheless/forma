//! Command submission, completion and device loss.

use super::*;

impl MetalDevice {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(super) unsafe fn install_completion_signal(&self, command: *mut c_void, scoped: bool) {
        if !scoped {
            return;
        }
        let service = Arc::downgrade(&self.scope_completion_service);
        // Metal copies this block before commit. Only a Send+Sync Weak signal
        // crosses the driver thread; user callbacks run in the collector.
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
        add(
            command,
            sel("addCompletedHandler:"),
            block2::RcBlock::as_ptr(&block),
        );
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
        unsafe {
            let s = selectors();
            let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();

            let cmd_buf_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
            if cmd_buf_ptr.is_null() {
                return;
            }
            let cmd_buf = ObjCObject::from_unretained(cmd_buf_ptr).unwrap();
            let enc = f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
            crate::encode_compute_dispatches(enc, &batch.ops);
            f_void(enc, s.end_encoding);
            let signal = batch
                .sinks
                .iter()
                .any(crate::CommandErrorSink::has_scoped_work);
            self.install_completion_signal(cmd_buf.as_ptr(), signal);
            f_void(cmd_buf.as_ptr(), s.commit);
            let idx = self
                .next_submission_index
                .load(std::sync::atomic::Ordering::Relaxed);
            let _ = self.device_loss_message(); // Reap completed work even when callers do not poll.
            self.pending_error_checks
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(PendingCommand {
                    command: CompletionCommand::Native(cmd_buf.clone()),
                    sinks: batch.sinks,
                });
            if signal {
                self.scope_completion_service.notify();
            }
            if let Ok(mut last) = self.last_cmd_buf.lock() {
                *last = Some((cmd_buf, idx));
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub(super) unsafe fn inspect_command_loss(
        &self,
        command: *mut c_void,
        failure: &mut Option<crate::Error>,
    ) -> bool {
        let f_status: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize = msg_send();
        let status = f_status(command, selectors().status);
        if status < 4 {
            return false;
        }
        if status == 5 {
            let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
            let error = f_obj(command, sel("error"));
            if !error.is_null() {
                let f_code: unsafe extern "C" fn(*mut c_void, *mut c_void) -> isize = msg_send();
                let code = f_code(error, sel("code")) as i64;
                let domain = nsstring_to_string(f_obj(error, sel("domain")));
                let metal_domain = domain == nsstring_to_string(MTLCommandBufferErrorDomain);
                if metal_domain && command_error_is_device_loss(code) {
                    let mut loss = self
                        .backend_device_loss
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if loss.is_none() {
                        *loss = Some(format!(
                            "Metal command buffer device loss (code {code}): {}",
                            nserror_description(error)
                        ));
                    }
                } else {
                    *failure = Some(command_failure_error(
                        metal_domain,
                        code,
                        format!(
                            "Metal command buffer failure (domain {domain}, code {code}): {}",
                            nserror_description(error)
                        ),
                    ));
                }
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
        let mut loss = self.backend_device_loss.lock().unwrap();
        if loss.is_none() {
            *loss = Some(message);
        }
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
        unsafe {
            let Some(cb) = self.merged(command_buffers) else {
                return SubmissionIndex(idx);
            };
            // Each fast path hands the buffer back when it does not apply.
            let cb = match self.batch_untimed_compute(cb, &error_sink) {
                Ok(()) => return SubmissionIndex(idx),
                Err(cb) => cb,
            };
            let cb = match self.run_without_commands(cb) {
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
            let mut ops = std::mem::take(&mut extra.ops);
            first_cb.ops.append(&mut ops);
            self.recycle_cmd_ops(ops);
            first_cb
                .on_submit_callbacks
                .append(&mut extra.on_submit_callbacks);
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
    ) -> Result<(), crate::CommandBuffer> {
        use crate::RecordedOp;
        let is_pure_untimed_compute = cb.on_submit_callbacks.is_empty()
            && !cb.ops.is_empty()
            && cb.ops.iter().all(|op| {
                matches!(
                    op,
                    RecordedOp::ComputePass {
                        dispatches,
                        start_ts: None,
                        end_ts: None,
                    } if !dispatches.is_empty()
                )
            });
        if !is_pure_untimed_compute {
            return Err(cb);
        }
        let mut should_flush = false;
        if let Ok(mut pending) = self.pending_compute_ops.lock() {
            if let Some(sink) = error_sink.clone() {
                add_error_sink(&mut pending.sinks, sink);
            }
            let mut ops = std::mem::take(&mut cb.ops);
            for op in ops.drain(..) {
                if let RecordedOp::ComputePass { mut dispatches, .. } = op {
                    pending.ops.append(&mut dispatches);
                    self.recycle_compute_dispatches(dispatches);
                }
            }
            self.recycle_cmd_ops(ops);
            should_flush = !pending.ops.is_empty();
        }
        if should_flush {
            self.flush_pending_compute();
        }
        Ok(())
    }

    /// A submission with no commands only waits for earlier work and runs its callbacks.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn run_without_commands(
        &self,
        mut cb: crate::CommandBuffer,
    ) -> Result<(), crate::CommandBuffer> {
        if !cb.ops.is_empty() {
            return Err(cb);
        }
        self.wait_idle();
        self.recycle_cmd_ops(std::mem::take(&mut cb.ops));
        for callback in cb.on_submit_callbacks {
            callback();
        }
        Ok(())
    }

    /// A single small copy between CPU-visible buffers is done with a memory copy once earlier
    /// GPU work has finished.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe fn copy_small_buffer_on_cpu(
        &self,
        mut cb: crate::CommandBuffer,
    ) -> Result<(), crate::CommandBuffer> {
        use crate::RecordedOp;
        let is_small_uma_copy = matches!(
            cb.ops.as_slice(),
            [RecordedOp::CopyBufferToBuffer { src, dst, size, .. }]
                if *size <= 65536 && !src.inner.ptr.is_null() && !dst.inner.ptr.is_null()
        );
        if !is_small_uma_copy {
            return Err(cb);
        }
        self.wait_idle();
        if let Some(RecordedOp::CopyBufferToBuffer {
            src,
            src_offset,
            dst,
            dst_offset,
            size,
        }) = cb.ops.pop()
        {
            if size > 0 {
                // SAFETY: `copy_buffer_to_buffer` refuses a range outside either buffer and a
                // copy within one buffer, so both ranges are in bounds and do not overlap.
                ptr::copy_nonoverlapping(
                    src.inner.ptr.add(src_offset as usize),
                    dst.inner.ptr.add(dst_offset as usize),
                    size as usize,
                );
            }
        }
        self.recycle_cmd_ops(std::mem::take(&mut cb.ops));
        for callback in cb.on_submit_callbacks {
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
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe fn commit_timed(
        &self,
        sub_cmd_ptr: *mut c_void,
        error_sinks: &[crate::CommandErrorSink],
        start_ts: Option<(crate::QuerySet, u32)>,
        end_ts: Option<(crate::QuerySet, u32)>,
        allow_instant: bool,
    ) {
        let s = selectors();
        let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
        let signal = Self::has_scoped_work(error_sinks);
        self.install_completion_signal(sub_cmd_ptr, signal);
        f_void(sub_cmd_ptr, s.commit);
        f_void(sub_cmd_ptr, s.wait_until_completed);
        if let Some(command) = ObjCObject::from_unretained(sub_cmd_ptr) {
            self.pending_error_checks
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(PendingCommand {
                    command: CompletionCommand::Native(command),
                    sinks: error_sinks.to_vec(),
                });
            if signal {
                self.scope_completion_service.notify();
            }
        }

        let f_f64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f64 = msg_send();
        let t_start_s = f_f64(sub_cmd_ptr, s.gpu_start_time);
        let t_end_s = f_f64(sub_cmd_ptr, s.gpu_end_time);
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

    /// Encodes the pending compute batch and the commands of `cb` into one command buffer and
    /// commits it. Timed passes get command buffers of their own.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe fn encode_and_commit(
        &self,
        mut cb: crate::CommandBuffer,
        error_sink: Option<crate::CommandErrorSink>,
        idx: u64,
    ) {
        use crate::{RecordedOp, encode_compute_dispatches, encode_non_compute_op};
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

        let pool = f_obj(f_obj(s.cls_ns_autorelease_pool, s.alloc), s.init);
        let cmd_buf_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
        assert!(!cmd_buf_ptr.is_null(), "Failed to create MTLCommandBuffer");
        let cmd_buf = ObjCObject::from_unretained(cmd_buf_ptr).unwrap();

        // The compute encoder stays open across consecutive untimed compute passes.
        let mut active_compute_enc: *mut c_void = ptr::null_mut();
        let end_compute = |enc: &mut *mut c_void| {
            if !enc.is_null() {
                f_void(*enc, s.end_encoding);
                *enc = ptr::null_mut();
            }
        };
        if !pending.is_empty() {
            active_compute_enc = f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
            encode_compute_dispatches(active_compute_enc, &pending);
            pending.clear();
        }
        let callbacks = std::mem::take(&mut cb.on_submit_callbacks);

        let mut ops = std::mem::take(&mut cb.ops);
        for op in ops.drain(..) {
            match op {
                RecordedOp::ComputePass {
                    dispatches,
                    start_ts,
                    end_ts,
                } => {
                    if dispatches.is_empty() {
                        self.recycle_compute_dispatches(dispatches);
                        for (query_set, index) in [start_ts, end_ts].into_iter().flatten() {
                            encode_non_compute_op(
                                self,
                                cmd_buf.as_ptr(),
                                RecordedOp::WriteTimestamp { query_set, index },
                            );
                        }
                        continue;
                    }
                    if start_ts.is_some() || end_ts.is_some() {
                        end_compute(&mut active_compute_enc);
                        let sub_cmd_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
                        let sub_enc = f_obj(sub_cmd_ptr, s.compute_command_encoder);
                        encode_compute_dispatches(sub_enc, &dispatches);
                        self.recycle_compute_dispatches(dispatches);
                        f_void(sub_enc, s.end_encoding);
                        self.commit_timed(sub_cmd_ptr, &error_sinks, start_ts, end_ts, true);
                        continue;
                    }
                    if active_compute_enc.is_null() {
                        active_compute_enc = f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
                    }
                    encode_compute_dispatches(active_compute_enc, &dispatches);
                    self.recycle_compute_dispatches(dispatches);
                }
                RecordedOp::RenderPass(mut rp) => {
                    end_compute(&mut active_compute_enc);
                    let start_ts = rp.start_ts.take();
                    let end_ts = rp.end_ts.take();
                    if start_ts.is_some() || end_ts.is_some() {
                        let sub_cmd_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
                        crate::encode_render_pass(self, sub_cmd_ptr, rp);
                        self.commit_timed(sub_cmd_ptr, &error_sinks, start_ts, end_ts, false);
                        continue;
                    }
                    crate::encode_render_pass(self, cmd_buf.as_ptr(), rp);
                }
                other => {
                    end_compute(&mut active_compute_enc);
                    encode_non_compute_op(self, cmd_buf.as_ptr(), other);
                }
            }
        }
        self.recycle_cmd_ops(ops);
        end_compute(&mut active_compute_enc);

        let signal = Self::has_scoped_work(&error_sinks);
        self.install_completion_signal(cmd_buf.as_ptr(), signal);
        f_void(cmd_buf.as_ptr(), s.commit);
        if !pool.is_null() {
            f_void(pool, s.drain);
        }
        let _ = self.device_loss_message(); // Reap completed work even when callers do not poll.
        self.pending_error_checks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(PendingCommand {
                command: CompletionCommand::Native(cmd_buf.clone()),
                sinks: error_sinks,
            });
        if signal {
            self.scope_completion_service.notify();
        }
        if let Ok(mut last) = self.last_cmd_buf.lock() {
            *last = Some((cmd_buf, idx));
        }
        if !callbacks.is_empty() {
            self.wait_idle();
            for callback in callbacks {
                callback();
            }
        }
    }
}
