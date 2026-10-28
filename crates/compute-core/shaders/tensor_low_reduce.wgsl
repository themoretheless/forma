// Shared tensor_reduce traversal is specialized to IEEE bits. Header12 adds
// dtype and input kind (0: packed16, 1: f32-bit partials) to its first10 words.
fn low_load_value(address: u32) -> u32 {
    if params[11] == 1u { return input[address]; }
    let bits = (input[address / 2u] >> ((address & 1u) * 16u)) & 65535u;
    return low_decode(bits, params[10]);
}
fn low_finish_value(bits: u32) -> u32 {
    if params[9] == 1u { return bits; }
    return bitcast<u32>(bitcast<f32>(bits) / f32(params[9]));
}
