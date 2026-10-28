
// Each axis destination records the greatest logical index position + 1.
// Position zero is represented by 1; validated index_count <= u32::MAX keeps
// the sentinel and every position distinct. Invalid indices never access owners.
extern "C" __global__ void scatter_owners(const unsigned* indices, unsigned* owners,
    const U64* layout, U64 count, unsigned rank, U64 offset, U64 length) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 j = (U64)blockIdx.x * blockDim.x + threadIdx.x; j < count; j += step) {
        const U64 chosen = indices[index_of(j, layout, layout + rank, rank, offset)];
        if (chosen < length) atomicMax(owners + chosen, (unsigned)(j + 1));
    }
}

// CAS performs f32 arithmetic with the module's --ftz=false policy. Native
// global atom.add.f32 flushes subnormal inputs/results even without fast math.
// Integer bit comparison makes retry termination independent of float equality.
__device__ void scatter_fold(float* address, float update, unsigned op) {
    unsigned* bits = reinterpret_cast<unsigned*>(address);
    unsigned old = atomicCAS(bits, 0u, 0u);
    unsigned assumed;
    do {
        assumed = old;
        const float current = __uint_as_float(assumed);
        float next = current;
        switch (op) {
            case 1: next = current + update; break;
            case 2: next = current * update; break;
            case 3: next = current < update ? current : update; break;
            case 4: next = current > update ? current : update; break;
        }
        old = atomicCAS(bits, assumed, __float_as_uint(next));
    } while (old != assumed);
}
__device__ void scatter_fold(unsigned* address, unsigned update, unsigned op) {
    switch (op) {
        case 1: atomicAdd(address, update); break;
        case 3: atomicMin(address, update); break;
        case 4: atomicMax(address, update); break;
        case 2: {
            unsigned old = atomicCAS(address, 0u, 0u);
            unsigned assumed;
            do {
                assumed = old;
                old = atomicCAS(address, assumed, assumed * update);
            } while (old != assumed);
            break;
        }
    }
}

// The update logical shape is input-prefix + index-shape + input-suffix.
// Output is a distinct contiguous copy of the base. Owners are accessed only
// for Replace; for every prefix/suffix there is exactly one winning update.
template <typename T> struct ScatterTypedWrite {
    __device__ void operator()(T* address, T update, unsigned op) const {
        if (op == 0) *address = update;
        else scatter_fold(address, update, op);
    }
};
template <typename Input, typename Output, typename Write>
__device__ void scatter_impl(const unsigned* indices, const Input* updates, Output* output,
    const unsigned* owners, const U64* layout, U64 count, U64 index_count,
    U64 length, U64 inner, unsigned update_rank, unsigned index_rank,
    U64 update_offset, U64 index_offset, unsigned op, Write write) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    const U64* index_dims = layout + 2ULL * update_rank;
    const U64* index_strides = index_dims + index_rank;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const U64 j = (i / inner) % index_count;
        const U64 chosen = indices[index_of(j, index_dims, index_strides, index_rank, index_offset)];
        if (chosen < length && (op != 0 || owners[chosen] == (unsigned)(j + 1))) {
            const U64 destination = ((i / inner / index_count) * length + chosen) * inner + i % inner;
            const Input update = updates[index_of(i, layout, layout + update_rank, update_rank, update_offset)];
            write(output + destination, update, op);
        }
    }
}
#define SCATTER(T, NAME) \
extern "C" __global__ void NAME(const unsigned* indices, const T* updates, T* output, \
    const unsigned* owners, const U64* layout, U64 count, U64 index_count, U64 length, \
    U64 inner, unsigned update_rank, unsigned index_rank, U64 update_offset, U64 index_offset, unsigned op) { \
    scatter_impl(indices, updates, output, owners, layout, count, index_count, length, inner, \
        update_rank, index_rank, update_offset, index_offset, op, ScatterTypedWrite<T>{}); \
}
SCATTER(float, scatter_f32)
SCATTER(unsigned, scatter_u32)
