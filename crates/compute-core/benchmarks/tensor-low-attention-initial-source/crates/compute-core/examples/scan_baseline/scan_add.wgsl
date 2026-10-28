struct Params {
    count: u32,
    groups: u32,
    block_size: u32,
    _pad: u32,
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> block_offsets: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    for (var i = gid.x; i < params.count; i += params.groups * WG) {
        output[i] += block_offsets[i / params.block_size];
    }
}
