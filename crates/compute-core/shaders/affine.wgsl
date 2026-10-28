struct Params { count: u32, scale: f32, offset: f32, _pad: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let stride = groups.x * WG;
    for (var i = gid.x; i < params.count; i += stride) {
        output[i] = input[i] * params.scale + params.offset;
    }
}
