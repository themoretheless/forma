// WebGPU Compute Shader: Image Contour Tracer (Sub-Pixel Marching Squares).
//
// Evaluates 2D iso-contours directly from packed RGBA8 pixels:
// 1. Evaluates normalized scalar field (darkness = 1 - luma or alpha = A/255) at 4 cell corners.
// 2. Classifies 4-bit Marching Squares case (0..15) against iso_threshold.
// 3. Linearly interpolates sub-pixel boundary crossings along the 4 cell edges.
// 4. Emits oriented 2D contour segments [x0_mm, y0_mm, x1_mm, y1_mm] via atomic append.

struct ContourParams {
  grid_w: u32,
  grid_h: u32,
  mode: u32,          // 0 = dark (1 - luma), 1 = alpha
  max_segments: u32,
  threshold: f32,
  width_mm: f32,
  height_mm: f32,
  _pad0: f32,
};

@group(0) @binding(0) var<uniform> params: ContourParams;
// Packed RGBA8 pixels: 1 u32 per pixel (R | (G << 8) | (B << 16) | (A << 24))
@group(0) @binding(1) var<storage, read> rgba_pixels: array<u32>;
@group(0) @binding(2) var<storage, read_write> segment_counter: array<atomic<u32>, 1>;
// Emitted 2D segments: 4 floats per segment [x0_mm, y0_mm, x1_mm, y1_mm]
@group(0) @binding(3) var<storage, read_write> out_segments: array<f32>;

fn sample_scalar(ix: u32, iy: u32) -> f32 {
  let cx = min(ix, params.grid_w - 1u);
  let cy = min(iy, params.grid_h - 1u);
  let rgba = rgba_pixels[cy * params.grid_w + cx];

  let r = f32(rgba & 0xFFu) / 255.0;
  let g = f32((rgba >> 8u) & 0xFFu) / 255.0;
  let b = f32((rgba >> 16u) & 0xFFu) / 255.0;
  let a = f32((rgba >> 24u) & 0xFFu) / 255.0;

  if (params.mode == 1u) {
    return a;
  }
  // Rec.709 darkness weighted by alpha
  let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return (1.0 - luma) * a;
}

fn interp_frac(v0: f32, v1: f32, iso: f32) -> f32 {
  let denom = v1 - v0;
  if (abs(denom) < 1e-7) {
    return 0.5;
  }
  return clamp((iso - v0) / denom, 0.0, 1.0);
}

fn to_mm(gx: f32, gy: f32) -> vec2<f32> {
  let sx = params.width_mm / max(1.0, f32(params.grid_w - 1u));
  let sy = params.height_mm / max(1.0, f32(params.grid_h - 1u));
  // Flip Y so image top maps to +Y in sketch coordinates
  return vec2<f32>(gx * sx, (f32(params.grid_h - 1u) - gy) * sy);
}

const WG: u32 = 256;
var<workgroup> wg_counter: atomic<u32>;
var<workgroup> wg_base: u32;

