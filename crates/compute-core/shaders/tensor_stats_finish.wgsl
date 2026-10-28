@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> partials: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> previous: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> output: array<vec4<f32>>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var row = gid.x;
    let stride = params[12] * WG;
    while row < params[0] {
        var value = vec2<f32>(0.0);
        if params[7] == 0u { value = vec2<f32>(MAX_F32, -MAX_F32); }
        for (var part = 0u; part < params[6]; part++) {
            let next = partials[row * params[6] + part];
            if params[7] == 0u { value = vec2<f32>(min(value.x, next.x), max(value.y, next.y)); }
            else { value += next; }
        }
        if params[7] == 0u {
            var anchor = float_half(value.x) + float_half(value.y);
            if value.x == value.y { anchor = value.x; }
            let scale = max(abs(value.x - anchor), abs(value.y - anchor));
            output[row] = vec4<f32>(anchor, scale, value.y, 0.0);
        } else {
            let old = previous[row];
            if params[7] == 1u { output[row] = vec4<f32>(old.z, value.x, 0.0, 0.0); }
            if params[7] == 2u {
                output[row] = vec4<f32>(old.xy, clamp(value.x / f32(params[3]), -1.0, 1.0), 0.0);
            }
            if params[7] == 3u {
                // Values scaled into [-1,1] have population variance in [0,1].
                output[row] = vec4<f32>(old.xyz, clamp(value.x / f32(params[3]), 0.0, 1.0));
            }
        }
        if stride >= params[0] - row { break; }
        row += stride;
    }
}
