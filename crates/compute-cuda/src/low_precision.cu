
// Header-free conversion keeps NVRTC deployment independent of toolkit headers.
// F16 PTX conversion is RN ties-to-even and preserves representable subnormals.
// BF16 uses equivalent integer rounding on every supported target, including
// targets older than SM80 where BF16 GEMM itself is explicitly unavailable.
__device__ unsigned short encode_low(float value, unsigned dtype) {
    if (dtype == 0) {
        unsigned short bits;
        asm("cvt.rn.f16.f32 %0, %1;" : "=h"(bits) : "f"(value));
        return bits;
    }
    const unsigned bits = __float_as_uint(value);
    if ((bits & 0x7fffffffU) > 0x7f800000U)
        return (unsigned short)((bits >> 16) | 0x0040U);
    return (unsigned short)((bits + 0x7fffU + ((bits >> 16) & 1U)) >> 16);
}
__device__ float decode_low(unsigned short bits, unsigned dtype) {
    if (dtype == 0) {
        float value;
        asm("cvt.f32.f16 %0, %1;" : "=f"(value) : "h"(bits));
        return value;
    }
    return __uint_as_float((unsigned)bits << 16);
}
extern "C" __global__ void copy_low(const unsigned short* input, unsigned short* output,
    const U64* layout, U64 count, unsigned rank, U64 offset) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step)
        output[i] = input[index_of(i, layout, layout + rank, rank, offset)];
}
extern "C" __global__ void cast_to_low(const float* input, unsigned short* output,
    const U64* layout, U64 count, unsigned rank, U64 offset, unsigned dtype) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step)
        output[i] = encode_low(input[index_of(i, layout, layout + rank, rank, offset)], dtype);
}
extern "C" __global__ void cast_from_low(const unsigned short* input, float* output,
    const U64* layout, U64 count, unsigned rank, U64 offset, unsigned dtype) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step)
        output[i] = decode_low(input[index_of(i, layout, layout + rank, rank, offset)], dtype);
}
