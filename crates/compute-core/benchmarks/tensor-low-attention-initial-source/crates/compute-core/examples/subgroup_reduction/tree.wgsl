scratch[lid.x] = lane_sum;
for (var step = WG / 2u; step > 0u; step >>= 1u) {
    workgroupBarrier();
    if lid.x < step { scratch[lid.x] += scratch[lid.x + step]; }
}
if lid.x == 0u { output[wid.x] = scratch[0]; }
