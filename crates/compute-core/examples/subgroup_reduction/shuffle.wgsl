let subtotal = shuffle_sum(lane_sum, lane, subgroup_width);
if lane == subgroupBroadcastFirst(lane) { scratch[subgroup] = subtotal; }
workgroupBarrier();
if lid.x == 0u {
    var total = 0.0;
    for (var i = 0u; i < subgroup_count; i++) { total += scratch[i]; }
    output[wid.x] = total;
}
