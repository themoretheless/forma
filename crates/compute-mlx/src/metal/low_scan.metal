// Input is a metadata-only permutation with the scan axis last. Chunk sums
// and their exclusive f32 scan provide carries; the low input is decoded here
// directly for the second and final time. No f32 input expansion is allocated.
ulong rows = 1;
for (int d = 0; d < inp_ndim - 1; ++d) rows *= ulong(inp_shape[d]);
ulong count = inp_shape[inp_ndim - 1], parts = params[3];
ulong chunk = count / parts + ulong(count % parts != 0);
uint lane = thread_position_in_threadgroup.x;
threadgroup float values[256];
for (ulong group = threadgroup_position_in_grid.x; group < rows * parts;
     group += threadgroups_per_grid.x) {
    ulong row = group / parts, part = group % parts;
    ulong begin = min(part * chunk, count), end = min(begin + chunk, count);
    ulong j = begin + lane;
    float value = 0.0f;
    if (j < end) {
        ulong column = REVERSE ? count - 1 - j : j;
        ushort bits = as_type<ushort>(inp[elem_to_loc(row * count + column, inp_shape, inp_strides, inp_ndim)]);
        value = low_decode(bits, BF);
    }
    values[lane] = value;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for (uint offset = 1; offset < 256; offset <<= 1) {
        float previous = lane >= offset ? values[lane - offset] : 0.0f;
        // All old values must be read before any lane starts overwriting them.
        threadgroup_barrier(mem_flags::mem_threadgroup);
        if (lane >= offset) values[lane] += previous;
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (j < end) {
        float prefix = INCLUSIVE ? values[lane] : (lane > 0 ? values[lane - 1] : 0.0f);
        if (CARRY) prefix += carry[group];
        ulong column = REVERSE ? count - 1 - j : j;
        out[row * count + column] = prefix;
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
}
