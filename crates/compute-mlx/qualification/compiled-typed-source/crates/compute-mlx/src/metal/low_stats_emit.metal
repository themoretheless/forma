ulong count = ulong(params[1]) | (ulong(params[2]) << 32);
ulong size = 1;
for (int d = 0; d < inp_ndim; ++d) size *= ulong(inp_shape[d]);
for (ulong i = thread_position_in_grid.x; i < size; i += threads_per_grid.x) {
    ulong row = i / count;
    float value = low_decode(as_type<ushort>(inp[elem_to_loc(i, inp_shape, inp_strides, inp_ndim)]), BF);
    if (MODE == 0) out[i] = metal::precise::exp(value - state[row]) / center[row];
    else if (MODE == 1) out[i] = (value - state[row]) - metal::precise::log(center[row]);
    else {
        float anchor = state[row * 4], scale = state[row * 4 + 1];
        bool lifted = state[row * 4 + 2] != 0.0f;
        if (lifted) value = low_power2(value, 64);
        float centered = stats_scaled(value, anchor, scale) - center[row];
        out[i] = stats_normalized(centered, scale, variance[row], lifted, epsilon[0]);
    }
}
