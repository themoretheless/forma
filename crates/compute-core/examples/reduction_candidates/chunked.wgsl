struct Params { count: u32, groups: u32, pad0: u32, pad1: u32 };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
const ITEMS: u32 = 4;
var<workgroup> scratch: array<f32, WG>;
@compute @workgroup_size(WG)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    var sums: array<f32, ITEMS>;
    let stride = p.groups * WG * ITEMS;
    for (var base = wid.x * WG * ITEMS + lid.x; base < p.count; base += stride) {
        for (var item = 0u; item < ITEMS; item++) {
            let i = base + item * WG;
            if (i < p.count) { sums[item] += input[i]; }
        }
    }
    var sum = 0.0;
    for (var item = 0u; item < ITEMS; item++) { sum += sums[item]; }
    scratch[lid.x] = sum;
    for (var step = WG / 2u; step > 0u; step /= 2u) {
        workgroupBarrier();
        if (lid.x < step) { scratch[lid.x] += scratch[lid.x + step]; }
    }
    if (lid.x == 0u) { output[wid.x] = scratch[0]; }
}
