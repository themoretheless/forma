struct Params {
    count: u32,
    groups: u32,
    _pad0: u32,
    _pad1: u32,
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read> keep: array<u32>;
@group(0) @binding(3) var<storage, read> offsets: array<u32>;
@group(0) @binding(4) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    for (var i = gid.x; i < params.count; i += params.groups * WG) {
        if (keep[i] != 0u) {
            output[offsets[i]] = input[i];
        }
    }
}
