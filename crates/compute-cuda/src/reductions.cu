
// Operation discriminants match tensor_core::ReduceOp. The f32 infinities
// are internal identities: min/max reject empty contracted output on the host.
template <typename T> __device__ T reduce_identity(unsigned op);
template <> __device__ float reduce_identity<float>(unsigned op) {
    switch (op) {
        case 1: return 1.0f;
        case 2: return __int_as_float(0x7f800000);
        case 3: return __int_as_float((int)0xff800000u);
        default: return 0.0f;
    }
}
template <> __device__ unsigned reduce_identity<unsigned>(unsigned op) {
    switch (op) {
        case 1: return 1u;
        case 2: return 0xffffffffu;
        default: return 0u;
    }
}
template <typename T> __device__ T reduce_combine(T a, T b, unsigned op) {
    switch (op) {
        case 1: return a * b;
        case 2: return a < b ? a : b;
        case 3: return a > b ? a : b;
        default: return a + b;
    }
}
// Integer ordering selects the original f32 bits, including BF16 subnormal
// values loaded by low-precision reductions. Signed zeros are ordered -0 < +0
// so min/max preserve their specified signs. NaN is outside the contract.
__device__ unsigned reduce_float_key(float value) {
    const unsigned bits = __float_as_uint(value);
    return (bits & 0x80000000u) ? ~bits : bits ^ 0x80000000u;
}
template <> __device__ float reduce_combine<float>(float a, float b, unsigned op) {
    if (op == 2) return reduce_float_key(a) < reduce_float_key(b) ? a : b;
    if (op == 3) return reduce_float_key(a) > reduce_float_key(b) ? a : b;
    return op == 1 ? a * b : a + b;
}
template <typename T> struct ReduceIdentityLoad {
    __device__ T operator()(T value) const { return value; }
};
template <typename T> __device__ void reduce_block(T* shared, unsigned op) {
    __syncthreads();
    for (unsigned stride = 128; stride; stride >>= 1) {
        if (threadIdx.x < stride)
            shared[threadIdx.x] = reduce_combine(shared[threadIdx.x], shared[threadIdx.x + stride], op);
        __syncthreads();
    }
}

// Layout = outputDims, outputInputStrides, reduceDims, reduceInputStrides.
template <typename Input, typename T, typename Load>
__device__ void reduce_axes_impl(const Input* input, T* output,
    const U64* layout, U64 output_count, U64 reduce_count,
    unsigned output_rank, unsigned reduce_rank, U64 offset, unsigned op, T* shared, Load load) {
    for (U64 out = blockIdx.x; out < output_count; out += gridDim.x) {
        const U64 base = index_of(out, layout, layout + output_rank, output_rank, offset);
        const U64* dims = layout + 2ULL * output_rank;
        const U64* strides = dims + reduce_rank;
        T value = reduce_identity<T>(op);
        for (U64 r = threadIdx.x; r < reduce_count; r += blockDim.x)
            value = reduce_combine(value, load(input[index_of(r, dims, strides, reduce_rank, base)]), op);
        shared[threadIdx.x] = value;
        reduce_block(shared, op);
        if (threadIdx.x == 0) output[out] = shared[0];
        __syncthreads();
    }
}
#define REDUCE_AXES(T, NAME) \
extern "C" __global__ void NAME(const T* input, T* output, \
    const U64* layout, U64 output_count, U64 reduce_count, \
    unsigned output_rank, unsigned reduce_rank, U64 offset, unsigned op) { \
    __shared__ T shared[256]; \
    reduce_axes_impl(input, output, layout, output_count, reduce_count, \
        output_rank, reduce_rank, offset, op, shared, ReduceIdentityLoad<T>()); \
}
REDUCE_AXES(float, reduce_axes)
REDUCE_AXES(unsigned, reduce_axes_u32)

// Parallel first pass over strided input, followed by resident partial folds.
template <typename Input, typename T, typename Load>
__device__ void reduce_all_impl(const Input* input, T* output, const U64* layout,
    U64 count, unsigned rank, U64 offset, unsigned op, T* shared, Load load) {
    T value = reduce_identity<T>(op);
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step)
        value = reduce_combine(value, load(input[index_of(i, layout, layout + rank, rank, offset)]), op);
    shared[threadIdx.x] = value;
    reduce_block(shared, op);
    if (threadIdx.x == 0) output[blockIdx.x] = shared[0];
}
#define REDUCE_ALL(T, NAME) \
extern "C" __global__ void NAME(const T* input, T* output, const U64* layout, \
    U64 count, unsigned rank, U64 offset, unsigned op) { \
    __shared__ T shared[256]; \
    reduce_all_impl(input, output, layout, count, rank, offset, op, shared, ReduceIdentityLoad<T>()); \
}
REDUCE_ALL(float, reduce_all)
REDUCE_ALL(unsigned, reduce_all_u32)

template <typename T> __device__ void fill_impl(T* output, U64 count, T value) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step)
        output[i] = value;
}
extern "C" __global__ void fill_f32(float* output, U64 count, float value) { fill_impl(output, count, value); }
extern "C" __global__ void fill_u32(unsigned* output, U64 count, unsigned value) { fill_impl(output, count, value); }
