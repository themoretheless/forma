// A separate pass makes tail clearing race-free with scatter, including reuse
// after an earlier submission selected more elements. Empty input resets count.
struct Params {
    count: u32,
    groups: u32,
    _pad0: u32,
    _pad1: u32,
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> keep: array<u32>;
@group(0) @binding(2) var<storage, read> offsets: array<u32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
@group(0) @binding(4) var<storage, read_write> selected_count: array<u32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var total = 0u;
    if (params.count != 0u) {
        let last = params.count - 1u;
        total = offsets[last] + select(0u, 1u, keep[last] != 0u);
    }
    if (gid.x == 0u) {
        selected_count[0] = total;
    }
    for (var i = gid.x; i < params.count; i += params.groups * WG) {
        if (i >= total) {
            output[i] = 0.0;
        }
    }
}
