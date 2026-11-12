use super::*;
use crate::test_support::validate_wgsl;

#[test]
fn texture_shader_parses_and_validates_with_naga() {
    validate_wgsl(TEXTURE_SHADER, wgpu::naga::valid::Capabilities::empty());
}

#[test]
fn hit_testing_finds_tiles_and_rejects_gaps_and_padding() {
    // Tile 0 spans x in [8, 152]; the gap is (152, 160); tile 1 starts at 160.
    assert_eq!(tile_index_at(8.0, 0.0, 3), Some(0));
    assert_eq!(tile_index_at(151.9, 0.0, 3), Some(0));
    assert_eq!(tile_index_at(155.0, 0.0, 3), None, "gap is a dead zone");
    assert_eq!(tile_index_at(160.0, 0.0, 3), Some(1));
    assert_eq!(tile_index_at(7.9, 0.0, 3), None, "left padding");
    assert_eq!(tile_index_at(8.0, 0.0, 0), None, "no tiles");
    assert_eq!(tile_index_at(160.0, 0.0, 1), None, "past last tile");
    // Scrolled one full stride: tile 1 now starts at x=8.
    assert_eq!(tile_index_at(8.0, TILE_STRIDE, 3), Some(1));
}

#[test]
fn max_scroll_is_zero_when_everything_fits() {
    assert!(max_scroll(2000.0, 3).abs() < 0.001);
    assert!(max_scroll(100.0, 3) > 0.0);
    // Content for 3 tiles: 8 + 3*152 - 8 + 8 = 464; max scroll = 464 - 100.
    assert!((max_scroll(100.0, 3) - 364.0).abs() < 0.001);
}

#[test]
fn scroll_to_reveal_moves_only_when_needed() {
    // Tile 0 already visible: no movement.
    assert!(scroll_to_reveal(0, 500.0, 5, 0.0).abs() < 0.001);
    // Tile 4 at [616, 760] is past the 500 viewport: scroll right.
    let scroll = scroll_to_reveal(4, 500.0, 5, 0.0);
    assert!(tile_x(4, scroll) + TILE_WIDTH <= 500.0 - STRIP_PADDING + 0.001);
    // Scrolled far right, tile 0 is off-screen left: scroll back.
    let scroll = scroll_to_reveal(0, 500.0, 5, 300.0);
    assert!(tile_x(0, scroll) >= STRIP_PADDING - 0.001);
    // Reveal clamps to max_scroll instead of overshooting.
    let scroll = scroll_to_reveal(4, 500.0, 5, 0.0);
    assert!(scroll <= max_scroll(500.0, 5));
}

#[test]
fn strip_band_hit_region() {
    assert!(point_in_strip(913.0, 1024.0));
    assert!(!point_in_strip(911.0, 1024.0));
    assert!(point_in_strip(1024.0, 1024.0));
}

#[cfg(target_os = "macos")]
#[test]
fn thumbnail_queue_refusal_validation_slot_reuse_and_completion() {
    let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
    let budget = MemoryBudget::new(256);
    let mut strip =
        FilmstripRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, [640.0, 480.0])
            .with_upload_queue_budget(budget.clone());
    for (w, h, data) in [
        (0, 1, vec![]),
        (1, 1, vec![0; 3]),
        (u32::MAX, u32::MAX, vec![]),
    ] {
        assert!(matches!(
            strip.try_upload_tile(&device, &queue, w, h, &data),
            Err(FilmstripUploadError::Invalid)
        ));
    }
    assert_eq!(budget.peak(), 0);
    let pressure = budget.try_reserve(256).unwrap();
    assert!(matches!(
        strip.try_upload_tile(&device, &queue, 1, 1, &[255; 4]),
        Err(FilmstripUploadError::Memory(_))
    ));
    drop(pressure);
    let first = strip
        .try_upload_tile(&device, &queue, 1, 1, &[255; 4])
        .unwrap();
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(budget.used(), 0);
    assert!(matches!(
        strip.try_upload_tile(&device, &queue, 65, 1, &vec![255; 260]),
        Err(FilmstripUploadError::Memory(_))
    ));
    strip.remove_tile(first);
    let second = strip
        .try_upload_tile(&device, &queue, 1, 1, &[128; 4])
        .unwrap();
    assert_eq!(first, second);
    drop(strip);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(budget.used(), 0);
    assert_eq!(budget.peak(), 256);
}

#[cfg(target_os = "macos")]
#[test]
fn hdr_filmstrip_thumbnail_is_linear_and_releases_resources() {
    let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let textures = MemoryBudget::new(4);
    let uploads = MemoryBudget::new(256);
    let mut strip = FilmstripRenderer::new(&device, wgpu::TextureFormat::Rgba16Float, [128., 128.])
        .with_texture_budget(textures.clone())
        .with_upload_queue_budget(uploads.clone());
    let id = strip
        .try_upload_tile(&device, &queue, 1, 1, &[128, 128, 128, 255])
        .unwrap();
    strip.update(
        &device,
        &queue,
        &[FilmstripTile {
            x: 0.,
            texture: Some(id),
            label: String::new(),
            highlighted: false,
        }],
    );
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("HDR filmstrip target"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("HDR filmstrip readback"),
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    strip.encode(&mut encoder, &view);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d { x: 20, y: 40, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).unwrap()
    });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.recv().unwrap().unwrap();
    {
        let mapped = slice.get_mapped_range().unwrap();
        for channel in 0..3 {
            let bits = u16::from_le_bytes([mapped[channel * 2], mapped[channel * 2 + 1]]);
            let exponent = i32::from((bits >> 10) & 31) - 15;
            let actual = (1. + f32::from(bits & 1023) / 1024.) * 2f32.powi(exponent);
            let expected = ((128f32 / 255. + 0.055) / 1.055).powf(2.4);
            assert!(
                (actual - expected).abs() < 0.0005,
                "HDR thumbnail must be linear: {actual} vs {expected}"
            );
        }
        assert_eq!(u16::from_le_bytes([mapped[6], mapped[7]]), 0x3c00);
    }
    buffer.unmap();
    strip.remove_tile(id);
    drop(strip);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!((textures.used(), uploads.used()), (0, 0));
    assert!(pollster::block_on(error_scope.pop()).is_none());
}
