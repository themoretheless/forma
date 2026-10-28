// indexCount, indexRank, groups, indexOffset, axisLength; then (dim,stride).
@group(0) @binding(0) var<storage, read> p: array<u32>;
@group(0) @binding(1) var<storage, read> indices: array<u32>;
@group(0) @binding(2) var<storage, read_write> owners: array<atomic<u32>>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var i = gid.x;
    while i < p[0] {
        var remaining = i;
        var address = p[3];
        for (var axis = p[1]; axis > 0u; axis--) {
            let d = 5u + (axis - 1u) * 2u;
            address += (remaining % p[d]) * p[d + 1u];
            remaining /= p[d];
        }
        let selected = indices[address];
        if selected < p[4] { atomicMax(&owners[selected], i + 1u); }
        if p[2] * 256u >= p[0] - i { break; }
        i += p[2] * 256u;
    }
}
