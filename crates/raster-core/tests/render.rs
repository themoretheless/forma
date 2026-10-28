//! Headless rasterizer smoke tests. Skipped (with a notice) on machines
//! without an adapter, matching the compute-kernel test convention.

use raster_core::gpu_compute::GpuContext;
use raster_core::rasterizer::{Frame, Rasterizer};
use raster_core::uniform::{ObjectUniform, SceneUniform};
use raster_core::wgpu;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

const CLEAR: wgpu::Color = wgpu::Color { r: 0.05, g: 0.06, b: 0.08, a: 1.0 };
const CLEAR_BYTES: [u8; 4] = [13, 15, 20, 255];

/// Identity orthographic box over [-1, 1]². The triangle sits on the z = 0
/// model plane, which lands at NDC z = 0 — inside the WebGPU/Vulkan-style
/// [0, 1] depth range, so it passes the `Less` depth test against the cleared
/// 1.0. (An OpenGL-style [-1, 1] z projection would clip the primitive.)
fn test_scene() -> SceneUniform {
    let mut view_projection = [0.0f32; 16];
    view_projection[0] = 1.0;
    view_projection[5] = 1.0;
    view_projection[10] = 1.0;
    view_projection[15] = 1.0;
    SceneUniform::new(view_projection, [0.0, 0.0, 1.0, 1.0])
}

