# Direct low-storage statistics: matched WGSL measurements

Date: 2026-09-27. Apple M4 Max (40 GPU cores), Metal. Resident f16/BF16 inputs
feed either an explicit f32 cast plus the stable f32 statistics pipeline, or
the direct packed pipeline. Both produce the same f32 result shapes. The f32
baseline includes the corrected integer midpoint/scaling helpers; neither run
uses the previously failing midpoint implementation.

## Method and reference

[Source](../examples/bench_tensor_low_stats.rs) and
[shared harness](../examples/support/low_bench.rs). Run:

```sh
cargo run --release --offline --locked --manifest-path crates/Cargo.toml -p compute-core --example bench_tensor_low_stats
```

Programs and inputs are reused; upload, input/result/scratch allocation and
pipeline construction are outside timing. Each case has 200 ms warmup and 31 samples per mode/path,
with rotated path and measurement order. GPU timestamps bracket one shared
compute pass. Separate unprofiled wall samples include encoding, submission
and identical readback, including staging-buffer and ticket creation.
Every result is checked after timing, including warmup.
Moments reads and validates both mean and variance buffers on both paths.

Finite quarter-step inputs from -1 to 1 are represented exactly in both low
formats. An independent f64 anchored reference computes all five operations.
Comparisons use 7e-5 relative error for probabilities and variance, with a
minimum-normal f32 floor. Mean, log outputs and normalization use
7e-5 * max(abs(reference),1). This permits cancellation error in a near-zero
mean while rejecting an all-zero long softmax. Layer norm uses epsilon=0.125.
Conformance separately verifies tiny BF16, maximum values, final low rounding,
empty and singleton semantics; these are not inferred from this benchmark.

| Case | Logical shape | Axis | Input layout |
| --- | --- | ---: | --- |
| small | 32 x 33 | 1 | contiguous |
| rows | 1024 x 257 | 1 | contiguous |
| long | 1 x 131077 | 1 | contiguous |
| strided | 257 x 1025 | 1 | physical transpose, same logical values |
| nonlast | 17 x 4099 | 0 | contiguous |

Both formats and all five operations give 50 distinct cases. The unchanged
benchmark was run twice because the first strided BF16 softmax had an unusual
absolute timing jump in both paths. No source, input, reference, threshold or
sampling change was made between runs. All outputs passed in both runs.

## Both runs

Each run had lower direct-path GPU medians in 34/50 cases. Across runs,
**32 cases were faster in both, 14 slower in both, and four changed sides**.
The largest ratio was 1.26x in the first run and 1.25x in the repeat. The worst
observed direct-path slowdown was 11.1%, for long F16 moments in the repeat.
These are observations, without a significance test or application-level claim.

Milliseconds; ratio = cast_f32 / direct_low. A ratio below one means direct
packed execution was slower. Raw logs also retain p90 and every host/GPU sample.

