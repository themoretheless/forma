// One completed output per workgroup for canonical M==1 or N==1. Params
// share the GEMM ABI, but work(lo,hi) is the output count. Decode view bases
// once, then partition K over256 lanes with fixed native contraction strides.
uint lane = thread_position_in_threadgroup.x;
ulong m = params[0], n = params[1], k = params[2];
ulong work = ulong(params[5]) | (ulong(params[6]) << 32);
ulong a_stride = ulong(left_strides[left_ndim - 1]);
ulong b_stride = ulong(right_strides[right_ndim - 2]);
threadgroup ulong bases[2];
threadgroup float partials[256];
for (ulong output = threadgroup_position_in_grid.x; output < work;
     output += threadgroups_per_grid.x) {
    if (lane == 0) {
        ulong batch = output / (m * n);
        ulong row = (output / n) % m, column = output % n;
        bases[0] = elem_to_loc((batch * m + row) * k, left_shape, left_strides, left_ndim);
        bases[1] = elem_to_loc(batch * k * n + column, right_shape, right_strides, right_ndim);
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
    float sum = 0.0f;
    for (ulong inner = lane; inner < k; inner += 256) {
        float a = low_decode(as_type<ushort>(left[bases[0] + inner * a_stride]), BF);
        float b = low_decode(as_type<ushort>(right[bases[1] + inner * b_stride]), BF);
        if (BF) sum += low_product(a, b);
        else sum = metal::fma(a, b, sum);
    }
    partials[lane] = sum;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for (uint step = 128; step > 0; step >>= 1) {
        if (lane < step) partials[lane] += partials[lane + step];
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (lane == 0) out[output] = partials[0];
    // Protect both partials and bases before a bounded group owns another row.
    threadgroup_barrier(mem_flags::mem_threadgroup);
}
