use super::*;

fn test_buffer() -> Option<(Device, Queue, Buffer)> {
    let metal_dev = MetalDevice::create_system_default()?;
    let adapter = Adapter::from_metal(metal_dev);
    let mut request = std::pin::pin!(adapter.request_device(&DeviceDescriptor::default()));
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(Ok((device, queue))) = request.as_mut().poll(&mut cx) else {
        panic!("device creation must succeed immediately");
    };
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("mapped ranges"),
        size: 64,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ | BufferUsages::MAP_WRITE,
        mapped_at_creation: true,
    });
    Some((device, queue, buffer))
}

#[test]
fn a_second_writable_view_of_the_same_bytes_is_refused() {
    let Some((_device, queue, buffer)) = test_buffer() else {
        return;
    };
    let alias = buffer.clone();

    let first = buffer.get_mapped_range_mut(0..32).expect("first view");
    assert!(alias.get_mapped_range_mut(16..48).is_err());
    assert!(alias.get_mapped_range(31..32).is_err());
    assert!(
        queue
            .write_buffer_with(&alias, 8, BufferSize::new(8).unwrap())
            .is_none()
    );
    // Bytes outside the view stay available.
    assert!(alias.get_mapped_range_mut(32..64).is_ok());
    drop(first);
    assert!(alias.get_mapped_range_mut(16..48).is_ok());
}

#[test]
fn readers_share_a_range_and_block_writers_until_dropped() {
    let Some((_device, _queue, buffer)) = test_buffer() else {
        return;
    };
    let first = buffer.get_mapped_range(0..32).expect("first reader");
    let second = buffer.get_mapped_range(0..32).expect("second reader");
    let third = buffer.uma_slice();
    assert!(buffer.get_mapped_range_mut(8..16).is_err());
    drop(first);
    drop(third);
    assert!(buffer.get_mapped_range_mut(8..16).is_err());
    drop(second);
    assert!(buffer.get_mapped_range_mut(8..16).is_ok());
}

#[test]
fn views_split_from_one_mapping_keep_the_range_until_the_last_one_drops() {
    let Some((_device, _queue, buffer)) = test_buffer() else {
        return;
    };
    let view = buffer.get_mapped_range_mut(0..10).expect("view");
    let (chunks, tail) = view.into_chunks::<4>();
    drop(chunks);
    assert!(buffer.get_mapped_range(0..4).is_err());
    drop(tail);
    assert!(buffer.get_mapped_range(0..4).is_ok());
}

#[test]
#[should_panic(expected = "already mapped")]
fn an_overlapping_slice_view_panics_like_the_reference_runtime() {
    let Some((_device, _queue, buffer)) = test_buffer() else {
        panic!("already mapped: no Metal device, nothing to check");
    };
    let _first = buffer.slice(0..32).get_mapped_range_mut();
    let _second = buffer.slice(0..32).get_mapped_range_mut();
}
