// Template placeholder `WG` is substituted per wgpu backend.
struct Params {
    point_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> points: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

const WG: u32 = 256;
const INF: f32 = 3.4028234663852886e38;

var<workgroup> mins: array<vec4<f32>, WG>;
var<workgroup> maxes: array<vec4<f32>, WG>;
var<workgroup> sx: array<f32, WG>;
var<workgroup> sy: array<f32, WG>;
var<workgroup> sz: array<f32, WG>;
var<workgroup> sxx: array<f32, WG>;
var<workgroup> sxy: array<f32, WG>;
var<workgroup> sxz: array<f32, WG>;
var<workgroup> syy: array<f32, WG>;
var<workgroup> syz: array<f32, WG>;
var<workgroup> szz: array<f32, WG>;

@compute @workgroup_size(WG)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let i = gid.x;
    var x = 0.0;
    var y = 0.0;
    var z = 0.0;
    var lo = vec4<f32>(INF, INF, INF, 0.0);
    var hi = vec4<f32>(-INF, -INF, -INF, 0.0);
    if (i < params.point_count) {
        let base = i * 3u;
        x = points[base];
        y = points[base + 1u];
        z = points[base + 2u];
        let p = vec4<f32>(x, y, z, 0.0);
        lo = p;
        hi = p;
    }
    mins[lid.x] = lo;
    maxes[lid.x] = hi;
    sx[lid.x] = x;
    sy[lid.x] = y;
    sz[lid.x] = z;
    sxx[lid.x] = x * x;
    sxy[lid.x] = x * y;
    sxz[lid.x] = x * z;
    syy[lid.x] = y * y;
    syz[lid.x] = y * z;
    szz[lid.x] = z * z;
    workgroupBarrier();

    var stride = WG / 2u;
    loop {
        if (stride == 0u) {
            break;
        }
        if (lid.x < stride) {
            mins[lid.x] = min(mins[lid.x], mins[lid.x + stride]);
            maxes[lid.x] = max(maxes[lid.x], maxes[lid.x + stride]);
            sx[lid.x] += sx[lid.x + stride];
            sy[lid.x] += sy[lid.x + stride];
            sz[lid.x] += sz[lid.x + stride];
            sxx[lid.x] += sxx[lid.x + stride];
            sxy[lid.x] += sxy[lid.x + stride];
            sxz[lid.x] += sxz[lid.x + stride];
            syy[lid.x] += syy[lid.x + stride];
            syz[lid.x] += syz[lid.x + stride];
            szz[lid.x] += szz[lid.x + stride];
        }
        workgroupBarrier();
        stride = stride / 2u;
    }

    if (lid.x == 0u) {
        let base = wid.x * 15u;
        out[base] = mins[0].x;
        out[base + 1u] = mins[0].y;
        out[base + 2u] = mins[0].z;
        out[base + 3u] = maxes[0].x;
        out[base + 4u] = maxes[0].y;
        out[base + 5u] = maxes[0].z;
        out[base + 6u] = sx[0];
        out[base + 7u] = sy[0];
        out[base + 8u] = sz[0];
        out[base + 9u] = sxx[0];
        out[base + 10u] = sxy[0];
        out[base + 11u] = sxz[0];
        out[base + 12u] = syy[0];
        out[base + 13u] = syz[0];
        out[base + 14u] = szz[0];
    }
}
