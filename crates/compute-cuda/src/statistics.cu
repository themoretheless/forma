
// Metadata: outer dims, input strides, logical output strides; then the same
// three vectors for contracted axes. Offsets count the original input elements.
__device__ U64 stats_source(U64 row, U64 r, const U64* layout,
    unsigned outer_rank, unsigned reduce_rank, U64 offset) {
    const U64 base = index_of(row, layout, layout + outer_rank, outer_rank, offset);
    const U64* reduced = layout + 3ULL * outer_rank;
    return index_of(r, reduced, reduced + reduce_rank, reduce_rank, base);
}
__device__ U64 stats_destination(U64 row, U64 r, const U64* layout,
    unsigned outer_rank, unsigned reduce_rank) {
    const U64 base = index_of(row, layout, layout + 2ULL * outer_rank, outer_rank, 0);
    const U64* reduced = layout + 3ULL * outer_rank;
    return index_of(r, reduced, reduced + 2ULL * reduce_rank, reduce_rank, base);
}
__device__ void stats_block(double* shared, unsigned op) {
    __syncthreads();
    for (unsigned stride = 128; stride; stride >>= 1) {
        if (threadIdx.x < stride) {
            const double a = shared[threadIdx.x], b = shared[threadIdx.x + stride];
            shared[threadIdx.x] = op == 0 ? (a > b ? a : b) : a + b;
        }
        __syncthreads();
    }
}
struct StatsFloatLoad {
    __device__ double operator()(float value) const { return (double)value; }
};
// op: 0=max, 1=exp sum about row max, 2=deviation sum about the first value,
// 3=centered squared-deviation sum. f64 keeps finite-f32 squares/sums in range.
template<typename Input, typename Load>
__device__ void stats_partial_impl(const Input* input, const double* center,
    double* partials, const U64* layout, U64 rows, U64 reduce_count, U64 chunks,
    unsigned outer_rank, unsigned reduce_rank, U64 offset, unsigned op, double* shared, Load load) {
    const U64 jobs = rows * chunks;
    for (U64 job = blockIdx.x; job < jobs; job += gridDim.x) {
        const U64 row = job / chunks, chunk = job % chunks;
        const double anchor = load(input[stats_source(row, 0, layout, outer_rank, reduce_rank, offset)]);
        double value = op == 0 ? (double)reduce_identity<float>(3) : 0.0;
        for (U64 r = chunk * blockDim.x + threadIdx.x; r < reduce_count; r += chunks * blockDim.x) {
            const double x = load(input[stats_source(row, r, layout, outer_rank, reduce_rank, offset)]);
            if (op == 0) value = value > x ? value : x;
            else if (op == 1) value += (double)expf((float)(x - center[row]));
            else if (op == 2) value += x - anchor;
            else { const double d = (x - anchor) - center[row]; value += d * d; }
        }
        shared[threadIdx.x] = value;
        stats_block(shared, op);
        if (threadIdx.x == 0) partials[job] = shared[0];
        __syncthreads();
    }
}
extern "C" __global__ void stats_partial(const float* input, const double* center,
    double* partials, const U64* layout, U64 rows, U64 reduce_count, U64 chunks,
    unsigned outer_rank, unsigned reduce_rank, U64 offset, unsigned op) {
    __shared__ double shared[256];
    stats_partial_impl(input, center, partials, layout, rows, reduce_count, chunks,
        outer_rank, reduce_rank, offset, op, shared, StatsFloatLoad{});
}
extern "C" __global__ void stats_merge(const double* partials, double* result,
    U64 rows, U64 chunks, U64 reduce_count, unsigned op) {
    __shared__ double shared[256];
    for (U64 row = blockIdx.x; row < rows; row += gridDim.x) {
        double value = op == 0 ? (double)reduce_identity<float>(3) : 0.0;
        for (U64 chunk = threadIdx.x; chunk < chunks; chunk += blockDim.x) {
            const double x = partials[row * chunks + chunk];
            value = op == 0 ? (value > x ? value : x) : value + x;
        }
        shared[threadIdx.x] = value;
        stats_block(shared, op);
        if (threadIdx.x == 0) result[row] = op >= 2 ? shared[0] / (double)reduce_count : shared[0];
        __syncthreads();
    }
}
// op: 0=softmax, 1=log-softmax, 2=layer norm. The original variance is never
// rounded to f32 before normalization, so overflowing reported variance is safe.
template<typename Input, typename Load>
__device__ void stats_emit_impl(const Input* input, const double* first,
    const double* second, float* output, const U64* layout, U64 count, U64 reduce_count,
    unsigned outer_rank, unsigned reduce_rank, U64 offset, unsigned op, float epsilon, Load load) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 i = (U64)blockIdx.x * blockDim.x + threadIdx.x; i < count; i += step) {
        const U64 row = i / reduce_count, r = i % reduce_count;
        const double x = load(input[stats_source(row, r, layout, outer_rank, reduce_rank, offset)]);
        double value;
        if (op == 0) value = (double)expf((float)(x - first[row])) / second[row];
        else if (op == 1) value = (x - first[row]) - log(second[row]);
        else {
            const double anchor = load(input[stats_source(row, 0, layout, outer_rank, reduce_rank, offset)]);
            value = ((x - anchor) - first[row]) / sqrt(second[row] + (double)epsilon);
        }
        output[stats_destination(row, r, layout, outer_rank, reduce_rank)] = (float)value;
    }
}
extern "C" __global__ void stats_emit(const float* input, const double* first,
    const double* second, float* output, const U64* layout, U64 count, U64 reduce_count,
    unsigned outer_rank, unsigned reduce_rank, U64 offset, unsigned op, float epsilon) {
    stats_emit_impl(input, first, second, output, layout, count, reduce_count,
        outer_rank, reduce_rank, offset, op, epsilon, StatsFloatLoad{});
}
extern "C" __global__ void stats_lse(const double* maximum, const double* sum,
    float* output, U64 rows) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 row = (U64)blockIdx.x * blockDim.x + threadIdx.x; row < rows; row += step)
        output[row] = (float)(maximum[row] + log(sum[row]));
}
template<typename Input, typename Load>
__device__ void stats_moments_impl(const Input* input, const double* mean_delta,
    const double* variance, float* mean_output, float* variance_output, const U64* layout,
    U64 rows, unsigned outer_rank, unsigned reduce_rank, U64 offset, Load load) {
    const U64 step = (U64)blockDim.x * gridDim.x;
    for (U64 row = (U64)blockIdx.x * blockDim.x + threadIdx.x; row < rows; row += step) {
        const double anchor = load(input[stats_source(row, 0, layout, outer_rank, reduce_rank, offset)]);
        mean_output[row] = (float)(anchor + mean_delta[row]);
        variance_output[row] = (float)variance[row];
    }
}
extern "C" __global__ void stats_moments(const float* input, const double* mean_delta,
    const double* variance, float* mean_output, float* variance_output, const U64* layout,
    U64 rows, unsigned outer_rank, unsigned reduce_rank, U64 offset) {
    stats_moments_impl(input, mean_delta, variance, mean_output, variance_output, layout,
        rows, outer_rank, reduce_rank, offset, StatsFloatLoad{});
}
