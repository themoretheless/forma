// Final packed ABI: bounds[0..6], raw moments[6..15], centroid[15..18],
// population covariance[18..24] in xx xy xz yy yz zz order.
struct Params { count: u32, _pad0: u32, _pad1: u32, _pad2: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> reduced: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let field = gid.x;
    if field >= 24u { return; }
    if field < 15u { output[field] = reduced[field]; return; }
    let n = f32(params.count);
    let mean = vec3<f32>(reduced[6], reduced[7], reduced[8]) / n;
    if field < 18u { output[field] = mean[field - 15u]; return; }
    let a = array<u32, 6>(0u, 0u, 0u, 1u, 1u, 2u);
    let b = array<u32, 6>(0u, 1u, 2u, 1u, 2u, 2u);
    let k = field - 18u;
    output[field] = reduced[9u + k] / n - mean[a[k]] * mean[b[k]];
}
