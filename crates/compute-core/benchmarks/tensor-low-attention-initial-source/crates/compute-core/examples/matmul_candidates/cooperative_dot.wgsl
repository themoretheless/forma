// Cooperative dot product: one workgroup reduces the inner axis per output.
struct Params { rows: u32, inner: u32, columns: u32, groups: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
var<workgroup> scratch: array<f32, 64>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    for (var i = wid.x; i < params.rows * params.columns; i += params.groups) {
        let row = i / params.columns;
        let col = i % params.columns;
        var sum = 0.0;
        for (var k = lid.x; k < params.inner; k += 64u) {
            sum = fma(a[row * params.inner + k], b[k * params.columns + col], sum);
        }
        scratch[lid.x] = sum;
        for (var step = 32u; step > 0u; step /= 2u) {
            workgroupBarrier();
            if (lid.x < step) { scratch[lid.x] += scratch[lid.x + step]; }
        }
        if (lid.x == 0u) { output[i] = scratch[0]; }
        workgroupBarrier();
    }
}
