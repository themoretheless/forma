// One-to-one squared Euclidean distances for corresponding point pairs.
// Mirrors `distance_pairs.wgsl`. f32 arithmetic.

#include <cuda_runtime.h>

extern "C" __global__ void squared_distance_pairs(
    unsigned pair_count,
    const float* __restrict__ a_points,
    const float* __restrict__ b_points,
    float* __restrict__ out_dist
) {
    unsigned i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= pair_count) {
        return;
    }
    unsigned base = i * 3u;
    float dx = a_points[base] - b_points[base];
    float dy = a_points[base + 1u] - b_points[base + 1u];
    float dz = a_points[base + 2u] - b_points[base + 2u];
    out_dist[i] = dx * dx + dy * dy + dz * dz;
}
