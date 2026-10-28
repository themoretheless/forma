// Params: Hq,Hkv,Lq,Lk,D,Dv,group-size,value-tiles,rows(lo,hi),scale-bits,
// causal-enabled,causal-offset-bits. Native views already broadcast batches;
// all offsets refer to original low allocations via generated shape/strides.
const uint WIDTH = 64, KEYS = 32;
ulong hq = params[0], hkv = params[1], nq = params[2], nk = params[3];
ulong depth = params[4], channels = params[5], group_size = params[6];
ulong tiles = params[7], rows = ulong(params[8]) | (ulong(params[9]) << 32);
float scale = as_type<float>(params[10]);
uint lane = thread_position_in_threadgroup.x;
threadgroup float2 scratch[32];
threadgroup float weights[32];
for (ulong work = threadgroup_position_in_grid.x; work < rows * tiles;
     work += threadgroups_per_grid.x) {
    ulong row = work / tiles, channel = (work % tiles) * WIDTH + lane;
    ulong query_index = row % nq, head = (row / nq) % hq;
    ulong batch = row / (nq * hq), kvhead = head / group_size;
    float maximum = 0.0f, denominator = 0.0f, normalized = 0.0f;
    uint value_scale_bits = 0;
    for (ulong start = 0; start < nk; start += KEYS) {
        float score = -as_type<float>(0x7f7fffffu);
        bool kept = false;
        if (lane < KEYS) {
            ulong index = start + lane;
            kept = index < nk;
            if (kept && params[11] != 0)
                kept = long(index) <= long(query_index) + long(as_type<int>(params[12]));
            float bias = 0.0f;
            if (kept && MASK_MODE != 0) {
                uint bits = as_type<uint>(mask[elem_to_loc(row * nk + index, mask_shape, mask_strides, mask_ndim)]);
                if (MASK_MODE == 1) kept = bits != 0;
                else {
                    kept = bits != 0xff800000u;
                    if (kept) bias = as_type<float>(bits);
                }
            }
            if (kept) {
                float dot = 0.0f;
                for (ulong d = 0; d < depth; ++d) {
                    float q = low_decode(as_type<ushort>(query[elem_to_loc(row * depth + d, query_shape, query_strides, query_ndim)]), BF);
                    ulong k = ((batch * hkv + kvhead) * nk + index) * depth + d;
                    float v = low_decode(as_type<ushort>(key[elem_to_loc(k, key_shape, key_strides, key_ndim)]), BF);
                    dot += low_product(q, v);
                }
                score = low_product(dot, scale) + bias;
            }
            scratch[lane] = float2(score, float(kept));
        }
        for (uint step = KEYS / 2; step > 0; step >>= 1) {
            threadgroup_barrier(mem_flags::mem_threadgroup);
            if (lane < step) {
                float2 a = scratch[lane], b = scratch[lane + step];
                scratch[lane] = float2(max(a.x, b.x), a.y + b.y);
            }
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
        float2 tile = scratch[0];
        if (tile.y > 0.0f) {
            float next_maximum = tile.x, previous_weight = 0.0f;
            if (denominator > 0.0f) {
                next_maximum = max(maximum, tile.x);
                previous_weight = denominator * metal::precise::exp(maximum - next_maximum);
            }
            float weight = kept ? metal::precise::exp(score - next_maximum) : 0.0f;
            threadgroup_barrier(mem_flags::mem_threadgroup);
            if (lane < KEYS) {
                weights[lane] = weight;
                scratch[lane] = float2(weight, 0.0f);
            }
            for (uint step = KEYS / 2; step > 0; step >>= 1) {
                threadgroup_barrier(mem_flags::mem_threadgroup);
                if (lane < step) scratch[lane].x += scratch[lane + step].x;
            }
            threadgroup_barrier(mem_flags::mem_threadgroup);
            float next_denominator = previous_weight + scratch[0].x;
            if (channel < channels) {
                float cached[32];
                uint next_scale_bits = previous_weight == 0.0f ? 0 : value_scale_bits;
                for (uint item = 0; item < KEYS; ++item) {
                    cached[item] = 0.0f;
                    if (weights[item] > 0.0f) {
                        ulong address = ((batch * hkv + kvhead) * nk + start + item) * channels + channel;
                        cached[item] = low_decode(as_type<ushort>(value[elem_to_loc(address, value_shape, value_strides, value_ndim)]), BF);
                        next_scale_bits = max(next_scale_bits, as_type<uint>(cached[item]) & 0x7fffffffu);
                    }
                }
                if (next_scale_bits != 0) {
                    float next_scale = as_type<float>(next_scale_bits);
                    if (previous_weight == 0.0f) normalized = 0.0f;
                    else normalized *= (previous_weight / next_denominator)
                        * attention_unit(as_type<float>(value_scale_bits), next_scale);
                    for (uint item = 0; item < KEYS; ++item)
                        normalized += (weights[item] / next_denominator) * attention_unit(cached[item], next_scale);
                    normalized = clamp(normalized, -1.0f, 1.0f);
                } else normalized = 0.0f;
                value_scale_bits = next_scale_bits;
            }
            maximum = next_maximum;
            denominator = next_denominator;
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (channel < channels)
        out[row * channels + channel] = low_product(normalized, as_type<float>(value_scale_bits));
    threadgroup_barrier(mem_flags::mem_threadgroup);
}
