// Exact top-2 brute-force nearest-neighbor search for 3D points.
// Mirrors `nearest_two.wgsl`. f32 arithmetic.

#include <cuda_runtime.h>

extern "C" __global__ void nearest_two(
    unsigned query_count,
    unsigned target_count,
    const float* __restrict__ queries,
    const float* __restrict__ targets,
    unsigned* __restrict__ out_index0,
    float* __restrict__ out_dist0,
    unsigned* __restrict__ out_index1,
    float* __restrict__ out_dist1
) {
    unsigned i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= query_count) {
        return;
    }
    unsigned base = i * 3u;
    float qx = queries[base];
    float qy = queries[base + 1u];
    float qz = queries[base + 2u];
    unsigned best_index = 0xffffffffu;
    unsigned second_index = 0xffffffffu;
    float best_dist = 3.402823466e38f;
    float second_dist = 3.402823466e38f;
    for (unsigned t = 0u; t < target_count; t++) {
        unsigned tb = t * 3u;
        float dx = qx - targets[tb];
        float dy = qy - targets[tb + 1u];
        float dz = qz - targets[tb + 2u];
        float dist = dx * dx + dy * dy + dz * dz;
        if (dist < best_dist) {
            second_dist = best_dist;
            second_index = best_index;
            best_dist = dist;
            best_index = t;
        } else if (dist < second_dist) {
            second_dist = dist;
            second_index = t;
        }
    }
    out_index0[i] = best_index;
    out_dist0[i] = best_dist;
    out_index1[i] = second_index;
    out_dist1[i] = second_dist;
}
