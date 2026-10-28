# Matrix kernel study — 2026-09-27

## Contract and scope

The public operation remains a dense row-major f32 matrix product. Shape,
ownership and alias checks run before recording; intermediate data stays on the
GPU. The original general 32×32 tiled kernel remains the portable fallback.
This study changes only kernel selection on Metal. Other backends need their own
measurements before enabling these substitutions.

Toolchain: rustc 1.100.0-nightly (215a8af4b, 2026-09-15), Cargo
1.100.0-nightly (7941be6fb, 2026-09-11). Native Metal execution was required; no
adapter skip or CPU fallback supplied the GPU results. Shader compilation and
uploads are outside timing. The measurements are host-observed submission,
completion and full output readback, without GPU timestamps.

## Method

`bench_matmul_candidates --extended` checks every candidate against an independent
f64 product before timing. It runs four correctness warmups, at least 250 ms of
continued work to reduce startup clock transitions, then seventeen samples per
path. Candidate order rotates each iteration. Every candidate uses its own
retained output/bindings and the same encoder, copy/map and wait structure.
Full raw sample vectors, medians, nearest-rank p90 and maximum absolute errors
are saved. The final run also measures the actual `ComputeProgram::matmul` path.

Reproduce:

```sh
cargo run --release --offline -p compute-core --example bench_matmul_candidates -- --extended
COMPUTE_REQUIRE_GPU=1 cargo test --offline -p compute-core --test matrix
```

Run from the Cargo workspace (`crates`) and keep concurrent GPU workloads out of
the measurement window. Compiler activity and OS scheduling still affect host
latency; absolute timings changed between rounds. Compare paths within the same
round, with p90 as well as median. The separate `bench_matmul` example also
compares a single-threaded cache-blocked f32 Rust CPU baseline; it is not BLAS.

## Candidates

| Candidate | Mechanism | Decision |
| --- | --- | --- |
| General 32×32 | 64 lanes, sixteen register accumulators per lane, scalar shared loads, bounded tails | Portable baseline retained |
| Direct | One invocation per output; no shared storage or barriers | Retained in measured small/narrow ranges |
| Aligned scalar 32×32 | Removes tail guards and zero padding for aligned shapes | Rejected as a separate production path: vec4 tiles cover the same precondition with broader gains |
| Aligned vec4 32×32 | Aligned global/shared vector loads and vector stores; no padding guards | Retained for a bounded aligned range |
| Direct vec4 | Four output columns per invocation | Rejected: gains varied and it lost on several tested shapes |
| Cooperative dot | One 64-lane workgroup per output, tree reduction over K | Retained only for small outputs with long inner dimensions |

Frozen experiment sources live in `examples/matmul_candidates`. They intentionally
preserve the original baseline and rejected candidates for reproducibility.
Production sources live in `shaders/matmul*.wgsl`; only the retained kernels enter
the runtime catalog.

## Selection policy

The following order applies only on Metal, after empty output and shape checks:

1. Use cooperative dot when `m*n <= 1024` and `k >= 256`.
2. Use aligned vec4 tiles when `m`, `k` and `n` are positive multiples of 32,
   and either `m*n >= 16384` or `k >= 128`.
3. Use direct multiplication when `m*n <= 32768 && k <= 512`, or when
   `min(m,n) <= 16 && k <= 64`.
4. Use the original general tiled kernel otherwise.

These are bounded dispatch choices derived from local data, not an autotuner.
The direct kernel is deliberately not selected for all small-K products:
`511×63 @ 63×257` regressed. Nor is it selected for all short-wide products:
`16×512 @ 512×4096` changed from a small gain to a regression across rounds.
Unaligned output tails still use scalar-safe kernels. Zero-K products never use
vec4 bindings to empty four-byte placeholder buffers.

Cooperative reduction changes floating-point accumulation order. The public
contract remains finite f32 arithmetic with tolerance-based comparison; it does
not promise bitwise equality across selected kernels or backends.

