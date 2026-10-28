ulong extent = params[0], index_count = params[1];
ulong inner = ulong(params[2]) | (ulong(params[3]) << 32);
ulong count = 1;
for (int d = 0; d < base_ndim; ++d) count *= ulong(base_shape[d]);
for (ulong i = thread_position_in_grid.x; i < count; i += threads_per_grid.x) {
    ushort selected = as_type<ushort>(base[elem_to_loc(i, base_shape, base_strides, base_ndim)]);
    float total = low_decode(selected, BF);
    ulong destination = (i / inner) % extent;
    ulong before = i / inner / extent, after = i % inner;
    uint node = lists[destination];
    while (node != 0) {
        ulong index = ulong(node) - 1;
        ulong update = (before * index_count + index) * inner + after;
        ushort bits = as_type<ushort>(updates[elem_to_loc(update, updates_shape, updates_strides, updates_ndim)]);
        if (OP == 3) selected = low_order(selected) < low_order(bits) ? selected : bits;
        else if (OP == 4) selected = low_order(selected) > low_order(bits) ? selected : bits;
        else if (OP == 1) total += low_decode(bits, BF);
        else total *= low_decode(bits, BF);
        node = lists[extent + index];
    }
    STORE
}