fn morph_frame<'a>(meshes: &'a [&'a raster_core::rasterizer::DrawMesh]) -> Frame<'a> {
    Frame { clear: CLEAR, meshes, ..Frame::default() }
}
/// A triangle on the z = 0 plane; the morph source shifts it +0.3 in x, so
/// weight 0 shows the shifted (source) shape and weight 1 the vertex-buffer
/// (target) shape.
fn triangle() -> (Vec<f32>, Vec<u32>, Vec<f32>) {
    let target = [(-0.6f32, -0.4f32), (0.6, -0.4), (0.0, 0.6)];
    let mut vertices = Vec::with_capacity(18);
    let mut morph = Vec::with_capacity(9);
    for (x, y) in target {
        vertices.extend_from_slice(&[x, y, 0.0, 0.0, 0.0, 1.0]);
        morph.extend_from_slice(&[x + 0.3, y, 0.0]);
    }
    (vertices, vec![0, 1, 2], morph)
}

fn pixel(bytes: &[u8], x: u32, y: u32) -> [u8; 4] {
    let start = ((y * WIDTH + x) * 4) as usize;
    [bytes[start], bytes[start + 1], bytes[start + 2], bytes[start + 3]]
}

fn red_at(bytes: &[u8], x: u32, y: u32) -> bool {
    let [r, _g, b, _a] = pixel(bytes, x, y);
    r > 100 && r > b + 60
}

fn background_at(bytes: &[u8], x: u32, y: u32) -> bool {
    pixel(bytes, x, y) == CLEAR_BYTES
}

fn colored_uniform(r: f32, g: f32, b: f32) -> ObjectUniform {
    let mut uniform = ObjectUniform::default();
    uniform.color = [r, g, b, 1.0];
    uniform.style = [1.0, 0.0, 0.0, 0.0];
    uniform
}

#[test]
fn morph_weight_moves_the_triangle_on_the_gpu() {
    let Some(context) = GpuContext::new() else {
        eprintln!("raster-core: no GPU adapter available, skipping render test");
        return;
    };
    eprintln!("raster-core: rendering on {}", context.backend_label());
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
    rasterizer.set_scene(&test_scene());

    let (vertices, indices, morph) = triangle();
    let mesh = rasterizer.create_mesh(&vertices, &indices, &colored_uniform(1.0, 0.0, 0.0), Some(&morph));

    // Weight 0 shows the slot-1 source positions (shifted right).
    rasterizer.set_morph_weight(&mesh, 0.0);
    let at_source = rasterizer.render_to_rgba(WIDTH, HEIGHT, &morph_frame(&[&mesh]));
    assert!(red_at(&at_source, 55, 40), "source-only area is covered at weight 0");
    assert!(background_at(&at_source, 16, 40), "target-only area stays background at weight 0");

    // Weight 1 shows the vertex-buffer target positions.
    rasterizer.set_morph_weight(&mesh, 1.0);
    let at_target = rasterizer.render_to_rgba(WIDTH, HEIGHT, &morph_frame(&[&mesh]));
    assert!(red_at(&at_target, 16, 40), "target-only area is covered at weight 1");
    assert!(background_at(&at_target, 55, 40), "source-only area returns to background at weight 1");

    // Halfway blend: midpoint covered, both extreme-only areas uncovered.
    rasterizer.set_morph_weight(&mesh, 0.5);
    let at_half = rasterizer.render_to_rgba(WIDTH, HEIGHT, &morph_frame(&[&mesh]));
    assert!(red_at(&at_half, 36, 40), "mid blend covers the midpoint");
    assert!(background_at(&at_half, 16, 40), "left edge retreats at half blend");
    assert!(background_at(&at_half, 55, 40), "right edge retreats at half blend");
}

#[test]
fn mesh_without_morph_source_draws_its_target_vertices() {
    let Some(context) = GpuContext::new() else {
        eprintln!("raster-core: no GPU adapter available, skipping render test");
        return;
    };
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
    rasterizer.set_scene(&test_scene());

    let (vertices, indices, _morph) = triangle();
    // No morph source: slot 1 binds the zero dummy and the uniform's rest
    // weight (1) keeps it inert — mix(dummy, pos, 1) = pos — so the target
    // vertices draw unchanged. (A rest weight of 0 would collapse the mesh
    // toward the origin; callers must never write a sub-rest weight without
    // binding a real morph source, and the browser renderer holds the same
    // invariant by retiring morphs back to weight 1.)
    let mesh = rasterizer.create_mesh(&vertices, &indices, &colored_uniform(0.0, 0.0, 1.0), None);
    let mesh_refs = [&mesh];
    let frame = morph_frame(&mesh_refs);
    let first = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    let second = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    assert_eq!(first, second, "repeated renders are deterministic");
    assert!(pixel(&first, 16, 40)[2] > 100, "target triangle covers the left area in blue");
    assert!(background_at(&first, 55, 40), "source-only area stays background");
}

#[test]
fn caller_records_render_and_readback_in_one_submission() {
    let Some(context) = GpuContext::new() else {
        assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(), "GPU adapter required");
        return;
    };
    let mut rasterizer = Rasterizer::new(context.clone(), wgpu::TextureFormat::Rgba8Unorm);
    assert_eq!(rasterizer.context.device, context.device);
    rasterizer.set_scene(&test_scene());
    let (vertices, indices, _) = triangle();
    let mesh = rasterizer.create_mesh(&vertices, &indices, &colored_uniform(1.0, 0.0, 0.0), None);
    let meshes = [&mesh];
    let frame = morph_frame(&meshes);
    let texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("recorded raster target"),
        size: wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    // WIDTH=64 gives the required 256-byte row alignment.
    let bytes = (WIDTH * HEIGHT * 4) as usize;
    let staging = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("recorded raster readback"), size: bytes as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = context.device.create_command_encoder(&Default::default());
    rasterizer.record(&mut encoder, &texture.create_view(&Default::default()), WIDTH, HEIGHT, &frame);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &staging, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(WIDTH * 4), rows_per_image: Some(HEIGHT) } },
        wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
    );
    context.queue.submit([encoder.finish()]);
    let rgba = raster_core::gpu_compute::read_buffer(&context.device, &staging, bytes);
    assert!(red_at(&rgba, WIDTH / 2, HEIGHT / 2));
    assert!(background_at(&rgba, 0, 0));
}

#[test]
fn rgba_readback_handles_padded_rows_and_rejects_invalid_sizes() {
    let Some(context) = GpuContext::new() else { return };
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
    let frame = Frame { clear: wgpu::Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 }, ..Frame::default() };
    for (width, height) in [(1, 3), (65, 2)] {
        let rgba = rasterizer.render_rgba_async(width, height, &frame).unwrap()
            .wait(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(rgba.len(), (width * height * 4) as usize);
        assert!(rgba.chunks_exact(4).all(|pixel| pixel == [0,255,0,255]));
    }
    assert!(rasterizer.try_render_to_rgba(0, 3, &frame).is_err());
    assert!(rasterizer.try_render_to_rgba(u32::MAX, 3, &frame).is_err());
}
