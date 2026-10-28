# Typed MLX compiled replay

Two complete native runs were measured after typed program qualification and
source freeze on Apple M4 Max, MLX 0.32.1 / MLX-C 0.6.0_4. Both runs use the same
release binary and 106 frozen source files. All measured results passed their
exact references; every compiled program traced once.

Compiled host medians were lower in all six cases in both runs, with
eager/compiled ratios from 1.015 to 1.125. There are p90 regressions and substantial
absolute timing changes between runs. These measurements do not establish a
general speedup or a GPU kernel improvement.

## Workloads

The [benchmark](../examples/bench_compiled_typed.rs) compares fresh eager results
with fresh compiled results from the same already resident input handles. It
covers six cases:

| Cases | Computation | Two result buffers |
| --- | --- | --- |
| F16 and BF16 arithmetic | `[513,257]` x, `[257]` weight and scalar bias; low multiply, low add, low square | Raw low y, f32 sum(y) |
| F16 and BF16 matrix multiplication | `[2,17,65] @ [1,65,33]`, with batch broadcast and partial tiles | Direct f32 matrix result, f32 sum(result) |
| Small u32 | `[3,257]`; compare to a scalar, select input or scalar replacement, sum | Exact u32 selected values, wrapping u32 sum |
| Large u32 | Same operations on `[513,257]` | Exact u32 selected values, wrapping u32 sum |

Each case has two resident input versions with different values. Version zero
uses contiguous inputs; version one uses transposed physical storage with the
same logical shape. Both matrix operands change strides. Broadcast weights,
bias, thresholds and the singleton matrix batch retain their logical semantics.

The low arithmetic and direct low matmul paths exercise the existing custom
Metal operations. They do not merely pass low-dtype handles through a compiled
identity graph. The u32 selection fixture includes `0x01000001` against
`0x01000000`, values above `i32::MAX`, and values near `u32::MAX`.

## Reference and accuracy scope

Arithmetic inputs are quarters in `[-0.5,0.5]`, weights are `0.5` or `1`, and
bias is `0.25`. Every multiply/add/square result is exactly representable in
both F16 and BF16. Squared outputs have denominator 64 and numerator at most 36.
For 131,841 elements the sum numerator is at most 4,746,276, below 2^24. Thus
every f32 reduction ordering remains exact. The reference computes logical
addresses explicitly and evaluates values in f64. Low results are compared as
raw u16 bits; f32 outputs are compared exactly to the f64 values.

Matrix inputs are quarters in `[-1,1]`. Products have denominator 16 and
absolute numerator at most 16. The absolute numerator bound for all products
across the matrix result and its sum is 1,166,880, below 2^24. This also makes
every intermediate f32 partial sum exactly representable. The f64 oracle uses
explicit batch/row/column loops, including the right-hand batch broadcast.

The u32 reference uses ordinary integer comparisons and `wrapping_add`, with
explicit transpose addresses. Every selected element and scalar sum must match
exactly. These bounded workloads isolate execution, layout and dtype behavior;
they do not characterize general floating-point error or transcendental math.

## Timing and replay protocol

- The timer starts before eager construction or typed compiled invocation.
  Handle construction, graph allocation, evaluation, two complete result reads
  and explicit synchronization are included equally.
- Input upload, input/view evaluation, reference construction, initial tracing
  and kernel compilation are outside timing. Result validation, trace checks and
  destruction of final output handles are also outside each timer.
- Each sample constructs new output arrays. Re-evaluating a cached result is
  never a timing sample.
- Both resident versions execute through both paths before warmup. Every
  compiled execution must leave `trace_count()` equal to one; an unavailable or
  disabled compiler is an error, not an eager timing labeled as compiled.
- Warm both paths for at least 200 ms per case. Collect 31 samples per path,
  alternating eager/compiled order. Versions cycle `0,0,1,1,...` independently of
  path order. Both paths in each pair receive identical current handles.
- Validate every warmup and measured output against its reference after the
  timer. Report every raw sample, median and nearest-rank p90.

Each complete run contains 12 sample series, 372 measured invocations and
744 validated result buffers. The two runs contain 24 median/p90 pairs, 744
measured invocations and 1,488 validated result buffers. Warmup and preflight
executions are additional and are not included in those totals.

