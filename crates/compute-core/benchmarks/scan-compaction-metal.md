# Scan and stable compaction, Metal, 2026-09-27

The retained scan assigns four adjacent elements to each lane. It first builds
local prefixes in registers and Blelloch-scans only the lane totals. A 256-lane
workgroup now covers 1024 elements with the same shared-memory tree as the old
256-element block. Exclusive scan retains u32 wrapping addition and hierarchical
GPU folds for arbitrary supported lengths.

Compaction keeps the first scan level local and separately scans block totals.
Its final shader combines local/block offsets while scattering, writes count,
and clears the unused tail. This removes the full-size offset-add pass and the
separate tail dispatch. Scatter destinations are `[0,count)` and tail destinations
are `[count,capacity)`, so those writes cannot race even across workgroups.
Uniform zero/all-selected cases clear/copy directly. No mask or count is read
back to choose those branches.

At 1,048,576 elements the compaction schedule drops from seven to three dispatches;
at 4,194,304 it drops from seven to five. The full-size offset buffer remains:
the saving is its extra read/write pass, fewer scan groups and fewer dispatches.
The original public `SCAN_BLOCKS_WGSL`, `SCAN_ADD_WGSL`, `COMPACT_SCATTER_WGSL`
and `COMPACT_FINISH_WGSL` sources retain their contracts. The new four-element
source is exported as `SCAN_BLOCKS4_WGSL`.

## Matched measurement

Apple M4 Max (40 GPU cores), Metal, native release build. Each case uses three
warmups and 15 measured repetitions, rotating baseline/production order.
Compilation, allocation and input uploads are outside timing. Both paths use one
compute pass and one submission. Wall time includes completion and readback.

`compact_full` reads every capacity element plus count (4n+4 bytes), including the
zero tail. `compare_compact_sum` performs the full GPU comparison, stable
compaction and sum, then reads only the same sum/count pair on both paths (8 bytes).
Full selected arrays, count and zero tails are checked before timing every case.
The pipeline's comparison and reduction kernels/plans are identical between
paths. Its scalar readback therefore does not remove compaction work or give one
path an output-transfer advantage.

The baseline is frozen in `examples/scan_baseline/` and reconstructed by
`examples/bench_scan_compact.rs`. It retains the original per-element Blelloch
scan, full-size offset propagation, scatter and separate count/tail pass.
Masks are 0%, 50% and 100% selected; the standalone mask uses nonbinary nonzero
values so normalization is exercised. Inputs and thresholds are reused across
runs. This benchmark is independent of the generated fused-sum optimization.

## Median wall latency, milliseconds

| Elements | Selected | Compact baseline → production | Compare/compact/sum baseline → production |
| ---: | ---: | ---: | ---: |
| 4096 | 0% | 0.245084 → 0.239375 | 0.177458 → 0.175000 |
| 4096 | 50% | 0.222083 → 0.213583 | 0.151750 → 0.148875 |
| 4096 | 100% | 0.213709 → 0.214458 | 0.155000 → 0.162833 |
| 1,048,576 | 0% | 0.946625 → 0.755500 | 0.924583 → 0.642209 |
| 1,048,576 | 50% | 0.982916 → 0.770333 | 0.942583 → 0.627709 |
| 1,048,576 | 100% | 1.116750 → 0.851459 | 0.885667 → 0.614458 |
| 4,194,304 | 0% | 5.018750 → 3.509166 | 1.970083 → 1.230709 |
| 4,194,304 | 50% | 3.896250 → 3.085625 | 1.545584 → 1.118292 |
| 4,194,304 | 100% | 4.386666 → 3.196708 | 1.526125 → 1.069500 |

The larger cases improve 1.25–1.43× with full compacted-output readback and
1.38–1.60× in the complete compare/compact/sum pipeline. The 4096-element cases
are effectively flat at this measurement scale; the 100% pipeline case is about
8 microseconds slower. No small-input improvement or universal backend speedup
is claimed. These are host latency measurements on one machine, with desktop
runtime variability, not kernel-only timestamps.

[Raw medians/p90/output-byte CSV](scan-compact-metal.csv).

```sh
cargo run --offline --release --manifest-path crates/Cargo.toml \
  -p compute-core --example bench_scan_compact
```

Correctness includes repeated mask changes, stale output/count resets, stable
ordering, exact selected counts, u32 wrapping, allocation tails and 1024-element
boundaries. A dedicated shader test dispatches only three physical groups over
twelve logical blocks to exercise repeated scratch reuse with wrapping and
normalization. End-to-end coverage includes 1M+1 and more than the old
65,535×256 dispatch grid.
