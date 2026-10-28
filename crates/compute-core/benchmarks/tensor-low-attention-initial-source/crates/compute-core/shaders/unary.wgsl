struct Params { count: u32, op: u32, _pad0: u32, _pad1: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let stride = groups.x * WG;
    for (var i = gid.x; i < params.count; i += stride) {
        let x = input[i];
        switch params.op {
            case 0u: { output[i] = -x; }
            case 1u: { output[i] = abs(x); }
            case 2u: { output[i] = x * x; }
            case 3u: { output[i] = sqrt(x); }
            case 4u: { output[i] = 1.0 / x; }
            case 5u: { output[i] = exp(x); }
            case 6u: { output[i] = log(x); }
            case 7u: { output[i] = sin(x); }
            case 8u: { output[i] = cos(x); }
            default: { output[i] = 0.0; }
        }
    }
}
