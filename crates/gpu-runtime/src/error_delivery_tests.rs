use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
fn validation(message: &str) -> Error {
    Error::Validation {
        source: Arc::new(std::io::Error::other(message.to_owned())),
        description: message.into(),
    }
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
#[ignore = "requires a real Metal adapter"]
fn distinct_logical_devices_have_distinct_queues_on_one_adapter() {
    use std::future::Future;
    let adapter =
        Adapter::from_metal(MetalDevice::create_system_default().expect("Metal required"));
    let descriptor = DeviceDescriptor::default();
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let mut first = std::pin::pin!(adapter.request_device(&descriptor));
    let std::task::Poll::Ready(Ok((device_a, queue_a))) = first.as_mut().poll(&mut context) else {
        panic!("request_device must complete")
    };
    let mut second = std::pin::pin!(adapter.request_device(&descriptor));
    let std::task::Poll::Ready(Ok((device_b, queue_b))) = second.as_mut().poll(&mut context) else {
        panic!("request_device must complete")
    };
    assert_ne!(device_a, device_b);
    assert_ne!(
        queue_a, queue_b,
        "separate logical devices must not share queue identity"
    );
    assert_eq!(queue_a, queue_a.clone());
    assert_eq!(queue_b, queue_b.clone());
    let seen_a = Arc::new(AtomicUsize::new(0));
    let out_a = seen_a.clone();
    let seen_b = Arc::new(AtomicUsize::new(0));
    let out_b = seen_b.clone();
    device_a.on_uncaptured_error(Arc::new(move |_| {
        out_a.fetch_add(1, Ordering::Relaxed);
    }));
    device_b.on_uncaptured_error(Arc::new(move |_| {
        out_b.fetch_add(1, Ordering::Relaxed);
    }));
    device_a.record_error(validation("device a only"));
    assert_eq!(seen_a.load(Ordering::Relaxed), 1);
    assert_eq!(seen_b.load(Ordering::Relaxed), 0);
    drop(device_a);
    assert_eq!(
        queue_a,
        queue_a.clone(),
        "queue identity survives device handle drop"
    );
    eprintln!(
        "QUEUE_IDENTITY_RESULT logical_devices=2 shared_adapter=true isolated_errors=true clone_stable=true"
    );
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
#[ignore = "requires Metal; injects completion failures, not a hardware fault"]
fn command_completion_routes_to_submit_owner_with_scope_and_reentry() {
    use std::future::Future;
    let adapter =
        Adapter::from_metal(MetalDevice::create_system_default().expect("Metal required"));
    let descriptor = DeviceDescriptor::default();
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let mut first = std::pin::pin!(adapter.request_device(&descriptor));
    let std::task::Poll::Ready(Ok((device_a, queue_a))) = first.as_mut().poll(&mut context) else {
        panic!("request_device")
    };
    let mut second = std::pin::pin!(adapter.request_device(&descriptor));
    let std::task::Poll::Ready(Ok((device_b, queue_b))) = second.as_mut().poll(&mut context) else {
        panic!("request_device")
    };
    let seen = Arc::new(AtomicUsize::new(0));
    let output = seen.clone();
    let device_a = Arc::new(device_a);
    let weak = Arc::downgrade(&device_a);
    device_a.on_uncaptured_error(Arc::new(move |error| {
        assert!(matches!(error, Error::Internal { .. }));
        output.fetch_add(1, Ordering::Relaxed);
        weak.upgrade().unwrap().poll(PollType::Poll).unwrap();
    }));
    device_b.on_uncaptured_error(Arc::new(|_| panic!("wrong logical owner")));
    let buffer = device_a.create_buffer(&BufferDescriptor {
        label: Some("command owner probe"),
        size: 64,
        usage: BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device_a.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.clear_buffer(&buffer, 0, None);
    queue_a.submit([encoder.finish()]);
    assert!(
        device_a
            .metal
            .last_command_owned_by_for_test(&queue_a.error_sink)
    );
    let shader = device_a.create_shader_module(ShaderModuleDescriptor {
        label: Some("owner compute"),
        source: ShaderSource::Wgsl("@compute @workgroup_size(1) fn main() {}".into()),
    });
    let pipeline = device_a.create_compute_pipeline(&ComputePipelineDescriptor {
        label: Some("owner compute"),
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: PipelineCompilationOptions::default(),
        cache: None,
    });
    for (device, queue) in [(&*device_a, &queue_a), (&device_b, &queue_b)] {
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
            pass.set_pipeline(&pipeline);
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit([encoder.finish()]);
        assert!(
            device
                .metal
                .last_command_owned_by_for_test(&queue.error_sink),
            "pure compute lost logical owner"
        );
    }
    // A different logical device can reap shared backend completions.
    device_a
        .metal
        .inject_command_failure_for_test(queue_a.error_sink.clone(), "owner a failure");
    device_b.poll(PollType::Poll).unwrap();
    device_b.poll(PollType::Poll).unwrap();
    assert_eq!(seen.load(Ordering::Relaxed), 1);
    let guard = device_a.push_error_scope(ErrorFilter::Internal);
    device_a
        .metal
        .inject_command_failure_for_test(queue_a.error_sink.clone(), "captured completion");
    device_b.poll(PollType::Poll).unwrap();
    assert_eq!(seen.load(Ordering::Relaxed), 1);
    assert!(
        device_a
            .error_stack
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .error()
            .as_ref()
            .unwrap()
            .to_string()
            .contains("captured completion")
    );
    let mut popped = std::pin::pin!(guard.pop());
    assert!(matches!(
        popped.as_mut().poll(&mut context),
        std::task::Poll::Ready(Some(Error::Internal { .. }))
    ));
    let oom_guard = device_a.push_error_scope(ErrorFilter::OutOfMemory);
    device_a
        .metal
        .inject_classified_command_failure_for_test(queue_a.error_sink.clone(), true, 8);
    device_b.poll(PollType::Poll).unwrap();
    assert!(matches!(
        device_a.error_stack.lock().unwrap().last().unwrap().error(),
        Some(Error::OutOfMemory { .. })
    ));
    assert_eq!(seen.load(Ordering::Relaxed), 1);
    // Another NSError domain's code8 must bypass the OOM scope.
    device_a
        .metal
        .inject_classified_command_failure_for_test(queue_a.error_sink.clone(), false, 8);
    device_b.poll(PollType::Poll).unwrap();
    assert_eq!(seen.load(Ordering::Relaxed), 2);
    let mut popped_oom = std::pin::pin!(oom_guard.pop());
    assert!(matches!(
        popped_oom.as_mut().poll(&mut context),
        std::task::Poll::Ready(Some(Error::OutOfMemory { .. }))
    ));
    device_b.poll(PollType::Poll).unwrap();
    assert_eq!(seen.load(Ordering::Relaxed), 2);
    let guard = device_a.push_error_scope(ErrorFilter::Internal);
    let mut encoder = device_a.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.clear_buffer(&buffer, 0, None);
    queue_a.submit([encoder.finish()]);
    device_a
        .metal
        .inject_command_failure_for_test(queue_a.error_sink.clone(), "async captured error");
    let mut pending = std::pin::pin!(guard.pop());
    let newer = device_a.push_error_scope(ErrorFilter::Internal);
    struct ReentrantWake {
        device: Arc<Device>,
        thread: std::thread::Thread,
    }
    impl std::task::Wake for ReentrantWake {
        fn wake(self: Arc<Self>) {
            self.device.poll(PollType::Poll).unwrap();
            self.thread.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(ReentrantWake {
        device: device_a.clone(),
        thread: std::thread::current(),
    }));
    let mut async_context = std::task::Context::from_waker(&waker);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let std::task::Poll::Ready(error) = pending.as_mut().poll(&mut async_context) {
            assert!(error.unwrap().to_string().contains("async captured error"));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "scope future never completed"
        );
        std::thread::park_timeout(std::time::Duration::from_millis(100));
    }
    let mut newer = std::pin::pin!(newer.pop());
    assert!(matches!(
        newer.as_mut().poll(&mut context),
        std::task::Poll::Ready(None)
    ));
    let success = device_a.push_error_scope(ErrorFilter::Validation);
    let mut encoder = device_a.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.clear_buffer(&buffer, 0, None);
    queue_a.submit([encoder.finish()]);
    let mut success = std::pin::pin!(success.pop());
    loop {
        if let std::task::Poll::Ready(error) = success.as_mut().poll(&mut async_context) {
            assert!(error.is_none());
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "successful scope never completed"
        );
        std::thread::park_timeout(std::time::Duration::from_millis(100));
    }
    while device_a
        .metal
        .scope_completion_service
        .native_notifications()
        == 0
    {
        assert!(
            std::time::Instant::now() < deadline,
            "native completion signal never arrived"
        );
        std::thread::park_timeout(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        device_a.metal.blocking_wait_calls.load(Ordering::Relaxed),
        0,
        "async completion entered a blocking backend wait"
    );
    eprintln!(
        "NATIVE_SCOPE_SIGNAL_RESULT callbacks=true blocking_wait_calls=0 successful_and_failed_scopes=true"
    );
    eprintln!(
        "ASYNC_SCOPE_RESULT captured_at_submit=true pop_waits=true completion_worker=true reentrant_waker=true newer_scope_clean=true physical_fault=false"
    );
    eprintln!(
        "COMMAND_COMPLETION_OOM_RESULT scoped_oom=true foreign_domain_internal=true owner_isolation=true duplicate=false physical_fault=false"
    );
    eprintln!(
        "COMMAND_COMPLETION_RESULT owner_isolation=true callback_reentry=true duplicate=false scope_capture=true physical_fault=false"
    );
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
#[ignore = "requires Metal; holds synthetic submit leases to test completion service bursts"]
fn scope_completion_service_coalesces_bursts_and_retires_cancelled_requests() {
    use std::future::Future;
    let adapter =
        Adapter::from_metal(MetalDevice::create_system_default().expect("Metal required"));
    let descriptor = DeviceDescriptor::default();
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let mut request = std::pin::pin!(adapter.request_device(&descriptor));
    let std::task::Poll::Ready(Ok((device, queue))) = request.as_mut().poll(&mut context) else {
        panic!("request_device")
    };
    struct Notify(std::thread::Thread);
    impl std::task::Wake for Notify {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Notify(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    for round in 0..2 {
        let mut leases = Vec::new();
        let mut futures = Vec::new();
        for _ in 0..64 {
            let guard = device.push_error_scope(ErrorFilter::Validation);
            leases.push(queue.error_sink.capture());
            futures.push(Some(Box::pin(guard.pop())));
        }
        for future in &mut futures {
            assert!(
                future
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .poll(&mut context)
                    .is_pending()
            );
        }
        assert_eq!(
            device.metal.scope_completion_service.stats().1,
            round + 1,
            "burst spawned more than one worker"
        );
        for (i, future) in futures.iter_mut().enumerate() {
            if i % 2 == 1 {
                *future = None;
            }
        }
        for (i, lease) in leases.into_iter().enumerate() {
            lease.command_failure(validation(&format!("burst-{round}-{i}")));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            for (i, future) in futures.iter_mut().enumerate() {
                if let Some(pending) = future {
                    if let std::task::Poll::Ready(error) = pending.as_mut().poll(&mut context) {
                        assert!(
                            error
                                .unwrap()
                                .to_string()
                                .contains(&format!("burst-{round}-{i}"))
                        );
                        *future = None;
                    }
                }
            }
            let (running, starts, waiting, capacity) =
                device.metal.scope_completion_service.stats();
            if futures.iter().all(Option::is_none) && !running {
                assert_eq!(waiting, 0);
                assert_eq!(capacity, 0, "idle service retained registration storage");
                assert_eq!(starts, round + 1);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "completion service did not retire"
            );
            std::thread::park_timeout(std::time::Duration::from_millis(1));
        }
    }
    let mut leases = Vec::new();
    let mut cancelled = Vec::new();
    for _ in 0..64 {
        let guard = device.push_error_scope(ErrorFilter::Validation);
        leases.push(queue.error_sink.capture());
        let mut future = Box::pin(guard.pop());
        assert!(future.as_mut().poll(&mut context).is_pending());
        cancelled.push(future);
    }
    drop(cancelled);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let (running, _, waiting, capacity) = device.metal.scope_completion_service.stats();
        if !running {
            assert_eq!(waiting, 0);
            assert_eq!(capacity, 0);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "cancelled registrations kept service running while leases remained live"
        );
        std::thread::park_timeout(std::time::Duration::from_millis(1));
    }
    assert_eq!(leases.len(), 64);
    drop(leases);
    let retired = device.metal.scope_completion_service.stats();
    device.metal.scope_completion_service.native_completed();
    assert_eq!(
        device.metal.scope_completion_service.stats(),
        retired,
        "late completion restarted an idle service"
    );
    eprintln!("LATE_SCOPE_SIGNAL_RESULT retired_service_unchanged=true no_new_worker=true");
    eprintln!(
        "SCOPE_CANCELLATION_RESULT cancelled=64 live_submission_leases=64 service_retired_before_lease_release=true"
    );
    eprintln!(
        "SCOPE_SERVICE_RESULT rounds=2 scopes_per_round=64 cancelled_per_round=32 workers_per_burst=1 retired=true idle_capacity=0 synthetic_leases=true"
    );
}
#[test]
fn scope_guards_pop_their_own_errors_in_either_order() {
    use std::future::Future;
    for outer_first in [false, true] {
        let stack = Arc::new(Mutex::new(Vec::new()));
        let handler = Mutex::new(None);
        let outer = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
        dispatch_error(&stack, &handler, validation("outer"));
        let inner = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
        dispatch_error(&stack, &handler, validation("inner"));
        let (first, second, expected_first, expected_second) = if outer_first {
            (outer, inner, "outer", "inner")
        } else {
            (inner, outer, "inner", "outer")
        };
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let mut popped = std::pin::pin!(first.pop());
        let std::task::Poll::Ready(Some(error)) = popped.as_mut().poll(&mut context) else {
            panic!("scope error")
        };
        assert!(error.to_string().contains(expected_first));
        assert_eq!(stack.lock().unwrap().len(), 1);
        // An unrelated newly-pushed scope must survive the old guard pop.
        let unrelated = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Internal);
        let mut popped = std::pin::pin!(second.pop());
        let std::task::Poll::Ready(Some(error)) = popped.as_mut().poll(&mut context) else {
            panic!("scope error")
        };
        assert!(error.to_string().contains(expected_second));
        assert_eq!(stack.lock().unwrap().len(), 1);
        assert_eq!(stack.lock().unwrap()[0].filter, ErrorFilter::Internal);
        let mut popped = std::pin::pin!(unrelated.pop());
        assert!(matches!(
            popped.as_mut().poll(&mut context),
            std::task::Poll::Ready(None)
        ));
        assert!(stack.lock().unwrap().is_empty());
    }
}
#[test]
fn concurrent_scope_pops_preserve_each_guard_error() {
    use std::future::Future;
    let stack = Arc::new(Mutex::new(Vec::new()));
    let handler = Mutex::new(None);
    let guards: Vec<_> = (0..8)
        .map(|i| {
            let guard = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
            let message = format!("scope-{i}");
            dispatch_error(&stack, &handler, validation(&message));
            (guard, message)
        })
        .collect();
    let threads: Vec<_> = guards
        .into_iter()
        .map(|(guard, message)| {
            std::thread::spawn(move || {
                let mut context = std::task::Context::from_waker(std::task::Waker::noop());
                let mut popped = std::pin::pin!(guard.pop());
                let std::task::Poll::Ready(Some(error)) = popped.as_mut().poll(&mut context) else {
                    panic!("scope error")
                };
                assert!(error.to_string().contains(&message));
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert!(stack.lock().unwrap().is_empty());
}
#[test]
fn submitted_scope_survives_pop_and_new_scope_with_waker_notification() {
    use std::future::Future;
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let stack = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(Mutex::new(None));
    let guard = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
    let sink = CommandErrorSink {
        stack: stack.clone(),
        handler,
        scopes: None,
        captured: false,
    }
    .capture();
    let part = sink.clone();
    let mut popped = std::pin::pin!(guard.pop());
    let newer = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = std::task::Waker::from(wakes.clone());
    let mut context = std::task::Context::from_waker(&waker);
    assert!(popped.as_mut().poll(&mut context).is_pending());
    sink.command_failure(validation("submitted before pop"));
    drop(sink);
    assert!(
        popped.as_mut().poll(&mut context).is_pending(),
        "another command part still owns this submission"
    );
    drop(part);
    assert!(wakes.0.load(Ordering::Relaxed) > 0);
    let std::task::Poll::Ready(Some(error)) = popped.as_mut().poll(&mut context) else {
        panic!("captured error")
    };
    assert!(error.to_string().contains("submitted before pop"));
    let mut newer = std::pin::pin!(newer.pop());
    assert!(matches!(
        newer.as_mut().poll(&mut context),
        std::task::Poll::Ready(None)
    ));
}
#[test]
fn cancelled_scope_pop_releases_waker_before_submission_lease() {
    use std::future::Future;
    struct Resource {
        entry: crate::types::ErrorScopeEntry,
        dropped: Arc<AtomicUsize>,
    }
    impl Drop for Resource {
        fn drop(&mut self) {
            let _ = self.entry.error();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    struct TaskWaker {
        _resource: Arc<Resource>,
        calls: Arc<AtomicUsize>,
    }
    impl std::task::Wake for TaskWaker {
        fn wake(self: Arc<Self>) {
            self.calls.fetch_add(1, Ordering::Relaxed);
        }
    }
    let stack = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(Mutex::new(None));
    let guard = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
    let sink = CommandErrorSink {
        stack,
        handler,
        scopes: None,
        captured: false,
    }
    .capture();
    let dropped = Arc::new(AtomicUsize::new(0));
    let resource = Arc::new(Resource {
        entry: sink.scopes.as_ref().unwrap().0[0].clone(),
        dropped: dropped.clone(),
    });
    let weak = Arc::downgrade(&resource);
    let calls = Arc::new(AtomicUsize::new(0));
    let waker = std::task::Waker::from(Arc::new(TaskWaker {
        _resource: resource,
        calls: calls.clone(),
    }));
    let mut future = Box::pin(guard.pop());
    {
        let mut context = std::task::Context::from_waker(&waker);
        assert!(future.as_mut().poll(&mut context).is_pending());
    }
    drop(waker);
    assert!(
        weak.upgrade().is_some(),
        "pending future must retain its task notification"
    );
    drop(future);
    assert!(
        weak.upgrade().is_none(),
        "cancelled future retained executor resource through live submission"
    );
    assert_eq!(
        dropped.load(Ordering::Relaxed),
        1,
        "task destructor must reenter scope state without deadlock"
    );
    sink.command_failure(validation("late error after cancellation"));
    assert!(
        sink.scopes.as_ref().unwrap().0[0]
            .error()
            .unwrap()
            .to_string()
            .contains("late error after cancellation")
    );
    drop(sink);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "completion woke a cancelled task"
    );
}
#[test]
fn submission_without_scopes_does_not_capture_a_later_scope() {
    use std::future::Future;
    let stack = Arc::new(Mutex::new(Vec::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let output = calls.clone();
    let handler: Arc<Mutex<Option<UncapturedErrorHandler>>> =
        Arc::new(Mutex::new(Some(Arc::new(move |_| {
            output.fetch_add(1, Ordering::Relaxed);
        }))));
    let sink = CommandErrorSink {
        stack: stack.clone(),
        handler,
        scopes: None,
        captured: false,
    }
    .capture();
    assert!(
        sink.scopes.is_none(),
        "empty scope capture must not allocate a snapshot"
    );
    let newer = ErrorScopeGuard::push(stack, ErrorFilter::Validation);
    sink.command_failure(validation("unscoped submission"));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let mut popped = std::pin::pin!(newer.pop());
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(matches!(
        popped.as_mut().poll(&mut context),
        std::task::Poll::Ready(None)
    ));
}
#[test]
fn matching_innermost_scope_captures_first_without_uncaptured_delivery() {
    let stack = Mutex::new(vec![
        crate::types::ErrorScopeEntry::new(ErrorFilter::Validation),
        crate::types::ErrorScopeEntry::new(ErrorFilter::OutOfMemory),
        crate::types::ErrorScopeEntry::new(ErrorFilter::Validation),
    ]);
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let handler: Mutex<Option<UncapturedErrorHandler>> = Mutex::new(Some(Arc::new(move |_| {
        seen.fetch_add(1, Ordering::Relaxed);
    })));
    dispatch_error(&stack, &handler, validation("first"));
    dispatch_error(&stack, &handler, validation("second"));
    let scopes = stack.lock().unwrap();
    assert!(scopes[0].error().is_none() && scopes[1].error().is_none());
    assert!(
        scopes[2]
            .error()
            .as_ref()
            .unwrap()
            .to_string()
            .contains("first")
    );
    assert_eq!(count.load(Ordering::Relaxed), 0);
}
#[test]
fn uncaptured_handler_can_replace_itself_and_reenter_error_scopes() {
    let stack = Arc::new(Mutex::new(Vec::new()));
    let handler: Arc<Mutex<Option<UncapturedErrorHandler>>> = Arc::new(Mutex::new(None));
    let weak = Arc::downgrade(&handler);
    let nested_stack = stack.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    *handler.lock().unwrap() = Some(Arc::new(move |_| {
        seen.fetch_add(1, Ordering::Relaxed);
        let holder = weak.upgrade().unwrap();
        *holder.lock().unwrap() = None;
        nested_stack
            .lock()
            .unwrap()
            .push(crate::types::ErrorScopeEntry::new(ErrorFilter::Validation));
        dispatch_error(&nested_stack, &holder, validation("nested"));
    }));
    dispatch_error(&stack, &handler, validation("outer"));
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert!(handler.lock().unwrap().is_none());
    assert!(
        stack.lock().unwrap()[0]
            .error()
            .as_ref()
            .unwrap()
            .to_string()
            .contains("nested")
    );
}
