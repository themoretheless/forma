// Directed point-cloud Chamfer partial reduction. Mirrors `chamfer.wgsl`.
// f32 arithmetic.

#include <cuda_runtime.h>

extern "C" __global__ void directed_chamfer_reduce(
    unsigned query_count,
    unsigned target_count,
    const float* __restrict__ queries,
    const float* __restrict__ targets,
    float* __restrict__ out_sum,
    float* __restrict__ out_max
) {
    __shared__ float sums[256];
    __shared__ float maxes[256];

    unsigned local = threadIdx.x;
    unsigned q = blockIdx.x * blockDim.x + local;
    float best = 0.0f;
    if (q < query_count) {
        unsigned qb = q * 3u;
        float qx = queries[qb];
        float qy = queries[qb + 1u];
        float qz = queries[qb + 2u];
        best = 3.4028234663852886e38f;
        for (unsigned t = 0; t < target_count; ++t) {
            unsigned tb = t * 3u;
            float dx = qx - targets[tb];
            float dy = qy - targets[tb + 1u];
            float dz = qz - targets[tb + 2u];
            float dist = dx * dx + dy * dy + dz * dz;
            best = fminf(best, dist);
        }
    }
    sums[local] = best;
    maxes[local] = best;
    __syncthreads();

    for (unsigned stride = 128u; stride > 0u; stride >>= 1u) {
        if (local < stride) {
            sums[local] += sums[local + stride];
            maxes[local] = fmaxf(maxes[local], maxes[local + stride]);
        }
        __syncthreads();
    }

    if (local == 0u) {
        out_sum[blockIdx.x] = sums[0];
        out_max[blockIdx.x] = maxes[0];
    }
}
