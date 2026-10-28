
struct Params { rows: u32, inner: u32, columns: u32, pad: u32 };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    for (var i = gid.x; i < p.rows * p.columns; i += groups.x * WG) {
        let row = i / p.columns;
        let col = i % p.columns;
        var value = 0.0;
        for (var k = 0u; k < p.inner; k++) {
            value = fma(a[row * p.inner + k], b[k * p.columns + col], value);
        }
        out[i] = value;
    }
}
