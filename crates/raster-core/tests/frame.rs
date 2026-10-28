//! Frame composition tests: edges, lines, grid, deep-selection underlay and
//! instanced draws in one pipeline pass, validating coverage, depth ordering
//! and blend behavior pixel by pixel. GPU-dependent; skipped without an
//! adapter.

use raster_core::gpu_compute::GpuContext;
use raster_core::rasterizer::{Frame, Rasterizer, write_ppm};
use raster_core::uniform::{ObjectUniform, SceneUniform};
use raster_core::wgpu;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const CLEAR: wgpu::Color = wgpu::Color { r: 0.05, g: 0.06, b: 0.08, a: 1.0 };
const CLEAR_BYTES: [u8; 4] = [13, 15, 20, 255];

fn identity() -> [f32; 16] {
    let mut m = [0.0f32; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}

fn translated(tx: f32, ty: f32, tz: f32) -> [f32; 16] {
    let mut m = identity();
    m[12] = tx;
    m[13] = ty;
    m[14] = tz;
    m
}

fn scene() -> SceneUniform {
    SceneUniform::new(identity(), [0.0, 0.0, 1.0, 1.0])
}

/// Grid-enabled scene: inverse VP plus a grid step of 0.5 world units.
fn grid_scene() -> SceneUniform {
    let mut s = scene();
    s.inverse_vp = identity();
    s.options = [0.0, 0.5, 0.0, 0.0];
    s
}

/// Unit triangle fan on z = 0, normals up.
fn triangle() -> (Vec<f32>, Vec<u32>) {
    let pts = [(-0.6f32, -0.4f32), (0.6, -0.4), (0.0, 0.6)];
    let mut vertices = Vec::with_capacity(18);
    for (x, y) in pts {
        vertices.extend_from_slice(&[x, y, 0.0, 0.0, 0.0, 1.0]);
    }
    (vertices, vec![0, 1, 2])
}

fn uniform(color: [f32; 3], style: [f32; 4], model: [f32; 16]) -> ObjectUniform {
    ObjectUniform { model, nmat: identity(), color: [color[0], color[1], color[2], 1.0], style, morph: [1.0, 0.0, 0.0, 0.0], ..ObjectUniform::default() }
}

fn pixel(bytes: &[u8], x: u32, y: u32) -> [u8; 4] {
    let start = ((y * WIDTH + x) * 4) as usize;
    [bytes[start], bytes[start + 1], bytes[start + 2], bytes[start + 3]]
}

fn background_at(bytes: &[u8], x: u32, y: u32) -> bool {
    pixel(bytes, x, y) == CLEAR_BYTES
}

fn snapshot(name: &str, bytes: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../output")
        .join(format!("raster-{name}.ppm"));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = write_ppm(&path, WIDTH, HEIGHT, bytes);
}

#[test]
fn edges_draw_on_top_of_the_mesh_with_depth_bias() {
    let Some(context) = GpuContext::new() else { return };
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
    rasterizer.set_scene(&scene());
    let (vertices, indices) = triangle();
    // Edge style: style.z is the edge alpha; dark edge color by default.
    let mesh = rasterizer.create_mesh(&vertices, &indices, &uniform([0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.0], identity()), None);
    let edges = rasterizer.create_edges(&vertices, &[0, 1, 1, 2, 2, 0], &uniform([0.0, 0.0, 0.0], [1.0, 0.0, 0.9, 0.0], identity()), None);

    let frame = Frame { clear: CLEAR, meshes: &[&mesh], edges: &[&edges], ..Frame::default() };
    let bytes = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    snapshot("edges", &bytes);

    // Edge endpoints and midpoints trace the triangle outline.
    let (x0, y0) = ((0.4 * (WIDTH as f32 - 1.0)) as u32, ((1.0 - (-0.4 + 1.0) / 2.0) * (HEIGHT as f32 - 1.0)) as u32);
    let _ = (x0, y0);
    // Bottom edge midpoint in NDC (0, -0.4): pixel (32, ~51). Edge color is
    // near-black with alpha 0.9 over blue fill.
    let p = pixel(&bytes, 32, 51);
    assert!(p[2] < 120, "bottom edge darkens the blue fill: {p:?}");
    // Fill center stays blue.
    assert!(pixel(&bytes, 32, 40)[2] > 100, "fill center stays blue");
    // A pixel off the triangle stays clear.
    assert!(background_at(&bytes, 4, 4), "corner stays clear");
}

#[test]
fn grid_lines_and_overlay_compose() {
    let Some(context) = GpuContext::new() else {
        eprintln!("raster-core: no GPU adapter available, skipping render test");
        return;
    };
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);

    // Frame A: grid only. The shader reconstructs the z = 0 world plane, so
    // the axes cross at NDC (0, 0) → pixel (32, 32): x-axis red below/above,
    // y-axis green left/right.
    rasterizer.set_scene(&grid_scene());
    let grid_frame = Frame { clear: CLEAR, grid: true, ..Frame::default() };
    let grid = rasterizer.render_to_rgba(WIDTH, HEIGHT, &grid_frame);
    snapshot("grid", &grid);
    let mut found_axis_red = false;
    let mut found_axis_green = false;
    for dx in 0..3 {
        for dy in 0..3 {
            let px = pixel(&grid, 32 + dx, 32 + dy);
            if px[0] > 180 && px[1] < 90 {
                found_axis_red = true;
            }
            if px[1] > 150 && px[1] > px[0] + 50 {
                found_axis_green = true;
            }
        }
    }
    assert!(found_axis_red, "grid x-axis renders red near the origin");
    assert!(found_axis_green, "grid y-axis renders green near the origin");

    // Frame B: line + overlay, no grid (the grid writes depth 0 across the
    // ground plane, which would depth-reject an overlay sitting at z = 0 —
    // the same reason the browser only draws overlays on selected faces that
    // sit off the ground). The overlay floats at z = 0.5, inside the
    // [0, 1] depth range.
    rasterizer.set_scene(&scene());
    let lines = rasterizer.create_line_batch(&[
        -0.9, 0.02, 0.5, 0.0, 0.9, 0.1, 1.0,
        0.9, 0.02, 0.5, 0.0, 0.9, 0.1, 1.0,
    ]);
    let overlay = rasterizer.create_overlay_batch(&[
        0.2, -0.2, 0.5, 1.0, 0.5, 0.0, 0.6,
        0.9, -0.2, 0.5, 1.0, 0.5, 0.0, 0.6,
        0.55, 0.5, 0.5, 1.0, 0.5, 0.0, 0.6,
    ]);
    let frame = Frame {
        clear: CLEAR,
        lines: Some(&lines),
        overlay: Some(&overlay),
        ..Frame::default()
    };
    let bytes = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    snapshot("lines-overlay", &bytes);

    // The green line crosses the middle at NDC y ≈ 0.02 → pixel row ≈ 31.
    let mut found_green = false;
    for y in 29..34 {
        for x in 4..60 {
            let p = pixel(&bytes, x, y);
            if p[1] > 150 && p[1] > p[0] + 40 {
                found_green = true;
                break;
            }
        }
    }
    assert!(found_green, "green line is visible across the middle row");

    // The overlay triangle blends orange over the clear color on the right.
    // Triangle centroid ≈ NDC (0.55, 0.03) → pixel ≈ (49, 31).
    let p = pixel(&bytes, 49, 31);
    assert!(p[0] > 120 && p[0] > p[2] + 40, "overlay blends orange on the right: {p:?}");

    // A far corner stays clear (no grid in this frame).
    assert!(background_at(&bytes, 2, 2), "top-left stays clear");
}

