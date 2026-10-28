// Experimental restriction: active IDs must form a dense prefix starting at
// zero. The butterfly is not correct for arbitrary sparse active masks. WG16
// coverage checks the measured Metal device only, not a portable guarantee.
fn shuffle_sum(value: f32, lane: u32, width: u32) -> f32 {
    let active_mask = subgroupBallot(true);
    var sum = value;
    for (var step = width / 2u; step > 0u; step >>= 1u) {
        let partner_lane = lane ^ step;
        let other = subgroupShuffleXor(sum, step);
        // A workgroup smaller than a physical subgroup has inactive tail
        // lanes. Their unspecified shuffle results must never enter the sum.
        if (active_mask[partner_lane / 32u] & (1u << (partner_lane % 32u))) != 0u { sum += other; }
    }
    return sum;
}