## Evidence before production integration

Round 1 used only four warmups. Its first 512³ samples showed a clock transition,
so that run was not sufficient to choose a policy. Round 2 added sustained warmup
and 23 shapes covering alignment, odd dimensions, long inner axes and policy
boundaries. The table gives paired median milliseconds from round 2:

| `(m,k,n)` | General baseline | Retained candidate | Candidate |
| --- | ---: | ---: | --- |
| 64,64,64 | 0.247500 | 0.198333 | Direct |
| 127,259,193 | 0.544792 | 0.383625 | Direct |
| 255,129,63 | 0.362209 | 0.265209 | Direct |
| 512,512,512 | 0.714708 | 0.574833 | Aligned vec4 |
| 1024,1024,1024 | 1.886084 | 1.792750 | Aligned vec4 |
| 256,1024,256 | 0.511750 | 0.332084 | Aligned vec4 |
| 4096,256,32 | 0.585875 | 0.418625 | Aligned vec4 |
| 65537,1,1 | 0.506084 | 0.204083 | Direct |
| 32,1024,32 | 0.976958 | 0.168000 | Cooperative dot |
| 8,2048,8 | 0.904917 | 0.129417 | Cooperative dot |

The 1024³ gain is small relative to run variation; it does not justify a broad
claim of improvement for every aligned shape. The small-output cooperative
wins are much larger and consistent across neighboring tested shapes.

Raw files:

- [Round 1](matmul-candidates-metal-round1.txt)
- [Round 2](matmul-candidates-metal-round2.txt)

## Integrated production result

[Round 3](matmul-production-metal-round3.txt) adds the actual public
`ComputeProgram::matmul` path to the same rotated candidate experiment. It verifies
all full outputs against f64 before timing and on every measured execution.
The following values are paired medians from this final run; brackets give p90.

| `(m,k,n)` | Original general kernel, ms | Selected public operation, ms |
| --- | ---: | ---: |
| 64,64,64 | 0.143708 [0.150250] | 0.134584 [0.145291] |
| 127,259,193 | 0.222125 [0.273083] | 0.195584 [0.200833] |
| 512,512,512 | 0.426834 [0.463667] | 0.355834 [0.404167] |
| 1024,1024,1024 | 2.106167 [2.206416] | 1.802875 [1.947167] |
| 256,1024,256 | 0.516333 [0.567667] | 0.357709 [0.392291] |
| 4096,256,8 | 0.221583 [0.236875] | 0.172458 [0.207291] |
| 65537,1,1 | 0.229833 [0.304292] | 0.156459 [0.251875] |
| 32,1024,32 | 0.506625 [0.512000] | 0.121125 [0.188000] |
| 16,2048,64 | 0.859166 [0.881667] | 0.164458 [0.178833] |
| 8,2048,8 | 0.674333 [0.708208] | 0.118708 [0.138708] |

The excluded regression shapes `511×63 @ 63×257` and `16×512 @ 512×4096`
continue to select the original kernel; measured differences were about 1%,
consistent with host variation. Gains for tiny workloads are small in absolute
terms. The `4096×16 @ 16×16` median improved only from 0.166709 to 0.158916 ms,
while p90 regressed from 0.223917 to 0.275541 ms; the policy does not promise
lower tail latency for every shape. Absolute times differ substantially from
older runs, so only within-run comparisons support the optimization claims.

Maximum absolute error of the production path in round 3 was 3.160e-5. The
cooperative reduction has different roundoff than the original sequential FMA
loop; its results were checked directly against f64. Six Metal integration tests
also passed, covering all selected kernels, repeated changed inputs, retained
output tails, empty/zero-K contracts, invalid shapes/owners/aliases, and workloads
beyond 65,535 workgroups. A separate CPU policy test checks bounds and that Vulkan
continues to select the general kernel.

[Source fingerprints](matmul-source-fingerprints.json) identify the matrix and
experiment sources used for the final study. They are not a full build manifest.
