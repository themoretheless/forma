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
fn low_coordinate(x: f32, lifted: bool) -> f32 {
    if lifted { return float_up64(x); }
    return x;
}
fn low_restore(bits: u32, lifted: bool, power: u32) -> u32 {
    if lifted { return float_down_bits(bits, power); }
    return bits;
}
fn low_normalized(centered: f32, variance: f32, scale: f32, sqrt_epsilon: f32, lifted: bool) -> f32 {
    if scale == 0.0 { return 0.0; }
    if lifted {
        // sqrt(eps) is normal, including for min-subnormal epsilon. Dividing
        // before undoing the lift avoids both an overflowing lifted epsilon
        // and subnormal operands when the final result is normal.
        let ratio = bitcast<f32>(low_restore(bitcast<u32>(scale / sqrt_epsilon), true, 64u));
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

