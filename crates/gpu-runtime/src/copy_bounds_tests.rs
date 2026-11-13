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

const SIDE: u32 = 4;

fn texture(device: &Device) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::COPY_SRC | TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Records one texture copy against a 4x4 RGBA texture and a buffer of `buffer_size` bytes.
fn texture_copy(
    buffer_size: u64,
    record: impl FnOnce(&mut CommandEncoder, &Texture, &Buffer),
) -> Option<Option<Error>> {
    let (device, queue) = device()?;
    let texture = texture(&device);
    let buffer = device.create_buffer(&BufferDescriptor {
        label: None,
        size: buffer_size,
        usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    record(&mut encoder, &texture, &buffer);
    let error = block_on_ready(scope.pop());
    queue.submit(Some(encoder.finish()));
    device.poll(PollType::wait_indefinitely()).unwrap();
    Some(error)
}

fn whole(texture: &Texture, origin: Origin3d) -> TexelCopyTextureInfo<'_> {
    TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin,
        aspect: TextureAspect::All,
    }
}

fn rows(buffer: &Buffer, offset: u64) -> TexelCopyBufferInfo<'_> {
    TexelCopyBufferInfo {
        buffer,
        layout: TexelCopyBufferLayout {
            offset,
            bytes_per_row: Some(SIDE * 4),
            rows_per_image: Some(SIDE),
        },
    }
}

fn extent(width: u32, height: u32) -> Extent3d {
    Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

#[test]
fn texture_copies_that_fit_are_accepted() {
    let full = u64::from(SIDE * SIDE * 4);
    let to_buffer = texture_copy(full, |encoder, texture, buffer| {
        encoder.copy_texture_to_buffer(
            whole(texture, Origin3d::ZERO),
            rows(buffer, 0),
            extent(SIDE, SIDE),
        );
    });
    let Some(error) = to_buffer else { return };
    assert!(error.is_none(), "{error:?}");
    let to_texture = texture_copy(full, |encoder, texture, buffer| {
        encoder.copy_buffer_to_texture(
            rows(buffer, 0),
            whole(texture, Origin3d::ZERO),
            extent(SIDE, SIDE),
        );
    });
    assert!(matches!(to_texture, Some(None)), "{to_texture:?}");
}

#[test]
fn a_texture_copy_needs_the_whole_region_in_the_buffer() {
    let full = u64::from(SIDE * SIDE * 4);
    for (buffer_size, offset) in [(full - 1, 0), (full, 4), (16, 0)] {
        let to_buffer = texture_copy(buffer_size, |encoder, texture, buffer| {
            encoder.copy_texture_to_buffer(
                whole(texture, Origin3d::ZERO),
                rows(buffer, offset),
                extent(SIDE, SIDE),
            );
        });
        let Some(error) = to_buffer else { return };
        assert!(
            matches!(error, Some(Error::Validation { .. })),
            "texture to {buffer_size} bytes at {offset}: {error:?}"
        );
        let to_texture = texture_copy(buffer_size, |encoder, texture, buffer| {
            encoder.copy_buffer_to_texture(
                rows(buffer, offset),
                whole(texture, Origin3d::ZERO),
                extent(SIDE, SIDE),
            );
        });
        assert!(
            matches!(to_texture, Some(Some(Error::Validation { .. }))),
            "{buffer_size} bytes at {offset} to texture: {to_texture:?}"
        );
    }
}

#[test]
fn a_texture_copy_stays_inside_the_texture() {
    let full = u64::from(SIDE * SIDE * 4);
    let outside = Origin3d { x: 2, y: 0, z: 0 };
    let to_buffer = texture_copy(full, |encoder, texture, buffer| {
        encoder.copy_texture_to_buffer(
            whole(texture, outside),
            rows(buffer, 0),
            extent(SIDE, SIDE),
        );
    });
    let Some(error) = to_buffer else { return };
    assert!(matches!(error, Some(Error::Validation { .. })), "{error:?}");
    let to_texture = texture_copy(full, |encoder, texture, buffer| {
        encoder.copy_buffer_to_texture(
            rows(buffer, 0),
            whole(texture, outside),
            extent(SIDE, SIDE),
        );
    });
    assert!(
        matches!(to_texture, Some(Some(Error::Validation { .. }))),
        "{to_texture:?}"
    );
    let between = texture_copy(full, |encoder, texture, _| {
        encoder.copy_texture_to_texture(
            whole(texture, Origin3d::ZERO),
            whole(texture, outside),
            extent(SIDE, SIDE),
        );
    });
    assert!(
        matches!(between, Some(Some(Error::Validation { .. }))),
        "{between:?}"
    );
}

/// Writes `data` to a 4x4 RGBA texture and returns the validation error, if any.
fn write(data: &[u8], offset: u64, origin: Origin3d, size: Extent3d) -> Option<Option<Error>> {
    let (device, queue) = device()?;
    let texture = texture(&device);
    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.write_texture(
        whole(&texture, origin),
        data,
        TexelCopyBufferLayout {
            offset,
            bytes_per_row: Some(SIDE * 4),
            rows_per_image: Some(SIDE),
        },
        size,
    );
    Some(block_on_ready(scope.pop()))
}

#[test]
fn write_texture_reads_only_inside_the_data() {
    let full = (SIDE * SIDE * 4) as usize;
    let accepted = write(&vec![7; full], 0, Origin3d::ZERO, extent(SIDE, SIDE));
    let Some(error) = accepted else { return };
    assert!(error.is_none(), "{error:?}");
    // The last row may stop at its last texel.
    let tight = write(&vec![7; full + 4], 4, Origin3d::ZERO, extent(SIDE, SIDE));
    assert!(matches!(tight, Some(None)), "{tight:?}");

    for (len, offset) in [(full - 1, 0), (full, 4), (16, 0), (full, u64::MAX - 3)] {
        let refused = write(&vec![7; len], offset, Origin3d::ZERO, extent(SIDE, SIDE));
        assert!(
            matches!(refused, Some(Some(Error::Validation { .. }))),
            "{len} bytes at {offset}: {refused:?}"
        );
    }
}

#[test]
fn write_texture_stays_inside_the_texture() {
    let full = (SIDE * SIDE * 4) as usize;
    let outside = write(
        &vec![7; full],
        0,
        Origin3d { x: 1, y: 0, z: 0 },
        extent(SIDE, SIDE),
    );
    let Some(error) = outside else { return };
    assert!(matches!(error, Some(Error::Validation { .. })), "{error:?}");
}
