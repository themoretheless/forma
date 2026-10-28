# raster-core

Architecture and crate boundaries: [GPU library design](../../docs/design/gpu-library-architecture.md).

Headless WebGPU rasterizer for the open-scad-viewer shader stack: the same
`.wgsl` sources the browser renderer compiles in TypeScript, driven from Rust
via `wgpu` — for offscreen rendering, snapshot tests, and native embedding.

## What it owns

- **`shaders/`** — the six WGSL sources of truth (`mesh`, `deep_mesh`, `edge`,
  `line`, `grid`, `selection_overlay`), embedded with `include_str!`. The
  browser shader library in `src/services/shaders/` consumes texts generated
  from this directory by the `wgsl_export` codegen (bin target), and the
  `generated_ts_matches_the_wgsl_sources` test plus `wgsl_export --check`
  fail on drift.
- **`src/variants.rs`** — textual shader variants matching the browser
  renderer: immediate-style (per-draw style without a uniform rewrite) and
  instanced (per-instance `Obj` records from a read-only storage buffer).
  Composition panics on contract drift, so breakage fails at pipeline
  creation, not draw time.
- **`src/uniform.rs`** — CPU mirrors of the WGSL uniform structs with pinned
  float offsets and `write_f32` emitters. `ObjectUniform` rest state is
  `morph = [1, 0, 0, 0]`: weight 1 renders the vertex-buffer target and keeps
  the zero slot-1 morph dummy inert (`mix(dummy, pos, 1) = pos`).
- **`src/pipeline.rs`** — twelve cached pipelines: mesh opaque/transparent,
  deep mesh, edge, deep edge, line, grid, selection overlay, and the
  instanced mesh/edge triple, with the same blend/depth/vertex-layout
  decisions as the browser renderer.
- **`src/rasterizer.rs`** — the offscreen rasterizer. Build a [`Frame`]
  (clear color, grid, deep-selection x-ray overlay, meshes, instanced groups,
  edges, lines, overlay) and render it into a texture view or read back RGBA8
  bytes. `write_ppm` dumps frames as PPM for eyeballing.

## Depth convention

WebGPU clip space keeps NDC **z in [0, 1]** (Vulkan-style, unlike OpenGL's
[-1, 1]). Projections must place geometry inside that range; an
OpenGL-style projection with z ∈ [-1, 1] clips everything.

## Usage

```rust
use raster_core::gpu_compute::GpuContext;
use raster_core::rasterizer::{Frame, Rasterizer};
use raster_core::uniform::{ObjectUniform, SceneUniform};
use raster_core::wgpu;

let context = GpuContext::new().expect("GPU adapter");
let mut rasterizer = Rasterizer::new(context, wgpu::TextureFormat::Rgba8Unorm);
rasterizer.set_scene(&scene_uniform);

let mesh = rasterizer.create_mesh(&vertices, &indices, &object_uniform, None);
let frame = Frame { clear, meshes: &[&mesh], ..Frame::default() };
let rgba = rasterizer.render_to_rgba(512, 512, &frame);
```

GPU-backed tests skip with a notice on machines without an adapter, matching
the `gpu-compute` convention.

## Benchmark

```
cargo run --release --offline --example bench -p raster-core
```

Renders 200 animated meshes (≈4.8k triangles) at 512×512 with a per-frame
uniform upload per mesh — reference point on Apple M4 Max (Metal):
**~1.3 ms/frame (~750 fps)**.

## Tests

```
cargo test --offline --manifest-path crates/Cargo.toml -p raster-core
```

- `tests/shaders.rs` — naga validation of every shipped shader and variant,
  uniform layout offsets, variant contract anchors.
- `tests/render.rs` — GPU morph blend (weight 0/1/0.5), rest-weight
  semantics, determinism.
- `tests/frame.rs` — frame composition (edges on top, grid axes, line +
  overlay, deep-selection x-ray, instancing, transparent blending) with
  pixel assertions and PPM snapshots written to `output/raster-*.ppm`.

`render_rgba_async` returns an `RgbaReadback` ticket after one submission containing
render and copy commands. Poll `try_read`, cancel it, or use native `wait`.
`try_render_to_rgba` returns transport/size errors; the legacy `render_to_rgba`
convenience panics on failure. RGBA decoding removes padded rows above the shared
`gpu-compute::ByteReadback` transport. Resource upload lives in `resources.rs`;
frame command recording remains in `rasterizer.rs`.
