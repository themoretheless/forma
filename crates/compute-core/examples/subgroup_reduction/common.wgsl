// Native wgpu subgroup study. Naga 30 accepts the builtins directly; its
// `enable subgroups` directive is not implemented. Host explicitly opts in.
struct Params { count: u32, groups: u32, _pad0: u32, _pad1: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
const ITEMS: u32 = 8;
var<workgroup> scratch: array<f32, WG>;
// HELPERS
@compute @workgroup_size(WG)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
    // SUBGROUP INPUTS
) {
    var accumulators: array<f32, ITEMS>;
    let stride = params.groups * WG * ITEMS;
    for (var base = wid.x * WG * ITEMS + lid.x; base < params.count;) {
        for (var item = 0u; item < ITEMS; item++) {
            let offset = item * WG;
            if offset < params.count - base {
                accumulators[item] += input[base + offset];
            }
        }
        if params.count - base <= stride { break; }
        base += stride;
    }
    for (var step = ITEMS / 2u; step > 0u; step >>= 1u) {
        for (var item = 0u; item < step; item++) {
            accumulators[item] += accumulators[item + step];
        }
    }
    let lane_sum = accumulators[0];
    // No invocation returns early: empty inputs and tail lanes contribute zero.
    // REDUCE
}
