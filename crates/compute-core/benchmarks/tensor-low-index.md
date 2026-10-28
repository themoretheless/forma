# Packed low indexing and scans on Metal

Date: 2026-09-27. Device: Apple M4 Max, 40 GPU cores, macOS 26.7.
Matched resident comparisons use the same low input, u32 masks/indices,
result dtype, result shape and invalid/selected counts.

## Method

[Runnable benchmark](../examples/bench_tensor_low_index.rs),
[shared timing harness](../examples/support/low_bench.rs),
[raw samples](tensor-low-index-metal.txt),
[source fingerprints](tensor-low-index-source-fingerprints.json).

The baseline expands the input to f32, calls the public f32 operation, and
converts back when the result is low. The direct path reads packed input.
Select expands both input arrays. Scan produces f32 in both paths. Gather and
Compact return the same extra GPU count, copied and checked in both paths.

Programs and allocations are prepared once. Uploads, recording and compilation
are excluded. Each case warms up for 200 ms, then collects 31 samples with
rotating path order and alternating profiled/unprofiled order. GPU timestamps
cover only the shared compute pass. Separate host measurements include encoder
construction, submission, waiting and matching value/count readbacks. Reference
checks follow timing and validate every result after every execution.

Fixed finite quarter-valued low encodings make the cast baseline equivalent.
Expected selection/gather/compaction values use independent raw encodings and
CPU index arithmetic; f64 row prefixes supply the scan reference. All values
are exactly representable in f32. Exhaustive NaN payload/subnormal behavior is
covered by conformance, not inferred from these finite timing inputs.

Shapes: small=17x19, long=1x131077, strided=257x1025 with a physically transposed
source. Select uses a full-shaped mask and second low operand. Compact selects
three of every five logical elements. Gather contracts axis 1, with ceil(K/4)
valid permuted indices plus two invalid indices. Scan is inclusive forward
along axis 1. Other scan modes and mask densities are not timed.

Development build, exclusive task GPU window; other app/OS activity is not
controlled. Absolute timings, especially tiny workloads, vary with device
state. These local matched microbenchmarks do not establish browser, end-to-end
application or cross-backend speedups.

## Median results, milliseconds

| Case | Baseline GPU | Direct GPU | GPU speedup | Baseline host+readback | Direct host+readback |
| --- | ---: | ---: | ---: | ---: | ---: |
| small_F16_select | 0.045917 | 0.020042 | 2.29x | 0.363125 | 0.266334 |
| small_F16_gather | 0.056875 | 0.037667 | 1.51x | 0.414667 | 0.366000 |
| small_F16_compact | 0.064917 | 0.043375 | 1.50x | 0.382833 | 0.322959 |
| small_F16_scan | 0.008250 | 0.006666 | 1.24x | 0.242500 | 0.220625 |
| small_Bf16_select | 0.045250 | 0.019708 | 2.30x | 0.322209 | 0.239458 |
| small_Bf16_gather | 0.057250 | 0.037834 | 1.51x | 0.393750 | 0.333333 |
| small_Bf16_compact | 0.063875 | 0.043750 | 1.46x | 0.396875 | 0.344084 |
| small_Bf16_scan | 0.029666 | 0.022958 | 1.29x | 0.265959 | 0.238542 |
| long_F16_select | 0.122375 | 0.033709 | 3.63x | 0.809334 | 0.652167 |
| long_F16_gather | 0.087667 | 0.043041 | 2.04x | 0.537750 | 0.446167 |
| long_F16_compact | 0.145708 | 0.095917 | 1.52x | 0.897791 | 0.797000 |
| long_F16_scan | 0.159417 | 0.144000 | 1.11x | 1.589084 | 1.278750 |
| long_Bf16_select | 0.120042 | 0.033583 | 3.57x | 0.827833 | 0.677084 |
| long_Bf16_gather | 0.091750 | 0.045625 | 2.01x | 0.535250 | 0.447500 |
| long_Bf16_compact | 0.151334 | 0.098291 | 1.54x | 0.911375 | 0.813167 |
| long_Bf16_scan | 0.162875 | 0.142375 | 1.14x | 1.591708 | 1.268666 |
| strided_F16_select | 0.224667 | 0.052667 | 4.27x | 1.596875 | 1.082250 |
| strided_F16_gather | 0.130625 | 0.045583 | 2.87x | 0.692833 | 0.561417 |
| strided_F16_compact | 0.233458 | 0.139334 | 1.68x | 1.774875 | 1.577166 |
| strided_F16_scan | 0.245709 | 0.217583 | 1.13x | 2.598375 | 2.545000 |
| strided_Bf16_select | 0.220167 | 0.052958 | 4.16x | 1.610875 | 1.078792 |
| strided_Bf16_gather | 0.127209 | 0.046458 | 2.74x | 0.688791 | 0.564084 |
| strided_Bf16_compact | 0.243416 | 0.141459 | 1.72x | 1.603709 | 1.241791 |
| strided_Bf16_scan | 0.249958 | 0.215625 | 1.16x | 2.616291 | 2.554000 |

All 24 matched GPU cases improved in this run:

- Select: 2.29-4.27x.
- Gather: 1.51-2.87x.
- Compact: 1.46-1.72x.
- Scan: 1.11-1.29x.

Readback dominates the larger host scan timings, so those improvements are
smaller than the GPU changes. Compare was not timed. This report makes no
performance claim for CUDA, MLX, different mask densities or arbitrary layouts.

## Visible temporary arrays

For N input elements and G gathered output elements, both paths retain the
same packed inputs and final result. Relative to direct packed execution,
the expansion baseline additionally allocates:

| Operation | Conversion-related f32 storage removed |
| --- | ---: |
| Select | 12*N bytes: two expanded inputs and selected f32 output |
| Gather | 4*N + 4*G bytes: expanded input and gathered f32 result |
| Compact | 8*N bytes: expanded input and f32 capacity output |
| F32 scan | 4*N bytes: expanded input |

Both compaction paths still need mask-prefix scratch and a count. Both scan
paths need f32 block totals/carries and the f32 final output. Non-last scan
axes can require a full f32 result in axis-last order and a final layout copy;
that is not a low-input conversion and is not measured by these axis-1 cases.
Packed compaction uses atomic halfword updates because two logical output
slots may share one word; raw payloads and odd-offset neighbors stay intact.

At the strided shape, N=263425; select removes 3,161,100 bytes of conversion
intermediates. Counts exclude metadata, readback, driver scratch and registers
and are not measured peak memory. Each odd low allocation includes a padding
halfword.

See [execution contracts](tensor-low-index-contracts.md) and the
[complete qualification](../../../docs/qualification/tensor-low-indexing-2026-09-27.md).
