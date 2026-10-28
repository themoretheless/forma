# Resident tensor softmax on Metal

Date: 2026-09-27. This compares the new recorded `tensor_softmax` with a
composition of the existing public tensor operations: max, subtract, exp, sum,
divide. Both paths retain inputs and intermediates on the device.

## Result

The final implementation reduced median GPU time on all four measured shapes.
Groups with 2–256 elements use one workgroup and write the output directly.
Larger groups use bounded partial reductions and per-group summaries, avoiding
full-input shifted and exponential arrays.

| Logical shape, axis 1 | Composition GPU ms | New GPU ms | Change | Composition host ms | New host ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| 32 × 33 | 0.023667 | 0.009041 | −61.8% | 0.372041 | 0.261750 |
| 1 × 1,048,579 | 0.860958 | 0.725916 | −15.7% | 8.827875 | 8.653792 |
| 4,096 × 257 | 1.384625 | 0.919167 | −33.6% | 9.316583 | 8.847375 |
| 1,025 × 1,024, strides [1, 1025] | 0.990917 | 0.572250 | −42.2% | 8.904375 | 8.463541 |

These observations apply to this WGSL implementation on the local Metal
backend. They do not establish performance on other devices, CUDA or MLX, or
performance of the other statistics operations.

### Short-group regression found before specialization

The first implementation used separate summary stages for every group size.
On 32 × 33, its GPU median was 0.066042 ms versus 0.021792 ms for composition,
although its host median was lower. On the three larger shapes the GPU medians
were already 17–42% lower. This motivated the single-workgroup path.

The final table compares both paths again within the same run. Comparing only
old and new runs would mix the implementation change with device state and
timing noise. The first small-case GPU p90 values were particularly noisy:
0.090500 ms for composition and 0.081292 ms for the summary path. Final p90
values were 0.023958 and 0.009125 ms respectively. Raw samples are retained;
these two runs do not establish a general causal performance model.

## Method

- Same input values, strides and f32 output shape for both paths. The strided
  case retains the physical transpose throughout the calculation.
- Upload, allocation, pipeline creation and program recording are excluded.
  Inputs, output buffers and recorded programs are reused.
- Initial calls precede 200 ms of alternating warmup. There are 31 measured
  samples per path and timing mode, with rotating path order. Timestamped and
  unprofiled calls alternate their order between iterations.
- GPU time is the hardware timestamp interval around the shared compute pass.
  Host time includes encoder creation, submission and synchronous output
  readback from a separate unprofiled run. It is not pure dispatch overhead.
- Every execution is checked against an independent f64 max-shifted softmax.
  The bound is `7e-5 * max(expected, 1e-30)`, per element. Reference checking is
  outside the timed interval; both paths passed every check.
- Cargo uses its debug profile. Host medians are not optimized application
  throughput. This is one host and one final run, with no confidence interval.

Run from the repository root:

```sh
cargo run --offline --locked --manifest-path crates/Cargo.toml \
  -p compute-core --example bench_tensor_normalization
```

## Evidence

- [Final raw samples](tensor-normalization-metal.txt),
  [benchmark source](../examples/bench_tensor_normalization.rs),
  [final source fingerprints](tensor-normalization-source-fingerprints.json).
- [Initial raw samples](tensor-normalization-initial-metal.txt),
  [initial fingerprints](tensor-normalization-initial-source-fingerprints.json),
  [frozen initial implementation](tensor-normalization-initial-source/normalization.rs).
  All 12 initial source files are preserved by basename in that directory and
  verified against the original path-to-SHA256 manifest. The initial source
  archive is evidence, not a separately buildable crate.
- [Numerical contracts and focused tests](tensor-normalization-contracts.md),
  [full regression](../../../docs/qualification/tensor-statistics-2026-09-27.md).
