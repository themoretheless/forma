
// Native u16 Q/K/V, decoded directly to registers before f64 arithmetic.
// Masks retain their public u32/f32 types. Scores and online PV state share
// the original attention traversal; no full Q/K/V expansion is allocated.
struct AttentionLowLoad {
    unsigned dtype;
    __device__ double operator()(unsigned short value) const {
        return (double)decode_low(value, dtype);
    }
};
extern "C" __global__ void attention_low(const unsigned short* query,
    const unsigned short* key, const unsigned short* value,
    const unsigned* keep, const float* additive,
    float* output, double* accumulator, const U64* layout,
    U64 rows, U64 keys, U64 depth, U64 value_depth, unsigned rank,
    float scale, unsigned flags, int causal_offset, unsigned dtype) {
    __shared__ AttentionShared shared;
    attention_impl(query, key, value, keep, additive, output, accumulator, layout,
        rows, keys, depth, value_depth, rank, scale, flags, causal_offset,
        shared, AttentionLowLoad{dtype});
}
