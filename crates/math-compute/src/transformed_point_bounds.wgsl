// Template placeholder `WG` is substituted per wgpu backend.
struct Params {
    point_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    row0: vec4<f32>,
    row1: vec4<f32>,
    row2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> points: array<f32>;
@group(0) @binding(2) var<storage, read_write> out_min: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_max: array<f32>;

const WG: u32 = 256;
const INF: f32 = 3.4028234663852886e38;

var<workgroup> mins: array<vec4<f32>, WG>;
var<workgroup> maxes: array<vec4<f32>, WG>;

@compute @workgroup_size(WG)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = gid.x;
    var lo = vec4<f32>(INF, INF, INF, 0.0);
    var hi = vec4<f32>(-INF, -INF, -INF, 0.0);
    if (i < params.point_count) {
        let base = i * 3u;
        let px = points[base];
        let py = points[base + 1u];
        let pz = points[base + 2u];
        let p = vec4<f32>(
            params.row0.x * px + params.row0.y * py + params.row0.z * pz + params.row0.w,
            params.row1.x * px + params.row1.y * py + params.row1.z * pz + params.row1.w,
            params.row2.x * px + params.row2.y * py + params.row2.z * pz + params.row2.w,
            0.0,
        );
        lo = p;
        hi = p;
    }
    mins[lid.x] = lo;
    maxes[lid.x] = hi;
    workgroupBarrier();

    var stride = WG / 2u;
    loop {
        if (stride == 0u) {
            break;
        }
        if (lid.x < stride) {
            mins[lid.x] = min(mins[lid.x], mins[lid.x + stride]);
            maxes[lid.x] = max(maxes[lid.x], maxes[lid.x + stride]);
        }
        workgroupBarrier();
        stride = stride / 2u;
    }

    if (lid.x == 0u) {
        let base = wid.x * 3u;
        out_min[base] = mins[0].x;
        out_min[base + 1u] = mins[0].y;
        out_min[base + 2u] = mins[0].z;
        out_max[base] = maxes[0].x;
        out_max[base + 1u] = maxes[0].y;
        out_max[base + 2u] = maxes[0].z;
    }
}
