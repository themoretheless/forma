# WGSL tensor reductions and vector multiplication

Correctness qualification on Metal, 2026-09-27. This tranche adds operations;
it makes no new throughput claim.

## Recorded API

`ComputeProgram::tensor_reduce(op, input, axes, keep_dims)` and its `_into`
variant accept `GpuTensor<f32>` and `GpuTensor<u32>`. Operations come from the
shared `tensor_core::ReduceOp`: sum, product, min and max. `tensor_mean` and
`tensor_mean_into` accept f32. The native `ComputeRuntime` implements the shared
`TensorReduceBackend`. Existing `tensor_sum` delegates to the same planner.

- Arbitrary input strides, broadcast dimensions and offsets are supported.
- Outputs are contiguous and must use storage distinct from inputs. Nonzero
  output offsets are supported; surrounding storage remains unchanged.
- Empty axes materialize the input unchanged. Empty contractions produce zero
  for sum and one for product. Min, max and mean reject empty contractions with
  nonempty output; empty output is valid for every operation.
- u32 sums and products wrap modulo 2^32. Integer values never pass through f32.
- f32 inputs and intermediates must be finite. Reduction order is parallel;
  compare arithmetic results with a tolerance. Mean divides by the full logical
  contraction count once, after the last sum stage.
- Prepared programs retain allocations and bindings; updating input contents
  reuses the same plan. No intermediate submission or CPU readback is needed.

The shared planner uses up to 256 parts per output and at most 4096 scalar
partials (16 KiB). It prepares both dispatch stages before appending either.
Small contractions and large independent output sets use one stage. Complete
contiguous f32 sums retain the measured array reduction schedule. Other f32
sums retain `TENSOR_SUM_WGSL` and its eight-word header inside the common planner;
the generic shader `TENSOR_REDUCE_WGSL` handles other operations, u32 and mean.
Both shaders use the same partial scheduling. This selection preserves the
earlier specialized sum after the controlled regression study below.

## Vector multiplication

`tensor_matmul` and `tensor_matmul_into` use shared `MatmulPlan` validation.
A left vector is viewed as `[1, K]`, a right vector as `[K, 1]`; the promoted
singleton dimensions are removed from the logical output. Thus two vectors
produce a scalar. Matrix and batch dimensions retain normal broadcasting.

Promotion changes only layout metadata, including for strided or zero-stride
vectors. The existing tiled batched shader writes the promoted matrix result
directly into the final output storage. Zero-length vector products return
zero, zero-sized batches return empty output, and scalar operands are rejected.

## Validation

The strict real-GPU command passed all 23 tests:

```sh
COMPUTE_REQUIRE_GPU=1 cargo test --manifest-path crates/Cargo.toml \
  -p compute-core --test tensor_reduce --test tensor --test tensor_index \
  --test tensor_backend -- --test-threads=1
```

The six new tests include common backend conformance, all operations and axis
subsets, f32 and u32 tails through 131075 values per row, empty identities,
wrapping arithmetic, noncontiguous inputs, output sentinels, updated inputs and
validation failures. A prepared vector/batched-matrix multiplication feeds a
matrix/vector product and mean; only final outputs are read in one submission.
The common fixture adds 131077-element reductions and extreme finite values.
After restoring the specialized f32 sum shader, all six new tests passed again;
the full output is in `tensor-reductions-retained-metal-tests.txt`.

CPU checks passed for all 32 shipped shader templates and the separately
generated u32 reduction specialization. Strict Clippy covers the library and
new integration test. The executed test excerpt is retained in
`tensor-reductions-metal-tests.txt`; test durations include host and validation
overhead and are not performance measurements. Other GPU backends and browser
execution remain unqualified by this local run.

## Controlled sum regression study

`bench_tensor_reduction_generic` compares three paths over identical resident
inputs and final readback: the frozen original single-group shader, the frozen
prior hierarchical sum, and the current public API. Each case has 200 ms held
warmup and 31 rotated samples. GPU timestamps cover the shared compute pass;
host samples omit timestamps and include the final readback. Every execution
checks an independent f64 oracle. The frozen hierarchical source matches the
previous study's SHA256
`46c8fdb72daf32866a2666c559bec84684ece88ad35d87a23c20b74a3187e62b`.

The initial generic shader added a uniform operation selection and division.
Its GPU median exceeded the old hierarchy by 4.4–7.9% in these cases. This
candidate was rejected for f32 sum; it remains useful for the new operations.
The comparison measures the whole candidate shader and does not isolate a
specific instruction as the cause.

| Case | Prior GPU, µs | Generic shader GPU, µs | Change |
|---|---:|---:|---:|
| 16 rows × 17 values | 5.834 | 6.250 | +7.1% |
| Transposed full sum, 1,049,600 values | 38.583 | 40.292 | +4.4% |
| Two axes, 8 outputs | 38.291 | 40.292 | +5.2% |
| 65 outputs | 40.666 | 42.667 | +4.9% |
| Broadcast, 2 × 2,000,003 values | 71.500 | 77.125 | +7.9% |

The retained API uses the original sum shader and shared validation/planning.
A separate paired run produced the following medians; compare columns within
this run because device and host timing vary across runs.

| Case | Single group GPU, µs | Prior hierarchy GPU, µs | Retained API GPU, µs | Prior host, µs | Retained host, µs |
|---|---:|---:|---:|---:|---:|
| Small rows | 5.791 | 5.833 | 5.917 | 103.000 | 102.333 |
| Transposed full sum | 2,610.166 | 38.458 | 38.750 | 187.917 | 188.583 |
| Two axes | 320.000 | 41.333 | 41.333 | 173.958 | 173.625 |
| 65 outputs | 46.291 | 43.584 | 43.333 | 143.041 | 143.583 |
| Broadcast | 2,512.166 | 74.958 | 75.042 | 216.917 | 231.291 |

Retained GPU medians differ from the old hierarchy by −0.6% to +1.4%. The
broadcast case's host median is 6.6% higher despite similar GPU timing; this
run does not establish a cause. Other host medians differ by less than 0.7%.
This supports preserving the existing specialized shader, without claiming a
new speedup or uniform host-latency equivalence.

Raw samples are retained in `tensor-reduction-generic-metal.txt` (rejected
generic sum) and `tensor-reduction-specialized-metal.txt` (retained policy).
`tensor-reduction-generic-fingerprints.json` records shader, benchmark, retained
implementation and raw-output hashes. Earlier reports and fingerprints were
left unchanged.

```sh
cargo run --manifest-path crates/Cargo.toml -p compute-core --release \
  --example bench_tensor_reduction_generic
```
