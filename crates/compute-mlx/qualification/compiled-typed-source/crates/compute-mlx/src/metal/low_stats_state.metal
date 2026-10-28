ulong rows = 1;
for (int d = 0; d < lo_ndim; ++d) rows *= ulong(lo_shape[d]);
for (ulong row = thread_position_in_grid.x; row < rows; row += threads_per_grid.x) {
    float lower = lo[row], upper = hi[row];
    uint magnitude = max(as_type<uint>(lower) & 0x7fffffffu, as_type<uint>(upper) & 0x7fffffffu);
    bool lifted = magnitude != 0 && magnitude < 0x1f800000u; // 2^-64
    if (lifted) {
        lower = low_power2(lower, 64);
        upper = low_power2(upper, 64);
    }
    float anchor = lower * 0.5f + upper * 0.5f;
    float scale = max(abs(lower - anchor), abs(upper - anchor));
    out[row * 4] = anchor;
    out[row * 4 + 1] = scale;
    out[row * 4 + 2] = float(lifted);
    out[row * 4 + 3] = 0.0f;
}
