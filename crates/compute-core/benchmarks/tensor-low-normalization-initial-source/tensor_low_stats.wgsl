// Packed statistics append (dtype, original operation) after the shared row
// descriptors. Hierarchical summaries use two vec4 slots: values, lift flag.
fn low_stats_extra() -> u32 { return 13u + 3u * (params[1] + params[4]); }
fn low_extrema(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return bitcast<vec2<f32>>(vec2<u32>(
        float_bits_min(bitcast<u32>(a.x), bitcast<u32>(b.x)),
        float_bits_max(bitcast<u32>(a.y), bitcast<u32>(b.y))));
}
fn low_needs_lift(extrema: vec2<f32>) -> bool {
    let magnitude = max(bitcast<u32>(extrema.x) & 0x7fffffffu,
                        bitcast<u32>(extrema.y) & 0x7fffffffu);
    return params[low_stats_extra() + 1u] >= 3u && magnitude < 0x1f800000u;
}
// Exact x*2^64, used only for |x|<2^-64. Integer normalization avoids flushing
// a BF16 subnormal before it reaches the normal arithmetic coordinate system.
fn low_coordinate(x: f32, lifted: bool) -> f32 {
    if !lifted { return x; }
    let bits = bitcast<u32>(x);
    let sign = bits & 0x80000000u;
    let exponent = (bits >> 23u) & 255u;
    if exponent != 0u { return bitcast<f32>(bits + (64u << 23u)); }
    let mantissa = bits & 0x7fffffu;
    if mantissa == 0u { return x; }
    let leading = 31u - countLeadingZeros(mantissa);
    return bitcast<f32>(sign | ((leading + 42u) << 23u)
        | ((mantissa << (23u - leading)) & 0x7fffffu));
}
// Exact nearest-even division by 2^power (power=64 or128). Coordinates are
// finite. An already-subnormal coordinate cannot survive this downshift.
fn low_restore(bits: u32, lifted: bool, power: u32) -> u32 {
    if !lifted { return bits; }
    let sign = bits & 0x80000000u;
    let exponent = (bits >> 23u) & 255u;
    if exponent > power { return bits - (power << 23u); }
    let shift = power + 1u - exponent;
    if exponent == 0u || shift > 24u { return sign; }
    let significand = (bits & 0x7fffffu) | 0x800000u;
    let value = significand >> shift;
    let remainder = significand & ((1u << shift) - 1u);
    let midpoint = 1u << (shift - 1u);
    let round_up = remainder > midpoint || (remainder == midpoint && (value & 1u) != 0u);
    return sign | (value + select(0u, 1u, round_up));
}
fn low_normalized(centered: f32, variance: f32, scale: f32, sqrt_epsilon: f32, lifted: bool) -> f32 {
    if scale == 0.0 { return 0.0; }
    if lifted {
        // sqrt(eps) is normal, including for min-subnormal epsilon. Dividing
        // before undoing the lift avoids both an overflowing lifted epsilon
        // and subnormal operands when the final result is normal.
        let ratio = (scale / sqrt_epsilon) * 5.421010862427522e-20;
        if ratio >= 1.0 {
            let inverse = 1.0 / ratio;
            return centered / sqrt(variance + inverse * inverse);
        }
        return centered * ratio / sqrt(1.0 + variance * ratio * ratio);
    }
    if scale >= sqrt_epsilon {
        let ratio = sqrt_epsilon / scale;
        return centered / sqrt(variance + ratio * ratio);
    }
    let ratio = scale / sqrt_epsilon;
    return centered * ratio / sqrt(1.0 + variance * ratio * ratio);
}
