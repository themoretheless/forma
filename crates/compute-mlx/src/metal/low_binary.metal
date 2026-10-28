ulong count = 1;
for (int d = 0; d < lhs_ndim; ++d) count *= ulong(lhs_shape[d]);
for (ulong i = thread_position_in_grid.x; i < count; i += threads_per_grid.x) {
    ushort left = as_type<ushort>(lhs[elem_to_loc(i, lhs_shape, lhs_strides, lhs_ndim)]);
    ushort right = as_type<ushort>(rhs[elem_to_loc(i, rhs_shape, rhs_strides, rhs_ndim)]);
    ushort bits;
    if (OP == 4) bits = low_order(left) < low_order(right) ? left : right;
    else if (OP == 5) bits = low_order(left) > low_order(right) ? left : right;
    else {
        float a = low_decode(left, BF), b = low_decode(right, BF);
        float value;
        if (OP == 0) value = a + b;
        else if (OP == 1) value = a - b;
        else if (OP == 2) value = low_product(a, b);
        else value = a / b;
        bits = low_encode(value, BF);
    }
    out[i] = as_type<OUT>(bits);
}
