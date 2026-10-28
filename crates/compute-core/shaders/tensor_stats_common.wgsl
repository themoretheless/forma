const MAX_F32: f32 = 3.4028234663852886e38;

// Detect IEEE round-to-nearest multiplication overflow using the exact 48-bit
// significand product. Never ask the floating-point backend to form infinity.
fn product_overflows(a: f32, b: f32) -> bool {
    let ab = bitcast<u32>(a);
    let bb = bitcast<u32>(b);
    let exponent_sum = ((ab >> 23u) & 255u) + ((bb >> 23u) & 255u);
    if exponent_sum < 380u { return false; }
    if exponent_sum > 381u { return true; }
    let am = (ab & 0x7fffffu) | 0x800000u;
    let bm = (bb & 0x7fffffu) | 0x800000u;
    let low = (am & 65535u) * (bm & 65535u);
    let middle = (am >> 16u) * (bm & 65535u) + (am & 65535u) * (bm >> 16u);
    let product_low = low + (middle << 16u);
    let product_high = (am >> 16u) * (bm >> 16u) + (middle >> 16u)
        + select(0u, 1u, product_low < low);
    if exponent_sum == 381u {
        return product_high > 0x7fffu
            || (product_high == 0x7fffu && product_low >= 0xffc00000u);
    }
    return product_high == 0xffffu && product_low >= 0xff800000u;
}

// Halving first keeps subtraction finite even across opposite f32 extremes.
// Power-of-two scaling preserves ordinary representable differences; ordinary
// WGSL subnormal/underflow limits still apply.
fn half_shift(x: f32, maximum: f32) -> f32 {
    return float_half(x) - float_half(maximum);
}
fn shifted_exp(x: f32, maximum: f32) -> f32 {
    let half = half_shift(x, maximum);
    if product_overflows(abs(half), 2.0) { return 0.0; }
    return exp(half * 2.0);
}

fn scaled_delta(x: f32, anchor: f32, scale: f32) -> f32 {
    let delta = x - anchor;
    // Some shader backends implement division with a reciprocal and flush
    // subnormals. Keep its divisor away from both reciprocal extremes.
    if scale > 18446744073709551616.0 {
        return bitcast<f32>(float_down_bits(bitcast<u32>(delta), 64u)) / bitcast<f32>(float_down_bits(bitcast<u32>(scale), 64u));
    }
    if scale < 5.421010862427522e-20 {
        return float_up64(delta) / float_up64(scale);
    }
    return delta / scale;
}