@compute @workgroup_size(WG)
fn main(
  @builtin(global_invocation_id) id: vec3<u32>,
  @builtin(local_invocation_id) lid: vec3<u32>
) {
  if (lid.x == 0u) {
    atomicStore(&wg_counter, 0u);
  }
  workgroupBarrier();

  var local_segs: array<vec4<f32>, 2>;
  var local_count = 0u;

  if (params.grid_w >= 2u && params.grid_h >= 2u) {
    let cells_w = params.grid_w - 1u;
    let cells_h = params.grid_h - 1u;
    let total_cells = cells_w * cells_h;
    let cell_idx = id.x;

    if (cell_idx < total_cells) {
      let cx = cell_idx % cells_w;
      let cy = cell_idx / cells_w;

      let v0 = sample_scalar(cx, cy);
      let v1 = sample_scalar(cx + 1u, cy);
      let v2 = sample_scalar(cx + 1u, cy + 1u);
      let v3 = sample_scalar(cx, cy + 1u);

      let iso = params.threshold;
      var mask = 0u;
      if (v0 >= iso) { mask |= 1u; }
      if (v1 >= iso) { mask |= 2u; }
      if (v2 >= iso) { mask |= 4u; }
      if (v3 >= iso) { mask |= 8u; }

      if (mask != 0u && mask != 15u) {
        let fx = f32(cx);
        let fy = f32(cy);

        let e_top = to_mm(fx + interp_frac(v0, v1, iso), fy);
        let e_right = to_mm(fx + 1.0, fy + interp_frac(v1, v2, iso));
        let e_bottom = to_mm(fx + interp_frac(v3, v2, iso), fy + 1.0);
        let e_left = to_mm(fx, fy + interp_frac(v0, v3, iso));

        var p0_a = vec2<f32>(0.0);
        var p1_a = vec2<f32>(0.0);
        var p0_b = vec2<f32>(0.0);
        var p1_b = vec2<f32>(0.0);
        var cand_count = 1u;

        switch (mask) {
          case 1u:  { p0_a = e_left; p1_a = e_top; }
          case 2u:  { p0_a = e_top; p1_a = e_right; }
          case 3u:  { p0_a = e_left; p1_a = e_right; }
          case 4u:  { p0_a = e_right; p1_a = e_bottom; }
          case 5u:  {
            cand_count = 2u;
            let center = 0.25 * (v0 + v1 + v2 + v3);
            if (center >= iso) {
              p0_a = e_left; p1_a = e_bottom;
              p0_b = e_right; p1_b = e_top;
            } else {
              p0_a = e_left; p1_a = e_top;
              p0_b = e_right; p1_b = e_bottom;
            }
          }
          case 6u:  { p0_a = e_top; p1_a = e_bottom; }
          case 7u:  { p0_a = e_left; p1_a = e_bottom; }
          case 8u:  { p0_a = e_bottom; p1_a = e_left; }
          case 9u:  { p0_a = e_bottom; p1_a = e_top; }
          case 10u: {
            cand_count = 2u;
            let center = 0.25 * (v0 + v1 + v2 + v3);
            if (center >= iso) {
              p0_a = e_top; p1_a = e_left;
              p0_b = e_bottom; p1_b = e_right;
            } else {
              p0_a = e_top; p1_a = e_right;
              p0_b = e_bottom; p1_b = e_left;
            }
          }
          case 11u: { p0_a = e_bottom; p1_a = e_right; }
          case 12u: { p0_a = e_right; p1_a = e_left; }
          case 13u: { p0_a = e_right; p1_a = e_top; }
          case 14u: { p0_a = e_top; p1_a = e_left; }
          default: { cand_count = 0u; }
        }

        if (cand_count >= 1u) {
          let da = p1_a - p0_a;
          if (dot(da, da) > 1e-12) {
            local_segs[local_count] = vec4<f32>(p0_a.x, p0_a.y, p1_a.x, p1_a.y);
            local_count += 1u;
          }
        }
        if (cand_count == 2u) {
          let db = p1_b - p0_b;
          if (dot(db, db) > 1e-12) {
            local_segs[local_count] = vec4<f32>(p0_b.x, p0_b.y, p1_b.x, p1_b.y);
            local_count += 1u;
          }
        }
      }
    }
  }

  var local_slot = 0u;
  if (local_count > 0u) {
    local_slot = atomicAdd(&wg_counter, local_count);
  }
  workgroupBarrier();

  if (lid.x == 0u) {
    let wg_total = atomicLoad(&wg_counter);
    if (wg_total == 1u) {
      wg_base = atomicAdd(&segment_counter[0], 1u);
    } else if (wg_total > 1u) {
      wg_base = atomicAdd(&segment_counter[0], wg_total);
    } else {
      wg_base = 0u;
    }
  }
  workgroupBarrier();

  for (var s = 0u; s < local_count; s++) {
    let slot = wg_base + local_slot + s;
    if (slot < params.max_segments) {
      let off = slot * 4u;
      let seg = local_segs[s];
      out_segments[off + 0u] = seg.x;
      out_segments[off + 1u] = seg.y;
      out_segments[off + 2u] = seg.z;
      out_segments[off + 3u] = seg.w;
    }
  }
}