Only host wall time is measured. A ratio cannot identify GPU kernel duration or
attribute a change to a particular fusion. The benchmark must retain regressions
and run-to-run variation alongside improvements.

## Measured host wall times

Times below are milliseconds; ratios are eager median / compiled median.

### Run 1

| Case | Eager median | Compiled median | Ratio | Eager p90 | Compiled p90 |
| --- | ---: | ---: | ---: | ---: | ---: |
| F16 arithmetic | 0.438834 | 0.416500 | 1.054 | 0.457125 | 0.430000 |
| BF16 arithmetic | 0.867542 | 0.829042 | 1.046 | 0.905292 | 0.905792 |
| F16 matrix multiplication | 0.218916 | 0.211250 | 1.036 | 0.221250 | 0.214875 |
| BF16 matrix multiplication | 0.221000 | 0.212708 | 1.039 | 0.223459 | 0.216042 |
| Small u32 selection/sum | 0.250250 | 0.224625 | 1.114 | 0.261750 | 0.287667 |
| Large u32 selection/sum | 0.265625 | 0.236083 | 1.125 | 0.273458 | 0.253708 |

### Run 2

| Case | Eager median | Compiled median | Ratio | Eager p90 | Compiled p90 |
| --- | ---: | ---: | ---: | ---: | ---: |
| F16 arithmetic | 0.375833 | 0.351833 | 1.068 | 0.379708 | 0.355667 |
| BF16 arithmetic | 0.375208 | 0.351792 | 1.067 | 0.381875 | 0.354792 |
| F16 matrix multiplication | 0.240750 | 0.237291 | 1.015 | 0.250750 | 0.245208 |
| BF16 matrix multiplication | 0.242208 | 0.236583 | 1.024 | 0.257000 | 0.245417 |
| Small u32 selection/sum | 0.210541 | 0.206208 | 1.021 | 0.233333 | 0.214250 |
| Large u32 selection/sum | 0.220834 | 0.205791 | 1.073 | 0.224417 | 0.211625 |

Run 1 has two compiled p90 regressions: BF16 arithmetic is 0.000500 ms higher,
and small u32 selection/sum is 0.025917 ms higher. Every compiled p90 is lower in
run 2. BF16 arithmetic changes markedly between runs for both paths; the logs
do not establish why. The report retains the paired comparisons and both
absolute runs without attributing those changes to a particular component.

## Reproduction and evidence

```sh
cargo build --release --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-mlx --example bench_compiled_typed
cargo run --release --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-mlx --example bench_compiled_typed
```

Native GPU support and the public MLX compilation ABI are required. The benchmark
respects `MLX_DISABLE_COMPILE`; it fails its trace assertion if native trace reuse
is disabled. No benchmark result is replaced by a cached output or CPU fallback.

- Raw samples: [run 1](compiled-typed-metal.txt),
  [run 2](compiled-typed-metal-repeat.txt).
- [Summary](compiled-typed-summary.json) and [independent CPU audit](compiled-typed-audit.json)
  contain every recomputed median/p90 and the exact coverage counts.
- A [separate reviewer audit](../../../docs/qualification/tensor-mlx-typed-programs-2026-09-27/independent-benchmark-audit.json)
  independently confirms all 24 median/p90 pairs, 12 ratios and trace counts.
- [Source and binary fingerprints](compiled-typed-source-fingerprints.json) record
  the frozen binary, both logs, the derived results and before/after verification.
- [Qualification source archive](../qualification/compiled-typed-source-manifest.json)
  contains 106 files, including the harness, production Rust/Metal, shared tensor
  source, dependency manifests and lockfile.

The release binary SHA256 is
`a482bd084905b50c42738963ed34240b1baf439c1570dfeb5cda64d2a2e4ad3d`.
All 106 source hashes, the source archive manifest and that binary remain
unchanged after both runs. Twelve compiled programs each moved from zero traces
to one. There were no timing-run failures and all 1,488 measured output buffers
passed exact validation.

Existing [f32 compiled results](compiled.md) remain a separate experiment. Their
measurements must not be presented as results for the typed graphs in this file.
