# Attention from packed low storage: Metal measurements

## Workload and timing

The benchmark runs five geometries in both f16 and BF16 on Apple M4 Max
(40 GPU cores), macOS 26.7, Metal. Both paths start from identical resident
low-storage Q/K/V and return f32:

- `cast_f32`: convert all three operands, then execute shared f32 attention.
- `direct_low`: decode operands inside the streaming attention shader.

Both use the same masks, key partition policy, final output shape and readback.
The independent reference uses the original exact quarter-valued host inputs
and evaluates dense attention in f64. Output shapes are checked before
measurement; every measured execution and warmup validates output lengths
and values after timing, with `3e-4 * max(abs(reference), 1)` tolerance.
Extreme/subnormal numerical behavior is covered by the separate conformance
fixture; these timings use ordinary finite operands.

Programs, pipelines, inputs, actual outputs and scratch buffers are created
outside timing and reused. A 200 ms warmup precedes 31 samples per path and
mode, with rotating path and timing order. GPU timestamps cover the shared
compute pass, including casts for the baseline. Separate unprofiled host
measurements include encoding, submission, staging/ticket creation, all output
readbacks and waiting. Host and GPU samples come from separate executions.
Compilation, uploads and setup are excluded; no cold-start or low-output
performance claim follows. All raw samples, medians and p90 values are retained.

| Shape | Hq/Hkv | Queries | Keys | Q/K depth | V depth | Removed f32 Q/K/V bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| small | 1/1 | 17 | 65 | 17 | 33 | 14,156 |
| decode | 4/2 | 1 | 4097 | 64 | 64 | 4,196,352 |
| prefill | 2/2 | 128 | 257 | 64 | 64 | 328,704 |
| wide_value_gqa | 4/2 | 65 | 257 | 32 | 129 | 364,296 |
| strided_masked | 6/2 | 7 | 65 | 17 | 33 | 28,856 |

The final shape transposes physical Q/K/V and its keep mask, includes a fully
closed row and excluded keys, and uses a signed causal offset. Other shapes
have no mask. The byte count is exactly `4 * (Q.numel + K.numel + V.numel)`.
It describes removed operand buffers, not measured peak RSS or total device
allocation. Decode uses 17 key parts in both paths: 17,408 bytes of value
partials plus 544 bytes of row state. Other shapes use one part and an eight-
byte dummy state. Metadata, four-byte unused-mask sentinel, output and readback
buffers are separate. No full score/probability matrix is materialized.

## Retained result

The final source uses dtype specialization, guarded F16 conversion and a
256-word query cache only for F16. Across two unchanged runs, direct low
attention has lower GPU medians in **3/10 cases** and higher medians in **7/10**.
All three wins are BF16: small (about 1%, too small for a broad speed claim),
decode and strided/masked. The measured GPU ratio `cast_f32 / direct_low` ranges
from **0.690x to 1.112x**; the largest slowdown is **45.0%**. The main guaranteed
allocation benefit is removing the complete f32 Q/K/V buffers described above.

| Case | Run 1 cast/direct GPU ms | Ratio | Run 2 cast/direct GPU ms | Ratio |
| --- | ---: | ---: | ---: | ---: |
| small_F16 | 0.061792 / 0.067708 | 0.913x | 0.061541 / 0.068208 | 0.902x |
| small_Bf16 | 0.060750 / 0.060166 | 1.010x | 0.061417 / 0.060750 | 1.011x |
| decode_F16 | 0.275625 / 0.313625 | 0.879x | 0.283250 / 0.315166 | 0.899x |
| decode_Bf16 | 0.280209 / 0.251917 | 1.112x | 0.274208 / 0.249042 | 1.101x |
| prefill_F16 | 0.235541 / 0.339250 | 0.694x | 0.235625 / 0.341667 | 0.690x |
| prefill_Bf16 | 0.235250 / 0.274250 | 0.858x | 0.235417 / 0.275333 | 0.855x |
| wide_value_gqa_F16 | 0.350125 / 0.472875 | 0.740x | 0.351083 / 0.472125 | 0.744x |
| wide_value_gqa_Bf16 | 0.349375 / 0.388166 | 0.900x | 0.350750 / 0.390458 | 0.898x |
| strided_masked_F16 | 0.050709 / 0.057750 | 0.878x | 0.050208 / 0.057167 | 0.878x |
| strided_masked_Bf16 | 0.050875 / 0.047542 | 1.070x | 0.049917 / 0.047959 | 1.041x |

Host medians include submission and readback and must not be interpreted as
kernel time. Their paired values are retained separately:

| Case | Run 1 cast/direct host ms | Run 2 cast/direct host ms |
| --- | ---: | ---: |
| small_F16 | 0.212833 / 0.216750 | 0.183709 / 0.185625 |
| small_Bf16 | 0.171417 / 0.173042 | 0.175125 / 0.177125 |
| decode_F16 | 0.402500 / 0.430375 | 0.414750 / 0.432042 |
| decode_Bf16 | 0.424584 / 0.390625 | 0.429584 / 0.396459 |
| prefill_F16 | 0.408708 / 0.507708 | 0.407958 / 0.512083 |
| prefill_Bf16 | 0.409792 / 0.447792 | 0.407625 / 0.446208 |
| wide_value_gqa_F16 | 0.528875 / 0.744416 | 0.509417 / 0.678791 |
| wide_value_gqa_Bf16 | 0.535167 / 0.572000 | 0.509500 / 0.551416 |
| strided_masked_F16 | 0.164792 / 0.172208 | 0.157083 / 0.162500 |
| strided_masked_Bf16 | 0.164458 / 0.163125 | 0.154250 / 0.151208 |

