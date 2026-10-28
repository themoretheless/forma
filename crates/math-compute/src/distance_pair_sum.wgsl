struct Params {
    pair_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

const WG: u32 = 256;
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a_points: array<f32>;
@group(0) @binding(2) var<storage, read> b_points: array<f32>;
@group(0) @binding(3) var<storage, read_write> partial_sums: array<f32>;

var<workgroup> scratch: array<f32, WG>;


@compute @workgroup_size(WG)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    var value = 0.0;
    let i = gid.x;
    if (i < params.pair_count) {
        let base = i * 3u;
        let dx = a_points[base] - b_points[base];
        let dy = a_points[base + 1u] - b_points[base + 1u];
        let dz = a_points[base + 2u] - b_points[base + 2u];
        value = dx * dx + dy * dy + dz * dz;
    }
    scratch[lid.x] = value;
    workgroupBarrier();

    var stride = WG / 2u;
    loop {
        if (lid.x < stride) {
            scratch[lid.x] = scratch[lid.x] + scratch[lid.x + stride];
        }
        workgroupBarrier();
        if (stride == 1u) {
            break;
        }
        stride = stride / 2u;
    }
    if (lid.x == 0u) {
        partial_sums[wid.x] = scratch[0];
    }
}
