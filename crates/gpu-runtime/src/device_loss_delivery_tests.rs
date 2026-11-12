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
        .metal
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
