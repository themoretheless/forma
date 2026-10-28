// WebGPU Compute Shader: Parallel NURBS & B-Spline Surface Evaluation & Tessellation.
//
// Evaluates Cox-de Boor basis functions and partial derivatives on GPU,
// emitting watertight vertex positions, surface normals, UV parameters,
// and triangle index buffers directly into VRAM.

struct NurbsUniform {
  degree_u: u32,
  degree_v: u32,
  num_cp_u: u32,
  num_cp_v: u32,
  num_knots_u: u32,
  num_knots_v: u32,
  samples_u: u32,
  samples_v: u32,
  u_min: f32,
  u_max: f32,
  v_min: f32,
  v_max: f32,
  is_rational: u32,
  total_vertices: u32,
  total_quads: u32,
  _pad: u32,
};

@group(0) @binding(0) var<uniform> params: NurbsUniform;
@group(0) @binding(1) var<storage, read> knots_u: array<f32>;
@group(0) @binding(2) var<storage, read> knots_v: array<f32>;
// Control points stored as 4 floats (x, y, z, w) where w is the rational weight
@group(0) @binding(3) var<storage, read> control_points: array<vec4<f32>>;
// Output vertex buffer: x, y, z, nx, ny, nz, u, v (stride 8 floats)
@group(0) @binding(4) var<storage, read_write> out_vertices: array<f32>;
// Output index buffer: 6 u32 indices per quad (2 triangles)
@group(0) @binding(5) var<storage, read_write> out_indices: array<u32>;

// Binary search for knot span: returns index i such that knots[i] <= u < knots[i+1]
fn find_knot_span_u(u: f32) -> u32 {
  let p = params.degree_u;
  let n = params.num_cp_u - 1u;
  if (u >= knots_u[n + 1u]) {
    return n;
  }
  if (u <= knots_u[p]) {
    return p;
  }
  var low = p;
  var high = n + 1u;
  var mid = (low + high) / 2u;
  while (u < knots_u[mid] || u >= knots_u[mid + 1u]) {
    if (u < knots_u[mid]) {
      high = mid;
    } else {
      low = mid;
    }
    mid = (low + high) / 2u;
  }
  return mid;
}

fn find_knot_span_v(v: f32) -> u32 {
  let q = params.degree_v;
  let m = params.num_cp_v - 1u;
  if (v >= knots_v[m + 1u]) {
    return m;
  }
  if (v <= knots_v[q]) {
    return q;
  }
  var low = q;
  var high = m + 1u;
  var mid = (low + high) / 2u;
  while (v < knots_v[mid] || v >= knots_v[mid + 1u]) {
    if (v < knots_v[mid]) {
      high = mid;
    } else {
      low = mid;
    }
    mid = (low + high) / 2u;
  }
  return mid;
}

struct BasisDeriv {
  basis: array<f32, 8>,
  ders: array<f32, 8>,
};

// Cox-de Boor basis function and first derivative evaluation (The NURBS Book A2.2 / A2.3)
fn eval_basis_u(span: u32, u: f32) -> BasisDeriv {
  var res: BasisDeriv;
  let p = params.degree_u;
  var ndu: array<f32, 64>; // (p+1) x (p+1) stored as row * 8 + col
  var left: array<f32, 8>;
  var right: array<f32, 8>;

  ndu[0] = 1.0;
  for (var j = 1u; j <= p; j++) {
    left[j] = u - knots_u[span + 1u - j];
    right[j] = knots_u[span + j] - u;
    var saved = 0.0;
    for (var r = 0u; r < j; r++) {
      let denom = right[r + 1u] + left[j - r];
      ndu[j * 8u + r] = denom;
      var temp = 0.0;
      if (abs(denom) > 1e-12) {
        temp = ndu[r * 8u + (j - 1u)] / denom;
      }
      ndu[r * 8u + j] = saved + right[r + 1u] * temp;
      saved = left[j - r] * temp;
    }
    ndu[j * 8u + j] = saved;
  }

  // Basis values (order 0)
  for (var j = 0u; j <= p; j++) {
    res.basis[j] = ndu[j * 8u + p];
  }

  // 1st derivative (order 1)
  // 1st derivative (order 1)
  let fp = f32(p);
  for (var r = 0u; r <= p; r++) {
    var d = 0.0;
    if (r == 0u) {
      let den = ndu[p * 8u + 0u];
      if (abs(den) > 1e-12) {
        d = -fp * ndu[0u * 8u + (p - 1u)] / den;
      }
    } else if (r == p) {
      let den = ndu[p * 8u + (p - 1u)];
      if (abs(den) > 1e-12) {
        d = fp * ndu[(p - 1u) * 8u + (p - 1u)] / den;
      }
    } else {
      let den1 = ndu[p * 8u + (r - 1u)];
      let den2 = ndu[p * 8u + r];
      let t1 = select(0.0, fp * ndu[(r - 1u) * 8u + (p - 1u)] / den1, abs(den1) > 1e-12);
      let t2 = select(0.0, fp * ndu[r * 8u + (p - 1u)] / den2, abs(den2) > 1e-12);
      d = t1 - t2;
    }
    res.ders[r] = d;
  }

  return res;
}

