// Second pass: residual sums and products about the first pass's centroid.
// The first six zero fields let the shared 15-f32 statistics fold be reused.
struct Params { count: u32, _pad0: u32, _pad1: u32, _pad2: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> points: array<f32>;
@group(0) @binding(2) var<storage, read> statistics: array<f32>;
@group(0) @binding(3) var<storage, read_write> partials: array<f32>;
const WG: u32 = 128;
var<workgroup> sums: array<vec3<f32>, WG>;
var<workgroup> products_x: array<vec3<f32>, WG>;
var<workgroup> products_yz: array<vec3<f32>, WG>;
@compute @workgroup_size(WG)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    var d = vec3<f32>(0.0);
    if gid.x < params.count {
        let base = gid.x * 3u;
        d = vec3<f32>(points[base], points[base + 1u], points[base + 2u])
            - vec3<f32>(statistics[15], statistics[16], statistics[17]);
    }
    sums[lid.x] = d;
    products_x[lid.x] = d.x * d;
    products_yz[lid.x] = vec3<f32>(d.y * d.y, d.y * d.z, d.z * d.z);
    workgroupBarrier();
    var stride = WG / 2u;
    loop {
        if stride == 0u { break; }
        if lid.x < stride {
            sums[lid.x] += sums[lid.x + stride];
            products_x[lid.x] += products_x[lid.x + stride];
            products_yz[lid.x] += products_yz[lid.x + stride];
        }
        workgroupBarrier();
        stride /= 2u;
    }
    if lid.x == 0u {
        let base = wid.x * 15u;
        for (var field = 0u; field < 6u; field++) { partials[base + field] = 0.0; }
        partials[base + 6u] = sums[0].x;
        partials[base + 7u] = sums[0].y;
        partials[base + 8u] = sums[0].z;
        partials[base + 9u] = products_x[0].x;
        partials[base + 10u] = products_x[0].y;
        partials[base + 11u] = products_x[0].z;
        partials[base + 12u] = products_yz[0].x;
        partials[base + 13u] = products_yz[0].y;
        partials[base + 14u] = products_yz[0].z;
    }
}
