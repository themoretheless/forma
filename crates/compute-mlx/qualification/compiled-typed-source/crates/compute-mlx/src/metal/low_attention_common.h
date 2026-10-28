// |value| <= scale. Keep reciprocal/operand exponents away from both f32
// limits, including a tiny numerator whose quotient is nevertheless normal.
inline float attention_unit(float value, float scale) {
    uint value_bits = as_type<uint>(value) & 0x7fffffffu;
    uint scale_bits = as_type<uint>(scale);
    if (scale_bits < 0x1f800000u ||
        (value_bits != 0 && value_bits < 0x800000u && scale_bits < 0x5f800000u))
        return low_power2(value, 64) / low_power2(scale, 64);
    if (scale_bits > 0x5f800000u)
        return low_power2(value, -64) / low_power2(scale, -64);
    return value / scale;
}