| Case / dtype / operation | Run 1 cast ms | Run 1 direct ms | Ratio 1 | Run 2 cast ms | Run 2 direct ms | Ratio 2 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| small / F16 / Softmax | 0.012041 | 0.009834 | 1.22x | 0.011750 | 0.009667 | 1.22x |
| small / F16 / LogSoftmax | 0.012084 | 0.009917 | 1.22x | 0.011666 | 0.009625 | 1.21x |
| small / F16 / LogSumExp | 0.011875 | 0.009834 | 1.21x | 0.044250 | 0.036458 | 1.21x |
| small / F16 / Moments | 0.013333 | 0.011625 | 1.15x | 0.049917 | 0.043375 | 1.15x |
| small / F16 / LayerNorm | 0.014042 | 0.011541 | 1.22x | 0.013334 | 0.011250 | 1.19x |
| small / Bf16 / Softmax | 0.012125 | 0.009917 | 1.22x | 0.011709 | 0.009625 | 1.22x |
| small / Bf16 / LogSoftmax | 0.011709 | 0.009750 | 1.20x | 0.012708 | 0.010208 | 1.24x |
| small / Bf16 / LogSumExp | 0.011917 | 0.009792 | 1.22x | 0.011875 | 0.009667 | 1.23x |
| small / Bf16 / Moments | 0.013958 | 0.011667 | 1.20x | 0.013542 | 0.011500 | 1.18x |
| small / Bf16 / LayerNorm | 0.013292 | 0.011458 | 1.16x | 0.013583 | 0.011458 | 1.19x |
| rows / F16 / Softmax | 0.086375 | 0.080208 | 1.08x | 0.087083 | 0.078708 | 1.11x |
| rows / F16 / LogSoftmax | 0.086125 | 0.080208 | 1.07x | 0.086166 | 0.079334 | 1.09x |
| rows / F16 / LogSumExp | 0.074917 | 0.069375 | 1.08x | 0.074125 | 0.067583 | 1.10x |
| rows / F16 / Moments | 0.102917 | 0.097709 | 1.05x | 0.101875 | 0.095625 | 1.07x |
| rows / F16 / LayerNorm | 0.110209 | 0.109458 | 1.01x | 0.111042 | 0.109000 | 1.02x |
| rows / Bf16 / Softmax | 0.085875 | 0.076958 | 1.12x | 0.086416 | 0.076833 | 1.12x |
| rows / Bf16 / LogSoftmax | 0.086250 | 0.074500 | 1.16x | 0.086458 | 0.074333 | 1.16x |
| rows / Bf16 / LogSumExp | 0.070625 | 0.063209 | 1.12x | 0.074000 | 0.066542 | 1.11x |
| rows / Bf16 / Moments | 0.094208 | 0.087416 | 1.08x | 0.102375 | 0.094333 | 1.09x |
| rows / Bf16 / LayerNorm | 0.112125 | 0.105208 | 1.07x | 0.111791 | 0.105625 | 1.06x |
| long / F16 / Softmax | 0.056292 | 0.057542 | 0.98x | 0.056042 | 0.057583 | 0.97x |
| long / F16 / LogSoftmax | 0.054542 | 0.052875 | 1.03x | 0.055333 | 0.057875 | 0.96x |
| long / F16 / LogSumExp | 0.049709 | 0.050959 | 0.98x | 0.049750 | 0.048750 | 1.02x |
| long / F16 / Moments | 0.068833 | 0.075167 | 0.92x | 0.068208 | 0.075792 | 0.90x |
| long / F16 / LayerNorm | 0.072875 | 0.077042 | 0.95x | 0.078208 | 0.081833 | 0.96x |
| long / Bf16 / Softmax | 0.055667 | 0.054875 | 1.01x | 0.055416 | 0.051375 | 1.08x |
| long / Bf16 / LogSoftmax | 0.055916 | 0.055250 | 1.01x | 0.056042 | 0.055542 | 1.01x |
| long / Bf16 / LogSumExp | 0.051041 | 0.050250 | 1.02x | 0.049542 | 0.052000 | 0.95x |
| long / Bf16 / Moments | 0.068084 | 0.075167 | 0.91x | 0.074291 | 0.074542 | 1.00x |
| long / Bf16 / LayerNorm | 0.078833 | 0.079458 | 0.99x | 0.071791 | 0.070708 | 1.02x |
| strided / F16 / Softmax | 0.070167 | 0.055667 | 1.26x | 0.070375 | 0.058125 | 1.21x |
| strided / F16 / LogSoftmax | 0.070459 | 0.060042 | 1.17x | 0.071208 | 0.059917 | 1.19x |
| strided / F16 / LogSumExp | 0.055625 | 0.046000 | 1.21x | 0.059458 | 0.048000 | 1.24x |
| strided / F16 / Moments | 0.071916 | 0.069792 | 1.03x | 0.079000 | 0.073917 | 1.07x |
| strided / F16 / LayerNorm | 0.091292 | 0.082250 | 1.11x | 0.089666 | 0.079459 | 1.13x |
| strided / Bf16 / Softmax | 0.251417 | 0.216125 | 1.16x | 0.069625 | 0.056625 | 1.23x |
| strided / Bf16 / LogSoftmax | 0.071459 | 0.057792 | 1.24x | 0.071625 | 0.058958 | 1.21x |
| strided / Bf16 / LogSumExp | 0.059375 | 0.047375 | 1.25x | 0.058459 | 0.046666 | 1.25x |
| strided / Bf16 / Moments | 0.068542 | 0.060209 | 1.14x | 0.068125 | 0.060209 | 1.13x |
| strided / Bf16 / LayerNorm | 0.089875 | 0.074792 | 1.20x | 0.091333 | 0.080125 | 1.14x |
| nonlast / F16 / Softmax | 0.097084 | 0.100416 | 0.97x | 0.096625 | 0.100125 | 0.97x |
| nonlast / F16 / LogSoftmax | 0.096708 | 0.100209 | 0.97x | 0.096625 | 0.100208 | 0.96x |
| nonlast / F16 / LogSumExp | 0.094667 | 0.098417 | 0.96x | 0.094958 | 0.098375 | 0.97x |
| nonlast / F16 / Moments | 0.125167 | 0.128667 | 0.97x | 0.122500 | 0.128042 | 0.96x |
| nonlast / F16 / LayerNorm | 0.124042 | 0.129166 | 0.96x | 0.123833 | 0.128334 | 0.96x |
| nonlast / Bf16 / Softmax | 0.097250 | 0.099750 | 0.97x | 0.096250 | 0.099375 | 0.97x |
| nonlast / Bf16 / LogSoftmax | 0.096375 | 0.099459 | 0.97x | 0.096458 | 0.099583 | 0.97x |
| nonlast / Bf16 / LogSumExp | 0.097500 | 0.098042 | 0.99x | 0.094791 | 0.097875 | 0.97x |
| nonlast / Bf16 / Moments | 0.125458 | 0.128125 | 0.98x | 0.122500 | 0.128041 | 0.96x |
| nonlast / Bf16 / LayerNorm | 0.124000 | 0.128583 | 0.96x | 0.560792 | 0.588834 | 0.95x |

