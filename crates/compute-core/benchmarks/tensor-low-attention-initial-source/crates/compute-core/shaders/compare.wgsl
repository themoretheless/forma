// Finite-input f32 comparisons produce exact 0/1 masks for GPU selection.
struct Params { count: u32, op: u32, a_step: u32, b_step: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<u32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let stride = groups.x * WG;
    for (var i = gid.x; i < params.count; i += stride) {
        let av = a[i * params.a_step];
        let bv = b[i * params.b_step];
        var keep = false;
        switch params.op {
            case 0u: { keep = av == bv; }
            case 1u: { keep = av != bv; }
            case 2u: { keep = av < bv; }
            case 3u: { keep = av <= bv; }
            case 4u: { keep = av > bv; }
            case 5u: { keep = av >= bv; }
            default: {}
        }
        output[i] = select(0u, 1u, keep);
    }
}
