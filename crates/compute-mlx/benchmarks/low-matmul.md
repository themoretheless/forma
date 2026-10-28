# MLX direct low-input matmul with f32 output

The [benchmark](../examples/bench_low_matmul.rs) compares two paths from the
same resident f16 or BF16 tensor handles to an f32 result:

1. Cast both operands to f32, then call ordinary f32 matmul.
2. Call `matmul_low_f32` directly on the low tensors.

Both paths read the entire f32 result. The benchmark measures wall time;
it does not report GPU timestamps or kernel-only time. The first qualified
shared-memory tiled kernel, SIMD-group candidate and final routed variant
are measured below, with each stage preserved separately.

## Avoiding cached-output measurements

MLX arrays retain evaluated results. Every execution in this benchmark
constructs a fresh operation graph, including fresh operand casts for the
baseline. Re-evaluating an already completed matmul result is never a sample.
The shared low input handles and their prepared views are reused.

The timer starts before graph construction and ends after `read_f32` and
stream synchronization. It includes graph construction, allocation work,
MLX evaluation, contiguous result access, copying all result values to a Rust
vector and `synchronize`. Input upload, view construction, input evaluation,
first-use compilation, oracle construction, output validation and destruction
of the final result handle are outside the timing interval. MLX's warmed
allocator and compiled-kernel caches remain enabled for both paths.

The adapter's [`read_f32`](../src/native.rs) materializes its input, calls
`mlx_array_eval`, obtains the f32 pointer and copies the values. The installed
MLX-C 0.6.0 array header requires evaluation before data access, and its stream
header defines `mlx_synchronize` as synchronization with the selected stream.
MLX 0.32.1's array header records separate unscheduled, evaluated and available
states. These local interfaces were inspected when preparing the benchmark.
Explicit synchronization after the read is included equally in both paths.

Each case first checks and evaluates both low views, then discards one
validated first-use execution per path. At least 200 ms of alternating
warmup follows. There are 31 measured samples per path, with the order
reversed on successive iterations. Full results are checked after each
timer, including warmup results. The log prints every raw sample, median,
p90 and baseline/direct median ratio. There is no aggregate speedup claim
across unlike shapes.

## Cases

Every row runs for both f16 and BF16: 12 cases and 744 measured outputs.

| Case | Left | Right | Result |
| --- | --- | --- | --- |
| Vector dot | `[257]` | `[257]` | scalar |
| Decode vector | `[1025]` | `[1025,129]` | `[129]` |
| Matrix × vector | `[129,1025]` | `[1025]` | `[129]` |
| Partial tiles | `[37,65]` | `[65,51]` | `[37,51]` |
| Moderate GEMM | `[128,257]` | `[257,192]` | `[128,192]` |
| Batch, transpose and broadcast | `[2,17,65]` | `[2,65,21]` | `[2,17,21]` |

For the last case, left storage is `[2,65,17]` with its last two axes
transposed. Right storage is `[1,21,65]`, also transposed, then broadcast
across two batches. Both timing paths receive exactly these low views.

Inputs are deterministic integers in `[-16,16]` divided by 16 and are exact
in both low formats. An independent CPU f64 loop computes every expected
matrix entry from the physical input values and logical view addresses.
Products are multiples of 1/256; the largest contraction has 1,025 terms
with magnitude at most one each. Products and every possible partial sum
therefore remain exactly representable in f32. Equality with the f64 oracle
is checked exactly. This isolates algorithm/layout correctness and avoids a
loose timing-workload tolerance. It does not characterize error for general
floating-point inputs, which belongs to the conformance tests.

## Memory accounting

`physical_low_input_bytes` is twice the number of uploaded storage elements.
The broadcast case shares one physical right-hand batch even though its
logical shape has two. `logical_removed_f32_operand_bytes` is
`4 * (left.numel + right.numel)`: the logical f32 operand size omitted by the
direct API. It is not a measured allocation reduction or peak-memory count;
MLX can preserve broadcast strides and reuse allocations. The common f32
result, runtime/kernel scratch and graph bookkeeping are excluded from that
operand count.

The final source declares 3,104 bytes of GEMM workgroup arrays: three
`float[256]` tiles, four u32 flags and two u64 batch bases. The vector path
declares 1,040 bytes: `float[256]` partials and two u64 bases. Both upload
seven u32 parameters (28 bytes of payload). These are source-level sizes,
verified against the frozen kernels. They exclude compiler registers,
spills, allocation alignment and driver bookkeeping; they are not measured
peak memory. No full f32 input operand is introduced by either direct path.

