//! Offscreen rasterization benchmark: renders a frame of N animated meshes
//! and reports ms/frame plus upload bytes per frame (the same metric the
//! browser morph benchmark tracks).
//!
//! Run: cargo run --release --offline --example bench -p raster-core

use raster_core::gpu_compute::GpuContext;
use raster_core::rasterizer::{Frame, Rasterizer};
use raster_core::uniform::{ObjectUniform, SceneUniform};
use raster_core::wgpu;
use std::time::Instant;

const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;

fn identity() -> [f32; 16] {
    let mut m = [0.0f32; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}

/// Perspective-ish ortho with z in the WebGPU [0, 1] range.
fn scene() -> SceneUniform {
    let mut vp = identity();
    // Scale the unit cube down so the meshes tile the view.
    vp[0] = 0.18;
    vp[5] = 0.18;
    SceneUniform::new(vp, [0.0, 0.0, 1.0, 1.0])
}

/// A pyramid-ish fan: one center vertex plus a ring, normals up.
fn fan(segments: usize) -> (Vec<f32>, Vec<u32>) {
    let mut vertices = Vec::with_capacity((segments + 2) * 6);
    let mut indices = Vec::with_capacity(segments * 6);
    vertices.extend_from_slice(&[0.0, 0.0, 0.9, 0.0, 0.0, 1.0]);
    for i in 0..segments {
        let a = i as f32 / segments as f32 * std::f32::consts::TAU;
        vertices.extend_from_slice(&[a.cos(), a.sin(), 0.1, 0.0, 0.0, 1.0]);
    }
    for i in 0..segments {
        indices.extend_from_slice(&[0, 1 + i as u32, 1 + ((i + 1) % segments) as u32]);
    }
    (vertices, indices)
}

fn main() {
    let Some(context) = GpuContext::new() else {
        eprintln!("no GPU adapter available");
        return;
    };
    println!("backend: {}", context.backend_label());
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
    rasterizer.set_scene(&scene());

    let mesh_count = 200usize;
    let (vertices, indices) = fan(24);
    let mut meshes = Vec::with_capacity(mesh_count);
    let side = (mesh_count as f32).sqrt().ceil() as usize;
    for i in 0..mesh_count {
        let gx = (i % side) as f32 - side as f32 / 2.0;
        let gy = (i / side) as f32 - side as f32 / 2.0;
        let mut model = identity();
        model[0] = 0.35;
        model[5] = 0.35;
        model[12] = gx * 2.2;
        model[13] = gy * 2.2;
        let mut uniform = ObjectUniform { model, nmat: identity(), ..ObjectUniform::default() };
        uniform.color = [
            0.35 + 0.6 * (i as f32 / mesh_count as f32),
            0.5,
            0.9 - 0.6 * (i as f32 / mesh_count as f32),
            1.0,
        ];
        meshes.push(rasterizer.create_mesh(&vertices, &indices, &uniform, None));
    }

    let clear = wgpu::Color { r: 0.05, g: 0.06, b: 0.08, a: 1.0 };
    let mesh_refs: Vec<_> = meshes.iter().collect();

    // Warmup.
    for tick in 0..10 {
        animate(&rasterizer, &meshes, tick);
        let frame = Frame { clear, meshes: &mesh_refs, ..Frame::default() };
        let _ = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    }

    let frames = 200;
    let start = Instant::now();
    for tick in 0..frames {
        animate(&rasterizer, &meshes, tick);
        let frame = Frame { clear, meshes: &mesh_refs, ..Frame::default() };
        let _ = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    }
    let elapsed = start.elapsed();
    println!(
        "meshes: {mesh_count} ({})  target: {WIDTH}x{HEIGHT}",
        vertices.len() / 6 * mesh_count,
    );
    println!(
        "frame: {:.3} ms  ({} frames, {:.1} fps)",
        elapsed.as_secs_f64() * 1000.0 / frames as f64,
        frames,
        frames as f64 / elapsed.as_secs_f64(),
    );
    println!("per-frame uniform upload: {} bytes", mesh_count * 16);
}

/// Animates one float per mesh (the morph weight) — the same per-frame
/// upload profile as the browser morph path (one writeBuffer per mesh).
fn animate(rasterizer: &Rasterizer, meshes: &[raster_core::rasterizer::DrawMesh], tick: usize) {
    for (i, mesh) in meshes.iter().enumerate() {
        let w = 0.5 + 0.5 * (tick as f32 * 0.05 + i as f32 * 0.1).sin();
        rasterizer.set_morph_weight(mesh, w);
    }
}
