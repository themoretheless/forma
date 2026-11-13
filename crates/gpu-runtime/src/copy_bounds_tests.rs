//! Buffer copies and clears outside a buffer are validation errors, not writes.
use super::*;

fn block_on_ready<T>(fut: impl Future<Output = T>) -> T {
    let mut pinned = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match pinned.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(val) => val,
        std::task::Poll::Pending => panic!("expected immediate future completion"),
    }
}

fn device() -> Option<(Device, Queue)> {
    let adapter = Adapter::from_metal(MetalDevice::create_system_default()?);
    Some(
        block_on_ready(adapter.request_device(&DeviceDescriptor::default()))
            .expect("device creation must succeed"),
    )
}

fn buffer(device: &Device, queue: &Queue, fill: u8) -> Buffer {
    let buffer = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 16,
        usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, &[fill; 16]);
    buffer
}

/// Records one command, submits it and returns the validation error with both buffers.
fn run(
    record: impl FnOnce(&mut CommandEncoder, &Buffer, &Buffer),
) -> Option<(Option<Error>, Vec<u8>, Vec<u8>)> {
    let (device, queue) = device()?;
    let source = buffer(&device, &queue, 1);
    let destination = buffer(&device, &queue, 2);
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    record(&mut encoder, &source, &destination);
    let error = block_on_ready(scope.pop());
    queue.submit(Some(encoder.finish()));
    let source_bytes = source.uma_slice().to_vec();
    let destination_bytes = destination.uma_slice().to_vec();
    Some((error, source_bytes, destination_bytes))
}

#[test]
fn a_copy_inside_both_buffers_is_performed() {
    let Some((error, _, destination)) = run(|encoder, src, dst| {
        encoder.copy_buffer_to_buffer(src, 4, dst, 8, 8);
    }) else {
        return;
    };
    assert!(error.is_none(), "{error:?}");
    assert_eq!(
        destination,
        [2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1]
    );
}

#[test]
fn a_copy_past_either_buffer_is_refused() {
    for (src_offset, dst_offset, size) in [
        (12, 0, 8),
        (0, 12, 8),
        (0, 0, 17),
        (u64::MAX - 3, 0, 8),
        (0, u64::MAX - 3, 8),
        (17, 0, 0),
    ] {
        let Some((error, _, destination)) = run(|encoder, src, dst| {
            encoder.copy_buffer_to_buffer(src, src_offset, dst, dst_offset, size);
        }) else {
            return;
        };
        assert!(
            matches!(error, Some(Error::Validation { .. })),
            "{src_offset} -> {dst_offset}, {size} bytes: {error:?}"
        );
        assert_eq!(
            destination, [2; 16],
            "{src_offset} -> {dst_offset}, {size} bytes"
        );
    }
}

#[test]
fn a_copy_within_one_buffer_is_refused() {
    let Some((error, source, _)) = run(|encoder, src, _| {
        encoder.copy_buffer_to_buffer(src, 0, src, 4, 8);
    }) else {
        return;
    };
    assert!(matches!(error, Some(Error::Validation { .. })), "{error:?}");
    assert_eq!(source, [1; 16]);
}

#[test]
fn a_clear_past_the_buffer_is_refused() {
    for (offset, size) in [(8, Some(16)), (17, None), (u64::MAX - 3, Some(8))] {
        let Some((error, _, destination)) = run(|encoder, _, dst| {
            encoder.clear_buffer(dst, offset, size);
        }) else {
            return;
        };
        assert!(
            matches!(error, Some(Error::Validation { .. })),
            "clear {offset} {size:?}: {error:?}"
        );
        assert_eq!(destination, [2; 16], "clear {offset} {size:?}");
    }
    let Some((error, _, destination)) = run(|encoder, _, dst| {
        encoder.clear_buffer(dst, 8, None);
    }) else {
        return;
    };
    assert!(error.is_none(), "{error:?}");
    assert_eq!(
        destination,
        [2, 2, 2, 2, 2, 2, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}
