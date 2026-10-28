// Fused point-cloud bounds + centroid/covariance partial reduction.
// Mirrors `point_cloud_stats.wgsl`. f32 arithmetic.

#include <cuda_runtime.h>

extern "C" __global__ void point_cloud_stats_reduce(
    unsigned point_count,
    const float* __restrict__ points,
    float* __restrict__ out
) {
    __shared__ float sh[15 * 256];

    unsigned local = threadIdx.x;
    unsigned i = blockIdx.x * blockDim.x + local;
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
    float mnx = 3.4028234663852886e38f;
    float mny = 3.4028234663852886e38f;
    float mnz = 3.4028234663852886e38f;
    float mxx = -3.4028234663852886e38f;
    float mxy = -3.4028234663852886e38f;
    float mxz = -3.4028234663852886e38f;
    if (i < point_count) {
        unsigned base = i * 3u;
        x = points[base];
        y = points[base + 1u];
        z = points[base + 2u];
        mnx = mxx = x;
        mny = mxy = y;
        mnz = mxz = z;
    }
    unsigned sb = local * 15u;
    sh[sb] = mnx;
    sh[sb + 1u] = mny;
    sh[sb + 2u] = mnz;
    sh[sb + 3u] = mxx;
    sh[sb + 4u] = mxy;
    sh[sb + 5u] = mxz;
    sh[sb + 6u] = x;
    sh[sb + 7u] = y;
    sh[sb + 8u] = z;
    sh[sb + 9u] = x * x;
    sh[sb + 10u] = x * y;
    sh[sb + 11u] = x * z;
    sh[sb + 12u] = y * y;
    sh[sb + 13u] = y * z;
    sh[sb + 14u] = z * z;
    __syncthreads();

    for (unsigned stride = 128u; stride > 0u; stride >>= 1u) {
        if (local < stride) {
            unsigned a = local * 15u;
            unsigned b = (local + stride) * 15u;
            sh[a] = fminf(sh[a], sh[b]);
            sh[a + 1u] = fminf(sh[a + 1u], sh[b + 1u]);
            sh[a + 2u] = fminf(sh[a + 2u], sh[b + 2u]);
            sh[a + 3u] = fmaxf(sh[a + 3u], sh[b + 3u]);
            sh[a + 4u] = fmaxf(sh[a + 4u], sh[b + 4u]);
            sh[a + 5u] = fmaxf(sh[a + 5u], sh[b + 5u]);
            #pragma unroll
            for (unsigned k = 6u; k < 15u; ++k) {
                sh[a + k] += sh[b + k];
            }
        }
        __syncthreads();
    }

    if (local == 0u) {
        unsigned base = blockIdx.x * 15u;
        #pragma unroll
        for (unsigned k = 0; k < 15u; ++k) {
            out[base + k] = sh[k];
        }
    }
}
