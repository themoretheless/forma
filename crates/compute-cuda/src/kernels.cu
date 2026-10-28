// Runtime compiled with NVRTC; no fast-math or implicit low-precision mode.
// All layout arithmetic is in scalar elements, never bytes.
typedef unsigned long long U64;

__device__ U64 index_of(U64 linear, const U64* dims, const U64* strides,
                       unsigned rank, U64 offset) {
    for (unsigned axis = rank; axis > 0; --axis) {
        const unsigned d = axis - 1;
        offset += (linear % dims[d]) * strides[d];
        linear /= dims[d];
    }
    return offset;
}

__device__ float apply_unary(float x, unsigned op) {
    switch (op) {
        case 0: return -x;
        case 1: return fabsf(x);
        case 2: return x * x;
        case 3: return sqrtf(x);
        case 4: return 1.0f / x;
        case 5: return expf(x);
        case 6: return logf(x);
        case 7: return sinf(x);
        case 8: return cosf(x);
        default: return x; // Internal identity used for materialization.
    }
}
__device__ float apply_binary(float a, float b, unsigned op) {
    switch (op) {
        case 0: return a + b;
        case 1: return a - b;
        case 2: return a * b;
        case 3: return a / b;
        case 4: return fminf(a, b);
        case 5: return fmaxf(a, b);
        default: return 0.0f;
    }
}

extern "C" __global__ void unary(const float* input, float* output,
    const U64* layout, U64 count, unsigned rank, U64 offset, unsigned op,
    float scale, float bias) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const U64 source = index_of(i, layout, layout + rank, rank, offset);
        const float value = apply_unary(input[source], op);
        output[i] = op == 9 ? value * scale + bias : (op == 11 ? value / scale : value);
    }
}

extern "C" __global__ void binary(const float* a, const float* b, float* output,
    const U64* layout, U64 count, unsigned rank, U64 offset_a, U64 offset_b, unsigned op) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const U64 ia = index_of(i, layout, layout + rank, rank, offset_a);
        const U64 ib = index_of(i, layout, layout + 2ULL * rank, rank, offset_b);
        output[i] = apply_binary(a[ia], b[ib], op);
    }
}

// Unsigned arithmetic deliberately wraps modulo 2^32. Division is rejected
// during host preflight; this entry shares the float binary layout ABI.
extern "C" __global__ void binary_u32(const unsigned* a, const unsigned* b, unsigned* output,
    const U64* layout, U64 count, unsigned rank, U64 offset_a, U64 offset_b, unsigned op) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const unsigned x = a[index_of(i, layout, layout + rank, rank, offset_a)];
        const unsigned y = b[index_of(i, layout, layout + 2ULL * rank, rank, offset_b)];
        switch (op) {
            case 0: output[i] = x + y; break;
            case 1: output[i] = x - y; break;
            case 2: output[i] = x * y; break;
            case 4: output[i] = x < y ? x : y; break;
            case 5: output[i] = x > y ? x : y; break;
            default: output[i] = 0; break;
        }
    }
}