#[test]
fn deep_selection_underlay_draws_through_geometry() {
    let Some(context) = GpuContext::new() else { return };
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
    rasterizer.set_scene(&scene());
    let (vertices, indices) = triangle();

    // Opaque blue quad-ish triangle at z = 0.4 (closer, NDC z 0.4 covers the
    // left-center); the selected red triangle sits at z = 0 (farther).
    let near = uniform([0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.0], translated(0.0, 0.0, 0.4));
    let near_mesh = rasterizer.create_mesh(&vertices, &indices, &near, None);
    let sel = uniform([0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], identity());
    let sel_mesh = rasterizer.create_mesh(&vertices, &indices, &sel, None);
    let sel_edges = rasterizer.create_edges(&vertices, &[0, 1, 1, 2, 2, 0], &sel, None);

    // Without deep: the near blue triangle fully hides the red one.
    let plain = Frame { clear: CLEAR, meshes: &[&near_mesh], ..Frame::default() };
    let without = rasterizer.render_to_rgba(WIDTH, HEIGHT, &plain);
    // With deep underlay: the orange tint bleeds through the blue.
    let with_deep = Frame { clear: CLEAR, deep: Some((&sel_mesh, &sel_edges)), meshes: &[&near_mesh], ..Frame::default() };
    let with = rasterizer.render_to_rgba(WIDTH, HEIGHT, &with_deep);
    snapshot("deep", &with);

    let p_plain = pixel(&without, 32, 40);
    let p_deep = pixel(&with, 32, 40);
    assert!(p_plain[2] > 100 && p_plain[0] < 60, "plain frame is blue at center: {p_plain:?}");
    assert!(p_deep[0] > p_plain[0] + 15, "deep underlay adds orange through the blue: {p_deep:?} vs {p_plain:?}");
}

