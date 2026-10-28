// WebGPU Compute Shader: G-Code Toolpath Segment & Layer Filter Accelerator.
//
// Filters and projects packed G-Code move rows (7 floats per move:
// [x, y, z, e, feedrateMmS, layerIndex, extrudedFlag]) into 3D/2D line segments:
// 1. Parallel layer range [min_layer .. max_layer] and travel/extrusion filtering.
// 2. Feedrate normalization [0..1] for thermal/speed color mapping.
// 3. Emits 8 floats per visible segment: [x0, y0, z0, feedrate_norm, x1, y1, z1, extruded_flag].

struct GcodeParams {
  num_moves: u32,
  min_layer: u32,
  max_layer: u32,
  include_travel: u32,
  include_extrusion: u32,
  max_segments: u32,
  min_feedrate: f32,
  max_feedrate: f32,
};

@group(0) @binding(0) var<uniform> params: GcodeParams;
@group(0) @binding(1) var<storage, read> move_rows: array<f32>;
@group(0) @binding(2) var<storage, read_write> segment_counter: array<atomic<u32>, 1>;
@group(0) @binding(3) var<storage, read_write> out_segments: array<f32>;

const ROW_STRIDE: u32 = 7u;
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

  let move_idx = id.x;
  var valid = false;
  var seg_a = vec4<f32>(0.0);
  var seg_b = vec4<f32>(0.0);

  if (move_idx > 0u && move_idx < params.num_moves) {
    let cur_base = move_idx * ROW_STRIDE;
    let prev_base = (move_idx - 1u) * ROW_STRIDE;

    let layer_f = move_rows[cur_base + 5u];
    let layer = u32(max(0.0, round(layer_f)));
    if (layer >= params.min_layer && layer <= params.max_layer) {
      let extruded_flag = move_rows[cur_base + 6u];
      let is_extrusion = extruded_flag >= 0.5;
      let allow_move = select(params.include_travel != 0u, params.include_extrusion != 0u, is_extrusion);
      if (allow_move) {
        let x0 = move_rows[prev_base + 0u];
        let y0 = move_rows[prev_base + 1u];
        let z0 = move_rows[prev_base + 2u];

        let x1 = move_rows[cur_base + 0u];
        let y1 = move_rows[cur_base + 1u];
        let z1 = move_rows[cur_base + 2u];

        let dx = x1 - x0;
        let dy = y1 - y0;
        let dz = z1 - z0;
        if (dx * dx + dy * dy + dz * dz > 1e-12) {
          let feed = move_rows[cur_base + 4u];
          let span = max(1e-6, params.max_feedrate - params.min_feedrate);
          let feed_norm = clamp((feed - params.min_feedrate) / span, 0.0, 1.0);
          seg_a = vec4<f32>(x0, y0, z0, feed_norm);
          seg_b = vec4<f32>(x1, y1, z1, select(0.0, 1.0, is_extrusion));
          valid = true;
        }
      }
    }
  }

  var local_slot = 0u;
  if (valid) {
    local_slot = atomicAdd(&wg_counter, 1u);
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

  if (valid) {
    let slot = wg_base + local_slot;
    if (slot < params.max_segments) {
      let off = slot * 8u;
      out_segments[off + 0u] = seg_a.x;
      out_segments[off + 1u] = seg_a.y;
      out_segments[off + 2u] = seg_a.z;
      out_segments[off + 3u] = seg_a.w;
      out_segments[off + 4u] = seg_b.x;
      out_segments[off + 5u] = seg_b.y;
      out_segments[off + 6u] = seg_b.z;
      out_segments[off + 7u] = seg_b.w;
    }
  }
}