## Reproduction and evidence

Run on an otherwise idle GPU after native correctness qualification:

```sh
cargo build --release --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-mlx --example bench_low_matmul
cargo run --release --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-mlx --example bench_low_matmul
```

The executable requires a real available MLX Metal backend and direct
low-input/f32-output support for both dtypes. It returns an error or fails
an assertion when support or correctness is missing; there is no skipped
successful benchmark. Preserve two complete raw runs and source/log hashes
with the final qualification evidence. Do not compare cached evaluations,
change inputs between paths or combine timing runs from different sources.

## Initial tiled-kernel results

MLX 0.32.1 on Apple M4 Max, after all 69 native tests passed. Both paired
runs validated all 744 measured output buffers exactly, plus every warmup
result. All 83 captured source files and the compiled benchmark binary
remained unchanged across the two runs.

The direct path was slower in all 12 cases in both runs. These wall times
include graph construction and readback, so they do not isolate kernel
execution. The first decode run also differed materially from the second;
both raw runs are retained without choosing the better run. The audit
recomputes all 48 median/p90 pairs from the 1,488 raw samples. It does not
attribute differences between separate runs to a particular cause.

| Case | Dtype | Run 1 cast/direct ms | Run 2 cast/direct ms |
| --- | --- | ---: | ---: |
| vector_dot | F16 | 0.143125 / 0.160375 | 0.169666 / 0.185917 |
| vector_dot | Bf16 | 0.128500 / 0.166250 | 0.132833 / 0.176916 |
| decode_vector | F16 | 0.154541 / 0.401208 | 0.141917 / 0.261750 |
| decode_vector | Bf16 | 0.164166 / 0.456334 | 0.133000 / 0.342958 |
| matrix_vector | F16 | 0.135209 / 0.284250 | 0.153750 / 0.313667 |
| matrix_vector | Bf16 | 0.154750 / 0.386417 | 0.167958 / 0.444458 |
| odd_gemm | F16 | 0.136834 / 0.143750 | 0.132125 / 0.145250 |
| odd_gemm | Bf16 | 0.132792 / 0.151000 | 0.137292 / 0.152667 |
| moderate_gemm | F16 | 0.139041 / 0.236833 | 0.134083 / 0.234667 |
| moderate_gemm | Bf16 | 0.141166 / 0.259916 | 0.126791 / 0.250875 |
| batch_transpose_broadcast | F16 | 0.137583 / 0.149750 | 0.130500 / 0.143209 |
| batch_transpose_broadcast | Bf16 | 0.130542 / 0.154333 | 0.119959 / 0.140166 |

Evidence: [first raw run](low-matmul-initial-metal.txt),
[repeat raw run](low-matmul-initial-metal-repeat.txt),
[recomputed medians and p90](low-matmul-initial-summary.json),
[CPU audit of all 1,488 samples](low-matmul-initial-audit.json),
[source, binary and log hashes](low-matmul-initial-source-fingerprints.json).
The [qualified source archive](../qualification/low-matmul-tiled-source-manifest.json)
also includes the benchmark source. No GPU-only or peak-memory measurement
is inferred from these results.

## SIMD-group candidate results

The SIMD-group candidate passed all 70 native MLX tests before timing.
The same benchmark source, input values, timing intervals and sample counts
were used. Every element of all 1,488 measured output buffers again matched
the f64 oracle exactly. All 84 captured source files and the compiled binary
remained unchanged across the two runs.

The direct path remains slower than the cast-plus-native-matmul baseline in
all 12 cases in both runs. These host results do not establish an overall
benefit from the SIMD-group candidate. Differences between separate stages
or repeats cannot isolate GPU kernel performance or their cause.

