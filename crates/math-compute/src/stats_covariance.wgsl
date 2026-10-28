// Covariance about an approximate centroid needs the residual-mean correction.
// Only covariance changes; bounds, raw moments and centroid retain their ABI.
struct Params { count: u32, _pad0: u32, _pad1: u32, _pad2: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> reduced: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= 6u { return; }
    let n = f32(params.count);
    let residual_mean = vec3<f32>(reduced[6], reduced[7], reduced[8]) / n;
    let a = array<u32, 6>(0u, 0u, 0u, 1u, 1u, 2u);
    let b = array<u32, 6>(0u, 1u, 2u, 1u, 2u, 2u);
    output[18u + k] = reduced[9u + k] / n - residual_mean[a[k]] * residual_mean[b[k]];
}
