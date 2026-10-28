// Fused QK -> masked online softmax -> PV. Workgroup64 handles one query and
// 64 output channels, streaming key tiles32. No score/probability matrix.
// Header31: workCount,groups,batchRank,Hq,Lq,Lk,D,Dv,groupSize,tilesV,
// qOffset,kOffset,vOffset,maskOffset,outOffset,
// qHeadStride,qRowStride,qDepthStride,kHeadStride,kRowStride,kDepthStride,
// vHeadStride,vRowStride,vDepthStride,maskHeadStride,maskQueryStride,
// maskKeyStride,maskMode,scaleBits,causalEnabled,causalOffsetBits.
// Batch descriptors: (dim,qStride,kStride,vStride,maskStride).
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> query: array<f32>;
@group(0) @binding(2) var<storage, read> key: array<f32>;
@group(0) @binding(3) var<storage, read> value: array<f32>;
@group(0) @binding(4) var<storage, read> mask: array<u32>;
@group(0) @binding(5) var<storage, read_write> output: array<f32>;
const WG: u32 = 64;
const KEYS: u32 = 32;
const MAX_F32: f32 = 3.4028234663852886e38;
var<workgroup> scratch: array<vec2<f32>, KEYS>;
var<workgroup> weights: array<f32, KEYS>;

