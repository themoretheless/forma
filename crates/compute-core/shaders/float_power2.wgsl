// Exact x*2^64, used only for |x|<2^-64. Integer normalization avoids flushing
// a BF16 subnormal before it reaches the normal arithmetic coordinate system.
fn float_up64(x: f32) -> f32 {
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
// Exact nearest-even division by 2^power. Integer operations also prevent
// the backend from reassociating a safe midpoint or normalization ratio.
fn float_down_bits(bits: u32, power: u32) -> u32 {
    let sign = bits & 0x80000000u;
    let exponent = (bits >> 23u) & 255u;
    if exponent > power { return bits - (power << 23u); }
    let shift = select(power + 1u - exponent, power, exponent == 0u);
    if shift > 24u { return sign; }
    let significand = (bits & 0x7fffffu) | select(0x800000u, 0u, exponent == 0u);
    let value = significand >> shift;
    let remainder = significand & ((1u << shift) - 1u);
    let midpoint = 1u << (shift - 1u);
    let round_up = remainder > midpoint || (remainder == midpoint && (value & 1u) != 0u);
    return sign | (value + select(0u, 1u, round_up));
}

fn float_half(x: f32) -> f32 { return bitcast<f32>(float_down_bits(bitcast<u32>(x), 1u)); }

