
__device__ unsigned low_scatter_key(unsigned value) {
    return (value & 0x8000U) ? ((~value) & 0xffffU) : (value ^ 0x8000U);
}
struct ScatterLowRawWrite {
    __device__ void operator()(unsigned short* address, unsigned short update, unsigned op) const {
        // Fresh output storage starts at a CUDA-aligned allocation and includes
        // an even number of u16 slots. Even an odd logical tail has a complete
        // physical word for this CAS; its neighboring halfword is preserved.
        const U64 pointer = reinterpret_cast<U64>(address);
        unsigned* word = reinterpret_cast<unsigned*>(pointer & ~3ULL);
        const unsigned shift = (unsigned)(pointer & 2ULL) * 8U;
        const unsigned mask = 0xffffU << shift;
        unsigned old = atomicCAS(word, 0U, 0U);
        unsigned assumed;
        do {
            assumed = old;
            const unsigned current = (assumed >> shift) & 0xffffU;
            unsigned next = update;
            if (op == 3) next = low_scatter_key(current) < low_scatter_key(update) ? current : update;
            else if (op == 4) next = low_scatter_key(current) > low_scatter_key(update) ? current : update;
            old = atomicCAS(word, assumed, (assumed & ~mask) | (next << shift));
        } while (old != assumed);
    }
};
struct ScatterLowFloatWrite {
    unsigned dtype;
    __device__ void operator()(float* address, unsigned short raw, unsigned op) const {
        const float update = decode_low(raw, dtype);
        // Owner election makes every winning Replace output unique.
        if (op == 0) { *address = update; return; }
        unsigned* bits = reinterpret_cast<unsigned*>(address);
        unsigned old = atomicCAS(bits, 0U, 0U);
        unsigned assumed;
        do {
            assumed = old;
            const float current = __uint_as_float(assumed);
            float next = current;
            if (op == 1) next = current + update;
            else if (op == 2) next = current * update;
            else if (op == 3) next = reduce_float_key(current) < reduce_float_key(update) ? current : update;
            else if (op == 4) next = reduce_float_key(current) > reduce_float_key(update) ? current : update;
            old = atomicCAS(bits, assumed, __float_as_uint(next));
        } while (old != assumed);
    }
};

extern "C" __global__ void scatter_low_raw(const unsigned* indices, const unsigned short* updates,
    unsigned short* output, const unsigned* owners, const U64* layout, U64 count,
    U64 index_count, U64 length, U64 inner, unsigned update_rank, unsigned index_rank,
    U64 update_offset, U64 index_offset, unsigned op) {
    scatter_impl(indices, updates, output, owners, layout, count, index_count, length, inner,
        update_rank, index_rank, update_offset, index_offset, op, ScatterLowRawWrite{});
}
extern "C" __global__ void scatter_low_f32(const unsigned* indices, const unsigned short* updates,
    float* output, const unsigned* owners, const U64* layout, U64 count,
    U64 index_count, U64 length, U64 inner, unsigned update_rank, unsigned index_rank,
    U64 update_offset, U64 index_offset, unsigned op, unsigned dtype) {
    scatter_impl(indices, updates, output, owners, layout, count, index_count, length, inner,
        update_rank, index_rank, update_offset, index_offset, op, ScatterLowFloatWrite{dtype});
}
