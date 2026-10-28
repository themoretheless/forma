// Portable, grid-strided elementwise operations. Each dispatch can cover
// arrays larger than max_compute_workgroups_per_dimension * WG.
struct Params { count: u32, op: u32, a_step: u32, b_step: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let stride = groups.x * WG;
    for (var i = gid.x; i < params.count; i += stride) {
        switch params.op {
            case 0u: { output[i] = a[i * params.a_step] + b[i * params.b_step]; }
            case 1u: { output[i] = a[i * params.a_step] - b[i * params.b_step]; }
            case 2u: { output[i] = a[i * params.a_step] * b[i * params.b_step]; }
            case 3u: { output[i] = a[i * params.a_step] / b[i * params.b_step]; }
            case 4u: { output[i] = min(a[i * params.a_step], b[i * params.b_step]); }
            case 5u: { output[i] = max(a[i * params.a_step], b[i * params.b_step]); }
            default: { output[i] = 0.0; }
        }
    }
}
