
// One block owns a query row. Each warp computes four of the 32 tile scores;
// every V component consumes those scores, so Q.K is never repeated per output.
// The only global temporary is an f64 accumulator with the output's size.
// Metadata: row dimensions; Q/K/V/mask row strides; then offsets Q/K/V/mask,
// Q feature stride, K key/feature strides, V key/feature strides, mask key
// stride and query-heads-per-KV-head. K/V row query strides are zero.
__device__ U64 attention_base(U64 row, const U64* dims, const U64* strides,
    unsigned rank, U64 offset, U64 group_size) {
    for (unsigned axis = rank; axis; --axis) {
        const unsigned d = axis - 1;
        U64 coordinate = row % dims[d];
        row /= dims[d];
        if (d == rank - 2) coordinate /= group_size;
        offset += coordinate * strides[d];
    }
    return offset;
}
__device__ double attention_warp_sum(double value) {
    for (unsigned distance = 16; distance; distance >>= 1)
        value += __shfl_down_sync(0xffffffffu, value, distance);
    return value;
}
struct AttentionShared {
    U64 bases[4];
    double scores[32], weights[32], maximum, denominator, alpha;
};
struct AttentionFloatLoad {
    __device__ double operator()(float value) const { return (double)value; }
};
template<typename Input, typename Load>
__device__ void attention_impl(const Input* query, const Input* key,
    const Input* value, const unsigned* keep, const float* additive,
    float* output, double* accumulator, const U64* layout,
    U64 rows, U64 keys, U64 depth, U64 value_depth, unsigned rank,
    float scale, unsigned flags, int causal_offset, AttentionShared& shared, Load load) {
    U64* bases = shared.bases;
    double* scores = shared.scores;
    double* weights = shared.weights;
    double& maximum = shared.maximum;
    double& denominator = shared.denominator;
    double& alpha = shared.alpha;
    const unsigned lane = threadIdx.x & 31, warp = threadIdx.x >> 5;
    const U64* tail = layout + 5ULL * rank;
    const double excluded = -(double)__int_as_float(0x7f800000);
    for (U64 row = blockIdx.x; row < rows; row += gridDim.x) {
        if (threadIdx.x == 0) {
            bases[0] = attention_base(row, layout, layout + rank, rank, tail[0], 1);
            bases[1] = attention_base(row, layout, layout + 2ULL * rank, rank, tail[1], tail[10]);
            bases[2] = attention_base(row, layout, layout + 3ULL * rank, rank, tail[2], tail[10]);
            bases[3] = attention_base(row, layout, layout + 4ULL * rank, rank, tail[3], 1);
            maximum = excluded;
            denominator = 0.0;
        }
        for (U64 d = threadIdx.x; d < value_depth; d += blockDim.x)
            accumulator[row * value_depth + d] = 0.0;
        __syncthreads();
        const U64 query_index = row % layout[rank - 1];
        for (U64 begin = 0; begin < keys; begin += 32) {
            for (unsigned slot = warp; slot < 32; slot += 8) {
                const U64 k = begin + slot;
                bool allowed = k < keys;
                // Query/key lengths fit signed i64 by host byte-size checks.
                // Subtracting the nonnegative indices cannot overflow i64.
                if (allowed && (flags & 4))
                    allowed = (long long)k - (long long)query_index <= (long long)causal_offset;
                double bias = 0.0;
                if (allowed && (flags & 1)) allowed = keep[bases[3] + k * tail[9]] != 0;
                if (allowed && (flags & 2)) {
                    bias = (double)additive[bases[3] + k * tail[9]];
                    allowed = bias != excluded;
                }
                double dot = 0.0;
                if (allowed) {
                    for (U64 d = lane; d < depth; d += 32)
                        dot += load(query[bases[0] + d * tail[4]])
                            * load(key[bases[1] + k * tail[5] + d * tail[6]]);
                }
                // All 32 lanes participate, including excluded keys and tails.
                dot = attention_warp_sum(dot);
                if (lane == 0) scores[slot] = allowed ? dot * (double)scale + bias : excluded;
            }
            __syncthreads();
            if (warp == 0) {
                double tile_max = scores[lane];
                for (unsigned distance = 16; distance; distance >>= 1) {
                    const double other = __shfl_down_sync(0xffffffffu, tile_max, distance);
                    tile_max = tile_max > other ? tile_max : other;
                }
                if (lane == 0) {
                    const double next = maximum > tile_max ? maximum : tile_max;
                    // Excluded-only prefix tiles must not evaluate -inf-(-inf).
                    alpha = denominator == 0.0 ? 0.0 : exp(maximum - next);
                    maximum = next;
                }
            }
            __syncthreads();
            if (warp == 0) {
                const double weight = scores[lane] == excluded ? 0.0 : exp(scores[lane] - maximum);
                weights[lane] = weight;
                const double sum = attention_warp_sum(weight);
                if (lane == 0) denominator = denominator * alpha + sum;
            }
            __syncthreads();
            for (U64 d = threadIdx.x; d < value_depth; d += blockDim.x) {
                double result = accumulator[row * value_depth + d] * alpha;
                for (unsigned slot = 0; slot < 32; ++slot) {
                    // Zero weights cover invalid keys, masks and exp underflow;
                    // do not access V for any of those positions.
                    if (weights[slot] != 0.0)
                        result += weights[slot] * load(value[bases[2]
                            + (begin + slot) * tail[7] + d * tail[8]]);
                }
                accumulator[row * value_depth + d] = result;
            }
            __syncthreads();
        }
        for (U64 d = threadIdx.x; d < value_depth; d += blockDim.x)
            output[row * value_depth + d] = denominator == 0.0 ? 0.0f
                : (float)(accumulator[row * value_depth + d] / denominator);
        __syncthreads();
    }
}
extern "C" __global__ void attention_f32(const float* query, const float* key,
    const float* value, const unsigned* keep, const float* additive,
    float* output, double* accumulator, const U64* layout,
    U64 rows, U64 keys, U64 depth, U64 value_depth, unsigned rank,
    float scale, unsigned flags, int causal_offset) {
    __shared__ AttentionShared shared;
    attention_impl(query, key, value, keep, additive, output, accumulator, layout,
        rows, keys, depth, value_depth, rank, scale, flags, causal_offset,
        shared, AttentionFloatLoad{});
}
