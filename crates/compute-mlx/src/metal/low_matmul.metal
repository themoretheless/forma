// Four 32-lane SIMD groups own one 16x16 output tile. Each group retains
// an 8x8 float accumulator; BF16 tiles containing subnormal operands take
// a group-uniform protected scalar path through shared accumulator storage.
const uint TILE = 16;
uint lane = thread_position_in_threadgroup.x;
uint sg = lane / 32, sg_row = (sg / 2) * 8, sg_column = (sg % 2) * 8;
ulong m = params[0], n = params[1], k = params[2];
ulong tiles_m = params[3], tiles_n = params[4];
ulong work = ulong(params[5]) | (ulong(params[6]) << 32);
threadgroup float tile_left[256];
threadgroup float tile_right[256];
threadgroup float tile_result[256];
threadgroup uint tiny_flags[4];
threadgroup ulong batch_bases[2];
ulong a_row_stride = ulong(left_strides[left_ndim - 2]);
ulong a_inner_stride = ulong(left_strides[left_ndim - 1]);
ulong b_inner_stride = ulong(right_strides[right_ndim - 2]);
ulong b_column_stride = ulong(right_strides[right_ndim - 1]);
for (ulong tile = threadgroup_position_in_grid.x; tile < work;
     tile += threadgroups_per_grid.x) {
    ulong batch = tile / (tiles_m * tiles_n);
    ulong row_base = ((tile / tiles_n) % tiles_m) * TILE;
    ulong column_base = (tile % tiles_n) * TILE;
    if (lane == 0) {
        batch_bases[0] = elem_to_loc(batch * m * k, left_shape, left_strides, left_ndim);
        batch_bases[1] = elem_to_loc(batch * k * n, right_shape, right_strides, right_ndim);
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
    simdgroup_float8x8 accumulator = make_filled_simdgroup_matrix<float, 8, 8>(0.0f);
    for (ulong inner = 0; inner < k; inner += TILE) {
        bool has_tiny = false;
        for (uint cell = lane; cell < TILE * TILE; cell += 128) {
            uint r = cell / TILE, c = cell % TILE;
            float a = 0.0f, b = 0.0f;
            if (row_base + r < m && inner + c < k) {
                ulong address = batch_bases[0] + (row_base + r) * a_row_stride
                    + (inner + c) * a_inner_stride;
                a = low_decode(as_type<ushort>(left[address]), BF);
            }
            if (column_base + c < n && inner + r < k) {
                ulong address = batch_bases[1] + (inner + r) * b_inner_stride
                    + (column_base + c) * b_column_stride;
                b = low_decode(as_type<ushort>(right[address]), BF);
            }
            tile_left[cell] = a;
            tile_right[cell] = b;
            if (BF) {
                uint a_bits = as_type<uint>(a) & 0x7fffffffu;
                uint b_bits = as_type<uint>(b) & 0x7fffffffu;
                has_tiny = has_tiny || (a_bits != 0 && a_bits < 0x800000u)
                                   || (b_bits != 0 && b_bits < 0x800000u);
            }
        }
        if (BF) {
            // Every SIMD lane must take part in simd_any before lane0 writes.
            bool group_tiny = simd_any(has_tiny);
            if ((lane & 31) == 0) tiny_flags[sg] = uint(group_tiny);
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
        if (BF && (tiny_flags[0] | tiny_flags[1] | tiny_flags[2] | tiny_flags[3]) != 0) {
            simdgroup_store(accumulator, tile_result + sg_row * TILE + sg_column, TILE);
            threadgroup_barrier(mem_flags::mem_threadgroup);
            for (uint cell = lane; cell < TILE * TILE; cell += 128) {
                uint r = cell / TILE, c = cell % TILE;
                float sum = tile_result[cell];
                for (uint j = 0; j < TILE; ++j)
                    sum += low_product(tile_left[r * TILE + j], tile_right[j * TILE + c]);
                tile_result[cell] = sum;
            }
            threadgroup_barrier(mem_flags::mem_threadgroup);
            simdgroup_load(accumulator, tile_result + sg_row * TILE + sg_column, TILE);
        } else {
            for (uint j = 0; j < TILE; j += 8) {
                simdgroup_float8x8 a, b;
                simdgroup_load(a, tile_left + sg_row * TILE + j, TILE);
                simdgroup_load(b, tile_right + j * TILE + sg_column, TILE);
                simdgroup_multiply_accumulate(accumulator, a, b, accumulator);
            }
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    simdgroup_store(accumulator, tile_result + sg_row * TILE + sg_column, TILE);
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for (uint cell = lane; cell < TILE * TILE; cell += 128) {
        ulong row = row_base + cell / TILE, column = column_base + cell % TILE;
        if (row < m && column < n)
            out[(batch * m + row) * n + column] = tile_result[cell];
    }
    threadgroup_barrier(mem_flags::mem_threadgroup);
}
