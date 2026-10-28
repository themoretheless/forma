ulong rows = 1;
for (int d = 0; d < center_ndim; ++d) rows *= ulong(center_shape[d]);
for (ulong row = thread_position_in_grid.x; row < rows; row += threads_per_grid.x) {
    if (MODE == 0) out[row] = state[row] + metal::precise::log(center[row]);
    else {
        float anchor = state[row * 4], scale = state[row * 4 + 1];
        bool lifted = state[row * 4 + 2] != 0.0f;
        if (MODE == 1) out[row] = low_power2(anchor + scale * center[row], lifted ? -64 : 0);
        else out[row] = low_power2(scale * (scale * center[row]), lifted ? -128 : 0);
    }
}
