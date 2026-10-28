# Resident WGSL tensors and hierarchical axis sums

`GpuTensor` combines f32 GPU storage with `tensor-core::Shape` and `Layout`.
The common layout contract supplies rank, dimensions, nonnegative element
strides, offsets, broadcasting and bounds validation. Shader metadata uses
runtime-sized storage descriptors: the implementation has no fixed rank-eight
limit. Values, dimensions, strides and offsets must fit WGSL u32 addressing and
the actual device's storage limits.

## Recorded operations

The following `ComputeProgram` operations retain all intermediate data on the
GPU and can be submitted repeatedly after input writes:

- `tensor_materialize[_into]`: logical strided values to contiguous storage.
- `tensor_unary[_into]`: every existing unary f32 operation.
- `tensor_binary[_into]`: every binary f32 operation with trailing-axis broadcast.
- `tensor_sum[_into]`: any validated axis set, with optional retained dimensions.
- `tensor_matmul[_into]`: rank-two-or-higher products with broadcast batch axes,
  transposed/strided input views, odd tile tails and empty contractions.

`reshape`, `permute`, `broadcast_to` and `narrow` on a `GpuTensor` are metadata
views. A view reshape requires contiguous logical storage. The shared backend
adapter can materialize before reshaping when a copy is needed. Inputs can have
zero strides and overlapping read addresses. Outputs must be contiguous and
must use different backing storage from every input; nonzero output offsets
are supported. Validation precedes recording. Empty reduced dimensions and
zero matmul contraction dimensions write zero on every execution.

The batched matrix kernel uses shared 16×16 f32 tiles. This is portable WGSL
arithmetic; reduced precision and Tensor Core execution are not claimed.

`examples/tensor_program.rs` prepares batched matmul → bias broadcast → square
→ axis sum once, updates the input three times, and checks only the final
per-row energies against an f64 reference. No intermediate CPU transfer occurs.

## Hierarchical reductions

Small reductions use one cooperative workgroup per output. Larger reductions
can create independent partials for each output row:

```
parts = min(ceil(reduction_values / 4096), 256,
            max(1, floor(4096 / output_values)))
```

When `parts > 1`, at most 4,096 f32 partials (16 KiB) are allocated. The first
stage reduces strided input into these partials; the second reduces each row
into the caller's output. Both stages are recorded in the same compute pass.
All allocations and bindings are prepared before appending either dispatch.
Large output sets already provide parallelism and retain one part per output.
Both dispatches respect workgroup-count limits and use group-stride loops.

A complete contiguous tensor sum with zero input/output offsets reuses the
existing array reduction, including its measured Metal schedule. Prefixes keep
backing-storage tails outside the logical tensor untouched.

## Measured qualification

`bench_tensor_reduction` compares the actual prepared API with a frozen copy
of the initial one-workgroup-per-output tensor shader. This isolates the new
hierarchical algorithm; it is not a comparison against CUDA, MLX, CPU libraries
or earlier public tensor APIs. Both paths use identical resident input and
final output size. Allocation, compilation and upload precede measurement.

Each case receives at least 200 ms sustained warmup, then 31 rotated samples.
Host latency includes final readback, with no timestamp work in that execution.
GPU timing uses the corrected timer around the complete compute pass in a
separate execution. Every result is compared to f64; every GPU interval is
checked against the corresponding full profiled wall time.

Available Metal adapter, median milliseconds:

| Case | Baseline host | Hierarchical host | Baseline GPU | Hierarchical GPU |
|---|---:|---:|---:|---:|
| 16 outputs × 17 values | 0.104000 | 0.105916 | 0.005750 | 0.005667 |
| Transposed 1,049,600 values → scalar | 2.788000 | 0.171917 | 2.608958 | 0.038250 |
| Two axes, 8 outputs × 132,225 values | 0.462625 | 0.177208 | 0.317417 | 0.038000 |
| 65 outputs × 16,705 values | 0.152792 | 0.147833 | 0.046250 | 0.040083 |
| Broadcast, 2 outputs × 2,000,003 values | 2.679125 | 0.216208 | 2.511750 | 0.071250 |

For the transposed scalar case, host p90 changes from 2.949000 to 0.192458 ms;
GPU p90 changes from 2.698000 to 0.038750 ms. Small reductions retain a single
stage and show no meaningful host improvement. These measurements qualify this
Metal device only. All arithmetic remains f32; changing reduction order can
change rounding, particularly under cancellation.

## Evidence and reproduction

- `tensor-reduction-metal.txt`: full samples, medians, p90 and case metadata.
- `tensor-source-fingerprints.json`: source hashes for the retained experiment.
- `tests/tensor.rs`: ten GPU tests, passed in 1.84 seconds, covering twelve
  dimensions, all unary/binary operations, arbitrary axes, bounds/ownership/
  alias errors, dispatch-limit loops, zero dimensions, hierarchical partial
  caps, f64 parity and repeated programs with input/output sentinel tails.
- `tests/tensor_backend.rs`: shared backend conformance and precision rejection,
  two GPU tests passed in 0.15 seconds.
- All 22 shipped shaders pass Naga validation; the full tensor program example
  passed its three real-GPU iterations.

```sh
COMPUTE_REQUIRE_GPU=1 cargo test --manifest-path crates/Cargo.toml \
  -p compute-core --test tensor --test tensor_backend -- --test-threads=1
cargo run --manifest-path crates/Cargo.toml -p compute-core --release \
  --example tensor_program
cargo run --manifest-path crates/Cargo.toml -p compute-core --release \
  --example bench_tensor_reduction
```

Use an available GPU and serialize measurements with other GPU workloads.