The anomalous strided BF16 softmax was 0.251417/0.216125 ms in run 1 and
0.069625/0.056625 ms in run 2 (cast/direct). Its absolute jump did not repeat.
Its cause was not profiled, so it is not attributed to dtype or a kernel.
The first run remains visible rather than being replaced by the repeat.
The repeat has its own absolute jumps in small F16 logsumexp/moments and
non-last-axis BF16 layer norm, again affecting both paths. The two runs do not
establish stable absolute latency; the table retains these observations too.

Direct packed loads reduce conversion/storage traffic but introduce decoding,
integer range protection and larger row state. Those costs can outweigh the
removed cast, particularly for non-last-axis groups and long moments here.
This is an implementation tradeoff; no universal speedup is established.
Neither native shader-f16 arithmetic nor CUDA/MLX performance was measured.

## Visible allocation differences

N is the input element count and R is the number of groups. The direct path
removes the full **4N-byte f32 input conversion**. Actual f32 output buffers are
common to both paths; moments has two outputs. Groups of 2-256 values use the
shared-memory small kernel, without row summaries or a reduction partial array.
For longer groups, both use the same two-word partial buffer; direct low
summaries are eight words rather than four. Each retained stage therefore adds
16R bytes, plus a 16-byte difference in the dummy summary. Distribution has two
stages; moments/layer norm has three. For the measured nonempty cases, net
visible f32 scratch reduction is:

- K<=256: `4N` bytes.
- K>256, distribution: `4N - 32R - 16` bytes.
- K>256, moments/layer norm: `4N - 48R - 16` bytes.

| Case | Removed input buffer | Net distribution reduction | Net moments/layer-norm reduction |
| --- | ---: | ---: | ---: |
| small | 4,224 B | 4,224 B | 4,224 B |
| rows | 1,052,672 B | 1,019,888 B | 1,003,504 B |
| long | 524,308 B | 524,260 B | 524,244 B |
| strided | 1,053,700 B | 1,045,460 B | 1,041,348 B |
| nonlast | 278,732 B | 278,732 B | 278,732 B |

These count logical f32 arrays kept by the recorded programs, not peak RSS.
Input/output storage, metadata buffers, padding, readback and driver allocations
are separate. The benchmark prints only the removed input buffer size.
The additional row state is included in the net figures above.

## Evidence and limits

[Run 1](tensor-low-statistics-metal.txt),
[unchanged repeat](tensor-low-statistics-metal-repeat.txt),
[final source fingerprints](tensor-low-statistics-source-fingerprints.json),
[correctness contracts and diagnostic history](tensor-low-normalization-contracts.md),
[full qualification](../../../docs/qualification/tensor-low-statistics-2026-09-27.md).

Both runs have 31 samples per path/mode; all 400 printed median/p90 pairs
(800 values) were independently recomputed from their samples. The comparison
includes the conversion cost in the baseline. Low-output performance, broadcast input,
several simultaneous reduction axes, other devices and end-to-end application
workloads are unmeasured. Device-state changes and microsecond-scale noise limit
conclusions from small timing differences. No CUDA/MLX speed claim is made.
