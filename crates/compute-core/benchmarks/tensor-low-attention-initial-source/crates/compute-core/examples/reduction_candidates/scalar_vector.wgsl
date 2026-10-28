// Scalar storage ABI: contiguous vector arithmetic with no padding requirement.
struct Params { count: u32, groups: u32, pad0: u32, pad1: u32 };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
const VECTORS: u32 = 1;
var<workgroup> scratch: array<f32, WG>;
@compute @workgroup_size(WG)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    var sums: array<vec4<f32>, VECTORS>;
    let count4 = p.count / 4u + select(0u,1u,p.count % 4u != 0u);
    let stride = p.groups * WG * VECTORS;
    for (var base = wid.x * WG * VECTORS + lid.x; base < count4; base += stride) {
        for (var item = 0u; item < VECTORS; item++) {
            let i = base + item * WG;
            if (i < count4) {
                let j = i * 4u;
                var v = vec4<f32>(0.0);
                if (p.count - j >= 4u) {
                    v = vec4<f32>(input[j], input[j+1u], input[j+2u], input[j+3u]);
                } else {
                    for (var c = 0u; c < 4u; c++) {
                        if (c < p.count - j) { v[c] = input[j+c]; }
                    }
                }
                sums[item] += v;
            }
        }
    }
    var sum4 = vec4<f32>(0.0);
    for (var item = 0u; item < VECTORS; item++) { sum4 += sums[item]; }
    scratch[lid.x] = (sum4.x+sum4.y)+(sum4.z+sum4.w);
    for (var step = WG / 2u; step > 0u; step /= 2u) {
        workgroupBarrier();
        if (lid.x < step) { scratch[lid.x] += scratch[lid.x + step]; }
    }
    if (lid.x == 0u) { output[wid.x] = scratch[0]; }
}
