# Packed low-precision matrix inputs

Date: 2026-09-27. Backend: local Metal. This measures the portable WGSL
implementation; no native f16 feature or matrix hardware instruction is used.

## Result

Packing approximately halves input allocation bytes, including the documented
odd-lane padding. It did not improve matrix execution time in this run. On the
two larger workloads, f16 GPU medians increased 11.7–13.8% and bf16 increased
3.6–7.6%. The result supports a storage benefit, with a measured computation
cost on these shapes. It is not evidence about CUDA or MLX performance.

| M × K × N | Input bytes f32 / packed | f32 GPU ms | packed f16 GPU ms | packed bf16 GPU ms |
| --- | ---: | ---: | ---: | ---: |
| 17 × 19 × 21 | 2,888 / 1,448 | 0.010167 | 0.010667 | 0.010666 |
| 128 × 129 × 127 | 131,580 / 65,792 | 0.102291 | 0.114250 | 0.110083 |
| 256 × 257 × 255, strided left | 525,308 / 262,656 | 0.411375 | 0.468333 | 0.426208 |

| M × K × N | f32 host ms | packed f16 host ms | packed bf16 host ms |
| --- | ---: | ---: | ---: |
| 17 × 19 × 21 | 0.234916 | 0.239583 | 0.241500 |
| 128 × 129 × 127 | 0.429792 | 0.440125 | 0.438459 |
| 256 × 257 × 255, strided left | 1.085000 | 1.143792 | 1.084209 |

## Method and limits

- Each path uses the same values, layouts and f32 output. Values are exactly
  representable in both low formats, and each result is checked exactly against
  an independent f64 CPU product after every dispatch.
- The strided case retains the physical left transpose in all three paths.
- Upload, input casts, shader compilation and program recording are excluded.
  Input buffers and recorded programs are reused; only tile loads decode packed
  values. No complete f32 input expansion is created.
- After initial calls and 200 ms of warmup, 31 samples per path rotate execution
  order. Separate timestamped and unprofiled runs alternate their order.
- GPU time uses hardware timestamps around the shared compute pass. Host time
  includes encoder creation, submission and final synchronous readback, so it
  is not pure launch overhead. CPU reference checking occurs after timing.
- The smallest case has noisy tails (GPU p90 around 0.04 ms). Single-host,
  single-run medians do not establish cross-device performance or causality.
- Output allocation remains f32 in this comparison. Low output needs a final
  cast and was qualified numerically but was not benchmarked here.

Run:

```sh
cargo run --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-core --example bench_tensor_low
```

[Raw samples](tensor-low-metal.txt), [source](../examples/bench_tensor_low.rs),
[source fingerprints](tensor-low-source-fingerprints.json),
[focused GPU tests](tensor-low-metal-tests.txt).
