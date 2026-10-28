// Header: outputCount, outputRank, inputOffset, reductionCount, reductionRank,
// outputOffset, groups, reserved. Then outputRank (dim,inputStride) pairs,
// then reductionRank (dim,inputStride) pairs.
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
var<workgroup> scratch: array<f32, WG>;
fn decode(index: u32, start: u32, rank: u32) -> u32 {
    var remaining = index;
    var offset = 0u;
    for (var axis = rank; axis > 0u; axis--) {
        let descriptor = start + (axis - 1u) * 2u;
        offset += (remaining % params[descriptor]) * params[descriptor + 1u];
        remaining /= params[descriptor];
    }
    return offset;
}
@compute @workgroup_size(WG)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    var row = wid.x;
    while row < params[0] {
        let base = params[2] + decode(row, 8u, params[1]);
        var sum = 0.0;
        var item = lid.x;
        while item < params[3] {
            sum += input[base + decode(item, 8u + params[1] * 2u, params[4])];
            if WG >= params[3] - item { break; }
            item += WG;
        }
        scratch[lid.x] = sum;
        for (var step = WG / 2u; step > 0u; step /= 2u) {
            workgroupBarrier();
            if lid.x < step { scratch[lid.x] += scratch[lid.x + step]; }
        }
        if lid.x == 0u { output[params[5] + row] = scratch[0]; }
        workgroupBarrier();
        if params[6] >= params[0] - row { break; }
        row += params[6];
    }
}
