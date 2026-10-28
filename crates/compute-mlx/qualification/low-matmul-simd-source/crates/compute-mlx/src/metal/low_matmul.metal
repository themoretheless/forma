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
for (ulong tile = threadgroup_position_in_grid.x; tile < work;
     tile += threadgroups_per_grid.x) {
    ulong batch = tile / (tiles_m * tiles_n);
    ulong row_base = ((tile / tiles_n) % tiles_m) * TILE;
    ulong column_base = (tile % tiles_n) * TILE;
    simdgroup_float8x8 accumulator = make_filled_simdgroup_matrix<float, 8, 8>(0.0f);
    for (ulong inner = 0; inner < k; inner += TILE) {
        bool has_tiny = false;
        for (uint cell = lane; cell < TILE * TILE; cell += 128) {
            uint r = cell / TILE, c = cell % TILE;
            float a = 0.0f, b = 0.0f;
            if (row_base + r < m && inner + c < k) {
                ulong logical = (batch * m + row_base + r) * k + inner + c;
                a = low_decode(as_type<ushort>(left[elem_to_loc(logical, left_shape, left_strides, left_ndim)]), BF);
            }
            if (column_base + c < n && inner + r < k) {
                ulong logical = (batch * k + inner + r) * n + column_base + c;
                b = low_decode(as_type<ushort>(right[elem_to_loc(logical, right_shape, right_strides, right_ndim)]), BF);
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
