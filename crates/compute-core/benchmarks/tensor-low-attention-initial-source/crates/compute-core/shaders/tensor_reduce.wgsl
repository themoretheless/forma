// Header: outputCount, outputRank, inputOffset, reductionCount, reductionRank,
// outputOffset, groups, partsPerOutput, operation, divisor.
// Then outputRank (dim,inputStride) pairs and reductionRank pairs.
// operation: sum=0, product=1, min=2, max=3. Divisor is 1 except final f32 mean.
alias Value = f32;
const MIN_IDENTITY: Value = 3.4028234663852886e38;
const MAX_IDENTITY: Value = -3.4028234663852886e38;
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> input: array<Value>;
@group(0) @binding(2) var<storage, read_write> output: array<Value>;
const WG: u32 = 256;
var<workgroup> scratch: array<Value, WG>;
fn identity() -> Value {
    switch params[8] {
        case 1u: { return Value(1); }
        case 2u: { return MIN_IDENTITY; }
        case 3u: { return MAX_IDENTITY; }
        default: { return Value(0); }
    }
}
fn combine(a: Value, b: Value) -> Value {
    switch params[8] {
        case 1u: { return a * b; }
        case 2u: { return min(a, b); }
        case 3u: { return max(a, b); }
        default: { return a + b; }
    }
}
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
        let base = params[2] + decode(row, 10u, params[1]);
        var value = identity();
        var item = part * WG + lid.x;
        while item < params[3] {
            value = combine(value, input[base + decode(item, 10u + params[1] * 2u, params[4])]);
            if stride >= params[3] - item { break; }
            item += stride;
        }
        scratch[lid.x] = value;
        for (var step = WG / 2u; step > 0u; step /= 2u) {
            workgroupBarrier();
            if lid.x < step { scratch[lid.x] = combine(scratch[lid.x], scratch[lid.x + step]); }
        }
        if lid.x == 0u { output[params[5] + work] = scratch[0] / Value(params[9]); }
        workgroupBarrier();
        if params[6] >= work_count - work { break; }
        work += params[6];
    }
}
