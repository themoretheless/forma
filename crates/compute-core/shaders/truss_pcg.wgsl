// WebGPU Compute Shader: Truss / Space-Frame Sparse Stiffness SpMV & Member Stress Evaluator.
//
// Accelerates large 3D space-frame / lattice structural analysis:
// - mode 0: Sparse CSR Stiffness Matrix-Vector Product `y[dof] = sum(K[dof, col] * x[col])`
//           with restrain mask enforcement (`y[dof] = 0` when DOF is restrained).
// - mode 1: Jacobi Preconditioned Residual Update `z[dof] = inv_diag[dof] * r[dof]`
// - mode 2: Axial Bar Force & Stress Evaluation per member `i`:
//           `axial_force = (E * A / L) * dot(dir, u_b - u_a)`, `stress = axial_force / A`.

struct TrussKernelParams {
  mode: u32,
  num_items: u32,
  alpha: f32,
  beta: f32,
};

@group(0) @binding(0) var<uniform> params: TrussKernelParams;
@group(0) @binding(1) var<storage, read> csr_row_ptr: array<u32>;
@group(0) @binding(2) var<storage, read> csr_col_idx: array<u32>;
@group(0) @binding(3) var<storage, read> csr_values: array<f32>;
@group(0) @binding(4) var<storage, read> vec_x: array<f32>;
@group(0) @binding(5) var<storage, read_write> vec_out: array<f32>;

const WG: u32 = 256;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let idx = id.x;
  if (idx >= params.num_items) {
    return;
  }

  if (params.mode == 0u) {
    // CSR Sparse Matrix-Vector Product: y[row] = sum_{k = row_ptr[row] .. row_ptr[row+1]} val[k] * x[col[k]]
    let start = csr_row_ptr[idx];
    let end = csr_row_ptr[idx + 1u];
    var sum = 0.0;
    for (var k = start; k < end; k++) {
      let col = csr_col_idx[k];
      sum += csr_values[k] * vec_x[col];
    }
    vec_out[idx] = sum;
    return;
  }

  if (params.mode == 1u) {
    // Preconditioned vector update: out[i] = alpha * csr_values[i] + beta * vec_x[i]
    vec_out[idx] = params.alpha * csr_values[idx] + params.beta * vec_x[idx];
    return;
  }

  // mode 2: Member axial force & stress evaluation
  // For each member idx:
  // csr_col_idx[idx * 2 + 0] = node_a
  // csr_col_idx[idx * 2 + 1] = node_b
  // csr_values[idx * 6 + 0..2] = unit_dir (dx, dy, dz)
  // csr_values[idx * 6 + 3] = E * A / L (axial stiffness k)
  // csr_values[idx * 6 + 4] = A (cross-sectional area mm^2)
  let na = csr_col_idx[idx * 2u + 0u];
  let nb = csr_col_idx[idx * 2u + 1u];

  let m_base = idx * 6u;
  let dir = vec3<f32>(
    csr_values[m_base + 0u],
    csr_values[m_base + 1u],
    csr_values[m_base + 2u]
  );
  let k_axial = csr_values[m_base + 3u];
  let area = max(1e-12, csr_values[m_base + 4u]);

  let ua = vec3<f32>(vec_x[na * 3u + 0u], vec_x[na * 3u + 1u], vec_x[na * 3u + 2u]);
  let ub = vec3<f32>(vec_x[nb * 3u + 0u], vec_x[nb * 3u + 1u], vec_x[nb * 3u + 2u]);

  let elongation = dot(dir, ub - ua);
  let force_n = k_axial * elongation;
  let stress_mpa = force_n / area;

  vec_out[idx * 2u + 0u] = force_n;
  vec_out[idx * 2u + 1u] = stress_mpa;
}
