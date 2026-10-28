ulong count = 1;
for (int d = 0; d < inp_ndim; ++d) count *= ulong(inp_shape[d]);
for (ulong i = thread_position_in_grid.x; i < count; i += threads_per_grid.x) {
    ushort bits = as_type<ushort>(inp[elem_to_loc(i, inp_shape, inp_strides, inp_ndim)]);
    if (OP == 0) bits ^= ushort(0x8000);
    else if (OP == 1) bits &= ushort(0x7fff);
    else {
        float x = low_decode(bits, BF);
        float value;
        if (OP == 2) value = x * x;
        else if (OP == 3) value = metal::precise::sqrt(x);
        else if (OP == 4) value = 1.0f / x;
        else if (OP == 5) value = metal::precise::exp(x);
        else if (OP == 6) value = metal::precise::log(x);
        else if (OP == 7) value = metal::precise::sin(x);
        else value = metal::precise::cos(x);
        bits = low_encode(value, BF);
    }
    out[i] = as_type<OUT>(bits);
}
