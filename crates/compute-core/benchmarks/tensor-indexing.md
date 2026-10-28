# Typed tensor indexing on WGSL

`GpuTensor<T = f32>` now supports both f32 and u32 storage, preserving the
existing default f32 API. Layout transforms share storage and use the common
`tensor-core` shape, stride, offset and broadcasting rules. u32 values remain
integers throughout upload, shader execution and readback, including values
above the exact-integer range of f32.

`ComputeRuntime` implements `TensorIndexBackend`. Each common backend operation
submits GPU work without reading an intermediate result. Explicit
`ComputeProgram` methods prepare reusable chains in one submission:

| Recorded method | Result and contract |
|---|---|
| `tensor_materialize_u32[_into]` | Logical strided u32 values to contiguous output |
| `tensor_compare[_into]` | f32 or u32 broadcast comparisons, exact zero/one masks |
| `tensor_select[_into]` | Broadcast f32/u32 values and mask; nonzero means true |
| `tensor_scan[_into]` | Any axis, f32/u32, inclusive/exclusive and forward/reverse |
| `tensor_scan_u32` | Uses the existing optimized flat exclusive scan when applicable |
| `tensor_gather[_into]` | Values plus a scalar GPU invalid-index count |
| `tensor_compact[_into]` | Stable logical-order values plus a scalar GPU count |

Read inputs may be transposed, narrowed, broadcast, or have nonzero offsets.
Outputs must be contiguous, shape-compatible and distinct from every input and
other output. Caller-owned output offsets are supported. Cross-type aliases
are checked by storage identity, so reimporting one buffer as f32 and u32 does
not bypass validation.

## Execution details

### Axis scans

The requested axis is viewed as the last dimension, and a noncontiguous input
is materialized on the GPU. Each workgroup scans a 256-element row block with
shared memory. Block totals are scanned recursively, then their offsets are
added to local results. A final GPU copy restores the original axis order when
needed. No CPU intermediate or per-row submission occurs.

Reverse scans traverse each row from its last element while preserving output
shape and coordinate order. Integer arithmetic wraps modulo 2^32. f32 scans
follow parallel addition order and require finite inputs/intermediates; they do
not promise bitwise parity with sequential CPU accumulation.

### Gather

The index tensor's entire shape replaces the selected source axis. Scalar
indices remove that axis. The value kernel checks each index before loading:
invalid indices write zero. A separate GPU pass counts invalid **logical index
elements**, once each, independent of source dimensions that replicate them in
the output. This pass still runs when the gathered output is empty. A preceding
GPU reset prevents stale counts when a prepared program is reused.

### Stable compaction

The mask may broadcast to the input shape without expanding it. Its logical
values are materialized only when needed, then the existing optimized flat
scan computes local and block offsets. The typed scatter reads the original
strided input directly. Selected values preserve logical row-major order.
Output shape is `[input.numel()]`; the unused tail is zeroed on every execution,
and count is a scalar u32 tensor. The count must be respected by consumers that
cannot safely consume a zero-filled capacity tail.

## Validation and resource ownership

Multi-stage operations prepare their resources in a temporary program before
moving the ordered dispatches into the caller. `ComputeBatch::append` is the
small internal hook supporting this transaction; it neither submits nor reads
back. Binding groups retain every intermediate allocation. Device limits and
u32 metadata representation are checked before execution.

The index kernel cache is initialized separately from the f32 arithmetic cache,
so arithmetic-only runtimes do not compile the extra pipelines. Seven kernels
share f32/u32 source templates through an explicit WGSL value alias. Both
specializations are validated; no float conversion implements integer math.

## Verification

On the available Metal adapter, strict real-GPU execution passed:

- `tensor_index`: 5 tests in 0.71 seconds. Includes the shared indexing backend
  fixture and a prepared compare → select → reverse exclusive scan → gather →
  compact → sum chain. The chain changes inputs, threshold and indices between
  executions and copies only final values/counts in one submission.
- Scan boundary/reuse coverage includes 0, 1, 255, 256, 257, 65,537 and 131,075
  values per row, both directions, both inclusive modes, u32 overflow,
  noncontiguous inputs and offset outputs with sentinel tails.
- u32 compaction preserves large integer bits, clears stale tails and counts,
  and shares the existing scan engine. Empty-output gather still reports its
  invalid index count and resets it on subsequent runs.
- Cross-type storage aliases, foreign scalar counts and invalid axes are
  rejected without poisoning a prepared program.
- Existing `tensor` tests: 10 passed; common f32 `tensor_backend`: 2 passed.
- All 31 shipped WGSL templates and all seven u32 specializations pass Naga.
  Library and indexing tests pass clippy with `-D warnings`; Cargo still reports
  the pre-existing unrelated `raster-core` binary naming warning.

```sh
COMPUTE_REQUIRE_GPU=1 cargo test --manifest-path crates/Cargo.toml \
  -p compute-core --test tensor_index --test tensor --test tensor_backend \
  -- --test-threads=1
cargo test --manifest-path crates/Cargo.toml -p compute-core \
  --lib integer_specializations_validate
cargo test --manifest-path crates/Cargo.toml -p compute-core \
  --test kernels shipped_kernels_validate_with_naga
```

These are correctness results on Metal. No new indexing throughput claim or
cross-vendor hardware qualification is implied.
