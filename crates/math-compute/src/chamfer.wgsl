// Template placeholder `WG` is substituted per wgpu backend.
struct Params {
    query_count: u32,
    target_count: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> queries: array<f32>;
@group(0) @binding(2) var<storage, read> targets: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_sum: array<f32>;
@group(0) @binding(4) var<storage, read_write> out_max: array<f32>;

const WG: u32 = 256;

var<workgroup> sums: array<f32, WG>;
var<workgroup> maxes: array<f32, WG>;

@compute @workgroup_size(WG)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let q = gid.x;
    var best = 0.0;
    if (q < params.query_count) {
        let qb = q * 3u;
        let qx = queries[qb];
        let qy = queries[qb + 1u];
        let qz = queries[qb + 2u];
        best = 3.4028234663852886e38;
        var t = 0u;
        loop {
            if (t >= params.target_count) {
                break;
            }
            let tb = t * 3u;
            let dx = qx - targets[tb];
            let dy = qy - targets[tb + 1u];
            let dz = qz - targets[tb + 2u];
            best = min(best, dx * dx + dy * dy + dz * dz);
            t = t + 1u;
        }
    }
    sums[lid.x] = best;
    maxes[lid.x] = best;
    workgroupBarrier();

    var stride = WG / 2u;
    loop {
        if (stride == 0u) {
            break;
        }
        if (lid.x < stride) {
            sums[lid.x] = sums[lid.x] + sums[lid.x + stride];
            maxes[lid.x] = max(maxes[lid.x], maxes[lid.x + stride]);
        }
        workgroupBarrier();
        stride = stride / 2u;
    }

    if (lid.x == 0u) {
        out_sum[wid.x] = sums[0];
        out_max[wid.x] = maxes[0];
    }
}
