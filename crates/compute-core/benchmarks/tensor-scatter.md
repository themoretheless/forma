# WGSL typed tensor scatter

Metal correctness qualification, 2026-09-27. No scatter throughput claim is made.

## API and contracts

`ComputeProgram::tensor_scatter(op, input, indices, updates, axis)` accepts
`GpuTensor<f32>` or `GpuTensor<u32>` and returns `Scattered { values,
invalid_count }`. `tensor_scatter_into` records into caller-owned output and
scalar count storage. `ComputeRuntime` also implements the shared native
`TensorScatterBackend` trait.

The output retains the input shape. Updates broadcast to the gather-expanded
shape `input[..axis] + indices.shape + input[axis+1..]` without enlarging it.
Input, indices and updates may use arbitrary validated nonnegative strides,
including zero strides and nonzero offsets. Output storage must be contiguous
and distinct from every input and the count; output and count offsets work.

- `Replace` chooses the greatest logical row-major index position among
  duplicates, independently of physical index strides.
- `Add`, `Multiply`, `Min` and `Max` include the original base value and every
  valid update. Integer addition/multiplication wrap modulo 2^32.
- f32 inputs and intermediates must remain finite. Arithmetic accumulation
  order is parallel and callers should compare non-exact results with tolerance.
- Invalid indices leave the base unchanged. The GPU scalar counter counts each
  logical index once, including when output slices or the target axis are empty.
- Empty indices copy the base and set the count to zero. Repeated execution
  always resets the base copy, owner scratch and invalid count.

## Recorded implementation

1. Reset and compute the invalid count using the same helper as gather.
2. Copy the logical base into contiguous output storage.
3. For Replace, clear one owner word per target-axis element, then select the
   last valid index using `atomicMax(index_position + 1)`.
4. Visit each logical update. Replace writes only the elected owner. Integer
   add/min/max use native atomics; integer multiplication and f32 folds use
   compare-and-swap loops over u32 storage words. Integer bits never pass through
   floating-point arithmetic.

This avoids scanning all indices for each output value. Replace visits the base,
target-axis owners, indices and expanded updates a bounded number of times.
Other operations visit the base, indices and expanded updates, with additional
atomic retries under contention. Dense owner storage is allocated only for
Replace with nonempty output and updates. The apply kernel uses five storage
bindings; scatter pipelines are compiled in a separate lazy runtime cache.

Every stage is prepared in a temporary program and appended only after all
validation, allocations and bindings succeed. There is no intermediate submit,
readback, host index inspection or host fallback. Allocation sizes and logical
update counts are checked against device storage limits and u32 indexing.

## Verification

```sh
COMPUTE_REQUIRE_GPU=1 cargo test --manifest-path crates/Cargo.toml \
  -p compute-core --test tensor_scatter --test tensor_index -- --test-threads=1
```

All four new scatter tests passed on Metal (0.83 s), as did all five existing
indexing tests (0.69 s) after extracting the shared invalid-count helper. Full
command output is retained in `tensor-scatter-metal-tests.txt`.

Coverage includes the shared backend fixture; all operations and axes; scalar,
multidimensional and strided indices; broadcast updates; empty outputs and
empty axes; 65,539 duplicate indices; 4,097-way atomic contention; exact u32
wrapping; changing prepared-plan inputs; owner/count resets; output sentinels;
scatter→sum and scatter→gather→scan composition; foreign and cross-type aliases;
and an expanded update shape of 2^32 elements rejected without recording any
partial writes into the parent program.

All 35 shipped shader templates pass Naga. The generated u32 scatter variant is
validated separately, including its native atomic paths. Strict Clippy passes
for the library and new tests. These checks establish local Metal correctness;
other devices and browsers need their own runtime qualification.
