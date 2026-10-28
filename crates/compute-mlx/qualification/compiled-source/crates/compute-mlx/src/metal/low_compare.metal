ulong count = 1;
for (int d = 0; d < lhs_ndim; ++d) count *= ulong(lhs_shape[d]);
for (ulong i = thread_position_in_grid.x; i < count; i += threads_per_grid.x) {
    ushort a = as_type<ushort>(lhs[elem_to_loc(i, lhs_shape, lhs_strides, lhs_ndim)]);
    ushort b = as_type<ushort>(rhs[elem_to_loc(i, rhs_shape, rhs_strides, rhs_ndim)]);
    uint am = a & 0x7fffu, bm = b & 0x7fffu;
    uint infinity = BF ? 0x7f80u : 0x7c00u;
    bool unordered = am > infinity || bm > infinity;
    bool equal = a == b || (am == 0 && bm == 0);
    bool less = !equal && low_order(a) < low_order(b);
    bool greater = !equal && low_order(a) > low_order(b);
    bool result;
    if (OP == 0) result = !unordered && equal;
    else if (OP == 1) result = unordered || !equal;
    else if (OP == 2) result = !unordered && less;
    else if (OP == 3) result = !unordered && (less || equal);
    else if (OP == 4) result = !unordered && greater;
    else result = !unordered && (greater || equal);
    out[i] = uint(result);
}
