# Nearest-neighbor GPU optimization, 2026-09-27

## Change and selection

The original shader assigns one invocation to a query and serially scans every
target. The retained cooperative shader assigns one 64-lane workgroup to each
query. Lanes scan distinct target subsequences, then reduce `(distance, index)`
pairs. Equal distances choose the smaller index, preserving the original first
target rule. Empty targets and overflow retain `(u32::MAX, f32::MAX)`.

Recorded plans and the synchronous adapter share `NearestKernels` and the same
selection rule. The synchronous adapter also records the kernel and both output
readbacks in one submission; its former transport needed three submissions.
GPU buffers and bind groups remain cached across repeated synchronous calls,
including changes between the two algorithms at smaller input sizes.

Default cooperative dispatch is limited to Metal with 512..8192 targets and
256..(2 * targets) queries. This bounded heuristic follows the three measured
shapes below. It avoids extrapolating to many-query/few-target inputs or other
backends. Scalar dispatch remains the reference and fallback. This was measured
on one M4 Max; it is not a claim that every Metal adapter has the same crossover.
`NearestNeighborAlgorithm::for_shape` exposes the rule. CPU/CUDA placement and
`Acceleration::Auto` are unchanged.

## Method

- Apple M4 Max, 40 GPU cores, Metal; native release build, `gpu` feature.
- Same resident input buffers and both full output arrays for each path.
- Original scalar shader, raw cooperative shader, and actual recorded API all
  use one dispatch and one submission, with identical readback transport.
- Inputs are deterministic binary fractions. Every output index and distance
  is checked against the f64 CPU implementation before measurement. Separate
  tests cover near ties, overflow, empty clouds and target-group tails.
- Final run: seven warmup rotations followed by 45 measured rotations, with
  candidate order rotated each iteration. Timing includes encoding, submission,
  GPU completion, both full readbacks and typed host decoding. These are host
  latency measurements, not GPU timestamp measurements.
- Upload-inclusive mode also rewrites both input arrays. Synchronous mode
  includes f64-to-f32 conversion. Its baseline reconstructs the old scalar
  kernel and three-submission transport with retained buffers; it omits the
  old uniform rewrite and error-scope overhead, conservatively favoring baseline.
- CPU median uses seven measurements after one warmup. No other task GPU
  benchmarks ran concurrently. The desktop and OS may still affect latency.

Reproduce from the repository root:

```sh
NN_BENCH_REPEATS=45 NN_BENCH_WARMUPS=7 cargo run --offline --release \
  --manifest-path crates/Cargo.toml -p osv-math-compute --features gpu \
  --example bench_recorded_nearest
```

## Final medians, milliseconds

| Queries × targets | Resident scalar | Resident recorded | Speedup | Upload scalar | Upload recorded | CPU f64 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 256 × 512 | 0.411458 | 0.171166 | 2.40× | 0.279709 | 0.195333 | 0.124708 |
| 4096 × 4096 | 0.592708 | 0.218625 | 2.71× | 0.639083 | 0.256042 | 17.164833 |
| 16384 × 8192 | 1.440791 | 1.024625 | 1.41× | 1.530792 | 1.151083 | 143.792917 |

| Queries × targets | Sync scalar / three submits | Sync selected / one submit | Speedup |
| --- | ---: | ---: | ---: |
| 256 × 512 | 0.337583 | 0.182291 | 1.85× |
| 4096 × 4096 | 0.766708 | 0.322959 | 2.37× |
| 16384 × 8192 | 1.974583 | 1.524500 | 1.30× |

CPU is still faster for the smallest case. All modes run in separate rotated
windows; do not subtract their medians to estimate upload cost.

[Full final medians and p90](nearest-metal-production.csv) include the raw
cooperative path, which closely matches the recorded path in the final run.

## Candidate selection and variability

The initial [candidate comparison](nearest-metal-candidates.txt) tested shared
memory target tiling with 64/256 threads and cooperative reduction with 64/256
threads. Both cooperative choices beat target tiling on all three shapes;
64 lanes won the two larger workloads. Only the 64-lane cooperative variant and
the original scalar reference remain in production code.

The first [15-sample production run](nearest-metal-first-production.csv) had
wide tails and an unexplained large-case recorded/raw gap: 1.478 ms recorded,
1.130 ms raw cooperative, 1.361 ms scalar. That run did not establish a large-case
production improvement. Code inspection confirmed the same shader, workgroup
size and dispatch count, and a single longer rotated repeat resolved the gap:
1.025 ms recorded versus 1.048 ms raw cooperative, both below 1.441 ms scalar.
The retained result is based on that 45-sample repeat, not on raw-kernel timing
alone. No NVIDIA, other Apple GPU or browser performance claim is made.
