struct Params {
    query_count: u32,
    target_count: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> queries: array<f32>;
@group(0) @binding(2) var<storage, read> targets: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_index0: array<u32>;
@group(0) @binding(4) var<storage, read_write> out_dist0: array<f32>;
@group(0) @binding(5) var<storage, read_write> out_index1: array<u32>;
@group(0) @binding(6) var<storage, read_write> out_dist1: array<f32>;

const WG: u32 = 256;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.query_count) {
        return;
    }
    let base = i * 3u;
    let qx = queries[base];
    let qy = queries[base + 1u];
    let qz = queries[base + 2u];
    var best_index: u32 = 0xffffffffu;
    var second_index: u32 = 0xffffffffu;
    var best_dist: f32 = 3.402823466e38;
    var second_dist: f32 = 3.402823466e38;
    for (var t: u32 = 0u; t < params.target_count; t = t + 1u) {
        let tb = t * 3u;
        let dx = qx - targets[tb];
        let dy = qy - targets[tb + 1u];
        let dz = qz - targets[tb + 2u];
        let dist = dx * dx + dy * dy + dz * dz;
        if (dist < best_dist) {
            second_dist = best_dist;
            second_index = best_index;
            best_dist = dist;
            best_index = t;
        } else if (dist < second_dist) {
            second_dist = dist;
            second_index = t;
        }
    }
    out_index0[i] = best_index;
    out_dist0[i] = best_dist;
    out_index1[i] = second_index;
    out_dist1[i] = second_dist;
}
