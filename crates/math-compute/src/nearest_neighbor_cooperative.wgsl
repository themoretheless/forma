// One workgroup per query: lanes scan disjoint target subsequences, then
// cooperatively reduce the best distance and first matching target index.
struct Params { query_count: u32, target_count: u32, _pad0: u32, _pad1: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> queries: array<f32>;
@group(0) @binding(2) var<storage, read> targets: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_index: array<u32>;
@group(0) @binding(4) var<storage, read_write> out_dist: array<f32>;
const WG: u32 = 256;
var<workgroup> distances: array<f32, WG>;
var<workgroup> indices: array<u32, WG>;
@compute @workgroup_size(WG)
fn main(@builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    let i = wid.x;
    if i >= params.query_count { return; }
    let qx = queries[i*3u]; let qy = queries[i*3u+1u]; let qz = queries[i*3u+2u];
    var best_index = 0xffffffffu;
    var best_dist = 3.402823466e38;
    for (var t = lid.x; t < params.target_count; t += WG) {
        let dx = qx - targets[t*3u]; let dy = qy - targets[t*3u+1u]; let dz = qz - targets[t*3u+2u];
        let dist = dx*dx + dy*dy + dz*dz;
        if dist < best_dist { best_dist = dist; best_index = t; }
    }
    distances[lid.x] = best_dist;
    indices[lid.x] = best_index;
    workgroupBarrier();
    var stride = WG/2u;
    loop {
        if stride == 0u { break; }
        if lid.x < stride {
            let other = lid.x + stride;
            if distances[other] < distances[lid.x] || (distances[other] == distances[lid.x] && indices[other] < indices[lid.x]) {
                distances[lid.x] = distances[other]; indices[lid.x] = indices[other];
            }
        }
        workgroupBarrier();
        stride /= 2u;
    }
    if lid.x == 0u { out_index[i] = indices[0]; out_dist[i] = distances[0]; }
}
