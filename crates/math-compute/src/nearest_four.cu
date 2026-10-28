// Exact top-4 brute-force nearest-neighbor search for 3D points.
// Mirrors `nearest_four.wgsl`. f32 arithmetic.

#include <cuda_runtime.h>

extern "C" __global__ void nearest_four(
    unsigned query_count,
    unsigned target_count,
    const float* __restrict__ queries,
    const float* __restrict__ targets,
    unsigned* __restrict__ out_indices,
    float* __restrict__ out_distances
) {
    unsigned i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= query_count) {
        return;
    }
    unsigned base = i * 3u;
    float qx = queries[base];
    float qy = queries[base + 1u];
    float qz = queries[base + 2u];
    unsigned i0 = 0xffffffffu;
    unsigned i1 = 0xffffffffu;
    unsigned i2 = 0xffffffffu;
    unsigned i3 = 0xffffffffu;
    float d0 = 3.402823466e38f;
    float d1 = 3.402823466e38f;
    float d2 = 3.402823466e38f;
    float d3 = 3.402823466e38f;
    for (unsigned t = 0u; t < target_count; t++) {
        unsigned tb = t * 3u;
        float dx = qx - targets[tb];
        float dy = qy - targets[tb + 1u];
        float dz = qz - targets[tb + 2u];
        float dist = dx * dx + dy * dy + dz * dz;
        if (dist < d0) {
            d3 = d2;
            i3 = i2;
            d2 = d1;
            i2 = i1;
            d1 = d0;
            i1 = i0;
            d0 = dist;
            i0 = t;
        } else if (dist < d1) {
            d3 = d2;
            i3 = i2;
            d2 = d1;
            i2 = i1;
            d1 = dist;
            i1 = t;
        } else if (dist < d2) {
            d3 = d2;
            i3 = i2;
            d2 = dist;
            i2 = t;
        } else if (dist < d3) {
            d3 = dist;
            i3 = t;
        }
    }
    unsigned out = i * 4u;
    out_indices[out] = i0;
    out_indices[out + 1u] = i1;
    out_indices[out + 2u] = i2;
    out_indices[out + 3u] = i3;
    out_distances[out] = d0;
    out_distances[out + 1u] = d1;
    out_distances[out + 2u] = d2;
    out_distances[out + 3u] = d3;
}
