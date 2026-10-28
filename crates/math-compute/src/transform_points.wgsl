struct Params {
    count: u32, _pad0: u32, _pad1: u32, _pad2: u32,
    row0: vec4<f32>, row1: vec4<f32>, row2: vec4<f32>,
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= params.count { return; }
    let base = i * 3u;
    let point = vec4<f32>(input[base], input[base+1u], input[base+2u], 1.0);
    output[base] = dot(params.row0, point);
    output[base+1u] = dot(params.row1, point);
    output[base+2u] = dot(params.row2, point);
}
