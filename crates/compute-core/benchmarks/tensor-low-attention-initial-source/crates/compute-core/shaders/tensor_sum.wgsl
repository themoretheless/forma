// Header: outputCount, outputRank, inputOffset, reductionCount, reductionRank,
// outputOffset, groups, partsPerOutput. Then outputRank (dim,inputStride) pairs,
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
    let parts = params[7];
    let work_count = params[0] * parts;
    let stride = parts * WG;
    var work = wid.x;
    while work < work_count {
        let row = work / parts;
        let part = work % parts;
        let base = params[2] + decode(row, 8u, params[1]);
        var sum = 0.0;
        var item = part * WG + lid.x;
        while item < params[3] {
            sum += input[base + decode(item, 8u + params[1] * 2u, params[4])];
            if stride >= params[3] - item { break; }
            item += stride;
        }
        scratch[lid.x] = sum;
        for (var step = WG / 2u; step > 0u; step /= 2u) {
            workgroupBarrier();
            if lid.x < step { scratch[lid.x] += scratch[lid.x + step]; }
        }
        if lid.x == 0u { output[params[5] + work] = scratch[0]; }
        workgroupBarrier();
        if params[6] >= work_count - work { break; }
        work += params[6];
    }
}
