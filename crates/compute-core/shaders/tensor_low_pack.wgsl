// count,rank,groups,inputOffset,outputOffset,dtype,mode,wordCount;
// then(dim,inputStride). mode0 copies raw16bits; mode1 encodes f32 IEEE bits.
@group(0) @binding(0) var<storage, read> p: array<u32>;
@group(0) @binding(1) var<storage, read> input: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;
fn logical_bits(index: u32) -> u32 {
    var remaining = index;
    var address = p[3];
    for (var axis = p[1]; axis > 0u; axis--) {
        let d = 8u + (axis - 1u) * 2u;
        address += (remaining % p[d]) * p[d + 1u];
        remaining /= p[d];
    }
    if p[6] == 0u { return (input[address / 2u] >> ((address & 1u) * 16u)) & 0xffffu; }
    return low_encode(input[address], p[5]);
}
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var i = gid.x;
    while i < p[7] {
        let word = p[4] / 2u + i;
        var bits = output[word];
        for (var lane = 0u; lane < 2u; lane++) {
            let address = word * 2u + lane;
            if address >= p[4] && address - p[4] < p[0] {
                let value = logical_bits(address - p[4]);
                let shift = lane * 16u;
                bits = (bits & ~(0xffffu << shift)) | (value << shift);
            }
        }
        output[word] = bits;
        if p[2] * 256u >= p[7] - i { break; }
        i += p[2] * 256u;
    }
}
