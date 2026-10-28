// Fresh zero-initialized atomic u32 output: heads[extent], next[index_count],
// optional padding. Each logical index contributes one unique one-based token.
// No thread traverses a list in this dispatch. The dependent consumer starts
// only after every head and next write has completed, so publishing the head
// before next[token] is initialized cannot expose an incomplete list to it.
ulong extent = params[0], count = params[1];
for (ulong i = thread_position_in_grid.x; i < count; i += threads_per_grid.x) {
    uint destination = indices[elem_to_loc(i, indices_shape, indices_strides, indices_ndim)];
    if (ulong(destination) < extent) {
        uint previous = atomic_exchange_explicit(&out[destination], uint(i + 1), memory_order_relaxed);
        atomic_store_explicit(&out[extent + i], previous, memory_order_relaxed);
    }
}
