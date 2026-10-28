
// Native u16 input and output; f32 arithmetic only in registers/shared partials.
extern "C" __global__ void unary_low(const unsigned short* input, unsigned short* output,
    const U64* layout, U64 count, unsigned rank, U64 offset, unsigned op, unsigned dtype) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const unsigned short bits = input[index_of(i, layout, layout + rank, rank, offset)];
        if (op == 0) output[i] = bits ^ 0x8000u;
        else if (op == 1) output[i] = bits & 0x7fffu;
        else output[i] = encode_low(apply_unary(decode_low(bits, dtype), op), dtype);
    }
}
extern "C" __global__ void binary_low(const unsigned short* a, const unsigned short* b,
    unsigned short* output, const U64* layout, U64 count, unsigned rank,
    U64 offset_a, U64 offset_b, unsigned op, unsigned dtype) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const unsigned short ab = a[index_of(i, layout, layout + rank, rank, offset_a)];
        const unsigned short bb = b[index_of(i, layout, layout + 2ULL * rank, rank, offset_b)];
        const float av = decode_low(ab, dtype), bv = decode_low(bb, dtype);
        if (op == 4) output[i] = reduce_float_key(av) < reduce_float_key(bv) ? ab : bb;
        else if (op == 5) output[i] = reduce_float_key(av) > reduce_float_key(bv) ? ab : bb;
        else output[i] = encode_low(apply_binary(av, bv, op), dtype);
    }
}
struct ReduceLowLoad {
    unsigned dtype;
    __device__ float operator()(unsigned short value) const { return decode_low(value, dtype); }
};
extern "C" __global__ void reduce_axes_low(const unsigned short* input, float* output,
    const U64* layout, U64 output_count, U64 reduce_count,
    unsigned output_rank, unsigned reduce_rank, U64 offset, unsigned op, unsigned dtype) {
    __shared__ float shared[256];
    reduce_axes_impl(input, output, layout, output_count, reduce_count,
        output_rank, reduce_rank, offset, op, shared, ReduceLowLoad{dtype});
}
extern "C" __global__ void reduce_all_low(const unsigned short* input, float* output,
    const U64* layout, U64 count, unsigned rank, U64 offset, unsigned op, unsigned dtype) {
    __shared__ float shared[256];
    reduce_all_impl(input, output, layout, count, rank, offset, op, shared, ReduceLowLoad{dtype});
}
