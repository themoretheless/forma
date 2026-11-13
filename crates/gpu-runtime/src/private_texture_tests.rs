//! Textures on a device without unified memory live in private storage; data written to them
//! must still arrive.
use super::*;

fn block_on_ready<T>(fut: impl Future<Output = T>) -> T {
    let mut pinned = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match pinned.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(val) => val,
        std::task::Poll::Pending => panic!("expected immediate future completion"),
    }
}

/// Writes a pattern to a 4x4 texture and reads it back through a buffer.
fn round_trip(unified_memory: bool) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut metal = MetalDevice::create_system_default()?;
    // A discrete GPU reports no unified memory; private storage is valid on every device, so the
    // path can be exercised here too.
    metal.has_unified_memory = unified_memory;
    let adapter = Adapter::from_metal(metal);
    let (device, queue) = block_on_ready(adapter.request_device(&DeviceDescriptor::default()))
        .expect("device creation must succeed");
    const SIDE: u32 = 4;
    let extent = Extent3d {
        width: SIDE,
        height: SIDE,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::COPY_SRC | TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let pattern: Vec<u8> = (0..SIDE * SIDE * 4).map(|i| (i * 3 + 1) as u8).collect();
    // A leading pad checks that the layout offset is honoured.
    let mut data = vec![0xEE; 8];
    data.extend_from_slice(&pattern);
    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.write_texture(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        &data,
        TexelCopyBufferLayout {
            offset: 8,
            bytes_per_row: Some(SIDE * 4),
            rows_per_image: Some(SIDE),
        },
        extent,
    );
    let error = block_on_ready(scope.pop());
    assert!(error.is_none(), "{error:?}");

    let readback = device.create_buffer(&BufferDescriptor {
        label: None,
        size: u64::from(SIDE * SIDE * 4),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyBufferInfo {
            buffer: &readback,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIDE * 4),
                rows_per_image: Some(SIDE),
            },
        },
        extent,
    );
    queue.submit(Some(encoder.finish()));
    let read = readback.uma_slice().to_vec();
    Some((pattern, read))
}

#[test]
fn a_write_reaches_a_shared_texture() {
    let Some((written, read)) = round_trip(true) else {
        return;
    };
    assert_eq!(read, written);
}

#[test]
fn a_write_reaches_a_private_texture() {
    let Some((written, read)) = round_trip(false) else {
        return;
    };
    assert_eq!(read, written);
}
