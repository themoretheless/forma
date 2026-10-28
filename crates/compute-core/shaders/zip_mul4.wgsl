// Elementwise vec4 product: output[i] = a[i] * b[i].
// Vectorized building block for wide dot products and weighted masking;
// pair with block_sum over the flattened products. `count` is the number of
// vec4 lanes (not f32 elements).
struct Params {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> b: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> output: array<vec4<f32>>;

const WG: u32 = 256;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.count) {
        return;
    }
    output[i] = a[i] * b[i];
}
