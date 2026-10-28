// WebGPU Compute Marching Tetrahedra: six-tetrahedron cube decomposition with atomic triangle emission.
struct GridUniform {
  nx: u32,
  ny: u32,
  nz: u32,
  total_cells: u32,
  min_x: f32,
  min_y: f32,
  min_z: f32,
  iso_level: f32,
  step_x: f32,
  step_y: f32,
  step_z: f32,
  max_triangles: u32,
}

@group(0) @binding(0) var<uniform> grid: GridUniform;
@group(0) @binding(1) var<storage, read> field: array<f32>;
// Atomic counter for emitted triangles
@group(0) @binding(2) var<storage, read_write> tri_counter: array<atomic<u32>, 1>;
// Emitted vertex buffer: x, y, z, nx, ny, nz (stride 6 floats)
@group(0) @binding(3) var<storage, read_write> out_vertices: array<f32>;
// Emitted index buffer: 3 u32 per triangle
@group(0) @binding(4) var<storage, read_write> out_indices: array<u32>;

const CORNER_OFFSET = array<vec3<u32>, 8>(
  vec3<u32>(0u, 0u, 0u),
  vec3<u32>(1u, 0u, 0u),
  vec3<u32>(0u, 1u, 0u),
  vec3<u32>(1u, 1u, 0u),
  vec3<u32>(0u, 0u, 1u),
  vec3<u32>(1u, 0u, 1u),
  vec3<u32>(0u, 1u, 1u),
  vec3<u32>(1u, 1u, 1u)
);

const CUBE_TETS = array<vec4<u32>, 6>(
  vec4<u32>(0u, 1u, 3u, 7u),
  vec4<u32>(0u, 3u, 2u, 7u),
  vec4<u32>(0u, 2u, 6u, 7u),
  vec4<u32>(0u, 6u, 4u, 7u),
  vec4<u32>(0u, 4u, 5u, 7u),
  vec4<u32>(0u, 5u, 1u, 7u)
);

fn field_index(x: u32, y: u32, z: u32) -> u32 {
  let row = grid.nx + 1u;
  let slice = row * (grid.ny + 1u);
  return x + y * row + z * slice;
}

fn corner_pos(x: u32, y: u32, z: u32, c: u32) -> vec3<f32> {
  let off = CORNER_OFFSET[c];
  return vec3<f32>(
    grid.min_x + f32(x + off.x) * grid.step_x,
    grid.min_y + f32(y + off.y) * grid.step_y,
    grid.min_z + f32(z + off.z) * grid.step_z
  );
}

fn interpolate_point(p0: vec3<f32>, v0: f32, p1: vec3<f32>, v1: f32) -> vec3<f32> {
  let denom = v1 - v0;
  if (abs(denom) < 1e-6) {
    return (p0 + p1) * 0.5;
  }
  let t = clamp((grid.iso_level - v0) / denom, 0.0, 1.0);
  return p0 + t * (p1 - p0);
}

fn emit_tri(v0: vec3<f32>, v1_in: vec3<f32>, v2_in: vec3<f32>, hint: vec3<f32>) {
  let slot = atomicAdd(&tri_counter[0], 1u);
  if (slot >= grid.max_triangles) {
    return;
  }

  var v1 = v1_in;
  var v2 = v2_in;
  let d1 = v1 - v0;
  let d2 = v2 - v0;
  var norm = cross(d1, d2);
  if (dot(norm, hint) < 0.0) {
    let tmp = v1;
    v1 = v2;
    v2 = tmp;
    norm = -norm;
  }
  let len_sq = dot(norm, norm);
  if (len_sq > 1e-12) {
    norm = norm * inverseSqrt(len_sq);
  } else {
    norm = vec3<f32>(0.0, 0.0, 1.0);
  }

  let base_v = slot * 3u;
  let v_off = base_v * 6u;

  // Vertex 0
  out_vertices[v_off + 0u] = v0.x;
  out_vertices[v_off + 1u] = v0.y;
  out_vertices[v_off + 2u] = v0.z;
  out_vertices[v_off + 3u] = norm.x;
  out_vertices[v_off + 4u] = norm.y;
  out_vertices[v_off + 5u] = norm.z;

  // Vertex 1
  out_vertices[v_off + 6u] = v1.x;
  out_vertices[v_off + 7u] = v1.y;
  out_vertices[v_off + 8u] = v1.z;
  out_vertices[v_off + 9u] = norm.x;
  out_vertices[v_off + 10u] = norm.y;
  out_vertices[v_off + 11u] = norm.z;

  // Vertex 2
  out_vertices[v_off + 12u] = v2.x;
  out_vertices[v_off + 13u] = v2.y;
  out_vertices[v_off + 14u] = v2.z;
  out_vertices[v_off + 15u] = norm.x;
  out_vertices[v_off + 16u] = norm.y;
  out_vertices[v_off + 17u] = norm.z;

  let i_off = slot * 3u;
  out_indices[i_off + 0u] = base_v + 0u;
  out_indices[i_off + 1u] = base_v + 1u;
  out_indices[i_off + 2u] = base_v + 2u;
}

