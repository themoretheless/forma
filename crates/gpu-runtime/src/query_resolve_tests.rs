//! Resolving a query set never writes outside the destination buffer.
use super::*;

fn block_on_ready<T>(fut: impl Future<Output = T>) -> T {
    let mut pinned = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match pinned.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(val) => val,
        std::task::Poll::Pending => panic!("expected immediate future completion"),
    }
}

/// Resolves `queries` of a four-entry set into a 16-byte buffer at `offset` and returns the
/// validation error, if any, with the buffer contents afterwards.
fn resolve(queries: std::ops::Range<u32>, offset: u64) -> Option<(Option<Error>, Vec<u8>)> {
    let adapter = Adapter::from_metal(MetalDevice::create_system_default()?);
    let (device, queue) = block_on_ready(adapter.request_device(&DeviceDescriptor::default()))
        .expect("device creation must succeed");
    let query_set = device.create_query_set(&QuerySetDescriptor {
        label: None,
        ty: QueryType::Timestamp,
        count: 4,
    });
    let destination = device.create_buffer(&BufferDescriptor {
        label: Some("resolve target"),
        size: 16,
        usage: BufferUsages::QUERY_RESOLVE | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    for index in 0..4 {
        encoder.write_timestamp(&query_set, index);
    }
    encoder.resolve_query_set(&query_set, queries, &destination, offset);
    let error = block_on_ready(scope.pop());
    queue.submit(Some(encoder.finish()));
    let bytes = destination.uma_slice().to_vec();
    Some((error, bytes))
}

#[test]
fn a_range_that_fits_is_written() {
    let Some((error, bytes)) = resolve(0..2, 0) else {
        return;
    };
    assert!(error.is_none(), "{error:?}");
    assert!(
        bytes.iter().any(|&byte| byte != 0),
        "timestamps were written"
    );
}

#[test]
fn a_range_longer_than_the_buffer_is_refused() {
    let Some((error, bytes)) = resolve(0..4, 0) else {
        return;
    };
    assert!(matches!(error, Some(Error::Validation { .. })), "{error:?}");
    assert_eq!(
        bytes, [0; 16],
        "nothing is written when the range does not fit"
    );
}

#[test]
fn an_offset_past_the_end_is_refused() {
    for offset in [9, 16, u64::MAX - 7] {
        let Some((error, bytes)) = resolve(0..1, offset) else {
            return;
        };
        assert!(
            matches!(error, Some(Error::Validation { .. })),
            "offset {offset}: {error:?}"
        );
        assert_eq!(bytes, [0; 16], "offset {offset}");
    }
}
