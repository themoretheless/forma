// count,rank,groups,aOffset,bOffset,outOffset,dtype,op,wordCount;
// then (dim,aStride,bStride). Unary op0..8; binary op32..37.
@group(0) @binding(0) var<storage, read> p: array<u32>;
@group(0) @binding(1) var<storage, read> left: array<u32>;
@group(0) @binding(2) var<storage, read> right: array<u32>;
@group(0) @binding(3) var<storage, read_write> output: array<u32>;
fn result_bits(index: u32) -> u32 {
    var remaining = index;
    var a_address = p[3];
    var b_address = p[4];
    for (var axis = p[1]; axis > 0u; axis--) {
        let descriptor = 9u + (axis - 1u) * 3u;
        let coordinate = remaining % p[descriptor];
        a_address += coordinate * p[descriptor + 1u];
        b_address += coordinate * p[descriptor + 2u];
        remaining /= p[descriptor];
    }
    let a_bits = (left[a_address / 2u] >> ((a_address & 1u) * 16u)) & 65535u;
    if p[7] == 0u { return a_bits ^ 0x8000u; }
    if p[7] == 1u { return a_bits & 0x7fffu; }
    let a = low_decode(a_bits, p[6]);
    var result = 0.0;
    if p[7] >= 32u {
        let b_bits = (right[b_address / 2u] >> ((b_address & 1u) * 16u)) & 65535u;
        let b = low_decode(b_bits, p[6]);
        if p[7] == 36u { return low_encode(float_bits_min(a, b), p[6]); }
        if p[7] == 37u { return low_encode(float_bits_max(a, b), p[6]); }
        let x = bitcast<f32>(a);
        let y = bitcast<f32>(b);
        switch p[7] {
            case 32u: { result = x + y; }
            case 33u: { result = x - y; }
            case 34u: { result = x * y; }
            default: { result = x / y; }
        }
    } else {
        let x = bitcast<f32>(a);
        switch p[7] {
            case 2u: { result = x * x; }
            case 3u: { result = sqrt(x); }
            case 4u: { result = 1.0 / x; }
            case 5u: { result = exp(x); }
            case 6u: { result = log(x); }
            case 7u: { result = sin(x); }
            default: { result = cos(x); }
        }
    }
    return low_encode(bitcast<u32>(result), p[6]);
}
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var index = gid.x;
    while index < p[8] {
        let word = p[5] / 2u + index;
        var bits = output[word];
        for (var lane = 0u; lane < 2u; lane++) {
            let address = word * 2u + lane;
            if address >= p[5] && address - p[5] < p[0] {
                let value = result_bits(address - p[5]);
                let shift = lane * 16u;
                bits = (bits & ~(65535u << shift)) | (value << shift);
            }
        }
        output[word] = bits;
        if p[2] * 256u >= p[8] - index { break; }
        index += p[2] * 256u;
    }
}
