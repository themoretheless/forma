# Timestamp regression stability

The original integrated run failed a one-pair timing assertion: the short pass took 884.375 µs and the 128-dispatch pass took 6234.708 µs, below the required 8× separation. Earlier alternating submissions in that run had the expected short/long separation. The [original failure](timestamps-single-pair-noise.txt) is preserved.

The unchanged test then passed four focused serial runs and four runs of the complete profiling test binary with ordinary parallel test execution. Short-pass times ranged from roughly 8.6 to 37 µs across these runs; long-pass times also varied. This did not reproduce the prior stale-query failure. The failed interval alone cannot establish whether GPU scheduling or another source of timing noise caused its delay. The workload buffer was already initialized and repeatedly used before the same-encoder check. [Unchanged-test repetitions](timestamps-single-pair-repeated-metal.txt).

Only the regression test changed. It now records seven pairs for each encoder order, interleaves the orders, reads tickets in reverse, and retains the same 8× threshold between median long and short durations. Every individual sample must also fit the enclosing host interval and begin no earlier than the last fully completed submission's end timestamp. The latter directly rejects stale timestamps even when their duration looks plausible. Zero-only or consistently swapped short/long results still fail the unchanged separation threshold.

The revised test passed four focused serial runs and four complete profiling binaries with normal parallel test execution: 112 measured same-encoder pairs. Median long/short ratios ranged from 79.60× to 170.62× across the 16 order-specific groups; all per-sample freshness and host-bound checks passed. [Revised-test output](timestamps-repeated-pair-tests-metal.txt).

The production timer and its completion-before-resolution lifecycle did not change. These repetitions provide bounded native Metal evidence; they do not guarantee a performance ratio on every supported adapter or under arbitrary GPU contention.
