// Workgroup-strided grid sum reduction: partials[wid] = sum of the elements
// this workgroup covers, striding by groups * WG so a single dispatch reduces
// arbitrary lengths (callers pick groups <= the 65535 dispatch limit). Chain
// dispatches over the partials array — or use the compute-core `reduce_f32`
// helper — to fold down to a scalar.
struct Params {
    count: u32,
    groups: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> partials: array<f32>;

// WG must stay a power of two: the tree reduction halves the stride.
const WG: u32 = 256;

var<workgroup> scratch: array<f32, WG>;

@compute @workgroup_size(WG)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let stride = params.groups * WG;
    var sum = 0.0;
    for (var i = wid.x * WG + lid.x; i < params.count; i += stride) {
        sum += input[i];
    }
    scratch[lid.x] = sum;
    for (var step = WG / 2u; step > 0u; step = step >> 1u) {
        workgroupBarrier();
        if (lid.x < step) {
            scratch[lid.x] = scratch[lid.x] + scratch[lid.x + step];
        }
    }
    if (lid.x == 0u) {
        partials[wid.x] = scratch[0];
    }
}
