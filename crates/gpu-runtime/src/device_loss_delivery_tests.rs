use super::*;
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
#[ignore = "requires Metal; injects a logical backend notification, not physical device removal"]
fn poll_delivers_backend_loss_to_late_callback_once_with_reentry() {
    use std::future::Future;
    let adapter =
        Adapter::from_metal(MetalDevice::create_system_default().expect("Metal required"));
    let descriptor = DeviceDescriptor::default();
    let mut request = std::pin::pin!(adapter.request_device(&descriptor));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(Ok((device, _queue))) = request.as_mut().poll(&mut context) else {
        panic!("native request_device must complete")
    };
    let device = Arc::new(device);
    device
        .expect_metal()
        .inject_device_loss_for_test("injected Metal backend removal".into());
    device.poll(PollType::Poll).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let output = seen.clone();
    let weak = Arc::downgrade(&device);
    device.set_device_lost_callback(move |reason, message| {
        output.lock().unwrap().push((reason, message));
        weak.upgrade().unwrap().poll(PollType::Poll).unwrap();
    });
    for _ in 0..5 {
        device.poll(PollType::Poll).unwrap();
    }
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(
            DeviceLostReason::Unknown,
            "injected Metal backend removal".into()
        )]
    );
    eprintln!(
        "DEVICE_LOSS_POLL_RESULT callback_once=true late_registration=true reentry=true physical_removal=false"
    );
}

#[test]
fn late_registration_reentry_and_duplicate_loss_deliver_once() {
    let state = Arc::new(DeviceLossState::default());
    state.notify(DeviceLostReason::Unknown, "first backend loss".into());
    state.notify(DeviceLostReason::Destroyed, "later".into());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let output = seen.clone();
    let weak = Arc::downgrade(&state);
    state.set_callback(Arc::new(move |reason, message| {
        output.lock().unwrap().push((reason, message));
        // Reentry must not deadlock or issue a second callback.
        weak.upgrade()
            .unwrap()
            .notify(DeviceLostReason::Unknown, "reentrant".into());
    }));
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(DeviceLostReason::Unknown, "first backend loss".into())]
    );
    state.set_callback(Arc::new(|_, _| panic!("loss already delivered")));
    state.notify(DeviceLostReason::Destroyed, "duplicate".into());
}
#[test]
fn registration_before_loss_observes_first_message_once() {
    let state = DeviceLossState::default();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let output = seen.clone();
    state.set_callback(Arc::new(move |reason, message| {
        output.lock().unwrap().push((reason, message))
    }));
    assert!(seen.lock().unwrap().is_empty());
    state.notify(DeviceLostReason::Unknown, "removed".into());
    state.notify(DeviceLostReason::Unknown, "revoked".into());
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(DeviceLostReason::Unknown, "removed".into())]
    );
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn device_destroy_delivers_destroyed_immediately_and_isolates_siblings() {
    use std::future::Future;
    let Some(metal) = MetalDevice::create_system_default() else {
        return;
    };
    let adapter = Adapter::from_metal(metal);
    let descriptor = DeviceDescriptor::default();
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(Ok((device_a, queue_a))) =
        std::pin::pin!(adapter.request_device(&descriptor))
            .as_mut()
            .poll(&mut context)
    else {
        panic!("request_device failed")
    };
    let std::task::Poll::Ready(Ok((device_b, queue_b))) =
        std::pin::pin!(adapter.request_device(&descriptor))
            .as_mut()
            .poll(&mut context)
    else {
        panic!("sibling request_device failed")
    };

    let lost_a = Arc::new(Mutex::new(Vec::new()));
    let lost_b = Arc::new(Mutex::new(Vec::new()));
    let out_a = lost_a.clone();
    let out_b = lost_b.clone();
    device_a.set_device_lost_callback(move |reason, msg| out_a.lock().unwrap().push((reason, msg)));
    device_b.set_device_lost_callback(move |reason, msg| out_b.lock().unwrap().push((reason, msg)));

    device_a.destroy();
    let seen_a = lost_a.lock().unwrap();
    assert_eq!(seen_a.len(), 1);
    assert_eq!(seen_a[0].0, DeviceLostReason::Destroyed);
    assert!(
        lost_b.lock().unwrap().is_empty(),
        "sibling device stayed healthy"
    );

    let scope = device_a.push_error_scope(ErrorFilter::Validation);
    assert_eq!(queue_a.submit([]).0, 0);
    let std::task::Poll::Ready(Some(err)) = std::pin::pin!(scope.pop()).as_mut().poll(&mut context)
    else {
        panic!("destroyed queue submit should report validation error");
    };
    assert!(matches!(err, Error::Validation { .. }));

    let scope_b = device_b.push_error_scope(ErrorFilter::Validation);
    queue_b.submit([]);
    device_b.poll(PollType::wait_indefinitely()).unwrap();
    assert!(matches!(
        std::pin::pin!(scope_b.pop()).as_mut().poll(&mut context),
        std::task::Poll::Ready(None)
    ));
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn idle_device_receives_removal_notification_without_poll() {
    use std::future::Future;
    let Some(metal) = MetalDevice::create_system_default() else {
        return;
    };
    let adapter = Adapter::from_metal(metal);
    let descriptor = DeviceDescriptor::default();
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(Ok((device, _queue))) =
        std::pin::pin!(adapter.request_device(&descriptor))
            .as_mut()
            .poll(&mut context)
    else {
        panic!("request_device failed")
    };

    let seen = Arc::new(Mutex::new(Vec::new()));
    let out = seen.clone();
    device.set_device_lost_callback(move |reason, msg| out.lock().unwrap().push((reason, msg)));

    device
        .expect_metal()
        .handle_device_removal("MTLDeviceWasRemovedNotification");
    let captured = seen.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].0, DeviceLostReason::Unknown);
    assert!(captured[0].1.contains("MTLDeviceWasRemovedNotification"));
}

#[cfg(target_os = "macos")]
#[test]
fn enumerate_all_shares_single_observer_across_clones_and_filters_lost_adapters() {
    let devices = MetalDevice::enumerate_all();
    assert!(
        !devices.is_empty(),
        "macOS host must enumerate Metal device"
    );
    let first = devices[0].clone();
    assert!(devices[0].same_observer_for_test(&first));

    let Some(lost_metal) = MetalDevice::create_system_default() else {
        return;
    };
    let Some(healthy_metal) = MetalDevice::create_system_default() else {
        return;
    };
    lost_metal.handle_device_removal("MTLDeviceRemovalRequestedNotification");

    let selected = select_adapter(
        vec![
            Adapter::from_metal(lost_metal),
            Adapter::from_metal(healthy_metal),
        ],
        &RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            ..Default::default()
        },
    )
    .expect("healthy adapter must be selected when first candidate is lost");
    assert!(!selected.expect_metal().is_lost());
}
