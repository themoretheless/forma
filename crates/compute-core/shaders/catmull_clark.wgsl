// WebGPU Compute Shader: Catmull-Clark Subdivision Stencil & Sculpting Kernel.
//
// Evaluates Catmull-Clark vertex refinement and brush sculpting displacements
// in parallel across cage vertices, edges, and faces:
// - mode 0: Face point pass (centroid of face corners)
// - mode 1: Edge point pass (average of edge endpoints and incident face points)
// - mode 2: Vertex point pass ((F + 2R + (n-3)P)/n for interior, (6P + B0 + B1)/8 for boundary)
// - mode 3: Brush / Sculpting displacement pass with radial falloff (smoothstep / linear / constant)

struct SubdivParams {
  mode: u32,
  num_items: u32,
  brush_falloff: u32, // 0 = smoothstep, 1 = linear, 2 = constant
  brush_kind: u32,    // 0 = grab/translate, 1 = inflate, 2 = pinch
  brush_cx: f32,
  brush_cy: f32,
  brush_cz: f32,
  brush_radius: f32,
  brush_dx: f32,
  brush_dy: f32,
  brush_dz: f32,
  brush_strength: f32,
};

@group(0) @binding(0) var<uniform> params: SubdivParams;
@group(0) @binding(1) var<storage, read> in_positions: array<f32>;
@group(0) @binding(2) var<storage, read> aux_positions: array<f32>;
// Stencil table:
// - mode 0 (face): [offset, count] per face, followed by flat corner vertex indices in stencil_indices
// - mode 1 (edge): [v0, v1, f0, f1] per edge (f1 = 0xFFFFFFFFu for boundary edge)
// - mode 2 (vertex): [edge_offset, edge_count, face_offset, face_count, b0, b1] per vertex (6 u32s)
// - mode 3 (sculpt): normal vectors stored in aux_positions
@group(0) @binding(3) var<storage, read> stencil_meta: array<u32>;
@group(0) @binding(4) var<storage, read> stencil_indices: array<u32>;
@group(0) @binding(5) var<storage, read_write> out_positions: array<f32>;

fn get_pos(buf_idx: u32) -> vec3<f32> {
  let b = buf_idx * 3u;
  return vec3<f32>(in_positions[b + 0u], in_positions[b + 1u], in_positions[b + 2u]);
}

fn get_aux(buf_idx: u32) -> vec3<f32> {
  let b = buf_idx * 3u;
  return vec3<f32>(aux_positions[b + 0u], aux_positions[b + 1u], aux_positions[b + 2u]);
}

fn write_out(idx: u32, p: vec3<f32>) {
  let b = idx * 3u;
  out_positions[b + 0u] = p.x;
  out_positions[b + 1u] = p.y;
  out_positions[b + 2u] = p.z;
}

fn eval_falloff(dist: f32, radius: f32, kind: u32) -> f32 {
  if (dist >= radius || radius <= 1e-9) {
    return 0.0;
  }
  if (kind == 2u) {
    return 1.0;
  }
  let u = 1.0 - dist / radius;
  if (kind == 1u) {
    return u;
  }
  // Smoothstep falloff: u^2 * (3 - 2u)
  return u * u * (3.0 - 2.0 * u);
}

const WG: u32 = 256;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let idx = id.x;
  if (idx >= params.num_items) {
    return;
  }

  if (params.mode == 0u) {
    // Face point pass: average of face corner vertices
    let start = stencil_meta[idx * 2u + 0u];
    let count = stencil_meta[idx * 2u + 1u];
    var sum = vec3<f32>(0.0, 0.0, 0.0);
    if (count > 0u) {
      let inv_n = 1.0 / f32(count);
      for (var i = 0u; i < count; i++) {
        let v_idx = stencil_indices[start + i];
        sum += get_pos(v_idx) * inv_n;
      }
    }
    write_out(idx, sum);
    return;
  }

  if (params.mode == 1u) {
    // Edge point pass: average of (v0, v1) or (v0, v1, fp0, fp1)
    let v0 = stencil_meta[idx * 4u + 0u];
    let v1 = stencil_meta[idx * 4u + 1u];
    let f0 = stencil_meta[idx * 4u + 2u];
    let f1 = stencil_meta[idx * 4u + 3u];

    let p0 = get_pos(v0);
    let p1 = get_pos(v1);
    if (f1 == 0xFFFFFFFFu) {
      write_out(idx, (p0 + p1) * 0.5);
    } else {
      let fp0 = get_aux(f0);
      let fp1 = get_aux(f1);
      write_out(idx, (p0 + p1 + fp0 + fp1) * 0.25);
    }
    return;
  }

  if (params.mode == 2u) {
    // Vertex point pass:
    // stencil_meta has 6 u32s per vertex:
    // [nbr_offset, nbr_count, face_offset, face_count, b0, b1]
    let p = get_pos(idx);
    let nbr_off = stencil_meta[idx * 6u + 0u];
    let nbr_cnt = stencil_meta[idx * 6u + 1u];
    let face_off = stencil_meta[idx * 6u + 2u];
    let face_cnt = stencil_meta[idx * 6u + 3u];
    let b0 = stencil_meta[idx * 6u + 4u];
    let b1 = stencil_meta[idx * 6u + 5u];

    if (b0 != 0xFFFFFFFFu && b1 != 0xFFFFFFFFu) {
      // Boundary rule: (6 * P + B0 + B1) / 8
      let pb0 = get_pos(b0);
      let pb1 = get_pos(b1);
      write_out(idx, (6.0 * p + pb0 + pb1) * 0.125);
    } else if (nbr_cnt > 0u && face_cnt > 0u) {
      // Interior rule: (F + 2 * R + (n - 3) * P) / n
      var f_avg = vec3<f32>(0.0, 0.0, 0.0);
      let inv_f = 1.0 / f32(face_cnt);
      for (var i = 0u; i < face_cnt; i++) {
        let fi = stencil_indices[face_off + i];
        f_avg += get_aux(fi) * inv_f;
      }

      var r_avg = vec3<f32>(0.0, 0.0, 0.0);
      let inv_n = 1.0 / f32(nbr_cnt);
      for (var i = 0u; i < nbr_cnt; i++) {
        let ni = stencil_indices[nbr_off + i];
        r_avg += 0.5 * (p + get_pos(ni)) * inv_n;
      }

      let n_f = f32(nbr_cnt);
      write_out(idx, (f_avg + 2.0 * r_avg + (n_f - 3.0) * p) / n_f);
    } else {
      write_out(idx, p);
    }
    return;
  }

  // mode 3: Brush / Sculpting displacement pass
  let p = get_pos(idx);
  let center = vec3<f32>(params.brush_cx, params.brush_cy, params.brush_cz);
  let diff = p - center;
  let dist = length(diff);
  let w = eval_falloff(dist, params.brush_radius, params.brush_falloff);

  if (w <= 0.0) {
    write_out(idx, p);
    return;
  }

  if (params.brush_kind == 0u) {
    // Grab / Translate brush
    let disp = vec3<f32>(params.brush_dx, params.brush_dy, params.brush_dz);
    write_out(idx, p + w * disp);
  } else if (params.brush_kind == 1u) {
    // Inflate along vertex normal stored in aux_positions
    let n = get_aux(idx);
    write_out(idx, p + (w * params.brush_strength) * n);
  } else {
    // Pinch toward brush center
    write_out(idx, p - (w * params.brush_strength) * diff);
  }
}
