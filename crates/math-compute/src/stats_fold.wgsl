// Reduce packed partial statistics: min xyz, max xyz, sums xyz,
// outer-product sums xx xy xz yy yz zz. Every pass preserves that 15-f32 ABI.
struct Params { count: u32, _pad0: u32, _pad1: u32, _pad2: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> partials: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
const INF: f32 = 3.4028234663852886e38;
var<workgroup> values: array<array<f32, 15>, WG>;
@compute @workgroup_size(WG)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    for (var field = 0u; field < 15u; field++) {
        var value = 0.0;
        if field < 3u { value = INF; }
        else if field < 6u { value = -INF; }
        if gid.x < params.count { value = partials[gid.x * 15u + field]; }
        values[lid.x][field] = value;
    }
    workgroupBarrier();
    var stride = WG / 2u;
    loop {
        if stride == 0u { break; }
        if lid.x < stride {
            for (var field = 0u; field < 15u; field++) {
                let a = values[lid.x][field];
                let b = values[lid.x + stride][field];
                var value = a + b;
                if field < 3u { value = min(a, b); }
                else if field < 6u { value = max(a, b); }
                values[lid.x][field] = value;
            }
        }
        workgroupBarrier();
        stride /= 2u;
    }
    if lid.x == 0u {
        for (var field = 0u; field < 15u; field++) {
            output[wid.x * 15u + field] = values[0][field];
        }
    }
}
