
// Native two-byte input; only row summaries and the result are widened.
// decode_low preserves finite subnormal bits before the exact f64 conversion.
struct StatsLowLoad {
    unsigned dtype;
    __device__ double operator()(unsigned short value) const {
        return (double)decode_low(value, dtype);
    }
};
extern "C" __global__ void stats_partial_low(const unsigned short* input, const double* center,
    double* partials, const U64* layout, U64 rows, U64 reduce_count, U64 chunks,
    unsigned outer_rank, unsigned reduce_rank, U64 offset, unsigned op, unsigned dtype) {
    __shared__ double shared[256];
    stats_partial_impl(input, center, partials, layout, rows, reduce_count, chunks,
        outer_rank, reduce_rank, offset, op, shared, StatsLowLoad{dtype});
}
extern "C" __global__ void stats_emit_low(const unsigned short* input, const double* first,
    const double* second, float* output, const U64* layout, U64 count, U64 reduce_count,
    unsigned outer_rank, unsigned reduce_rank, U64 offset, unsigned op, float epsilon, unsigned dtype) {
    stats_emit_impl(input, first, second, output, layout, count, reduce_count,
        outer_rank, reduce_rank, offset, op, epsilon, StatsLowLoad{dtype});
}
extern "C" __global__ void stats_moments_low(const unsigned short* input, const double* mean_delta,
    const double* variance, float* mean_output, float* variance_output, const U64* layout,
    U64 rows, unsigned outer_rank, unsigned reduce_rank, U64 offset, unsigned dtype) {
    stats_moments_impl(input, mean_delta, variance, mean_output, variance_output, layout,
        rows, outer_rank, reduce_rank, offset, StatsLowLoad{dtype});
}
