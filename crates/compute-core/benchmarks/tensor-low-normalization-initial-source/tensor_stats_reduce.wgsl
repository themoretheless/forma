// Header: rows,rowRank,inputOffset,reductionCount,reductionRank,groups,parts,
// operation,sqrtEpsilonBits,outputOffset,varianceOffset,outputCount,outputGroups.
// Row descriptors then reduction descriptors: (dim,inputStride,denseStride).
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read> state: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> output: array<vec2<f32>>;
const WG: u32 = 256;
var<workgroup> scratch: array<vec2<f32>, WG>;
fn decode(index: u32, start: u32, rank: u32) -> u32 {
    var remaining = index;
    var offset = 0u;
    for (var axis = rank; axis > 0u; axis--) {
        let descriptor = start + (axis - 1u) * 3u;
        offset += (remaining % params[descriptor]) * params[descriptor + 1u];
        remaining /= params[descriptor];
    }
    return offset;
}
fn combine(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    if params[7] == 0u { return vec2<f32>(min(a.x, b.x), max(a.y, b.y)); }
    return a + b;
}
@compute @workgroup_size(WG)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let parts = params[6];
    let work_count = params[0] * parts;
    let stride = parts * WG;
    var work = wid.x;
    while work < work_count {
        let row = work / parts;
        let part = work % parts;
        let base = params[2] + decode(row, 13u, params[1]);
        var value = vec2<f32>(0.0);
        if params[7] == 0u { value = vec2<f32>(MAX_F32, -MAX_F32); }
        var item = part * WG + lid.x;
        while item < params[3] {
            let x = input[base + decode(item, 13u + params[1] * 3u, params[4])];
            if params[7] == 0u {
                value = combine(value, vec2<f32>(x));
            } else {
                let row_state = state[row];
                var transformed = 0.0;
                if params[7] == 1u {
                    transformed = shifted_exp(x, row_state.z);
                } else if row_state.y > 0.0 {
                    let scaled = scaled_delta(x, row_state.x, row_state.y);
                    if params[7] == 2u { transformed = scaled; }
                    else {
                        let centered = scaled - row_state.z;
                        transformed = centered * centered;
                    }
                }
                value.x += transformed;
            }
            if stride >= params[3] - item { break; }
            item += stride;
        }
        scratch[lid.x] = value;
        for (var step = WG / 2u; step > 0u; step /= 2u) {
            workgroupBarrier();
            if lid.x < step { scratch[lid.x] = combine(scratch[lid.x], scratch[lid.x + step]); }
        }
        if lid.x == 0u { output[work] = scratch[0]; }
        workgroupBarrier();
        if params[5] >= work_count - work { break; }
        work += params[5];
    }
}
