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
    if (i < params.point_count) {
        let base = i * 3u;
        x = points[base];
        y = points[base + 1u];
        z = points[base + 2u];
    }
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
        let base = wid.x * 9u;
        out[base] = sx[0];
        out[base + 1u] = sy[0];
        out[base + 2u] = sz[0];
        out[base + 3u] = sxx[0];
        out[base + 4u] = sxy[0];
        out[base + 5u] = sxz[0];
        out[base + 6u] = syy[0];
        out[base + 7u] = syz[0];
        out[base + 8u] = szz[0];
    }
}
