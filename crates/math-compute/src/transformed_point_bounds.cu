// Transformed point-cloud axis-aligned bounds partial reduction.
// Mirrors `transformed_point_bounds.wgsl`. f32 arithmetic.

#include <cuda_runtime.h>

extern "C" __global__ void transformed_point_bounds_reduce(
    unsigned point_count,
    const float* __restrict__ points,
    const float* __restrict__ transform,
    float* __restrict__ out_min,
    float* __restrict__ out_max
) {
    __shared__ float mins[256 * 3];
    __shared__ float maxes[256 * 3];

    unsigned local = threadIdx.x;
    unsigned i = blockIdx.x * blockDim.x + local;
    float x = 3.4028234663852886e38f;
    float y = 3.4028234663852886e38f;
    float z = 3.4028234663852886e38f;
    float hx = -3.4028234663852886e38f;
    float hy = -3.4028234663852886e38f;
    float hz = -3.4028234663852886e38f;
    if (i < point_count) {
        unsigned base = i * 3u;
        float px = points[base];
        float py = points[base + 1u];
        float pz = points[base + 2u];
        x = transform[0] * px + transform[1] * py + transform[2] * pz + transform[3];
        y = transform[4] * px + transform[5] * py + transform[6] * pz + transform[7];
        z = transform[8] * px + transform[9] * py + transform[10] * pz + transform[11];
        hx = x;
        hy = y;
        hz = z;
    }
    unsigned sb = local * 3u;
    mins[sb] = x;
    mins[sb + 1u] = y;
    mins[sb + 2u] = z;
    maxes[sb] = hx;
    maxes[sb + 1u] = hy;
    maxes[sb + 2u] = hz;
    __syncthreads();

    for (unsigned stride = 128u; stride > 0u; stride >>= 1u) {
        if (local < stride) {
            unsigned a = local * 3u;
            unsigned b = (local + stride) * 3u;
            mins[a] = fminf(mins[a], mins[b]);
            mins[a + 1u] = fminf(mins[a + 1u], mins[b + 1u]);
            mins[a + 2u] = fminf(mins[a + 2u], mins[b + 2u]);
            maxes[a] = fmaxf(maxes[a], maxes[b]);
            maxes[a + 1u] = fmaxf(maxes[a + 1u], maxes[b + 1u]);
            maxes[a + 2u] = fmaxf(maxes[a + 2u], maxes[b + 2u]);
        }
        __syncthreads();
    }

    if (local == 0u) {
        unsigned base = blockIdx.x * 3u;
        out_min[base] = mins[0];
        out_min[base + 1u] = mins[1];
        out_min[base + 2u] = mins[2];
        out_max[base] = maxes[0];
        out_max[base + 1u] = maxes[1];
        out_max[base + 2u] = maxes[2];
    }
}
