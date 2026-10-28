// Elementwise affine map: output[i] = input[i] * scale + offset.
// Building block for normalization, unit conversion, bias application.
struct Params {
    count: u32,
    scale: f32,
    offset: f32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

// `WG` is the workgroup-size anchor: the compute-core runtime may substitute
// a per-backend tuned value (powers of two only) before compilation.
const WG: u32 = 256;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.count) {
        return;
    }
    output[i] = input[i] * params.scale + params.offset;
}