const WG: u32 = 256;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let cell_idx = id.x;
  if (cell_idx >= grid.total_cells) {
    return;
  }

  let x = cell_idx % grid.nx;
  let y = (cell_idx / grid.nx) % grid.ny;
  let z = cell_idx / (grid.nx * grid.ny);

  // Read corner values
  var cv: array<f32, 8>;
  var has_neg = false;
  var has_pos = false;
  for (var c = 0u; c < 8u; c++) {
    let off = CORNER_OFFSET[c];
    let val = field[field_index(x + off.x, y + off.y, z + off.z)];
    cv[c] = val;
    if (val < grid.iso_level) { has_neg = true; } else { has_pos = true; }
  }

  // Early out if all corners lie on the same side
  if (!has_neg || !has_pos) {
    return;
  }

  var cp: array<vec3<f32>, 8>;
  for (var c = 0u; c < 8u; c++) {
    cp[c] = corner_pos(x, y, z, c);
  }

  // Decompose cube into 6 tetrahedra
  for (var t = 0u; t < 6u; t++) {
    let tet = CUBE_TETS[t];
    let v0 = cv[tet.x];
    let v1 = cv[tet.y];
    let v2 = cv[tet.z];
    let v3 = cv[tet.w];

    let p0 = cp[tet.x];
    let p1 = cp[tet.y];
    let p2 = cp[tet.z];
    let p3 = cp[tet.w];

    // Classify corners into negative (inside) and positive (outside)
    var neg: array<u32, 4>;
    var pos: array<u32, 4>;
    var neg_len = 0u;
    var pos_len = 0u;

    if (v0 < grid.iso_level) { neg[neg_len] = 0u; neg_len++; } else { pos[pos_len] = 0u; pos_len++; }
    if (v1 < grid.iso_level) { neg[neg_len] = 1u; neg_len++; } else { pos[pos_len] = 1u; pos_len++; }
    if (v2 < grid.iso_level) { neg[neg_len] = 2u; neg_len++; } else { pos[pos_len] = 2u; pos_len++; }
    if (v3 < grid.iso_level) { neg[neg_len] = 3u; neg_len++; } else { pos[pos_len] = 3u; pos_len++; }

    if (neg_len == 0u || pos_len == 0u) {
      continue;
    }

    let p_arr = array<vec3<f32>, 4>(p0, p1, p2, p3);
    let v_arr = array<f32, 4>(v0, v1, v2, v3);

    var hint = vec3<f32>(0.0);
    let inv_pos = 1.0 / f32(pos_len);
    for (var i = 0u; i < pos_len; i++) {
      hint += p_arr[pos[i]] * inv_pos;
    }
    let inv_neg = 1.0 / f32(neg_len);
    for (var i = 0u; i < neg_len; i++) {
      hint -= p_arr[neg[i]] * inv_neg;
    }

    if (neg_len == 1u && pos_len == 3u) {
      let a = neg[0];
      let c0 = pos[0];
      let c1 = pos[1];
      let c2 = pos[2];
      emit_tri(
        interpolate_point(p_arr[a], v_arr[a], p_arr[c0], v_arr[c0]),
        interpolate_point(p_arr[a], v_arr[a], p_arr[c1], v_arr[c1]),
        interpolate_point(p_arr[a], v_arr[a], p_arr[c2], v_arr[c2]),
        hint
      );
    } else if (neg_len == 3u && pos_len == 1u) {
      let a = pos[0];
      let c0 = neg[0];
      let c1 = neg[1];
      let c2 = neg[2];
      emit_tri(
        interpolate_point(p_arr[a], v_arr[a], p_arr[c0], v_arr[c0]),
        interpolate_point(p_arr[a], v_arr[a], p_arr[c1], v_arr[c1]),
        interpolate_point(p_arr[a], v_arr[a], p_arr[c2], v_arr[c2]),
        hint
      );
    } else if (neg_len == 2u && pos_len == 2u) {
      let a = neg[0];
      let b = neg[1];
      let c = pos[0];
      let d = pos[1];

      let ac = interpolate_point(p_arr[a], v_arr[a], p_arr[c], v_arr[c]);
      let bc = interpolate_point(p_arr[b], v_arr[b], p_arr[c], v_arr[c]);
      let ad = interpolate_point(p_arr[a], v_arr[a], p_arr[d], v_arr[d]);
      let bd = interpolate_point(p_arr[b], v_arr[b], p_arr[d], v_arr[d]);

      emit_tri(ac, bc, bd, hint);
      emit_tri(ac, bd, ad, hint);
    }
  }
}
