inline float stats_scaled(float value, float anchor, float scale) {
    if (scale == 0.0f) return 0.0f;
    float delta = value - anchor;
    // Keep both a reciprocal implementation and precise division in range.
    if (scale > 0x1p64f) return (delta * 0x1p-64f) / (scale * 0x1p-64f);
    if (scale < 0x1p-64f) return (delta * 0x1p64f) / (scale * 0x1p64f);
    return delta / scale;
}

inline float stats_normalized(float centered, float scale, float variance,
                              bool lifted, float root_epsilon) {
    if (scale == 0.0f) return 0.0f;
    if (lifted) {
        // Lifted scales are <= 1. For minimum epsilon, the intermediate ratio
        // is at most 2^75; undoing the lift keeps the ratio below about 1500.
        float ratio = (scale / root_epsilon) * 0x1p-64f;
        if (ratio >= 1.0f) {
            float inverse = 1.0f / ratio;
            return centered / metal::precise::sqrt(variance + inverse * inverse);
        }
        return (centered * ratio) / metal::precise::sqrt(1.0f + variance * ratio * ratio);
    }
    if (scale >= root_epsilon) {
        float ratio = root_epsilon / scale;
        return centered / metal::precise::sqrt(variance + ratio * ratio);
    }
    float ratio = scale / root_epsilon;
    return (centered * ratio) / metal::precise::sqrt(1.0f + variance * ratio * ratio);
}
