// Binary64 uses the common layout traversal and reduction tree. No value or
// arithmetic scalar passes through float; integer indexing stays U64.
__device__ U64 reduce_double_key(double value) {
    const U64 bits = (U64)__double_as_longlong(value);
    return (bits & 0x8000000000000000ULL) ? ~bits : bits ^ 0x8000000000000000ULL;
}
template <> __device__ double reduce_identity<double>(unsigned op) {
    switch (op) {
        case 1: return 1.0;
        case 2: return __longlong_as_double(0x7ff0000000000000LL);
        case 3: return __longlong_as_double((long long)0xfff0000000000000ULL);
        default: return 0.0;
    }
}
template <> __device__ double reduce_combine<double>(double a, double b, unsigned op) {
    if (op == 2) return reduce_double_key(a) < reduce_double_key(b) ? a : b;
    if (op == 3) return reduce_double_key(a) > reduce_double_key(b) ? a : b;
    return op == 1 ? a * b : a + b;
}
REDUCE_AXES(double, reduce_axes_f64)
REDUCE_ALL(double, reduce_all_f64)
extern "C" __global__ void fill_f64(double* output, U64 count, double value) {
    fill_impl(output, count, value);
}
__device__ double apply_unary_f64(double value, unsigned op, double scale, double bias) {
    switch (op) {
        case 0: return -value;
        case 1: return fabs(value);
        case 2: return value * value;
        case 3: return sqrt(value);
        case 4: return 1.0 / value;
        case 5: return exp(value);
        case 6: return log(value);
        case 7: return sin(value);
        case 8: return cos(value);
        case 9: return value * scale + bias;
        case 11: return value / scale;
        default: return value;
    }
}
extern "C" __global__ void unary_f64(const double* input, double* output,
    const U64* layout, U64 count, unsigned rank, U64 offset, unsigned op, double scale, double bias) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const double value = input[index_of(i, layout, layout + rank, rank, offset)];
        output[i] = apply_unary_f64(value, op, scale, bias);
    }
}
extern "C" __global__ void binary_f64(const double* a, const double* b, double* output,
    const U64* layout, U64 count, unsigned rank, U64 offset_a, U64 offset_b, unsigned op) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const double left = a[index_of(i, layout, layout + rank, rank, offset_a)];
        const double right = b[index_of(i, layout, layout + 2ULL * rank, rank, offset_b)];
        switch (op) {
            case 0: output[i] = left + right; break;
            case 1: output[i] = left - right; break;
            case 2: output[i] = left * right; break;
            case 3: output[i] = left / right; break;
            case 4: output[i] = reduce_combine(left, right, 2); break;
            case 5: output[i] = reduce_combine(left, right, 3); break;
        }
    }
}
