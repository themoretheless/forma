// count,rank,groups,inputOffset,outputOffset,dtype; then(dim,inputStride).
@group(0) @binding(0) var<storage, read> p: array<u32>;
@group(0) @binding(1) var<storage, read> input: array<u32>;
// f32 storage viewed as integer bits avoids floating-point cast intermediates.
@group(0) @binding(2) var<storage, read_write> output: array<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var i = gid.x;
    while i < p[0] {
        var remaining = i;
        var address = p[3];
        for (var axis = p[1]; axis > 0u; axis--) {
            let d = 6u + (axis - 1u) * 2u;
            address += (remaining % p[d]) * p[d + 1u];
            remaining /= p[d];
        }
        let bits = (input[address / 2u] >> ((address & 1u) * 16u)) & 0xffffu;
        output[p[4] + i] = low_decode(bits, p[5]);
        if p[2] * 256u >= p[0] - i { break; }
        i += p[2] * 256u;
    }
}
