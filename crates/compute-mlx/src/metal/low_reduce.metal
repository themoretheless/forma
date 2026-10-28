// params: uncontracted rank, original contraction low/high words, parts.
// The first pass consumes a metadata-only permutation of the original low
// input. The second pass consumes its bounded f32 partials.
ulong rows = 1, count = 1;
for (int d = 0; d < inp_ndim; ++d) {
    if (uint(d) < params[0]) rows *= ulong(inp_shape[d]);
    else count *= ulong(inp_shape[d]);
}
ulong parts = params[3];
ulong chunk = count / parts + ulong(count % parts != 0);
uint lane = thread_position_in_threadgroup.x;
threadgroup float values[256];
for (ulong group = threadgroup_position_in_grid.x; group < rows * parts;
     group += threadgroups_per_grid.x) {
    ulong row = group / parts, part = group % parts;
    ulong begin = min(part * chunk, count);
    ulong end = min(begin + chunk, count);
    PREPARE
    float total = low_identity(OP);
    for (ulong j = begin + lane; j < end; j += 256) {
        ulong column = REVERSE ? count - 1 - j : j;
        auto loaded = inp[elem_to_loc(row * count + column, inp_shape, inp_strides, inp_ndim)];
        float value = LOAD;
        TRANSFORM
        total = low_combine(total, value, OP);
    }
    values[lane] = total;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for (uint step = 128; step > 0; step >>= 1) {
        if (lane < step) values[lane] = low_combine(values[lane], values[lane + step], OP);
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (lane == 0) {
        float value = values[0];
        if (MEAN) value /= float(ulong(params[1]) | (ulong(params[2]) << 32));
        out[group] = value;
    }
    // Every lane must finish consuming shared storage before the next row.
    threadgroup_barrier(mem_flags::mem_threadgroup);
}
