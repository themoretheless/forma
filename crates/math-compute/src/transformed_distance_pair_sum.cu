// Partial reduction of transformed one-to-one squared Euclidean distances.
// Mirrors `transformed_distance_pair_sum.wgsl`. f32 arithmetic.

#include <cuda_runtime.h>

extern "C" __global__ void transformed_squared_distance_pair_sum(
    unsigned pair_count,
    const float* __restrict__ source_points,
    const float* __restrict__ target_points,
    const float* __restrict__ transform,
    float* __restrict__ partial_sums
) {
    __shared__ float scratch[256];
    unsigned local = threadIdx.x;
    unsigned i = blockIdx.x * blockDim.x + local;
    float value = 0.0f;
    if (i < pair_count) {
        unsigned base = i * 3u;
        float px = source_points[base];
        float py = source_points[base + 1u];
        float pz = source_points[base + 2u];
        float qx = target_points[base];
        float qy = target_points[base + 1u];
        float qz = target_points[base + 2u];
        float x = transform[0] * px + transform[1] * py + transform[2] * pz + transform[3];
        float y = transform[4] * px + transform[5] * py + transform[6] * pz + transform[7];
        float z = transform[8] * px + transform[9] * py + transform[10] * pz + transform[11];
        float dx = x - qx;
        float dy = y - qy;
        float dz = z - qz;
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
