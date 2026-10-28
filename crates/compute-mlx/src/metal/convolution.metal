// One output scalar per workgroup, with contraction split across 256 lanes.
// All native strides are in elements; MLX binds each view's base offset.
uint lane = thread_position_in_threadgroup.x;
ulong work = ulong(params[0]) | (ulong(params[1]) << 32);
ulong spatial = ulong(params[2]) | (ulong(params[3]) << 32);
ulong channels = ulong(params[4]) | (ulong(params[5]) << 32);
ulong outputs_per_group = ulong(params[6]) | (ulong(params[7]) << 32);
ulong kernel_volume = ulong(params[8]) | (ulong(params[9]) << 32);
ulong contraction = ulong(params[10]) | (ulong(params[11]) << 32);
uint rank = input_ndim - 2;
ulong sizes[3], strides[3], dilations[3], padding[3];
for (uint axis = 0; axis < rank; ++axis) {
    uint at = 12 + 8 * axis;
    sizes[axis] = ulong(params[at]) | (ulong(params[at + 1]) << 32);
    strides[axis] = ulong(params[at + 2]) | (ulong(params[at + 3]) << 32);
    dilations[axis] = ulong(params[at + 4]) | (ulong(params[at + 5]) << 32);
    padding[axis] = ulong(params[at + 6]) | (ulong(params[at + 7]) << 32);
}
threadgroup float partials[256];
for (ulong output = threadgroup_position_in_grid.x; output < work;
     output += threadgroups_per_grid.x) {
    ulong channel = (output / spatial) % ulong(weight_shape[0]);
    ulong batch = output / spatial / ulong(weight_shape[0]);
    ulong group = channel / outputs_per_group;
    ulong index = output % spatial;
    ulong origins[3];
    for (uint axis = rank; axis-- > 0;) {
        origins[axis] = (index % sizes[axis]) * strides[axis];
        index /= sizes[axis];
    }
    float sum = 0.0f;
    for (ulong inner = lane; inner < contraction; inner += 256) {
        ulong local_channel = inner / kernel_volume;
        ulong location = inner % kernel_volume;
        ulong input_at = batch * ulong(input_strides[0])
            + (group * channels + local_channel) * ulong(input_strides[1]);
        ulong weight_at = channel * ulong(weight_strides[0])
            + local_channel * ulong(weight_strides[1]);
        bool valid = true;
        for (uint axis = rank; axis-- > 0;) {
            ulong coordinate = location % ulong(weight_shape[axis + 2]);
            location /= ulong(weight_shape[axis + 2]);
            weight_at += coordinate * ulong(weight_strides[axis + 2]);
            ulong position = origins[axis] + coordinate * dilations[axis];
            bool inside = position >= padding[axis]
                && position - padding[axis] < ulong(input_shape[axis + 2]);
            valid = valid && inside;
            if (inside) input_at += (position - padding[axis]) * ulong(input_strides[axis + 2]);
        }
        if (valid) {
            CONV_PRODUCT
        }
    }
    partials[lane] = sum;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for (uint step = 128; step > 0; step >>= 1) {
        if (lane < step) partials[lane] += partials[lane + step];
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (lane == 0) out[output] = partials[0];
    threadgroup_barrier(mem_flags::mem_threadgroup);
}
