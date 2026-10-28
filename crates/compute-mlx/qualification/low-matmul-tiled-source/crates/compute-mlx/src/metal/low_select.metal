ulong count = 1;
for (int d = 0; d < lhs_ndim; ++d) count *= ulong(lhs_shape[d]);
for (ulong i = thread_position_in_grid.x; i < count; i += threads_per_grid.x) {
    uint flag = mask[elem_to_loc(i, mask_shape, mask_strides, mask_ndim)];
    ushort a = as_type<ushort>(lhs[elem_to_loc(i, lhs_shape, lhs_strides, lhs_ndim)]);
    ushort b = as_type<ushort>(rhs[elem_to_loc(i, rhs_shape, rhs_strides, rhs_ndim)]);
    out[i] = as_type<OUT>(flag != 0 ? a : b);
}
