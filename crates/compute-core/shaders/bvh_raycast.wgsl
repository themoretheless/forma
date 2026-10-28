// WebGPU Compute Shader: Batched BVH Raycaster & Multi-Ray Picking Accelerator.
//
// Evaluates N rays in parallel against a flat linear BVH node array:
// 1. O(log M) iterative stack-based slab traversal per ray thread.
// 2. Overflow-safe slab t_near / t_far calculation with ordered child traversal.
// 3. Emits closest hit [object_id, t_near] per ray (or [0xFFFFFFFFu, +inf] on miss).

struct RaycastParams {
  num_rays: u32,
  num_nodes: u32,
  root_index: u32,
  _pad0: u32,
};

// Flat BVH node (32 bytes = 8 floats/u32s):
// min_x, min_y, min_z, left_or_obj (as f32 bits or u32)
// max_x, max_y, max_z, right_or_flag (0xFFFFFFFFu if leaf, else right child index)
struct BvhNode {
  min_x: f32,
  min_y: f32,
  min_z: f32,
  left_or_obj: u32,
  max_x: f32,
  max_y: f32,
  max_z: f32,
  right_or_leaf: u32,
};

// Ray input (32 bytes = 8 floats):
// ox, oy, oz, t_min, dx, dy, dz, t_max
struct RayInput {
  ox: f32,
  oy: f32,
  oz: f32,
  t_min: f32,
  dx: f32,
  dy: f32,
  dz: f32,
  t_max: f32,
};

struct RayHitOutput {
  object_id: u32,
  t_hit: f32,
};

@group(0) @binding(0) var<uniform> params: RaycastParams;
@group(0) @binding(1) var<storage, read> nodes: array<BvhNode>;
@group(0) @binding(2) var<storage, read> rays: array<RayInput>;
@group(0) @binding(3) var<storage, read_write> hits: array<RayHitOutput>;

fn slab_test(
  node: BvhNode,
  ox: f32, oy: f32, oz: f32,
  dx: f32, dy: f32, dz: f32,
  inv_dir: vec3<f32>,
  t_min_in: f32,
  t_max_in: f32
) -> f32 {
  var t_near = t_min_in;
  var t_far = t_max_in;

  // X slab
  if (abs(dx) < 1e-12) {
    if (ox < node.min_x || ox > node.max_x) { return -1.0; }
  } else {
    let t0 = (node.min_x - ox) * inv_dir.x;
    let t1 = (node.max_x - ox) * inv_dir.x;
    t_near = max(t_near, min(t0, t1));
    t_far = min(t_far, max(t0, t1));
    if (t_near > t_far) { return -1.0; }
  }

  // Y slab
  if (abs(dy) < 1e-12) {
    if (oy < node.min_y || oy > node.max_y) { return -1.0; }
  } else {
    let t0 = (node.min_y - oy) * inv_dir.y;
    let t1 = (node.max_y - oy) * inv_dir.y;
    t_near = max(t_near, min(t0, t1));
    t_far = min(t_far, max(t0, t1));
    if (t_near > t_far) { return -1.0; }
  }

  // Z slab
  if (abs(dz) < 1e-12) {
    if (oz < node.min_z || oz > node.max_z) { return -1.0; }
  } else {
    let t0 = (node.min_z - oz) * inv_dir.z;
    let t1 = (node.max_z - oz) * inv_dir.z;
    t_near = max(t_near, min(t0, t1));
    t_far = min(t_far, max(t0, t1));
    if (t_near > t_far) { return -1.0; }
  }

  return t_near;
}

const WG: u32 = 256;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let ray_idx = id.x;
  if (ray_idx >= params.num_rays) {
    return;
  }

  if (params.num_nodes == 0u) {
    hits[ray_idx] = RayHitOutput(0xFFFFFFFFu, 1e30);
    return;
  }

  let ray = rays[ray_idx];
  let len_sq = ray.dx * ray.dx + ray.dy * ray.dy + ray.dz * ray.dz;
  if (len_sq <= 0.0) {
    hits[ray_idx] = RayHitOutput(0xFFFFFFFFu, 1e30);
    return;
  }

  let inv_dir = vec3<f32>(
    select(0.0, 1.0 / ray.dx, abs(ray.dx) >= 1e-12),
    select(0.0, 1.0 / ray.dy, abs(ray.dy) >= 1e-12),
    select(0.0, 1.0 / ray.dz, abs(ray.dz) >= 1e-12)
  );
  let t_min_clamped = max(0.0, ray.t_min);

  var best_t = ray.t_max;
  var best_obj = 0xFFFFFFFFu;

  var stack: array<u32, 32>;
  var stack_ptr = 0u;
  stack[0] = params.root_index;
  stack_ptr = 1u;

  while (stack_ptr > 0u) {
    stack_ptr -= 1u;
    let node_idx = stack[stack_ptr];
    if (node_idx >= params.num_nodes) {
      continue;
    }

    let node = nodes[node_idx];
    let t_entry = slab_test(
      node,
      ray.ox, ray.oy, ray.oz,
      ray.dx, ray.dy, ray.dz,
      inv_dir,
      t_min_clamped,
      best_t
    );

    if (t_entry < 0.0 || t_entry > best_t) {
      continue;
    }

    if (node.right_or_leaf == 0xFFFFFFFFu) {
      // Leaf node
      let obj_id = node.left_or_obj;
      if (t_entry < best_t || (t_entry == best_t && obj_id < best_obj)) {
        best_t = t_entry;
        best_obj = obj_id;
      }
    } else {
      // Internal node: push children with nearer child on top of stack
      let left_idx = node.left_or_obj;
      let right_idx = node.right_or_leaf;

      let left_node = nodes[left_idx];
      let right_node = nodes[right_idx];

      let t_left = slab_test(
        left_node,
        ray.ox, ray.oy, ray.oz,
        ray.dx, ray.dy, ray.dz,
        inv_dir,
        t_min_clamped,
        best_t
      );
      let t_right = slab_test(
        right_node,
        ray.ox, ray.oy, ray.oz,
        ray.dx, ray.dy, ray.dz,
        inv_dir,
        t_min_clamped,
        best_t
      );

      let hit_left = t_left >= 0.0 && t_left <= best_t;
      let hit_right = t_right >= 0.0 && t_right <= best_t;

      if (hit_left && hit_right) {
        if (t_left <= t_right) {
          if (stack_ptr + 2u <= 32u) {
            stack[stack_ptr] = right_idx;
            stack[stack_ptr + 1u] = left_idx;
            stack_ptr += 2u;
          }
        } else {
          if (stack_ptr + 2u <= 32u) {
            stack[stack_ptr] = left_idx;
            stack[stack_ptr + 1u] = right_idx;
            stack_ptr += 2u;
          }
        }
      } else if (hit_left) {
        if (stack_ptr + 1u <= 32u) {
          stack[stack_ptr] = left_idx;
          stack_ptr += 1u;
        }
      } else if (hit_right) {
        if (stack_ptr + 1u <= 32u) {
          stack[stack_ptr] = right_idx;
          stack_ptr += 1u;
        }
      }
    }
  }

  hits[ray_idx] = RayHitOutput(best_obj, best_t);
}
