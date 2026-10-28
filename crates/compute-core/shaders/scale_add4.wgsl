// Elementwise affine map on vec4 lanes: output[i] = input[i] * scale + offset.
// The vec4 variant streams 16 bytes per thread instead of 4 — on Apple
// Silicon this roughly doubles effective bandwidth over the scalar kernel.
// `count` is the number of vec4 lanes (not f32 elements).
struct Params {
    count: u32,
    scale: f32,
    offset: f32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output: array<vec4<f32>>;

// `WG` is the workgroup-size anchor: the compute-core runtime may substitute
// a per-backend tuned value (powers of two only) before compilation.
const WG: u32 = 256;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.count) {
        return;
    }
    output[i] = input[i] * params.scale + vec4<f32>(params.offset);
}