| Case | Dtype | Run 1 cast/direct ms | Run 2 cast/direct ms |
| --- | --- | ---: | ---: |
| vector_dot | F16 | 0.134875 / 0.159500 | 0.184167 / 0.219209 |
| vector_dot | Bf16 | 0.141917 / 0.162958 | 0.130917 / 0.156708 |
| decode_vector | F16 | 0.141958 / 0.338750 | 0.140000 / 0.340875 |
| decode_vector | Bf16 | 0.137875 / 0.344083 | 0.143292 / 0.352333 |
| matrix_vector | F16 | 0.169875 / 0.398625 | 0.162375 / 0.396458 |
| matrix_vector | Bf16 | 0.165833 / 0.401583 | 0.160334 / 0.407625 |
| odd_gemm | F16 | 0.132333 / 0.144667 | 0.124167 / 0.139791 |
| odd_gemm | Bf16 | 0.137667 / 0.153625 | 0.123333 / 0.140000 |
| moderate_gemm | F16 | 0.137959 / 0.246875 | 0.144125 / 0.250750 |
| moderate_gemm | Bf16 | 0.128250 / 0.236792 | 0.129375 / 0.239084 |
| batch_transpose_broadcast | F16 | 0.123708 / 0.146375 | 0.134958 / 0.160834 |
| batch_transpose_broadcast | Bf16 | 0.122417 / 0.145625 | 0.139375 / 0.162500 |

Evidence: [first SIMD raw run](low-matmul-simd-metal.txt),
[repeat raw run](low-matmul-simd-metal-repeat.txt),
[recomputed medians and p90](low-matmul-simd-summary.json),
[audit of 48 median/p90 pairs and 1,488 samples](low-matmul-simd-audit.json),
[source, binary and log hashes](low-matmul-simd-source-fingerprints.json).
The [qualified SIMD source overlay](../qualification/low-matmul-simd-source-manifest.json)
preserves its differences from the initial tiled source snapshot.

An [archive audit](low-matmul-archive-audit.json) verified all 83 initial,
84 SIMD and 85 final source hashes against retained source copies, all six
raw logs and all three parsed summaries and sample audits. The
workspace Cargo manifest is retained in the supplemental shared snapshot.

## Final routed-variant results

The retained variant passed all 72 native MLX tests before timing. It combines
hoisted matrix stride addressing, a K-parallel vector/dot kernel, and reuse
of existing input views when broadcast would leave the shape unchanged.
The comparison measures this complete variant; it cannot determine the
individual contribution of any one edit.

All 1,488 measured output buffers matched the f64 oracle exactly. The
benchmark source and workload are unchanged from both earlier stages, and
all 85 captured source files plus the release binary stayed unchanged during
the two final runs. The audit recomputed all 48 median/p90 pairs.

Dot and decode have lower direct-path medians for both dtypes in both runs
(four of 12 cases). Dot ratios are about 1.06–1.07×; decode ratios range
from 1.02× to 1.12×. The other eight cases remain near parity or slower.
The small differences and variation between repeats limit generalization.
These are host timings that include graph construction and full readback,
not GPU-only speedups.

| Case | Dtype | Run 1 cast/direct ms | Run 2 cast/direct ms |
| --- | --- | ---: | ---: |
| vector_dot | F16 | 0.122375 / 0.114833 | 0.127542 / 0.120208 |
| vector_dot | Bf16 | 0.121791 / 0.113750 | 0.123292 / 0.115542 |
| decode_vector | F16 | 0.163625 / 0.145875 | 0.126459 / 0.123917 |
| decode_vector | Bf16 | 0.140583 / 0.132917 | 0.122125 / 0.119083 |
| matrix_vector | F16 | 0.116709 / 0.119500 | 0.109125 / 0.117542 |
| matrix_vector | Bf16 | 0.112750 / 0.114917 | 0.115750 / 0.122791 |
| odd_gemm | F16 | 0.130083 / 0.135000 | 0.121083 / 0.121500 |
| odd_gemm | Bf16 | 0.122791 / 0.123792 | 0.122916 / 0.123959 |
| moderate_gemm | F16 | 0.123792 / 0.136333 | 0.122083 / 0.132833 |
| moderate_gemm | Bf16 | 0.123792 / 0.133042 | 0.127375 / 0.138875 |
| batch_transpose_broadcast | F16 | 0.124333 / 0.126667 | 0.125166 / 0.127000 |
| batch_transpose_broadcast | Bf16 | 0.122334 / 0.125667 | 0.122791 / 0.127792 |

Evidence: [first routed raw run](low-matmul-routed-metal.txt),
[repeat raw run](low-matmul-routed-metal-repeat.txt),
[recomputed medians and p90](low-matmul-routed-summary.json),
[audit of all 1,488 samples](low-matmul-routed-audit.json),
[source, binary and log hashes](low-matmul-routed-source-fingerprints.json).
The [qualified routed overlay](../qualification/low-matmul-routed-source-manifest.json)
preserves all changes from the SIMD-group snapshot. All earlier raw results
remain available above, including regressions.
