// Four adjacent output columns per invocation. Requires columns divisible by 4.
struct Params { rows: u32, inner: u32, columns: u32, groups: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> output: array<vec4<f32>>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let columns4 = params.columns / 4u;
    for (var i = gid.x; i < params.rows * columns4; i += groups.x * 256u) {
        let row = i / columns4;
        let col4 = i % columns4;
        var sum = vec4<f32>(0.0);
        for (var k = 0u; k < params.inner; k++) {
            sum = fma(vec4<f32>(a[row * params.inner + k]), b[k * columns4 + col4], sum);
        }
        output[i] = sum;
    }
}
