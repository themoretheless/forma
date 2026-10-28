// Total order for finite IEEE f32 values. Integers preserve BF16 subnormals
// independently of GPU floating-point flush modes; -0 sorts below +0.
fn float_bits_key(bits: u32) -> u32 {
    if (bits & 0x80000000u) != 0u { return ~bits; }
    return bits | 0x80000000u;
}
fn float_bits_min(a: u32, b: u32) -> u32 {
    return select(a, b, float_bits_key(b) < float_bits_key(a));
}
fn float_bits_max(a: u32, b: u32) -> u32 {
    return select(a, b, float_bits_key(b) > float_bits_key(a));
}
