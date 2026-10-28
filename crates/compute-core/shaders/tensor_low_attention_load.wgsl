// Dtype is specialized when the cached pipeline is compiled.
fn low_attention_bits(word: u32, address: u32) -> u32 {
    return low_decode((word >> (16u * (address % 2u))) & 0xffffu, LOW_DTYPE);
}
fn low_query_bits(address: u32) -> u32 { return low_attention_bits(query[address / 2u], address); }
fn low_key_bits(address: u32) -> u32 { return low_attention_bits(key[address / 2u], address); }
fn low_value(address: u32) -> f32 { return bitcast<f32>(low_attention_bits(value[address / 2u], address)); }
fn low_attention_product(a: u32, b: u32) -> f32 {
    // A BF16 subnormal times a large finite operand can be a normal f32.
    // Lift the subnormal before arithmetic and lower the other operand by
    // the same exact exponent distance. Integer boundaries prevent a fast
    // compiler from cancelling these protective factors.
    if (a & 0x7fffffffu) != 0u && (a & 0x7f800000u) == 0u {
        return float_up64(bitcast<f32>(a)) * bitcast<f32>(float_down_bits(b, 64u));
    }
    if (b & 0x7fffffffu) != 0u && (b & 0x7f800000u) == 0u {
        return bitcast<f32>(float_down_bits(a, 64u)) * float_up64(bitcast<f32>(b));
    }
    return bitcast<f32>(a) * bitcast<f32>(b);
}
