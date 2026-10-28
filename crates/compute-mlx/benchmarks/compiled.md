# MLX compiled replay from resident inputs

The [benchmark](../examples/bench_compiled.rs) compares ordinary eager graph
construction with a compiled MLX program for the same two outputs:

```text
y = square(x * w + b)
total = sum(y)
```

Both paths consume the same already evaluated f32 input handles and return
new output arrays on every invocation. Every output element and scalar sum
is checked against an independent f64 reference. Two isolated native runs
are retained below with their frozen source and sample audits.

## Timing boundary and replay

Reported times are host wall times. The timer starts before eager graph
construction or `compiled.run`, and ends after full readback of both output
buffers and explicit stream synchronization. Allocation, native graph
invocation, evaluation, contiguous result access and copies into Rust vectors
are included. Input upload/view construction/evaluation, first compilation,
oracle construction, result validation, trace-count checks and destruction
of the final output handles are excluded equally.

Each replay returns fresh arrays. The benchmark never uses repeated `eval`
on a completed result as a timing sample. The low-level MLX compilation
cache may reuse the traced program, but it must bind the current resident
input handles and evaluate new output values.

For each case, the compiled program starts with `trace_count() == 0`. Both
input versions are executed and checked before the warmup. Every compiled
execution must leave the trace count at one, including changing x values and
changing strides in the third case. Each case finishes by reporting the
counter. This checks native tracing behavior separately from result accuracy.

After first-use compilation and preflight, the benchmark warms both paths
for at least 200 ms and records 31 samples per path. Path order reverses on
each paired iteration. Input versions cycle `0,0,1,1,...`, so each layout
appears with both path orders. Both paths in a pair receive the identical
current input handles. The final odd sample gives one version one extra
sample equally in both paths. Medians, nearest-rank p90 and every raw timing
are printed. Each run contains 186 measured invocations and 372 fully
validated output buffers.

## Cases and reference

| Case | x shape | w shape | b shape | Input layouts |
| --- | --- | --- | --- | --- |
| Small | `[257]` | `[257]` | scalar | contiguous |
| Large | `[512,512]` | scalar | scalar | contiguous |
| Changing layout and broadcast | `[513,257]` | `[257]` | scalar | contiguous and transposed |

Every case has two resident x/w datasets with distinct expected y vectors.
The third case's second x comes from physical `[257,513]` storage transposed
into `[513,257]`; its first x is contiguous with the same logical shape.
The vector weights broadcast across rows. Only the input values and native
strides change between versions; the compiled input shapes remain fixed.

Inputs use `x ∈ {-2,-1,0,1,2}/4`, `w ∈ {0.5,1}`, and `b = 0.25`.
The product, addition and square are exactly representable in f32. Every y
is a nonnegative integer multiple of 1/64 with numerator at most 36. With
at most 262,144 output elements, every partial sum has numerator at most
9,437,184, below 2^24. Every reduction order is therefore exact in f32 for
this workload. The CPU oracle evaluates every element and total in f64 and
uses exact equality, not a broad numerical tolerance. This dataset checks
value binding, view addressing, broadcasting and both outputs; it does not
characterize numerical error for arbitrary f32 arithmetic.

## Reproduction and evidence

```sh
cargo build --release --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-mlx --example bench_compiled
cargo run --release --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-mlx --example bench_compiled
```

An available native MLX GPU and compile ABI are required. Missing support,
incorrect outputs or retracing fail the benchmark. It respects the external
MLX compile mode; it does not clear `MLX_DISABLE_COMPILE` or otherwise force
compilation globally. If compilation is disabled, the single-trace assertion
fails instead of reporting an eager execution as a compiled sample. First-use compilation and
both input/view versions are exercised before any reported samples. Two
complete matched runs, frozen source and binary hashes, and a raw-sample
audit are retained after native qualification. Wall-time ratios must not
be presented as GPU-only speedups or attributed to one optimizer operation.

## Qualified results

MLX 0.32.1 / MLX-C 0.6.0_4 on Apple M4 Max Metal, after the complete native
qualification passed. Both runs used the same frozen release binary and
89 archived source files. Each run validated 186 measured invocations and
372 output buffers; together they checked 744 buffers with zero numerical
failures. Every program started with trace count zero and finished at one:
six programs across the two processes, including the changing-layout case.

All three compiled medians were lower in both runs. Large-case host ratios
were 1.074–1.078×. Small-case median differences were only about 1–2%, and
its second-run compiled p90 was worse than eager. The changing-layout ratio
varied from 1.017× to 1.082×. These measurements support modest improvements
for this graph; they do not establish a universal speedup, lower tail latency
in every case, a GPU-only gain or the contribution of any particular fusion.

| Case | Run 1 median eager/compiled ms | Run 1 p90 eager/compiled ms | Run 2 median eager/compiled ms | Run 2 p90 eager/compiled ms |
| --- | ---: | ---: | ---: | ---: |
| small | 0.182667 / 0.179291 | 0.186375 / 0.182417 | 0.206292 / 0.203666 | 0.209750 / 0.218042 |
| large | 0.279541 / 0.259208 | 0.317625 / 0.292041 | 0.248917 / 0.231666 | 0.272959 / 0.250166 |
| changing_layout_broadcast | 0.213792 / 0.210250 | 0.217541 / 0.214958 | 0.227291 / 0.210125 | 0.258667 / 0.244208 |

The [first raw run](compiled-metal.txt) and [repeat](compiled-metal-repeat.txt)
retain every timing and reported trace count. The [summary](compiled-summary.json)
and [independent sample audit](compiled-audit.json) recompute all 12
median/p90 pairs and six host ratios from the 372 individual timings.
The [fingerprint manifest](compiled-source-fingerprints.json) records the
unchanged source/binary hashes and hashes both raw logs and derived results.
The [qualified full source archive](../qualification/compiled-source-manifest.json)
retains all 89 files, including this benchmark and the workspace manifest.
