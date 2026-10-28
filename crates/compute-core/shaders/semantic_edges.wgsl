// WebGPU Compute Shader: Semantic Edges & Crease Angle Detection.
//
// Classifies mesh edges into:
// 1. Boundary edges (silhouette / open boundary where only one face is incident).
// 2. Crease / feature edges (dihedral angle between adjacent faces exceeds threshold).
// Smooth interior edges (dihedral dot product >= threshold) are filtered out.

struct EdgeParams {
  total_edges: u32,
  total_triangles: u32,
  crease_dot_threshold: f32, // e.g. cos(30 deg) ~= 0.866; dot < threshold => sharp crease
  stride_floats: u32,        // Vertex buffer stride in floats (e.g. 3 for XYZ, 6 for XYZ+Norm, 8 for XYZ+Norm+UV)
  pos_offset: u32,           // Float offset of XYZ in vertex buffer
  max_output_edges: u32,
  _pad1: u32,
  _pad2: u32,
};

// Edge definition:
// v0, v1: Vertex indices of the edge
// tri0: First incident triangle index
// tri1: Second incident triangle index (or 0xFFFFFFFFu if open boundary edge)
struct EdgeInput {
  v0: u32,
  v1: u32,
  tri0: u32,
  tri1: u32,
};

@group(0) @binding(0) var<uniform> params: EdgeParams;
@group(0) @binding(1) var<storage, read> vertices: array<f32>;
@group(0) @binding(2) var<storage, read> indices: array<u32>;
@group(0) @binding(3) var<storage, read> edges: array<EdgeInput>;
@group(0) @binding(4) var<storage, read_write> edge_counter: array<atomic<u32>, 1>;
// Output buffer: Pairs of vertex indices [v0, v1] for each emitted semantic edge
@group(0) @binding(5) var<storage, read_write> out_edge_indices: array<u32>;

fn get_vertex_pos(v_idx: u32) -> vec3<f32> {
  let base = v_idx * params.stride_floats + params.pos_offset;
  return vec3<f32>(
    vertices[base + 0u],
    vertices[base + 1u],
    vertices[base + 2u]
  );
}

fn compute_triangle_normal(tri_idx: u32) -> vec3<f32> {
  let i0 = indices[tri_idx * 3u + 0u];
  let i1 = indices[tri_idx * 3u + 1u];
  let i2 = indices[tri_idx * 3u + 2u];

  let p0 = get_vertex_pos(i0);
  let p1 = get_vertex_pos(i1);
  let p2 = get_vertex_pos(i2);

  let d1 = p1 - p0;
  let d2 = p2 - p0;
  let n = cross(d1, d2);
  let len_sq = dot(n, n);
  if (len_sq > 1e-14) {
    return n * inverseSqrt(len_sq);
  }
  return vec3<f32>(0.0, 0.0, 1.0);
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

  let edge_idx = id.x;
  var is_semantic = false;
  var edge = EdgeInput(0u, 0u, 0u, 0u);

  if (edge_idx < params.total_edges) {
    edge = edges[edge_idx];
    if (edge.tri1 == 0xFFFFFFFFu) {
      // 1. Boundary edge (only 1 incident triangle)
      is_semantic = true;
    } else {
      // 2. Manifold interior edge: inspect dihedral angle
      let n0 = compute_triangle_normal(edge.tri0);
      let n1 = compute_triangle_normal(edge.tri1);
      let dot_prod = dot(n0, n1);

      if (dot_prod < params.crease_dot_threshold) {
        is_semantic = true;
      }
    }
  }

  var local_slot = 0u;
  if (is_semantic) {
    local_slot = atomicAdd(&wg_counter, 1u);
  }
  workgroupBarrier();

  if (lid.x == 0u) {
    let wg_total = atomicLoad(&wg_counter);
    if (wg_total == 1u) {
      wg_base = atomicAdd(&edge_counter[0], 1u);
    } else if (wg_total > 1u) {
      wg_base = atomicAdd(&edge_counter[0], wg_total);
    } else {
      wg_base = 0u;
    }
  }
  workgroupBarrier();

  if (is_semantic) {
    let slot = wg_base + local_slot;
    if (slot < params.max_output_edges) {
      let out_off = slot * 2u;
      out_edge_indices[out_off + 0u] = edge.v0;
      out_edge_indices[out_off + 1u] = edge.v1;
    }
  }
}
