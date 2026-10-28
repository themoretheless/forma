const MAX_F32: f32 = 3.4028234663852886e38;

// Exact exponent scaling prevents a compiler from cancelling the protective
// factors under fast-math reassociation and forming a subnormal reciprocal.
fn unit_ratio(numerator: f32, denominator: f32) -> f32 {
    if denominator > 18446744073709551616.0 {
        return bitcast<f32>(float_down_bits(bitcast<u32>(numerator), 64u)) / bitcast<f32>(float_down_bits(bitcast<u32>(denominator), 64u));
    }
    if denominator < 5.421010862427522e-20 {
        return float_up64(numerator) / float_up64(denominator);
    }
    return numerator / denominator;
}
fn shifted_exp(x: f32, maximum: f32) -> f32 {
    // A difference below -128 contributes less than the normal f32 range.
    // Halving first prevents overflow for opposite finite score extremes.
    let half = float_half(x) - float_half(maximum);
    if half < -64.0 { return 0.0; }
    return exp(half * 2.0);
}
