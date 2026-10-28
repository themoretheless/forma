struct Params {
    query_count: u32,
    target_count: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> queries: array<f32>;
@group(0) @binding(2) var<storage, read> targets: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_indices: array<u32>;
@group(0) @binding(4) var<storage, read_write> out_distances: array<f32>;

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
    var i0: u32 = 0xffffffffu;
    var i1: u32 = 0xffffffffu;
    var i2: u32 = 0xffffffffu;
    var i3: u32 = 0xffffffffu;
    var d0: f32 = 3.402823466e38;
    var d1: f32 = 3.402823466e38;
    var d2: f32 = 3.402823466e38;
    var d3: f32 = 3.402823466e38;
    for (var t: u32 = 0u; t < params.target_count; t = t + 1u) {
        let tb = t * 3u;
        let dx = qx - targets[tb];
        let dy = qy - targets[tb + 1u];
        let dz = qz - targets[tb + 2u];
        let dist = dx * dx + dy * dy + dz * dz;
        if (dist < d0) {
            d3 = d2;
            i3 = i2;
            d2 = d1;
            i2 = i1;
            d1 = d0;
            i1 = i0;
            d0 = dist;
            i0 = t;
        } else if (dist < d1) {
            d3 = d2;
            i3 = i2;
            d2 = d1;
            i2 = i1;
            d1 = dist;
            i1 = t;
        } else if (dist < d2) {
            d3 = d2;
            i3 = i2;
            d2 = dist;
            i2 = t;
        } else if (dist < d3) {
            d3 = dist;
            i3 = t;
        }
    }
    let out = i * 4u;
    out_indices[out] = i0;
    out_indices[out + 1u] = i1;
    out_indices[out + 2u] = i2;
    out_indices[out + 3u] = i3;
    out_distances[out] = d0;
    out_distances[out + 1u] = d1;
    out_distances[out + 2u] = d2;
    out_distances[out + 3u] = d3;
}
