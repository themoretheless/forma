// One workgroup per group for 2..256 values. Same metadata as stats_reduce.
// Every active lane retains its value through all shared reduction stages.
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;
@group(0) @binding(3) var<storage, read_write> variance: array<u32>;
const WG: u32 = 256;
var<workgroup> scratch: array<vec2<f32>, WG>;
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
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    var row = wid.x;
    let valid_lane = lid.x < params[3];
    let distribution = params[7] < 3u;
    while row < params[0] {
        let row_address = decode(row, 13u, params[1]);
        var item_address = vec2<u32>(0u);
        var x = 0.0;
        var extrema = vec2<f32>(MAX_F32, -MAX_F32);
        if valid_lane {
            item_address = decode(lid.x, 13u + params[1] * 3u, params[4]);
            x = input[params[2] + row_address.x + item_address.x];
            extrema = vec2<f32>(x);
        }
        scratch[lid.x] = extrema;
        for (var step = WG / 2u; step > 0u; step /= 2u) {
            workgroupBarrier();
            if lid.x < step {
                let a = scratch[lid.x];
                let b = scratch[lid.x + step];
                scratch[lid.x] = vec2<f32>(min(a.x, b.x), max(a.y, b.y));
            }
        }
        workgroupBarrier();
        extrema = scratch[0];
        var anchor = float_half(extrema.x) + float_half(extrema.y);
        if extrema.x == extrema.y { anchor = extrema.x; }
        let scale = max(abs(extrema.x - anchor), abs(extrema.y - anchor));
        var transformed = 0.0;
        if valid_lane {
            if distribution { transformed = shifted_exp(x, extrema.y); }
            else if scale > 0.0 { transformed = scaled_delta(x, anchor, scale); }
        }
        // Every lane must finish reading extrema before the scratch is reused.
        workgroupBarrier();
        scratch[lid.x] = vec2<f32>(transformed, 0.0);
        for (var step = WG / 2u; step > 0u; step /= 2u) {
            workgroupBarrier();
            if lid.x < step { scratch[lid.x].x += scratch[lid.x + step].x; }
        }
        workgroupBarrier();
        let sum = scratch[0].x;
        let mean = clamp(sum / f32(params[3]), -1.0, 1.0);
        var scaled_variance = 0.0;
        var centered = 0.0;
        if !distribution {
            if valid_lane { centered = transformed - mean; }
            workgroupBarrier();
            scratch[lid.x] = vec2<f32>(centered * centered, 0.0);
            for (var step = WG / 2u; step > 0u; step /= 2u) {
                workgroupBarrier();
                if lid.x < step { scratch[lid.x].x += scratch[lid.x + step].x; }
            }
            workgroupBarrier();
            scaled_variance = clamp(scratch[0].x / f32(params[3]), 0.0, 1.0);
        }
        if valid_lane {
            let destination = params[9] + row_address.y + item_address.y;
            switch params[7] {
                case 0u: { output[destination] = bitcast<u32>(transformed / sum); }
                case 1u: {
                    let half = half_shift(x, extrema.y);
                    var bits = 0xff800000u;
                    if !product_overflows(abs(half), 2.0) { bits = bitcast<u32>(half * 2.0 - log(sum)); }
                    output[destination] = bits;
                }
                case 2u: {
                    if lid.x == 0u { output[params[9] + row] = bitcast<u32>(extrema.y + log(sum)); }
                }
                case 3u: {
                    if lid.x == 0u {
                        output[params[9] + row] = bitcast<u32>(anchor + scale * mean);
                        let inner = scale * scaled_variance;
                        var bits = 0x7f800000u;
                        if !product_overflows(scale, inner) { bits = bitcast<u32>(scale * inner); }
                        variance[params[10] + row] = bits;
                    }
                }
                default: {
                    var normalized = 0.0;
                    if scale > 0.0 {
                        let sqrt_epsilon = bitcast<f32>(params[8]);
                        if scale >= sqrt_epsilon {
                            let ratio = sqrt_epsilon / scale;
                            normalized = centered / sqrt(scaled_variance + ratio * ratio);
                        } else {
                            let ratio = scale / sqrt_epsilon;
                            normalized = centered * ratio / sqrt(1.0 + scaled_variance * ratio * ratio);
                        }
                    }
                    output[destination] = bitcast<u32>(normalized);
                }
            }
        }
        workgroupBarrier();
        if params[5] >= params[0] - row { break; }
        row += params[5];
    }
}
