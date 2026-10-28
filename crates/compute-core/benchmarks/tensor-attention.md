# Resident attention on Metal

Date: 2026-09-27. Device: Apple M4 Max, 40 GPU cores. This compares the recorded
streaming WGSL attention API with composition of the public matmul, multiply,
softmax and matmul operations. Both use the same resident f32 operands and
grouped query-head mapping.

## Result

Splitting a long key sequence across workgroups made the measured decode case
**2.68× faster on GPU** than composition: 0.236292 versus 0.633917 ms. Host time
including readback decreased from 1.138333 to 0.531791 ms.

The other three GPU cases remain slower than composition. The current streaming
path provides bounded intermediate storage on those shapes, with a computation
cost. This does not establish a general attention speedup.

| Case | Hq / Hkv | Lq × Lk | D / Dv | Composition GPU ms | Streaming GPU ms | GPU time change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Small | 1 / 1 | 17 × 65 | 17 / 33 | 0.038208 | 0.060042 | +57.1% |
| Decode | 4 / 2 | 1 × 4097 | 64 / 64 | 0.633917 | 0.236292 | −62.7% |
| Prefill | 2 / 2 | 128 × 257 | 64 / 64 | 0.112666 | 0.265500 | +135.7% |
| Wide V, GQA | 4 / 2 | 65 × 257 | 32 / 129 | 0.135709 | 0.355541 | +162.0% |

| Case | Composition host ms | Streaming host ms | Composition score arrays, bytes | Streaming partial values + summaries, bytes |
| --- | ---: | ---: | ---: | ---: |
| Small | 0.378417 | 0.323250 | 13,260 | 0 |
| Decode | 1.138333 | 0.531791 | 196,656 | 17,408 + 544 |
| Prefill | 0.648750 | 0.637125 | 789,504 | 0 |
| Wide V, GQA | 0.785792 | 0.831167 | 801,840 | 0 |

Memory columns describe selected algorithm allocations, derived from the actual
recording plan. Composition retains three f32 score-shaped arrays: raw QK,
scaled logits and probabilities. It also has softmax summaries/partials.
Streaming uses no score-shaped arrays; the decode case allocates 17 partial
outputs per query plus two-value summaries. Counts exclude common inputs and
outputs, metadata, tiny dummy bindings, readback, workgroup/register storage,
possible compiler spills and driver allocation. These are not peak GPU-memory
or process-RSS measurements.

### Initial result and correction

The initial unsplit decode kernel took 3.823708 ms, versus composition's
0.626667 ms. Four query rows produced only four workgroups, each serially
streaming all 4097 keys. The new plan divides them into 17 key ranges and
merges the resulting normalized values using per-part maxima/denominators.
This supplies 68 streaming workgroups followed by a small merge dispatch.

The same benchmark was rerun with both paths after the change; final comparisons
use the matched final run. Initial and final composition GPU medians were
0.626667 and 0.633917 ms. Initial source snapshots and samples are retained.
The result supports the measured split-key improvement on this workload;
there are no occupancy counters or cross-device causal measurements.

## Execution and method

The online normalization recurrence is the tiled attention approach described
in [FlashAttention](https://arxiv.org/abs/2205.14135). This WGSL implementation
uses ordinary f32 shader operations; it does not claim the paper's optimized
matrix kernels or performance. Each 64-channel V tile recomputes QK. That and
the scalar dot-product loop remain optimization opportunities for prefill and
wide V.

- For at most 64 query rows and at least 512 keys, the planner selects up to
  64 parts, targeting roughly 256 keys each. Partial values are capped at
  4 MiB, with at most 32 KiB of summaries. If two parts do not fit, the single
  streaming path is retained. Other shapes use that single path.
- Split merging weights each partial output by
  `denominator_part * exp(max_part - max_global)`, then normalizes their sum.
  Fully masked parts have zero weight. All intermediate data stays on GPU.
- GQA composition reshapes Q into key-head/group axes and uses broadcast K/V
  views. It does not expand whole operands on the CPU or upload repeated K/V.
- Programs and input/output buffers are reused. Upload, pipeline compilation,
  allocation and program recording are outside timing.
- Initial executions precede 200 ms of alternating warmup. There are 31 samples
  per path/mode with rotating path order. Separate timestamped and unprofiled
  runs alternate their order between iterations.
- GPU timestamps bracket the shared compute pass. Host time includes encoder
  creation, recording, submission and complete synchronous output readback.
  Cargo uses its debug profile; host numbers are not release application
  throughput or pure launch overhead.
- Every execution matches an independent dense f64 QK → softmax → PV reference
  within `3e-4 * max(abs(expected), 1)`. Validation occurs after timing. These
  timings use finite moderate input values, no explicit mask and no causal mask;
  separate conformance tests cover those cases and extreme values.
- One device and two runs were measured. No confidence interval, CUDA/MLX
  timing, native half arithmetic or Tensor Core use is established here.

Run:

```sh
cargo run --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-core --example bench_tensor_attention
```

[Final samples](tensor-attention-metal.txt),
[benchmark source](../examples/bench_tensor_attention.rs),
[final source fingerprints](tensor-attention-source-fingerprints.json),
[initial samples](tensor-attention-initial-metal.txt),
[initial fingerprints](tensor-attention-initial-source-fingerprints.json),
[frozen initial source](tensor-attention-initial-source/crates/compute-core/src/tensor/attention.rs),
[focused split tests](tensor-attention-split-metal-tests.txt),
[full qualification](../../../docs/qualification/tensor-attention-2026-09-27.md).
All 16 initial source files are retained under their repository-relative paths
in `tensor-attention-initial-source` and match the original SHA256 manifest.
