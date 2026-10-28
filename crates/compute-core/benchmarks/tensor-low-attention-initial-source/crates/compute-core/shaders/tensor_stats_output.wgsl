@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read> state: array<vec4<f32>>;
// Integer stores preserve explicit IEEE infinity even with finite-math shader
// compilation. The destination allocations have the public f32 interpretation.
@group(0) @binding(3) var<storage, read_write> output: array<u32>;
@group(0) @binding(4) var<storage, read_write> variance: array<u32>;
const WG: u32 = 256;
fn decode(index: u32, start: u32, rank: u32) -> vec2<u32> {
    var remaining = index;
    var offset = vec2<u32>(0u);
    for (var axis = rank; axis > 0u; axis--) {
        let descriptor = start + (axis - 1u) * 3u;
        let coordinate = remaining % params[descriptor];
        offset += coordinate * vec2<u32>(params[descriptor + 1u], params[descriptor + 2u]);
        remaining /= params[descriptor];
    }
    return offset;
}
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var index = gid.x;
    let stride = params[12] * WG;
    let reduced = params[7] == 2u || params[7] == 3u;
    while index < params[11] {
        var row = index / params[3];
        var item = index % params[3];
        if reduced { row = index; item = 0u; }
        let row_address = decode(row, 13u, params[1]);
        let item_address = decode(item, 13u + params[1] * 3u, params[4]);
        var destination = row_address.y + item_address.y;
        if reduced { destination = row; }
        destination += params[9];
        let x = input[params[2] + row_address.x + item_address.x];
        var bits = 0u;
        if params[3] == 1u {
            if params[7] == 0u { bits = bitcast<u32>(1.0); }
            if params[7] == 2u || params[7] == 3u { bits = bitcast<u32>(x); }
            if params[7] == 3u { variance[params[10] + row] = 0u; }
        } else {
            let summary = state[row];
            switch params[7] {
                case 0u: { bits = bitcast<u32>(shifted_exp(x, summary.x) / summary.y); }
                case 1u: {
                    let half = half_shift(x, summary.x);
                    if product_overflows(abs(half), 2.0) { bits = 0xff800000u; }
                    else { bits = bitcast<u32>(half * 2.0 - log(summary.y)); }
                }
                case 2u: { bits = bitcast<u32>(summary.x + log(summary.y)); }
                case 3u: {
                    bits = bitcast<u32>(summary.x + summary.y * summary.z);
                    let inner = summary.y * summary.w;
                    var variance_bits = 0x7f800000u;
                    if !product_overflows(summary.y, inner) { variance_bits = bitcast<u32>(summary.y * inner); }
                    variance[params[10] + row] = variance_bits;
                }
                default: {
                    var normalized = 0.0;
                    if summary.y > 0.0 {
                        let centered = scaled_delta(x, summary.x, summary.y) - summary.z;
                        let sqrt_epsilon = bitcast<f32>(params[8]);
                        if summary.y >= sqrt_epsilon {
                            let ratio = sqrt_epsilon / summary.y;
                            normalized = centered / sqrt(summary.w + ratio * ratio);
                        } else {
                            let ratio = summary.y / sqrt_epsilon;
                            normalized = centered * ratio / sqrt(1.0 + summary.w * ratio * ratio);
                        }
                    }
                    bits = bitcast<u32>(normalized);
                }
            }
        }
        output[destination] = bits;
        if stride >= params[11] - index { break; }
        index += stride;
    }
}