[Raw first run](tensor-low-attention-metal.txt),
[raw repeat](tensor-low-attention-metal-repeat.txt),
[all samples and recomputed percentiles](../../../docs/qualification/tensor-low-attention-2026-09-27/benchmark-summary.json),
[final source hashes](tensor-low-attention-source-fingerprints.json).

## Optimization experiments

The same benchmark and f32 baseline were used throughout. The retained F16
cache adds 1 KiB of workgroup storage; it is filled only for depth <=256.
The pipeline reserves that storage even when larger depths use direct loads.
BF16 has no cache storage or added barrier. Global partial scheduling is
unchanged. The five measured shapes all have depth <=256; larger-depth
correctness tests do not establish their performance.

| Source stage | Faster in both runs | Slower in both | Worst direct GPU slowdown |
| --- | ---: | ---: | ---: |
| Initial generic decoder | 0 | 10 | 86.8% |
| Per-dtype decoder | 2 | 8 | 68.6% |
| Query cache for both dtypes | 2 | 8 | 41.2% |
| Guarded F16, no query cache | 2 | 8 | 66.3% |
| Retained: guarded F16 + F16-only query cache | 3 | 7 | 45.0% |

Caching both dtypes helped F16 but worsened BF16 relative to its cache-free
specialization, so the retained cache is restricted to F16. The guarded
cache-free conversion alone did not close the F16 gap. The final F16 decode,
prefill and wide-V direct medians are lower than the guarded cache-free
measurements, while the final direct path still trails explicit casts in all
five F16 cases. These are measurements of these kernels and workloads, not a
universal dtype or algorithm ranking.

Some historical runs contain large simultaneous timing changes in both paths,
including the first all-dtype-cache run and parts of the guarded repeat.
Their complete samples and p90 values remain visible. No GPU-clock, thermal
or compiler cause was measured, and no samples were removed. Small differences
between stages, especially the unchanged BF16 source, have no causal attribution.

| Stage | Source snapshot | Raw run / repeat | Recomputed summary |
| --- | --- | --- | --- |
| initial | [manifest](tensor-low-attention-initial-source-fingerprints.json) | [run](tensor-low-attention-initial-metal.txt) / [repeat](tensor-low-attention-initial-metal-repeat.txt) | [summary](../../../docs/qualification/tensor-low-attention-2026-09-27/initial-benchmark-summary.json) |
| specialized | [manifest](tensor-low-attention-specialized-source-fingerprints.json) | [run](tensor-low-attention-specialized-metal.txt) / [repeat](tensor-low-attention-specialized-metal-repeat.txt) | [summary](../../../docs/qualification/tensor-low-attention-2026-09-27/specialized-benchmark-summary.json) |
| query-cache | [manifest](tensor-low-attention-query-cache-source-fingerprints.json) | [run](tensor-low-attention-query-cache-metal.txt) / [repeat](tensor-low-attention-query-cache-metal-repeat.txt) | [summary](../../../docs/qualification/tensor-low-attention-2026-09-27/query-cache-benchmark-summary.json) |
| guarded-f16 | [manifest](tensor-low-attention-guarded-f16-source-fingerprints.json) | [run](tensor-low-attention-guarded-f16-metal.txt) / [repeat](tensor-low-attention-guarded-f16-metal-repeat.txt) | [summary](../../../docs/qualification/tensor-low-attention-2026-09-27/guarded-f16-benchmark-summary.json) |
| final F16 cache | [overlay](tensor-low-attention-f16-cache-source-fingerprints.json) / [full hashes](tensor-low-attention-source-fingerprints.json) | [run](tensor-low-attention-metal.txt) / [repeat](tensor-low-attention-metal-repeat.txt) | [summary](../../../docs/qualification/tensor-low-attention-2026-09-27/benchmark-summary.json) |

## Reproduce

```sh
cargo build --release --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-core --example bench_tensor_low_attention
crates/target/release/examples/bench_tensor_low_attention
python3 docs/qualification/tensor-low-attention-2026-09-27/audit_benchmarks.py
```

Run the executable twice and save its complete stdout/stderr as the two raw
logs linked above before running the audit. Native GPU access and timestamp
queries are required; missing capabilities fail the benchmark. The audit
recomputes 80 median/p90 pairs from 2,480 samples. It also accepts historical
stage names to reproduce their separate summaries.

[Benchmark source](../examples/bench_tensor_low_attention.rs),
[shared timing harness](../examples/support/low_bench.rs),
[contracts and planner allocation details](tensor-low-attention-contracts.md),
[full qualification](../../../docs/qualification/tensor-low-attention-2026-09-27.md).
