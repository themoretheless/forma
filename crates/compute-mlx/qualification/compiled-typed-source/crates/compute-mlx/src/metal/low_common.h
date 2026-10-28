// Exact storage conversions and finite extrema. No bfloat arithmetic is used:
// native Metal bfloat operations flush subnormals on the qualified MLX runtime.
inline float low_decode(ushort bits, bool bf) {
    return bf ? as_type<float>(uint(bits) << 16) : float(as_type<half>(bits));
}
inline ushort low_encode(float value, bool bf) {
    if (!bf) return as_type<ushort>(half(value));
    uint bits = as_type<uint>(value);
    if ((bits & 0x7fffffffu) > 0x7f800000u) return ushort(0x7fc0u);
    return ushort((bits + 0x7fffu + ((bits >> 16) & 1u)) >> 16);
}
inline uint low_order(ushort bits) {
    return (bits & 0x8000u) ? uint(ushort(~bits)) : uint(bits ^ 0x8000u);
}
inline uint float_order(float value) {
    uint bits = as_type<uint>(value);
    return (bits & 0x80000000u) ? ~bits : (bits ^ 0x80000000u);
}
inline float low_combine(float a, float b, uint op) {
    if (op == 0) return a + b;
    if (op == 1) return a * b;
    if (op == 2) return float_order(a) < float_order(b) ? a : b;
    return float_order(a) > float_order(b) ? a : b;
}
inline float low_identity(uint op) {
    if (op == 0) return 0.0f;
    if (op == 1) return 1.0f;
    return as_type<float>(op == 2 ? 0x7f800000u : 0xff800000u);
}

// Exact power-of-two scaling, including subnormal inputs/outputs. Integer
// normalization avoids losing tiny BF16 coordinates before centered math.
inline float low_power2(float value, int power) {
    uint bits = as_type<uint>(value), sign = bits & 0x80000000u;
    uint magnitude = bits & 0x7fffffffu;
    if (magnitude == 0 || magnitude >= 0x7f800000u) return value;
    int exponent = int(magnitude >> 23);
    uint significand = magnitude & 0x7fffffu;
    if (exponent == 0) {
        int shift = int(clz(significand)) - 8;
        significand <<= uint(shift);
        exponent = 1 - shift;
    } else significand |= 0x800000u;
    exponent += power;
    if (exponent >= 255) return as_type<float>(sign | 0x7f800000u);
    if (exponent > 0)
        return as_type<float>(sign | (uint(exponent) << 23) | (significand & 0x7fffffu));
    int shift = 1 - exponent;
    if (shift > 24) return as_type<float>(sign);
    uint rounded = significand >> uint(shift);
    uint remainder = significand & ((1u << uint(shift)) - 1u);
    uint halfway = 1u << uint(shift - 1);
    rounded += uint(remainder > halfway || (remainder == halfway && (rounded & 1u)));
    return as_type<float>(sign | rounded);
}

// Preserve normal products involving a decoded BF16 subnormal. Moving an
// exact power of two between operands avoids native input flushing. True
// subnormal arithmetic results still follow f32 underflow limits.
inline float low_product(float left, float right) {
    uint a = as_type<uint>(left) & 0x7fffffffu;
    uint b = as_type<uint>(right) & 0x7fffffffu;
    if (a != 0 && a < 0x800000u)
        return low_power2(left, 64) * low_power2(right, -64);
    if (b != 0 && b < 0x800000u)
        return low_power2(left, -64) * low_power2(right, 64);
    return left * right;
}

