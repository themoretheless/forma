// Dense output tiles with explicit original input strides. Params contain
// M,N,K,tile-rows,tile-columns,total-tiles(lo,hi). All lanes participate in
// every barrier, including matrix tails and bounded-grid reuse.
const uint TILE = 16;
uint lane = thread_position_in_threadgroup.x;
uint row_lane = lane / TILE, column_lane = lane % TILE;
ulong m = params[0], n = params[1], k = params[2];
ulong tiles_m = params[3], tiles_n = params[4];
ulong work = ulong(params[5]) | (ulong(params[6]) << 32);
threadgroup float tile_left[256];
threadgroup float tile_right[256];
for (ulong tile = threadgroup_position_in_grid.x; tile < work;
     tile += threadgroups_per_grid.x) {
    ulong batch = tile / (tiles_m * tiles_n);
    ulong row = ((tile / tiles_n) % tiles_m) * TILE + row_lane;
    ulong column = (tile % tiles_n) * TILE + column_lane;
    float accumulator = 0.0f;
    for (ulong inner = 0; inner < k; inner += TILE) {
        float a = 0.0f, b = 0.0f;
        if (row < m && inner + column_lane < k) {
            ulong logical = (batch * m + row) * k + inner + column_lane;
            a = low_decode(as_type<ushort>(left[elem_to_loc(logical, left_shape, left_strides, left_ndim)]), BF);
        }
        if (column < n && inner + row_lane < k) {
            ulong logical = (batch * k + inner + row_lane) * n + column;
            b = low_decode(as_type<ushort>(right[elem_to_loc(logical, right_shape, right_strides, right_ndim)]), BF);
        }
        tile_left[lane] = a;
        tile_right[lane] = b;
        threadgroup_barrier(mem_flags::mem_threadgroup);
        for (uint j = 0; j < TILE; ++j) {
            float a = tile_left[row_lane * TILE + j];
            float b = tile_right[j * TILE + column_lane];
            if (BF) accumulator += low_product(a, b);
            else accumulator = metal::fma(a, b, accumulator);
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (row < m && column < n)
        out[(batch * m + row) * n + column] = accumulator;
    threadgroup_barrier(mem_flags::mem_threadgroup);
}
