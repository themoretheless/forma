// Point-cloud centroid/covariance partial reduction. Mirrors `point_moments.wgsl`.
// f32 arithmetic.

#include <cuda_runtime.h>

extern "C" __global__ void point_moments_reduce(
    unsigned point_count,
    const float* __restrict__ points,
    float* __restrict__ out
) {
    __shared__ float sh[9 * 256];

    unsigned local = threadIdx.x;
    unsigned i = blockIdx.x * blockDim.x + local;
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
    if (i < point_count) {
        unsigned base = i * 3u;
        x = points[base];
        y = points[base + 1u];
        z = points[base + 2u];
    }
    unsigned sb = local * 9u;
    sh[sb] = x;
    sh[sb + 1u] = y;
    sh[sb + 2u] = z;
    sh[sb + 3u] = x * x;
    sh[sb + 4u] = x * y;
    sh[sb + 5u] = x * z;
    sh[sb + 6u] = y * y;
    sh[sb + 7u] = y * z;
    sh[sb + 8u] = z * z;
    __syncthreads();

    for (unsigned stride = 128u; stride > 0u; stride >>= 1u) {
        if (local < stride) {
            unsigned a = local * 9u;
            unsigned b = (local + stride) * 9u;
            #pragma unroll
            for (unsigned k = 0; k < 9u; ++k) {
                sh[a + k] += sh[b + k];
            }
        }
        __syncthreads();
    }

    if (local == 0u) {
        unsigned base = blockIdx.x * 9u;
        #pragma unroll
        for (unsigned k = 0; k < 9u; ++k) {
            out[base + k] = sh[k];
        }
    }
}
