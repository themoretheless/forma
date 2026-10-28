// Header: rows,parts,Dv,tilesV,workCount,groups,outputOffset.
// A part stores its normalized value, softmax maximum and denominator.
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> states: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> values: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
const WG: u32 = 64;
var<workgroup> scratch: array<f32, WG>;
var<workgroup> weights: array<f32, WG>;
@compute @workgroup_size(WG)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    var work = wid.x;
    while work < params[4] {
        let row = work / params[3];
        let channel = (work % params[3]) * WG + lid.x;
        var state = vec2<f32>(0.0);
        var maximum = -MAX_F32;
        if lid.x < params[1] {
            state = states[row * params[1] + lid.x];
            if state.y > 0.0 { maximum = state.x; }
        }
        scratch[lid.x] = maximum;
        for (var step = WG / 2u; step > 0u; step /= 2u) {
            workgroupBarrier();
            if lid.x < step { scratch[lid.x] = max(scratch[lid.x], scratch[lid.x + step]); }
        }
        workgroupBarrier();
        maximum = scratch[0];
        var weight = 0.0;
        if state.y > 0.0 { weight = state.y * shifted_exp(state.x, maximum); }
        workgroupBarrier();
        weights[lid.x] = weight;
        scratch[lid.x] = weight;
        for (var step = WG / 2u; step > 0u; step /= 2u) {
            workgroupBarrier();
            if lid.x < step { scratch[lid.x] += scratch[lid.x + step]; }
        }
        workgroupBarrier();
        let denominator = scratch[0];
        if channel < params[2] {
            var result = 0.0;
            if denominator > 0.0 {
                var cached: array<f32, WG>;
                var scale = 0.0;
                for (var part = 0u; part < params[1]; part++) {
                    cached[part] = 0.0;
                    if weights[part] > 0.0 {
                        cached[part] = values[(row * params[1] + part) * params[2] + channel];
                        scale = max(scale, abs(cached[part]));
                    }
                }
                if scale > 0.0 {
                    var normalized = 0.0;
                    for (var part = 0u; part < params[1]; part++) {
                        normalized += (weights[part] / denominator) * unit_ratio(cached[part], scale);
                    }
                    result = clamp(normalized, -1.0, 1.0) * scale;
                }
            }
            output[params[6] + row * params[2] + channel] = result;
        }
        workgroupBarrier();
        if params[5] >= params[4] - work { break; }
        work += params[5];
    }
}
