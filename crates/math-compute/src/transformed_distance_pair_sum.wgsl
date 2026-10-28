struct Params {
    pair_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    row0: vec4<f32>,
    row1: vec4<f32>,
    row2: vec4<f32>,
};

const WG: u32 = 256;
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> source_points: array<f32>;
@group(0) @binding(2) var<storage, read> target_points: array<f32>;
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
        let px = source_points[base];
        let py = source_points[base + 1u];
        let pz = source_points[base + 2u];
        let qx = target_points[base];
        let qy = target_points[base + 1u];
        let qz = target_points[base + 2u];
        let x = params.row0.x * px + params.row0.y * py + params.row0.z * pz + params.row0.w;
        let y = params.row1.x * px + params.row1.y * py + params.row1.z * pz + params.row1.w;
        let z = params.row2.x * px + params.row2.y * py + params.row2.z * pz + params.row2.w;
        let dx = x - qx;
        let dy = y - qy;
        let dz = z - qz;
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