fn eval_basis_v(span: u32, v: f32) -> BasisDeriv {
  var res: BasisDeriv;
  let q = params.degree_v;
  var ndu: array<f32, 64>;
  var left: array<f32, 8>;
  var right: array<f32, 8>;

  ndu[0] = 1.0;
  for (var j = 1u; j <= q; j++) {
    left[j] = v - knots_v[span + 1u - j];
    right[j] = knots_v[span + j] - v;
    var saved = 0.0;
    for (var r = 0u; r < j; r++) {
      let denom = right[r + 1u] + left[j - r];
      ndu[j * 8u + r] = denom;
      var temp = 0.0;
      if (abs(denom) > 1e-12) {
        temp = ndu[r * 8u + (j - 1u)] / denom;
      }
      ndu[r * 8u + j] = saved + right[r + 1u] * temp;
      saved = left[j - r] * temp;
    }
    ndu[j * 8u + j] = saved;
  }

  for (var j = 0u; j <= q; j++) {
    res.basis[j] = ndu[j * 8u + q];
  }

  let fq = f32(q);
  for (var r = 0u; r <= q; r++) {
    var d = 0.0;
    if (r == 0u) {
      let den = ndu[q * 8u + 0u];
      if (abs(den) > 1e-12) {
        d = -fq * ndu[0u * 8u + (q - 1u)] / den;
      }
    } else if (r == q) {
      let den = ndu[q * 8u + (q - 1u)];
      if (abs(den) > 1e-12) {
        d = fq * ndu[(q - 1u) * 8u + (q - 1u)] / den;
      }
    } else {
      let den1 = ndu[q * 8u + (r - 1u)];
      let den2 = ndu[q * 8u + r];
      let t1 = select(0.0, fq * ndu[(r - 1u) * 8u + (q - 1u)] / den1, abs(den1) > 1e-12);
      let t2 = select(0.0, fq * ndu[r * 8u + (q - 1u)] / den2, abs(den2) > 1e-12);
      d = t1 - t2;
    }
    res.ders[r] = d;
  }

  return res;
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let iu = id.x;
  let iv = id.y;

  // 1. Evaluate surface vertex if within grid
  if (iu < params.samples_u && iv < params.samples_v) {
    let tu = f32(iu) / f32(params.samples_u - 1u);
    let tv = f32(iv) / f32(params.samples_v - 1u);

    let u = params.u_min + tu * (params.u_max - params.u_min);
    let v = params.v_min + tv * (params.v_max - params.v_min);

    let span_u = find_knot_span_u(u);
    let span_v = find_knot_span_v(v);

    let bu = eval_basis_u(span_u, u);
    let bv = eval_basis_v(span_v, v);

    let p = params.degree_u;
    let q = params.degree_v;

    var pt_homo = vec4<f32>(0.0);
    var du_homo = vec4<f32>(0.0);
    var dv_homo = vec4<f32>(0.0);

    for (var r = 0u; r <= p; r++) {
      let cp_x = span_u - p + r;
      let nu = bu.basis[r];
      let dnu = bu.ders[r];

      for (var s = 0u; s <= q; s++) {
        let cp_y = span_v - q + s;
        let mv = bv.basis[s];
        let dmv = bv.ders[s];

        let cp_idx = cp_x + cp_y * params.num_cp_u;
        var cp = control_points[cp_idx];

        if (params.is_rational == 0u) {
          cp.w = 1.0;
        } else {
          // cp is (x, y, z, w), convert to homogeneous (w*x, w*y, w*z, w)
          cp = vec4<f32>(cp.x * cp.w, cp.y * cp.w, cp.z * cp.w, cp.w);
        }

        let w_pt = nu * mv;
        let w_du = dnu * mv;
        let w_dv = nu * dmv;

        pt_homo += cp * w_pt;
        du_homo += cp * w_du;
        dv_homo += cp * w_dv;
      }
    }

    var pos: vec3<f32>;
    var du: vec3<f32>;
    var dv: vec3<f32>;

    if (params.is_rational == 0u || abs(pt_homo.w) < 1e-12) {
      pos = pt_homo.xyz;
      du = du_homo.xyz;
      dv = dv_homo.xyz;
    } else {
      let inv_w = 1.0 / pt_homo.w;
      pos = pt_homo.xyz * inv_w;
      // Quotient rule: (A' * w - A * w') / w^2 = (A' - pos * w') / w
      du = (du_homo.xyz - pos * du_homo.w) * inv_w;
      dv = (dv_homo.xyz - pos * dv_homo.w) * inv_w;
    }

    var normal = cross(du, dv);
    let len = length(normal);
    if (len > 1e-7) {
      normal = normal / len;
    } else {
      normal = vec3<f32>(0.0, 0.0, 1.0);
    }

    let v_idx = iu + iv * params.samples_u;
    let v_off = v_idx * 8u;

    out_vertices[v_off + 0u] = pos.x;
    out_vertices[v_off + 1u] = pos.y;
    out_vertices[v_off + 2u] = pos.z;
    out_vertices[v_off + 3u] = normal.x;
    out_vertices[v_off + 4u] = normal.y;
    out_vertices[v_off + 5u] = normal.z;
    out_vertices[v_off + 6u] = tu;
    out_vertices[v_off + 7u] = tv;
  }

  // 2. Generate quad triangle indices
  if (iu < (params.samples_u - 1u) && iv < (params.samples_v - 1u)) {
    let q_idx = iu + iv * (params.samples_u - 1u);
    let i_off = q_idx * 6u;

    let v00 = iu + iv * params.samples_u;
    let v10 = (iu + 1u) + iv * params.samples_u;
    let v01 = iu + (iv + 1u) * params.samples_u;
    let v11 = (iu + 1u) + (iv + 1u) * params.samples_u;

    // Tri 1: CCW (v00 -> v10 -> v01)
    out_indices[i_off + 0u] = v00;
    out_indices[i_off + 1u] = v10;
    out_indices[i_off + 2u] = v01;

    // Tri 2: CCW (v10 -> v11 -> v01)
    out_indices[i_off + 3u] = v10;
    out_indices[i_off + 4u] = v11;
    out_indices[i_off + 5u] = v01;
  }
}
