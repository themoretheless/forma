# Centered GPU point-cloud covariance

Measured 2026-09-27 on Apple M4 Max (40 GPU cores), Metal, release build.
This change adds an explicit precision option; legacy/default GPU and CUDA
selection remain unchanged.

## Result and retained implementation

`MathGpuProgram::point_cloud_stats_stable[_into]` first records the existing
bounds/raw-moments plan. A second GPU traversal accumulates residuals
`d = p - centroid`, then computes covariance as `E[dd] - E[d]E[d]`.
Subtracting the residual mean matters because the first centroid rounds to f32.
Only the six covariance fields change. Bounds, sums, raw product sums and centroid
retain their values and the existing 24-f32 packed layout.

The initial centered shader used a 256-lane workgroup and nested scalar arrays
with a dynamic nine-field reduction loop. The retained shader uses 128 lanes,
three workgroup arrays of vec3, and three vector additions per tree level. It
reuses the existing hierarchical 15-scalar statistics fold. The first centered
implementation remains in `examples/stats_baseline` as a frozen benchmark
control, including its fold and finalizer.

`MathGpuSession::try_point_cloud_stats_stable` uses this same plan, uploads f32
points and reads the final 24 scalars. Kernels/runtime are cached, while each
synchronous call allocates its input/intermediate plan. Keep a recorded plan to
reuse those allocations.

## GPU time and accuracy

Median compute-pass timestamp milliseconds. Error is the largest absolute error
among the six covariance entries, against centered compensated CPU f64 statistics
of the **actual uploaded f32 coordinates**.

| Points | Offset | Legacy GPU ms | First centered GPU ms | Retained GPU ms | Legacy error | Retained error |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 4,097 | 0 | 0.326459 | 0.798625 | 0.644792 | 3.0e-8 | 3.0e-8 |
| 4,097 | 10,000 | 0.631833 | 1.559167 | 1.252417 | 4.253380 | 3.0e-8 |
| 4,097 | 1,000,000 | 0.322791 | 0.798000 | 0.645083 | 186,057.296326 | 3.0e-8 |
| 1,000,003 | 0 | 0.819666 | 3.538084 | 1.557459 | 1.23e-7 | 1.23e-7 |
| 1,000,003 | 10,000 | 1.388750 | 5.819250 | 2.315667 | 22.610284 | 1.23e-7 |
| 1,000,003 | 1,000,000 | 1.585083 | 7.069167 | 2.663958 | 146,124.484371 | 1.23e-7 |

At one million points the vector revision is 2.27–2.65× faster than the first
centered implementation. The retained centered algorithm still costs 1.68–1.90×
the legacy GPU time in these paired cases. At 4K it costs about twice the legacy
GPU time. This is an accuracy improvement with a measured additional cost.

## Host latency

Median milliseconds including all 24 output scalars. Resident and upload rows
use no timestamp profiler. Upload includes writing the same packed input into
an existing allocation before dispatch. CPU uses the compensated f64 reference.
Session measurements include upload, allocation, dispatch, readback and public
summary construction, with pipeline compilation excluded.

| Points | Offset | Legacy resident | Retained resident | Legacy upload | Retained upload | CPU | Legacy session | Retained session |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 4,097 | 0 | 0.500625 | 0.844333 | 0.560417 | 0.878541 | 0.125875 | 0.264625 | 0.987167 |
| 4,097 | 10,000 | 0.855959 | 1.492334 | 1.069459 | 1.907333 | 0.120417 | 0.317708 | 1.893625 |
| 4,097 | 1,000,000 | 0.506167 | 0.838584 | 0.523083 | 0.881709 | 0.121208 | 0.259292 | 0.988041 |
| 1,000,003 | 0 | 1.060125 | 1.826042 | 3.946125 | 4.697250 | 63.004792 | 13.458416 | 12.109750 |
| 1,000,003 | 10,000 | 2.079667 | 3.497459 | 7.044125 | 8.274125 | 54.887625 | 16.531375 | 15.163291 |
| 1,000,003 | 1,000,000 | 2.057333 | 3.502959 | 6.165833 | 7.874209 | 48.861625 | 22.641916 | 17.605500 |

Absolute latency varied substantially across cases and between runs, including
the unchanged CPU reference (about 29.5 ms in the initial million-point run,
48.9–63.0 ms in the final run). The benchmark reserved the GPU against other
agents' workloads but did not lock clocks or isolate all system CPU activity.
Use the rotated comparisons within each case; do not interpret different
offsets or separate runs as data-dependent performance claims. In particular,
the apparent synchronous speedup over legacy does not justify automatic
backend selection. Small clouds remain clearly faster on the CPU.

## Method and limits

- Two sizes, three translations `[offset, -2*offset, offset/2]`, identical finite
  planar point data, identical full output transport for all three GPU paths.
- 200 ms warmup per case, 31 rotated samples per resident/upload/profile mode;
  15 rotated synchronous samples and five CPU samples.
- Timestamp intervals cover the compute pass only. Their separate host timings
  include the profiler's post-completion resolve submission; those are retained
  in CSV but are not the ordinary resident timings above. All 558 measured GPU
  intervals were positive and bounded by their corresponding wall interval.
- CPU correctness uses centered f64 covariance, rather than subtracting raw
  second moments. Legacy raw fields 0..18 are checked for exact equality.
- Four new Metal tests cover singleton, odd/hierarchical sizes, translation
  invariance, covariance symmetry/eigenvalue tolerance, rounded-centroid
  correction, transform composition, repeated uploads/reuse, wrong lengths,
  aliases, foreign devices and shared cloned contexts. Five existing recorded
  domain tests also passed. CPU-only math tests: 68 passed.
- Arithmetic remains f32. Differences lost during input conversion above 2^24
  cannot be recovered; a test makes that limit explicit. Inputs, raw products
  and all intermediate sums must remain finite. Tiny negative eigenvalues can
  still arise from rounding. Recomputing covariance from raw `OUTER_SUM` loses
  the benefit of the centered covariance fields.

## Consumer trace and migration boundary

`photogrammetry-core/src/evaluation.rs` calls `point_cloud_stats_accelerated` for
both reconstructed and reference clouds (currently lines 318–319). The new
session method is an available opt-in for the same `PointCloudStats` result.
No default switch was made: callers should choose it for accuracy or retained
GPU data, with small host clouds staying on the CPU.

`point_principal_axes` and `point_fit_plane` consume the separate moments API,
which remains unchanged. ICP's `registration.rs` scores transformed squared
errors; its f32 transform/subtraction can lose a small residual before the
positive sum reduction. This covariance change does not address that error.

## Reproduction and evidence

```sh
cargo run --offline --release --manifest-path crates/Cargo.toml \
  -p osv-math-compute --features gpu --example bench_stable_stats
COMPUTE_REQUIRE_GPU=1 cargo test --offline --manifest-path crates/Cargo.toml \
  -p osv-math-compute --features gpu --test gpu_stable_stats --test gpu_recorded_domains \
  -- --test-threads=1
```

- [Final rotated raw samples and medians](stable-stats-metal.csv)
- [Initial two-path experiment](stable-stats-metal-initial.csv)
- [Backend/method metadata](stable-stats-metal-context.txt)