#[test]
fn instanced_draws_place_each_record_its_own_transform() {
    let Some(context) = GpuContext::new() else { return };
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
    rasterizer.set_scene(&scene());
    let (vertices, indices) = triangle();

    let pool = rasterizer.create_instance_pool(&[
        uniform([1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], translated(-0.7, 0.0, 0.0)),
        uniform([0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.0], translated(0.7, 0.0, 0.0)),
    ]);
    let geometry = rasterizer.create_instanced_geometry(&vertices, &indices, None);

    let frame = Frame {
        clear: CLEAR,
        instances: &[(&geometry, &pool, 0, 2, false)],
        ..Frame::default()
    };
    let bytes = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    snapshot("instanced", &bytes);

    // Red instance (shifted -0.7) is mostly clipped on the left; its visible
    // span at row 44 reaches NDC x ≈ -0.12 → pixel ≈ 28, so sample (20, 44).
    let p = pixel(&bytes, 20, 44);
    assert!(p[0] > 100 && p[0] > p[2] + 60, "left instance renders red: {p:?}");
    // Blue instance (shifted +0.7) covers the right side; sample (45, 44).
    let p = pixel(&bytes, 45, 44);
    assert!(p[2] > 100 && p[2] > p[0] + 60, "right instance renders blue: {p:?}");
    // Background where no instance covers.
    assert!(background_at(&bytes, 60, 60), "bottom-right corner stays clear");
}

#[test]
fn shadow_pass_darkens_occluded_floor_and_toggles_off_exactly() {
    let Some(context) = GpuContext::new() else { return };
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
    rasterizer.set_scene(&scene());

    // Floor: large quad at z = 0.6 (farther: with the identity VP, larger NDC
    // z loses the Less depth test). Blocker: triangle at z = 0.2 near the
    // origin, casting a shadow onto the floor along the default key light
    // (0.55, 0.75, 0.45).
    let mut floor_vertices = Vec::with_capacity(24);
    for (x, y) in [(-1.5f32, -1.5f32), (1.5, -1.5), (1.5, 1.5), (-1.5, 1.5)] {
        floor_vertices.extend_from_slice(&[x, y, 0.6, 0.0, 0.0, 1.0]);
    }
    let floor = rasterizer.create_mesh(
        &floor_vertices,
        &[0, 1, 2, 0, 2, 3],
        &uniform([0.8, 0.8, 0.8], [1.0, 0.0, 0.0, 0.0], identity()),
        None,
    );
    let (blocker_vertices, blocker_indices) = triangle();
    let blocker = rasterizer.create_mesh(
        &blocker_vertices,
        &blocker_indices,
        &uniform([0.2, 0.2, 0.2], [1.0, 0.0, 0.0, 0.0], translated(0.0, 0.0, 0.2)),
        None,
    );

    let frame = Frame { clear: CLEAR, meshes: &[&floor, &blocker], ..Frame::default() };

    // Baseline: shadows never enabled — the pre-shadow look.
    let off = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    snapshot("shadow-off", &off);

    // Shadows on: the pass runs and occluded floor pixels darken.
    rasterizer.set_shadow_bounds(Some(([0.0, 0.0, 0.4], 1.5)));
    rasterizer.set_shadows_enabled(true);
    rasterizer.set_scene(&scene());
    let on = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    snapshot("shadow-on", &on);

    let mut darkened = 0usize;
    for i in 0..(WIDTH * HEIGHT) as usize {
        let (a, b) = (&off[i * 4..i * 4 + 3], &on[i * 4..i * 4 + 3]);
        if a.iter().zip(b).all(|(x, y)| *y + 10 < *x) {
            darkened += 1;
        }
    }
    assert!(darkened > 50, "blocker shadow darkens a floor region: {darkened} px");

    // Lit floor far from the blocker keeps its exact color (the shadow only
    // touches the occluded region).
    assert_eq!(pixel(&off, 3, 3), pixel(&on, 3, 3), "lit floor corner unchanged");

    // Toggling back off restores the pre-shadow look pixel for pixel.
    rasterizer.set_shadows_enabled(false);
    rasterizer.set_scene(&scene());
    let off_again = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    assert_eq!(off, off_again, "disabling shadows restores the exact baseline");
}

#[test]
fn transparent_mesh_blends_over_opaque_and_skips_depth_write() {
    let Some(context) = GpuContext::new() else { return };
    let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
    rasterizer.set_scene(&scene());
    let (vertices, indices) = triangle();

    // Opaque blue triangle pushed back to z = 0.5; the red ghost floats in
    // front of it at z = 0.2 (the eye sits at +z, so "nearer" is smaller z
    // inside the [0, 1] WebGPU depth range).
    let opaque = rasterizer.create_mesh(&vertices, &indices, &uniform([0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.0], translated(0.0, 0.0, 0.5)), None);
    let ghost = rasterizer.create_mesh(&vertices, &indices, &uniform([1.0, 0.0, 0.0], [0.5, 0.0, 0.0, 0.0], translated(0.3, 0.0, 0.2)), None);
    assert!(ghost.transparent, "alpha 0.5 marks the draw transparent");

    let frame = Frame { clear: CLEAR, meshes: &[&opaque, &ghost], ..Frame::default() };
    let bytes = rasterizer.render_to_rgba(WIDTH, HEIGHT, &frame);
    snapshot("transparent", &bytes);

    // The ghost (shifted +0.3 x, slightly nearer) blends red over blue:
    // purple-ish in the overlap.
    let p = pixel(&bytes, 45, 40);
    assert!(p[0] > 60 && p[2] > 60, "overlap blends red over blue: {p:?}");
}
