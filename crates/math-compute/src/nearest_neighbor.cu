// CUDA port of `NEAREST_NEIGHBOR_WGSL` (crates/math-core/src/nearest_neighbor.wgsl).
// One thread per query point scans every target and keeps the closest
// (brute-force). f32 arithmetic, like the WGSL path.
//
// Regenerate the checked-in PTX with `npm run build:cuda-kernels` (nvcc).

#include <cuda_runtime.h>

extern "C" __global__ void nearest_neighbor(
    unsigned query_count,
    unsigned target_count,
    const float* __restrict__ queries,
    const float* __restrict__ targets,
    unsigned* __restrict__ out_index,
    float* __restrict__ out_dist
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
    float best_dist = 3.402823466e38f;
    for (unsigned t = 0u; t < target_count; t++) {
        unsigned tb = t * 3u;
        float dx = qx - targets[tb];
        float dy = qy - targets[tb + 1u];
        float dz = qz - targets[tb + 2u];
        float dist = dx * dx + dy * dy + dz * dz;
        if (dist < best_dist) {
            best_dist = dist;
            best_index = t;
        }
    }
    out_index[i] = best_index;
    out_dist[i] = best_dist;
}
