// Partial reduction of one-to-one squared Euclidean distances.
// Mirrors `distance_pair_sum.wgsl`. f32 arithmetic.

#include <cuda_runtime.h>

extern "C" __global__ void squared_distance_pair_sum(
    unsigned pair_count,
    const float* __restrict__ a_points,
    const float* __restrict__ b_points,
    float* __restrict__ partial_sums
) {
    __shared__ float scratch[256];
    unsigned local = threadIdx.x;
    unsigned i = blockIdx.x * blockDim.x + local;
    float value = 0.0f;
    if (i < pair_count) {
        unsigned base = i * 3u;
        float dx = a_points[base] - b_points[base];
        float dy = a_points[base + 1u] - b_points[base + 1u];
        float dz = a_points[base + 2u] - b_points[base + 2u];
        value = dx * dx + dy * dy + dz * dz;
    }
    scratch[local] = value;
    __syncthreads();
    for (unsigned stride = 128u; stride > 0u; stride >>= 1u) {
        if (local < stride) {
            scratch[local] += scratch[local + stride];
        }
        __syncthreads();
    }
    if (local == 0u) {
        partial_sums[blockIdx.x] = scratch[0];
    }
}
