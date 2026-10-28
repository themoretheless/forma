use gpu_compute::{ByteReadback, GpuContext, ReadbackError, try_read_buffer, wgpu};
use std::time::Duration;
fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    assert!(
        context.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "GPU required"
    );
    context
}
fn source(context: &GpuContext) -> wgpu::Buffer {
    let buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    context.queue.write_buffer(
        &buffer,
        0,
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    );
    buffer
}
#[test]
fn ranges_empty_consumption_and_cancellation_are_distinct() {
    let Some(c) = context() else { return };
    let buffer = source(&c);
    let mut encoder = c.device.create_command_encoder(&Default::default());
    assert!(matches!(
        ByteReadback::copy_buffer(&c.device, &mut encoder, &buffer, 1, 4),
        Err(ReadbackError::InvalidRange)
    ));
    assert!(matches!(
        ByteReadback::copy_buffer(&c.device, &mut encoder, &buffer, 12, 8),
        Err(ReadbackError::InvalidRange)
    ));
    let mut part = ByteReadback::copy_buffer(&c.device, &mut encoder, &buffer, 4, 8).unwrap();
    let mut empty = ByteReadback::copy_buffer(&c.device, &mut encoder, &buffer, 0, 0).unwrap();
    let mut cancelled = ByteReadback::copy_buffer(&c.device, &mut encoder, &buffer, 0, 4).unwrap();
    assert_eq!(part.try_read().unwrap_err(), ReadbackError::NotSubmitted);
    cancelled.cancel();
    let index = c.queue.submit([encoder.finish()]);
    part.submitted(index.clone());
    empty.submitted(index.clone());
    cancelled.submitted(index);
    assert_eq!(
        part.wait(Duration::from_secs(5)).unwrap(),
        [5, 6, 7, 8, 9, 10, 11, 12]
    );
    assert_eq!(part.try_read().unwrap_err(), ReadbackError::Consumed);
    assert_eq!(empty.try_read().unwrap(), Some(vec![]));
    assert_eq!(cancelled.try_read().unwrap_err(), ReadbackError::Cancelled);
}
#[test]
fn staging_is_unmapped_after_success_and_after_cancel() {
    let Some(c) = context() else { return };
    let buffer = source(&c);
    let staging = c.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = c.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&buffer, 0, &staging, 0, 16);
    let mut ticket = ByteReadback::map_on_submit(&c.device, &encoder, &staging, 16).unwrap();
    ticket.cancel();
    c.queue.submit([encoder.finish()]);
    c.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    for _ in 0..2 {
        assert_eq!(
            try_read_buffer(&c.device, &staging, 16).unwrap(),
            (1u8..=16).collect::<Vec<_>>()
        );
    }
    assert!(matches!(
        try_read_buffer(&c.device, &buffer, 16),
        Err(ReadbackError::MissingUsage)
    ));
    assert!(!c.subgroup_report().supported); // Subgroups remain opt-in.
    assert!((c.enabled_features() - wgpu::Features::MAPPABLE_PRIMARY_BUFFERS).is_empty());
}

#[test]
fn mapped_consumers_unmap_on_success_and_unwind_and_consume_once() {
    let Some(c) = context() else { return };
    let buffer = source(&c);
    let staging = c.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = c.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&buffer, 0, &staging, 0, 16);
    let mut ticket = ByteReadback::map_on_submit(&c.device, &encoder, &staging, 16).unwrap();
    assert_eq!(
        ticket
            .try_read_with(|_| panic!("not submitted"))
            .unwrap_err(),
        ReadbackError::NotSubmitted
    );
    ticket.submitted(c.queue.submit([encoder.finish()]));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = ticket.wait_with(Duration::from_secs(5), |_| -> () {
            panic!("consumer panic")
        });
    }));
    assert!(caught.is_err());
    assert_eq!(
        ticket.try_read_with(|_| panic!("consumed")).unwrap_err(),
        ReadbackError::Consumed
    );
    // A panic must release both the view and mapping, permitting reuse.
    let sum = gpu_compute::try_read_buffer_with(&c.device, &staging, 16, |bytes| {
        bytes.iter().copied().map(u32::from).sum::<u32>()
    })
    .unwrap();
    assert_eq!(sum, 136);
    assert_eq!(
        try_read_buffer(&c.device, &staging, 16).unwrap(),
        (1..=16).collect::<Vec<u8>>()
    );
}

#[test]
fn mapped_consumer_empty_cancelled_and_pending_states_do_not_lose_results() {
    let Some(c) = context() else { return };
    let buffer = source(&c);
    let mut encoder = c.device.create_command_encoder(&Default::default());
    let mut empty = ByteReadback::copy_buffer(&c.device, &mut encoder, &buffer, 0, 0).unwrap();
    let mut cancelled = ByteReadback::copy_buffer(&c.device, &mut encoder, &buffer, 0, 4).unwrap();
    let mut pending = ByteReadback::copy_buffer(&c.device, &mut encoder, &buffer, 4, 8).unwrap();
    cancelled.cancel();
    assert_eq!(
        cancelled
            .try_read_with(|_| panic!("cancelled"))
            .unwrap_err(),
        ReadbackError::Cancelled
    );
    let completed = c.queue.submit([]);
    c.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    pending.submitted(completed);
    assert_eq!(
        pending.wait_with(Duration::ZERO, |_| panic!("pending")),
        Err(ReadbackError::Timeout)
    );
    let index = c.queue.submit([encoder.finish()]);
    empty.submitted(index.clone());
    pending.submitted(index);
    assert_eq!(empty.try_read_with(|bytes| bytes.len()).unwrap(), Some(0));
    assert_eq!(
        pending
            .wait_with(Duration::from_secs(5), |bytes| bytes.to_vec())
            .unwrap(),
        [5, 6, 7, 8, 9, 10, 11, 12]
    );
}
