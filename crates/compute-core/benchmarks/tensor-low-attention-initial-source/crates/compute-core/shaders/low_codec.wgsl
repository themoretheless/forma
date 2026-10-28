// Integer codecs preserve subnormals, signed zeros and infinities independently
// of floating-point flush modes. Casts canonicalize NaNs to quiet NaNs.
fn low_encode(bits: u32, dtype: u32) -> u32 {
    let magnitude = bits & 0x7fffffffu;
    let sign = (bits >> 16u) & 0x8000u;
    if dtype == 1u {
        if magnitude > 0x7f800000u { return sign | 0x7fc0u; }
        return (bits + 0x7fffu + ((bits >> 16u) & 1u)) >> 16u;
    }
    if magnitude >= 0x7f800000u {
        return sign | select(0x7c00u, 0x7e00u, magnitude > 0x7f800000u);
    }
    let exponent = (bits >> 23u) & 0xffu;
    let mantissa = bits & 0x7fffffu;
    if exponent >= 143u { return sign | 0x7c00u; }
    if exponent < 102u { return sign; }
    var significand = mantissa;
    var shift = 13u;
    var result = 0u;
    if exponent >= 113u {
        result = (exponent - 112u) << 10u;
    } else {
        significand |= 0x800000u;
        shift = 126u - exponent;
    }
    result |= significand >> shift;
    let remainder = significand & ((1u << shift) - 1u);
    let halfway = 1u << (shift - 1u);
    if remainder > halfway || (remainder == halfway && (result & 1u) != 0u) {
        result += 1u;
    }
    return sign | result;
}

fn low_decode(bits: u32, dtype: u32) -> u32 {
    if dtype == 1u { return bits << 16u; }
    let sign = (bits & 0x8000u) << 16u;
    let exponent = (bits >> 10u) & 0x1fu;
    let mantissa = bits & 0x3ffu;
    if exponent == 0x1fu { return sign | 0x7f800000u | (mantissa << 13u); }
    if exponent != 0u { return sign | ((exponent + 112u) << 23u) | (mantissa << 13u); }
    if mantissa == 0u { return sign; }
    let highest = 31u - countLeadingZeros(mantissa);
    return sign | ((highest + 103u) << 23u) | ((mantissa << (23u - highest)) & 0x7fffffu);
}
