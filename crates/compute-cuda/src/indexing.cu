
template <typename T>
__device__ void copy_view_impl(const T* input, T* output, const U64* layout,
    U64 count, unsigned rank, U64 offset) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step)
        output[i] = input[index_of(i, layout, layout + rank, rank, offset)];
}
extern "C" __global__ void copy_u32(const unsigned* input, unsigned* output,
    const U64* layout, U64 count, unsigned rank, U64 offset) {
    copy_view_impl(input, output, layout, count, rank, offset);
}
extern "C" __global__ void copy_f32(const float* input, float* output,
    const U64* layout, U64 count, unsigned rank, U64 offset) {
    copy_view_impl(input, output, layout, count, rank, offset);
}

template <typename T>
__device__ void compare_impl(const T* a, const T* b, unsigned* output,
    const U64* layout, U64 count, unsigned rank, U64 offset_a, U64 offset_b, unsigned op) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const T x = a[index_of(i, layout, layout + rank, rank, offset_a)];
        const T y = b[index_of(i, layout, layout + 2ULL * rank, rank, offset_b)];
        unsigned keep = 0;
        switch (op) {
            case 0: keep = x == y; break;
            case 1: keep = x != y; break;
            case 2: keep = x < y; break;
            case 3: keep = x <= y; break;
            case 4: keep = x > y; break;
            case 5: keep = x >= y; break;
        }
        output[i] = keep;
    }
}
#define COMPARE(T, NAME) \
extern "C" __global__ void NAME(const T* a, const T* b, unsigned* output, \
    const U64* layout, U64 count, unsigned rank, U64 offset_a, U64 offset_b, unsigned op) { \
    compare_impl(a, b, output, layout, count, rank, offset_a, offset_b, op); \
}
COMPARE(float, compare_f32)
COMPARE(unsigned, compare_u32)

template <typename T>
__device__ void where_impl(const unsigned* mask, const T* yes, const T* no, T* output,
    const U64* layout, U64 count, unsigned rank, U64 om, U64 oy, U64 on) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const unsigned keep = mask[index_of(i, layout, layout + rank, rank, om)];
        output[i] = keep ? yes[index_of(i, layout, layout + 2ULL * rank, rank, oy)]
                         : no[index_of(i, layout, layout + 3ULL * rank, rank, on)];
    }
}
#define WHERE(T, NAME) \
extern "C" __global__ void NAME(const unsigned* mask, const T* yes, const T* no, T* output, \
    const U64* layout, U64 count, unsigned rank, U64 om, U64 oy, U64 on) { \
    where_impl(mask, yes, no, output, layout, count, rank, om, oy, on); \
}
WHERE(float, where_f32)
WHERE(unsigned, where_u32)

// One 256-lane work-efficient Blelloch scan per row/chunk. Rows enumerate all
// dimensions except the scan axis; axis positions follow the requested direction.
template <typename T> struct ScanIdentityLoad {
    __device__ T operator()(T value) const { return value; }
};
template <typename Input, typename T, typename Load>
__device__ void scan_blocks_impl(const Input* input, T* output, T* totals,
    const U64* layout, U64 rows, U64 length, U64 inner, U64 chunks,
    unsigned row_rank, U64 offset, U64 axis_stride, unsigned inclusive,
    unsigned reverse, T* shared, Load load) {
    const unsigned lane = threadIdx.x;
    for (U64 task = blockIdx.x; task < rows * chunks; task += gridDim.x) {
        const U64 row = task / chunks;
        const U64 position = (task % chunks) * 256 + lane;
        const U64 coordinate = position < length ? (reverse ? length - 1 - position : position) : 0;
        const U64 base = index_of(row, layout, layout + row_rank, row_rank, offset);
        const T value = position < length ? load(input[base + coordinate * axis_stride]) : T(0);
        shared[lane] = value;
        __syncthreads();
        for (unsigned stride = 1; stride < 256; stride <<= 1) {
            const unsigned end = (lane + 1) * 2 * stride - 1;
            if (end < 256) shared[end] += shared[end - stride];
            __syncthreads();
        }
        if (lane == 0) {
            totals[task] = shared[255];
            shared[255] = T(0);
        }
        __syncthreads();
        for (unsigned stride = 128; stride; stride >>= 1) {
            const unsigned end = (lane + 1) * 2 * stride - 1;
            if (end < 256) {
                const T left = shared[end - stride];
                shared[end - stride] = shared[end];
                shared[end] += left;
            }
            __syncthreads();
        }
        if (position < length) {
            const U64 destination = ((row / inner) * length + coordinate) * inner + row % inner;
            output[destination] = inclusive ? shared[lane] + value : shared[lane];
        }
        __syncthreads();
    }
}
#define SCAN(T, NAME) \
extern "C" __global__ void NAME(const T* input, T* output, T* totals, \
    const U64* layout, U64 rows, U64 length, U64 inner, U64 chunks, unsigned row_rank, \
    U64 offset, U64 axis_stride, unsigned inclusive, unsigned reverse) { \
    __shared__ T shared[256]; \
    scan_blocks_impl(input, output, totals, layout, rows, length, inner, chunks, \
        row_rank, offset, axis_stride, inclusive, reverse, shared, ScanIdentityLoad<T>{}); \
}
SCAN(float, scan_f32)
SCAN(unsigned, scan_u32)

