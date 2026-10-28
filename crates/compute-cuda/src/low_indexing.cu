
// Integer IEEE ordering avoids arithmetic on subnormal values and signalling
// NaNs. Both formats share sign-magnitude ordering outside the NaN range.
__device__ unsigned compare_low_bits(unsigned short x, unsigned short y,
    unsigned op, unsigned dtype) {
    const unsigned infinity = dtype == 0 ? 0x7c00U : 0x7f80U;
    const unsigned mx = x & 0x7fffU, my = y & 0x7fffU;
    if (mx > infinity || my > infinity) return op == 1;
    const bool equal = x == y || (mx == 0 && my == 0);
    const unsigned kx = (x & 0x8000U) ? ((~x) & 0xffffU) : (x ^ 0x8000U);
    const unsigned ky = (y & 0x8000U) ? ((~y) & 0xffffU) : (y ^ 0x8000U);
    switch (op) {
        case 0: return equal;
        case 1: return !equal;
        case 2: return !equal && kx < ky;
        case 3: return equal || kx < ky;
        case 4: return !equal && kx > ky;
        case 5: return equal || kx > ky;
    }
    return 0;
}
extern "C" __global__ void compare_low(const unsigned short* a, const unsigned short* b,
    unsigned* output, const U64* layout, U64 count, unsigned rank,
    U64 offset_a, U64 offset_b, unsigned op, unsigned dtype) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const unsigned short x = a[index_of(i, layout, layout + rank, rank, offset_a)];
        const unsigned short y = b[index_of(i, layout, layout + 2ULL * rank, rank, offset_b)];
        output[i] = compare_low_bits(x, y, op, dtype);
    }
}

// Raw movement specializes the established checked traversal. No floating-point
// operation can change NaN payloads, zero signs, or low subnormal bits.
WHERE(unsigned short, where_low)
GATHER(unsigned short, gather_low)
COMPACT(unsigned short, compact_low)

struct ScanLowLoad {
    unsigned dtype;
    __device__ float operator()(unsigned short value) const {
        return decode_low(value, dtype);
    }
};
extern "C" __global__ void scan_low_f32(const unsigned short* input, float* output,
    float* totals, const U64* layout, U64 rows, U64 length, U64 inner, U64 chunks,
    unsigned row_rank, U64 offset, U64 axis_stride, unsigned inclusive,
    unsigned reverse, unsigned dtype) {
    __shared__ float shared[256];
    scan_blocks_impl(input, output, totals, layout, rows, length, inner, chunks,
        row_rank, offset, axis_stride, inclusive, reverse, shared, ScanLowLoad{dtype});
}
