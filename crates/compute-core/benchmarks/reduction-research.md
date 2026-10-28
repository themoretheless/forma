# Sum reduction research

This study measures full f32 reductions on the available Metal adapter. Results
are device-specific; other backends have no measured performance qualification.
All candidates use portable workgroup memory and arithmetic. Subgroup experiments
are a separate study.

## Method

`bench_reduction_candidates` compares a frozen copy of the previous
`BLOCK_SUM_WGSL` shader and schedule with different workgroup sizes, values per
lane, and limits on partial workgroups. Candidates record every reduction stage
inside one compute pass and copy the final four-byte scalar into a readback.
Compilation, allocations, binding creation and input upload precede measurement.
Inputs are resident and the prepared plans are reused.

Each length has at least 200 ms of sustained warmup, followed by 21 samples per
candidate and timing mode in the exploration sweeps, or 31 in the production
qualification. Candidate order rotates, and the order of the two
modes alternates:

- **Host:** ordinary command submission and a four-byte readback, with no query
  writes or query resolution in that execution.
- **GPU:** timestamps around all reduction dispatches in their shared compute
  pass. The corrected timer resolves counters in a separate submission after the
  measured work completes. This mode does not supply the host latency numbers.

Every execution compares its scalar with an f64 CPU reference. Every GPU timing
is checked against the full profiled execution wall time. Raw files retain
samples, median, p90, and workgroup counts for every stage. The timestamp-capable
device is the same in both modes.

## Candidate coverage

The first sweep covers WG64/128/256; 1, 4, 8, or 16 scalar values per lane;
4, 8, or 16 values via actual vec4 storage; and workgroup limits 256/1024/4096.
Scalar candidates use independent registers with coalesced loads spaced by the
workgroup size. The original shader uses a single dependent accumulator.

The focused sweep separates scheduling improvements from shader changes. It
also constructs vectors from scalar storage loads. This preserves arbitrary
four-byte buffer sizes, including unpadded final values, while retaining vector
arithmetic and independent register accumulators.

True vec4 storage is an experimental reference only: it requires 16-byte minimum
bindings and padding after a partial vector. General `GpuArray<f32>` has neither
requirement. The benchmark pads its shared inputs and partials so these
experimental candidates are compared under identical transport conditions.

## Retained policy and actual API results

`ComputeProgram::sum` and `sum_into` use the existing scalar-storage
`BLOCK_SUM_WGSL`. On Metal, for more than 65,536 values, the first dispatch has
`min(ceil(n / (256 * 16)), 256)` workgroups. Each lane grid-strides through its
assigned values. A second dispatch reduces at most 256 partials. For example,
16,000,003 values need `[256, 1]` groups instead of `[62501, 245, 1]`.

Inputs at or below 65,536 values and every other backend retain the old schedule.
The first policy included exactly 65,536 values; round 3 found a one-microsecond
GPU regression there, so that boundary was excluded before round 4.

The following medians are from the **actual prepared runtime program**, with
matching input, output, timing and transport. The frozen baseline also uses a
shared compute pass, so the large improvements below come from scheduling, not
from changing pass boundaries.

| Values | Baseline host ms | Production host ms | Baseline GPU ms | Production GPU ms |
|---:|---:|---:|---:|---:|
| 4,095 | 0.125250 | 0.126834 | 0.007334 | 0.007250 |
| 65,536 | 0.136709 | 0.140458 | 0.009542 | 0.009459 |
| 65,537 | 0.141625 | 0.138417 | 0.012459 | 0.010500 |
| 262,145 | 0.153167 | 0.139584 | 0.023125 | 0.010709 |
| 1,000,003 | 0.192375 | 0.134750 | 0.063667 | 0.013417 |
| 4,000,003 | 0.354083 | 0.149458 | 0.228750 | 0.026000 |
| 16,000,003 | 1.156375 | 0.323708 | 0.990791 | 0.152750 |

At 16M, host p90 changes from 1.231041 to 0.365625 ms, and GPU p90 from
1.019292 to 0.174208 ms. At tiny sizes the dispatches are unchanged; the raw
samples show ordinary host latency variation. No small-input speedup is claimed.

Public raw `Reduction` constructors retain their prior group scheduling for
caller-provided kernels. `Reduction::record` now places its dispatches in one
compute pass, and `record_in_pass` allows composition with other work and timing.
`sum_into` validates runtime ownership, output length and aliasing before adding
commands. Empty input actively overwrites its scalar result with zero on every
submission. Neither input nor output needs vec4 padding.

## Rejected alternatives

- Actual vec4 storage is fast, but adds a binding-size and tail-padding contract.
  The bounded original shader already gives most of the improvement.
- Constructing vectors from scalar loads preserves the ABI. WG128 with eight
  values per lane improves GPU medians slightly in several cases, but host
  results do not show a consistent additional win. It stays a research candidate.
- Limits of 1,024 or 4,096 workgroups can help individual large runs but increase
  partial work and vary across repeated runs. The final paired run favors 256 at
  both 4M and 16M.
- Applying the larger grain to tiny inputs increases arithmetic per workgroup
  without reducing enough dispatch overhead. The retained cutoff avoids this.

All candidate sources remain under `examples/reduction_candidates`. No new
shader, subgroup dependency, padded-array ABI or autotuning framework is added
to the production reduction.

## Evidence

- `reduction-candidates-metal-round1.txt`: 28-candidate scalar/vector sweep.
- `reduction-candidates-metal-round2.txt`: scalar ABI and scheduling follow-up.
- `reduction-production-metal-round3.txt`: first actual runtime policy, including
  the rejected 65,536 boundary.
- `reduction-production-metal-round4.txt`: retained cutoff, 31 rotated samples at
  14 lengths, both timing modes. This file supplies the table above.
- `reduction-tests-metal.txt`: five real-GPU tests, all passed in 3.04 seconds.
- `reduction-source-fingerprints.json`: source SHA-256 values and build identity.

## Numerical contract

These are parallel f32 reductions. The order of additions changes with the
schedule. Inputs and intermediate arithmetic must be finite. The API does not
promise compensated summation, bitwise equality with a sequential CPU sum, or
small relative error when cancellation makes the true sum approach zero.
Tests compare against f64 with a forward-error bound scaled by the input norm,
and cover cancellation, dynamic range, tails and repeated input updates.

## Reproduction

```sh
cargo run --manifest-path crates/Cargo.toml -p compute-core --release \
  --example bench_reduction_candidates
cargo run --manifest-path crates/Cargo.toml -p compute-core --release \
  --example bench_reduction_candidates -- --focused
cargo run --manifest-path crates/Cargo.toml -p compute-core --release \
  --example bench_reduction_candidates -- --production
COMPUTE_REQUIRE_GPU=1 cargo test --manifest-path crates/Cargo.toml \
  -p compute-core --test reduction -- --test-threads=1
```

Run with an available GPU and without overlapping GPU benchmarks. Cargo can
still print the unrelated existing `raster-core` binary naming warning.
