let subtotal = subgroupAdd(lane_sum);
if lane == subgroupBroadcastFirst(lane) { scratch[subgroup] = subtotal; }
workgroupBarrier();
// Fold the small subgroup-partials array without assuming any relationship
// between local invocation indices and subgroup lane IDs.
if lid.x == 0u {
    var total = 0.0;
    for (var i = 0u; i < subgroup_count; i++) { total += scratch[i]; }
    output[wid.x] = total;
}