template <typename T>
__device__ void add_scan_offsets_impl(T* output, const T* offsets, U64 count,
    U64 length, U64 inner, U64 chunks, unsigned reverse) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const U64 coordinate = (i / inner) % length;
        const U64 position = reverse ? length - 1 - coordinate : coordinate;
        const U64 row = (i / inner / length) * inner + i % inner;
        output[i] += offsets[row * chunks + position / 256];
    }
}
#define ADD_SCAN(T, NAME) \
extern "C" __global__ void NAME(T* output, const T* offsets, U64 count, \
    U64 length, U64 inner, U64 chunks, unsigned reverse) { \
    add_scan_offsets_impl(output, offsets, count, length, inner, chunks, reverse); \
}
ADD_SCAN(float, add_scan_f32)
ADD_SCAN(unsigned, add_scan_u32)

// Count each logical index exactly once, regardless of how many output copies
// use it. The host checks indices.numel <= u32::MAX before this accumulation.
extern "C" __global__ void invalid_indices(const unsigned* indices, unsigned* invalid,
    const U64* layout, U64 count, unsigned rank, U64 offset, U64 length) {
    __shared__ unsigned shared[256];
    const U64 step = (U64)blockDim.x * gridDim.x;
    unsigned local = 0;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step)
        local += (U64)indices[index_of(i, layout, layout + rank, rank, offset)] >= length;
    shared[threadIdx.x] = local;
    __syncthreads();
    for (unsigned stride = 128; stride; stride >>= 1) {
        if (threadIdx.x < stride) shared[threadIdx.x] += shared[threadIdx.x + stride];
        __syncthreads();
    }
    if (threadIdx.x == 0) atomicAdd(invalid, shared[0]);
}
template <typename T>
__device__ void gather_impl(const T* input, const unsigned* indices, T* output,
    const U64* layout, U64 count, U64 index_count, U64 length, U64 inner,
    unsigned input_rank, unsigned index_rank, U64 input_offset, U64 index_offset) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    const U64* index_dims = layout + 2ULL * input_rank;
    const U64* index_strides = index_dims + index_rank;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const U64 index_linear = (i / inner) % index_count;
        const U64 chosen = indices[index_of(index_linear, index_dims, index_strides, index_rank, index_offset)];
        if (chosen < length) {
            const U64 source_linear = ((i / inner / index_count) * length + chosen) * inner + i % inner;
            output[i] = input[index_of(source_linear, layout, layout + input_rank, input_rank, input_offset)];
        } else output[i] = T(0);
    }
}
#define GATHER(T, NAME) \
extern "C" __global__ void NAME(const T* input, const unsigned* indices, T* output, \
    const U64* layout, U64 count, U64 index_count, U64 length, U64 inner, \
    unsigned input_rank, unsigned index_rank, U64 input_offset, U64 index_offset) { \
    gather_impl(input, indices, output, layout, count, index_count, length, inner, \
        input_rank, index_rank, input_offset, index_offset); \
}
GATHER(float, gather_f32)
GATHER(unsigned, gather_u32)

extern "C" __global__ void normalize_mask(const unsigned* mask, unsigned* flags,
    const U64* layout, U64 count, unsigned rank, U64 offset) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step)
        flags[i] = mask[index_of(i, layout, layout + rank, rank, offset)] != 0;
}
template <typename T>
__device__ void compact_impl(const T* input, const unsigned* flags, const unsigned* prefix,
    T* output, unsigned* selected_count, const U64* layout, U64 count, unsigned rank, U64 offset) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        if (flags[i]) output[prefix[i]] = input[index_of(i, layout, layout + rank, rank, offset)];
        if (i == count - 1) selected_count[0] = prefix[i] + flags[i];
    }
}
#define COMPACT(T, NAME) \
extern "C" __global__ void NAME(const T* input, const unsigned* flags, const unsigned* prefix, \
    T* output, unsigned* selected_count, const U64* layout, U64 count, unsigned rank, U64 offset) { \
    compact_impl(input, flags, prefix, output, selected_count, layout, count, rank, offset); \
}
COMPACT(float, compact_f32)
COMPACT(unsigned, compact_u32)
