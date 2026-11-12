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
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        use crate::{RecordedOp, encode_compute_dispatches, encode_non_compute_op};
        use std::sync::atomic::Ordering;

        let idx = self.next_submission_index.fetch_add(1, Ordering::Relaxed);

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        unsafe {
            let mut iter = command_buffers.into_iter();
            let Some(mut first_cb) = iter.next() else {
                return SubmissionIndex(idx);
            };
            let mut extra_cbs: Vec<crate::CommandBuffer> = iter.collect();
            for mut extra in extra_cbs.drain(..) {
                let mut ops = std::mem::take(&mut extra.ops);
                first_cb.ops.append(&mut ops);
                self.recycle_cmd_ops(ops);
                first_cb
                    .on_submit_callbacks
                    .append(&mut extra.on_submit_callbacks);
            }
            let cbs = [first_cb];
            let is_pure_untimed_compute = !cbs.is_empty()
                && cbs.iter().all(|cb| {
                    cb.on_submit_callbacks.is_empty()
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
                        })
                });
            if is_pure_untimed_compute {
                let mut should_flush = false;
                if let Ok(mut pending) = self.pending_compute_ops.lock() {
                    if let Some(sink) = error_sink.clone() {
                        add_error_sink(&mut pending.sinks, sink);
                    }
                    for mut cb in cbs {
                        let mut ops = std::mem::take(&mut cb.ops);
                        for op in ops.drain(..) {
                            if let RecordedOp::ComputePass { mut dispatches, .. } = op {
                                pending.ops.append(&mut dispatches);
                                self.recycle_compute_dispatches(dispatches);
                            }
                        }
                        self.recycle_cmd_ops(ops);
                    }
                    if !pending.ops.is_empty() {
                        should_flush = true;
                    }
                }
                if should_flush {
                    self.flush_pending_compute();
                }
                return SubmissionIndex(idx);
            }

            let is_callback_only = cbs.iter().all(|cb| cb.ops.is_empty());
            if is_callback_only {
                self.wait_idle();
                for mut cb in cbs {
                    let ops = std::mem::take(&mut cb.ops);
                    self.recycle_cmd_ops(ops);
                    for callback in cb.on_submit_callbacks {
                        callback();
                    }
                }
                return SubmissionIndex(idx);
            }

            let is_small_uma_copy = cbs.len() == 1
                && cbs[0].ops.len() == 1
                && matches!(
                    &cbs[0].ops[0],
                    RecordedOp::CopyBufferToBuffer { src, dst, size, .. }
                        if *size <= 65536 && !src.inner.ptr.is_null() && !dst.inner.ptr.is_null()
                );
            if is_small_uma_copy {
                self.wait_idle();
                let mut cb = cbs.into_iter().next().unwrap();
                if let Some(RecordedOp::CopyBufferToBuffer {
                    src,
                    src_offset,
                    dst,
                    dst_offset,
                    size,
                }) = cb.ops.pop()
                {
                    if size > 0 {
                        ptr::copy_nonoverlapping(
                            src.inner.ptr.add(src_offset as usize),
                            dst.inner.ptr.add(dst_offset as usize),
                            size as usize,
                        );
                    }
                }
                let ops = std::mem::take(&mut cb.ops);
                self.recycle_cmd_ops(ops);
                for callback in cb.on_submit_callbacks {
                    callback();
                }
                return SubmissionIndex(idx);
            }

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

            let mut active_compute_enc: *mut c_void = ptr::null_mut();
            if !pending.is_empty() {
                active_compute_enc = f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
                encode_compute_dispatches(active_compute_enc, &pending);
                pending.clear();
            }
            let mut callbacks: Vec<Box<dyn FnOnce() + Send>> = Vec::new();

            for mut cb in cbs {
                callbacks.append(&mut cb.on_submit_callbacks);
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
                                if let Some((qs, i)) = start_ts {
                                    encode_non_compute_op(
                                        self,
                                        cmd_buf.as_ptr(),
                                        RecordedOp::WriteTimestamp {
                                            query_set: qs,
                                            index: i,
                                        },
                                    );
                                }
                                if let Some((qs, i)) = end_ts {
                                    encode_non_compute_op(
                                        self,
                                        cmd_buf.as_ptr(),
                                        RecordedOp::WriteTimestamp {
                                            query_set: qs,
                                            index: i,
                                        },
                                    );
                                }
                                continue;
                            }
                            if start_ts.is_some() || end_ts.is_some() {
                                if !active_compute_enc.is_null() {
                                    f_void(active_compute_enc, s.end_encoding);
                                    active_compute_enc = ptr::null_mut();
                                }
                                let sub_cmd_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
                                let sub_enc = f_obj(sub_cmd_ptr, s.compute_command_encoder);
                                encode_compute_dispatches(sub_enc, &dispatches);
                                self.recycle_compute_dispatches(dispatches);
                                f_void(sub_enc, s.end_encoding);
                                let signal = error_sinks
                                    .iter()
                                    .any(crate::CommandErrorSink::has_scoped_work);
                                self.install_completion_signal(sub_cmd_ptr, signal);
                                f_void(sub_cmd_ptr, s.commit);
                                f_void(sub_cmd_ptr, s.wait_until_completed);
                                if let Some(command) = ObjCObject::from_unretained(sub_cmd_ptr) {
                                    self.pending_error_checks
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .push(PendingCommand {
                                            command: CompletionCommand::Native(command),
                                            sinks: error_sinks.clone(),
                                        });
                                    if signal {
                                        self.scope_completion_service.notify();
                                    }
                                }

                                let f_f64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f64 =
                                    msg_send();
                                let t_start_s = f_f64(sub_cmd_ptr, s.gpu_start_time);
                                let t_end_s = f_f64(sub_cmd_ptr, s.gpu_end_time);
                                let now_ns = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_nanos() as u64)
                                    .unwrap_or(0);
                                let (start_ns, end_ns) = if t_start_s > 0.0 && t_end_s >= t_start_s
                                {
                                    ((t_start_s * 1e9) as u64, (t_end_s * 1e9) as u64)
                                } else {
                                    (now_ns, now_ns + 1000)
                                };
                                if let Some((qs, i)) = start_ts {
                                    if let Ok(mut ts) = qs.timestamps_ns.lock() {
                                        if (i as usize) < ts.len() {
                                            ts[i as usize] = start_ns;
                                        }
                                    }
                                }
                                if let Some((qs, i)) = end_ts {
                                    if let Ok(mut ts) = qs.timestamps_ns.lock() {
                                        if (i as usize) < ts.len() {
                                            ts[i as usize] = end_ns;
                                        }
                                    }
                                }
                                continue;
                            }
                            if active_compute_enc.is_null() {
                                active_compute_enc =
                                    f_obj(cmd_buf.as_ptr(), s.compute_command_encoder);
                            }
                            encode_compute_dispatches(active_compute_enc, &dispatches);
                            self.recycle_compute_dispatches(dispatches);
                        }
                        RecordedOp::RenderPass(mut rp) => {
                            if !active_compute_enc.is_null() {
                                f_void(active_compute_enc, s.end_encoding);
                                active_compute_enc = ptr::null_mut();
                            }
                            let start_ts = rp.start_ts.take();
                            let end_ts = rp.end_ts.take();
                            if start_ts.is_some() || end_ts.is_some() {
                                let sub_cmd_ptr = f_obj(self.queue.as_ptr(), s.command_buffer);
                                crate::encode_render_pass(self, sub_cmd_ptr, rp);
                                let signal = error_sinks
                                    .iter()
                                    .any(crate::CommandErrorSink::has_scoped_work);
                                self.install_completion_signal(sub_cmd_ptr, signal);
                                f_void(sub_cmd_ptr, s.commit);
                                f_void(sub_cmd_ptr, s.wait_until_completed);
                                if let Some(command) = ObjCObject::from_unretained(sub_cmd_ptr) {
                                    self.pending_error_checks
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .push(PendingCommand {
                                            command: CompletionCommand::Native(command),
                                            sinks: error_sinks.clone(),
                                        });
                                    if signal {
                                        self.scope_completion_service.notify();
                                    }
                                }

                                let f_f64: unsafe extern "C" fn(*mut c_void, *mut c_void) -> f64 =
                                    msg_send();
                                let t_start_s = f_f64(sub_cmd_ptr, s.gpu_start_time);
                                let t_end_s = f_f64(sub_cmd_ptr, s.gpu_end_time);
                                let now_ns = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_nanos() as u64)
                                    .unwrap_or(0);
                                let (start_ns, end_ns) = if t_start_s > 0.0 && t_end_s > t_start_s {
                                    ((t_start_s * 1e9) as u64, (t_end_s * 1e9) as u64)
                                } else {
                                    (now_ns, now_ns + 1000)
                                };
                                if let Some((qs, i)) = start_ts {
                                    if let Ok(mut ts) = qs.timestamps_ns.lock() {
                                        if (i as usize) < ts.len() {
                                            ts[i as usize] = start_ns;
                                        }
                                    }
                                }
                                if let Some((qs, i)) = end_ts {
                                    if let Ok(mut ts) = qs.timestamps_ns.lock() {
                                        if (i as usize) < ts.len() {
                                            ts[i as usize] = end_ns;
                                        }
                                    }
                                }
                                continue;
                            }
                            crate::encode_render_pass(self, cmd_buf.as_ptr(), rp);
                        }
                        other => {
                            if !active_compute_enc.is_null() {
                                f_void(active_compute_enc, s.end_encoding);
                                active_compute_enc = ptr::null_mut();
                            }
                            encode_non_compute_op(self, cmd_buf.as_ptr(), other);
                        }
                    }
                }
                self.recycle_cmd_ops(ops);
            }

            if !active_compute_enc.is_null() {
                f_void(active_compute_enc, s.end_encoding);
            }

            let signal = error_sinks
                .iter()
                .any(crate::CommandErrorSink::has_scoped_work);
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
                for cb in callbacks {
                    cb();
                }
            }
        }

        SubmissionIndex(idx)
    }
}