fn down64(x: f32) -> f32 {
    let bits = bitcast<u32>(x);
    if ((bits >> 23u) & 255u) <= 64u { return 0.0; }
    return bitcast<f32>(bits - (64u << 23u));
}
fn up64(x: f32) -> f32 {
    let bits = bitcast<u32>(x);
    if (bits & 0x7f800000u) == 0u { return x * 18446744073709551616.0; }
    return bitcast<f32>(bits + (64u << 23u));
}
// Exact exponent scaling prevents a compiler from cancelling the protective
// factors under fast-math reassociation and forming a subnormal reciprocal.
fn unit_ratio(numerator: f32, denominator: f32) -> f32 {
    if denominator > 18446744073709551616.0 {
        return down64(numerator) / down64(denominator);
    }
    if denominator < 5.421010862427522e-20 {
        return up64(numerator) / up64(denominator);
    }
    return numerator / denominator;
}
fn shifted_exp(x: f32, maximum: f32) -> f32 {
    // A difference below -128 contributes less than the normal f32 range.
    // Halving first prevents overflow for opposite finite score extremes.
    let half = x * 0.5 - maximum * 0.5;
    if half < -64.0 { return 0.0; }
    return exp(half * 2.0);
}
fn causal_keep(query_index: u32, key_index: u32) -> bool {
    if params[29] == 0u { return true; }
    let offset = bitcast<i32>(params[30]);
    if offset >= 0 {
        if key_index <= query_index { return true; }
        return key_index - query_index <= u32(offset);
    }
    let distance = 0u - params[30];
    return query_index >= distance && key_index <= query_index - distance;
}
fn batch_offsets(index: u32) -> vec4<u32> {
    var remaining = index;
    var offsets = vec4<u32>(0u);
    for (var axis = params[2]; axis > 0u; axis--) {
        let descriptor = 31u + (axis - 1u) * 5u;
        let coordinate = remaining % params[descriptor];
        offsets += coordinate * vec4<u32>(params[descriptor + 1u], params[descriptor + 2u],
            params[descriptor + 3u], params[descriptor + 4u]);
        remaining /= params[descriptor];
    }
    return offsets;
}
@compute @workgroup_size(WG)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    var work = wid.x;
    while work < params[0] {
        let row = work / params[9];
        let channel = (work % params[9]) * WG + lid.x;
        let query_index = row % params[4];
        let head = (row / params[4]) % params[3];
        let batch = row / (params[4] * params[3]);
        let offsets = batch_offsets(batch);
        let qbase = params[10] + offsets.x + head * params[15] + query_index * params[16];
        let kbase = params[11] + offsets.y + (head / params[8]) * params[18];
        let vbase = params[12] + offsets.z + (head / params[8]) * params[21];
        let mbase = params[13] + offsets.w + head * params[24] + query_index * params[25];
        var maximum = 0.0;
        var denominator = 0.0;
        var value_scale = 0.0;
        var normalized = 0.0;
        var start = 0u;
        while start < params[5] {
            var score = -MAX_F32;
            var kept = false;
            if lid.x < KEYS {
                let key_index = start + lid.x;
                kept = key_index < params[5] && causal_keep(query_index, key_index);
                var bias = 0.0;
                if kept && params[27] != 0u {
                    let bits = mask[mbase + key_index * params[26]];
                    if params[27] == 1u { kept = bits != 0u; }
                    else {
                        kept = bits != 0xff800000u;
                        if kept { bias = bitcast<f32>(bits); }
                    }
                }
                if kept {
                    var dot = 0.0;
                    for (var depth = 0u; depth < params[6]; depth++) {
                        dot += query[qbase + depth * params[17]]
                            * key[kbase + key_index * params[19] + depth * params[20]];
                    }
                    score = dot * bitcast<f32>(params[28]) + bias;
                }
                scratch[lid.x] = vec2<f32>(score, select(0.0, 1.0, kept));
            }
            for (var step = KEYS / 2u; step > 0u; step /= 2u) {
                workgroupBarrier();
                if lid.x < step {
                    let a = scratch[lid.x];
                    let b = scratch[lid.x + step];
                    scratch[lid.x] = vec2<f32>(max(a.x, b.x), a.y + b.y);
                }
            }
            workgroupBarrier();
            let tile = scratch[0];
            if tile.y > 0.0 {
                var next_maximum = tile.x;
                var previous_weight = 0.0;
                if denominator > 0.0 {
                    next_maximum = max(maximum, tile.x);
                    previous_weight = denominator * shifted_exp(maximum, next_maximum);
                }
                var probability = 0.0;
                if kept { probability = shifted_exp(score, next_maximum); }
                workgroupBarrier();
                if lid.x < KEYS {
                    weights[lid.x] = probability;
                    scratch[lid.x] = vec2<f32>(probability, 0.0);
                }
                for (var step = KEYS / 2u; step > 0u; step /= 2u) {
                    workgroupBarrier();
                    if lid.x < step { scratch[lid.x].x += scratch[lid.x + step].x; }
                }
                workgroupBarrier();
                let next_denominator = previous_weight + scratch[0].x;
                if channel < params[7] {
                    var cached: array<f32, KEYS>;
                    var next_scale = value_scale;
                    if previous_weight == 0.0 { next_scale = 0.0; }
                    for (var item = 0u; item < KEYS; item++) {
                        cached[item] = 0.0;
                        if weights[item] > 0.0 {
                            cached[item] = value[vbase + (start + item) * params[22] + channel * params[23]];
                            next_scale = max(next_scale, abs(cached[item]));
                        }
                    }
                    if next_scale > 0.0 {
                        if previous_weight == 0.0 { normalized = 0.0; }
                        else {
                            normalized *= (previous_weight / next_denominator)
                                * unit_ratio(value_scale, next_scale);
                        }
                        for (var item = 0u; item < KEYS; item++) {
                            normalized += (weights[item] / next_denominator) * unit_ratio(cached[item], next_scale);
                        }
                        normalized = clamp(normalized, -1.0, 1.0);
                    } else {
                        normalized = 0.0;
                    }
                    value_scale = next_scale;
                }
                maximum = next_maximum;
                denominator = next_denominator;
            }
            workgroupBarrier();
            if KEYS >= params[5] - start { break; }
            start += KEYS;
        }
        if channel < params[7] {
            output[params[14] + row * params[7] + channel] = normalized * value_scale;
        }
        workgroupBarrier();
        if params[1] >= params[0] - work { break; }
        work += params[1];
    }
}
