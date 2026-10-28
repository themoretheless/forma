// axisLength, groups. Owners must be cleared on every prepared execution.
@group(0) @binding(0) var<storage, read> p: array<u32>;
@group(0) @binding(1) var<storage, read_write> owners: array<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var i = gid.x;
    while i < p[0] {
        owners[i] = 0u;
        if p[1] * 256u >= p[0] - i { break; }
        i += p[1] * 256u;
    }
}
