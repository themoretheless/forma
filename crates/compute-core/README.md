# compute-core

## Software binary64

`ComputeRuntime` implements `TensorF64Backend` on native hosts even when the
GPU has no native f64. `GpuF64Tensor` stores each value as two u32 words;
uploads, views and readback preserve the bits. All numerical work runs in
integer WGSL kernels on the selected GPU. There is no f32 conversion or CPU
arithmetic fallback in this backend.

Supported: strided views and materialization, broadcast elementwise operations,
sum/product/min/max and mean reductions, and batched matrix multiplication.
Add/subtract/multiply/divide/sqrt use round-to-nearest, ties-to-even, including
subnormals, signed zero, infinities and NaNs. NaN payload propagation is not
standardized. Dynamic rounding modes and exception flags are not exposed.
`exp/log/sin/cos` use binary64 polynomial arithmetic; they are approximate,
not guaranteed correctly rounded. Trigonometric argument reduction uses a
1280-bit 2/pi constant across the finite binary64 exponent range.

This does not yet implement the f64 indexing/scatter extension traits,
prepared recording, neural-network operations or a browser async tensor API.
The shader arithmetic source is separately exported for composition.

[Implementation and qualification](../../docs/qualification/software-binary64-2026-09-27.md).


Architecture and crate boundaries: [GPU library design](../../docs/design/gpu-library-architecture.md).
Measured dot/sum improvements, subgroup experiments and covariance precision:
[reduction qualification](../../docs/qualification/shader-compute-reductions-2026-09-27.md).

Runtime library for WGSL compute kernels — the compute counterpart of
`raster-core`. Domain kernels (math-core, photogrammetry-core, geometry-bridge)
own their WGSL sources; what they share is the dispatch plumbing, and that
lives here.

## Resident tensors

`GpuTensor<T = f32>` adds checked shapes, strides and offsets to existing GPU
arrays. `f32` and `u32` values stay typed throughout execution. Transpose,
permutation, narrow and broadcast create shared views; recorded materialization
copies their logical values on the GPU. Rank-zero shapes represent scalars and
zero-length dimensions represent empty tensors.

`ComputeProgram` records tensor operations for repeated execution:

| Area | Methods and behavior |
| --- | --- |
| Arithmetic | `tensor_unary`, `tensor_binary`; trailing-axis broadcasting |
| Low arithmetic | `tensor_unary_low`, `tensor_binary_low`; packed f16/BF16 with f32 evaluation and final low rounding |
| Low reductions | `tensor_reduce_low_f32`, `tensor_mean_low_f32`; direct packed loads with f32 accumulation, plus low-result variants |
| Low indexing | `tensor_compare_low`, `tensor_select_low`, `tensor_gather_low`, `tensor_compact_low`; exact IEEE masks and raw payload movement |
| Low scatter | `tensor_scatter_low`, `tensor_scatter_low_f32`; all five modes, direct packed updates and final low rounding |
| Low scans | `tensor_scan_low_f32`, `tensor_scan_low`; direct packed loads, f32 prefix accumulation and optional final low rounding |
| Low statistics | `tensor_softmax_low_f32`, `tensor_log_softmax_low_f32`, `tensor_logsumexp_low_f32`, `tensor_moments_low_f32`, `tensor_layer_norm_low_f32`; direct low inputs, plus final-rounded low results |
| Matrix products | `tensor_matmul`; vectors, matrices and broadcast batches |
| Convolution | `tensor_conv`, `tensor_conv_low_f32`, `tensor_conv_low` and `_into` variants; grouped channel-first 1D/2D/3D cross-correlation |
| Reductions | `tensor_reduce` for f32/u32 sum, product, min, max; f32 `tensor_mean` |
| Masks | `tensor_compare`, `tensor_select`; exact u32 masks and broadcast selection |
| Scans | `tensor_scan`; any axis, inclusive/exclusive, forward/reverse |
| Gather | `tensor_gather`; resident indices, zero-filled invalid reads and GPU invalid count |
| Scatter | `tensor_scatter`; Replace/Add/Multiply/Min/Max, broadcast updates and GPU invalid count |
| Compaction | `tensor_compact`; stable logical order, fixed capacity, GPU count and zero tail |
| Statistics | `tensor_softmax`, `tensor_log_softmax`, `tensor_logsumexp`, `tensor_moments`, `tensor_layer_norm`; arbitrary axes and stable centered/scaled arithmetic |
| Attention | `tensor_attention`; GQA, broadcast batches, resident Keep/additive masks and signed causal offsets |
| Low attention | `tensor_attention_low_f32`, `tensor_attention_low`; direct packed Q/K/V, f32 accumulation and optional final low rounding |

The `_into` variants use validated caller-owned outputs. Prepared programs keep
their allocations and bindings; input writes followed by another recording
reuse the same program. Intermediate results and dynamic counts stay on the
GPU. See the runnable [tensor program](examples/tensor_program.rs).

Scatter Replace chooses the last logical index among duplicates. Other scatter
modes include the original value and every valid update; integer add/multiply
wrap and floating-point accumulation order can vary. Invalid indices are
counted once even for empty outputs. See [scatter contracts and tests](benchmarks/tensor-scatter.md)
and [reduction/vector contracts](benchmarks/tensor-reductions.md).

For portable native execution, `tensor-core` defines narrow `TensorBackend`,
`TensorIndexBackend`, `TensorReduceBackend`, `TensorScatterBackend`,
`TensorLowBackend`, `TensorLowOpsBackend`, `TensorLowIndexBackend`,
`TensorLowScatterBackend`, `TensorStatsBackend`, `TensorLowStatsBackend`,
`TensorAttentionBackend` and `TensorLowAttentionBackend` traits.
They are implemented by `ComputeRuntime`, `compute-cuda::CudaRuntime` and
`compute-mlx::MlxBackend`. Their allocation types remain backend-specific and
foreign-runtime tensors are rejected. The common native adapter synchronizes
at host readback; the recorded WGSL API also compiles for WASM and exposes
nonblocking readback. Browser execution requires separate qualification.

Coverage, numerical limits and hardware evidence are tracked in the
[tensor backend design](../../docs/design/tensor-backends-2026-09-27.md).

### f16 and bf16 storage

`GpuLowTensor` stores two 16-bit values per u32 word. Its shape, strides and
offsets count 16-bit elements; odd sizes add one padding lane. Empty storage
uses the minimum four-byte GPU allocation. `allocation_bytes()` reports actual
backing bytes. The existing `GpuArray<f32/u32>` and `GpuTensor` ABI is unchanged.

`tensor_materialize_low` preserves raw bits. `tensor_cast_to_low` and
`tensor_cast_to_f32` use integer GPU codecs with round-to-nearest, ties-to-even,
signed zero, subnormals and signed infinity on overflow. Cast NaNs remain NaNs;
raw upload/read and materialization also preserve their payload bits.

`tensor_matmul_low_f32` decodes packed values during tiled loads and accumulates
into f32 output. `tensor_matmul_low` adds one final rounding to the input dtype,
using only an f32 result intermediate. Whole operands are never expanded to f32.
Both support vectors, strided matrices, broadcast batches and empty contractions;
mixed f16/bf16 operands are rejected. `_into` methods accept distinct contiguous
outputs, including odd 16-bit offsets, and preserve neighboring storage lanes.

The native `TensorLowBackend` adapter provides synchronous raw readback and
convenient views/casts/products. Recorded programs compose these operations
with the existing kernels. `upload_low_bits` uploads exact u16 payloads;
`write_low_storage_bits` updates the full physical logical storage while keeping
prepared bindings. Use `packed_words()` for recorded raw transfers.

Capabilities report `LowStorage::Packed16x2` and direct matmul support for both
formats. This path requires no optional `SHADER_F16` feature and makes no claim
of native half arithmetic or Tensor Core use. See
[low-precision contracts and exhaustive tests](benchmarks/tensor-low-contracts.md)
and the [measured storage/execution tradeoff](benchmarks/tensor-low.md).

`TensorLowOpsBackend` adds all nine unary and six binary operations, sum,
product, min, max and mean. Arithmetic evaluates in f32 and rounds once to
the input low dtype. Binary dtypes must match. Negate/Abs preserve exact finite
bits; extrema retain subnormals and select -0 for Min and +0 for Max.
Reductions load packed inputs directly, accumulate in f32, and return either
f32 or one final low conversion. Arbitrary axes, strides and broadcast views
are supported. Recorded `_into` operations preserve neighboring halfwords
and validate output aliases before appending work. See the
[arithmetic contracts](benchmarks/tensor-low-ops-contracts.md) and
[matched benchmark](benchmarks/tensor-low-ops.md).

`TensorLowIndexBackend` adds IEEE comparisons for every low bit pattern, raw
selection, gather, stable compaction and prefix sums. Routing preserves NaN
payloads, signed zeros and subnormals; comparisons treat both zeros as equal
and NaNs as unordered. Gather counts invalid indices once and returns zero for
invalid reads. Compaction returns fixed capacity with a GPU count and zero tail.
Scans support every axis, inclusive/exclusive and forward/reverse traversal,
with f32 accumulation before optional low rounding. Inputs remain packed through
the first load; existing scan and indexing traversal code is reused. See
[low indexing contracts](benchmarks/tensor-low-index-contracts.md) and
[timings and memory accounting](benchmarks/tensor-low-index.md).

`TensorLowScatterBackend` adds all five scatter modes with either low or f32
output. Replace elects the last logical index and preserves raw low payloads;
Min/Max preserve finite subnormals and signed-zero ties. Add/Multiply fold direct
packed update loads into an f32 result accumulator, then round once for low
output. F32 output retains the value before low rounding. The shared scatter
traversal and owner/count helpers also support arbitrary strides, broadcast
updates and replay. See [low scatter contracts](benchmarks/tensor-low-scatter-contracts.md).

`TensorLowStatsBackend` adds all five statistical operations with f32 results
and one final cast for low results. Packed input loads share the stable f32
traversal and planner. Row summaries keep small BF16 coordinates in scaled
units so meaningful normalization results survive input subnormal handling.
No full f32 input, shifted-input or centered-input temporary is created.
See [low statistics qualification](../../docs/qualification/tensor-low-statistics-2026-09-27.md).

`TensorLowAttentionBackend` adds the same masked attention geometry for matching
f16/BF16 Q/K/V, with f32 output or one final low cast. Additive masks remain f32.
Direct packed loads reuse the tiled/split-key planner without full f32 operand
copies or score matrices. Integer exponent scaling preserves normal products
of BF16 subnormal operands and large finite partners. Both outputs have `_into`
variants with ownership, alias, offset and transactional-recording checks.
The direct path removes operand temporaries; measured speed depends on shape
and dtype. See [matched timings](benchmarks/tensor-low-attention.md) and
[low attention qualification](../../docs/qualification/tensor-low-attention-2026-09-27.md).

### Statistics and attention

Statistics use max-shifted exponentials and scaled centered moments, avoiding
raw squared moments and overflowing uncentered sums. Softmax, log-softmax and
layer norm preserve shape; logsumexp and moments reduce arbitrary axes with
optional dimension retention. Gamma/beta transforms compose with binary ops.
See [numerical contracts](benchmarks/tensor-normalization-contracts.md) and
[softmax measurements](benchmarks/tensor-normalization.md).

`tensor_attention(query, key, value, mask, options)` computes forward scaled
dot-product attention entirely on the device. Inputs are matrices or
`[..., heads, sequence, depth]`; Q heads can share grouped K/V heads. Mask
types, scale and causal alignment come from `tensor_core::{AttentionMask,
AttentionOptions}`. Fully closed rows return zeros. The `_into` form preserves
caller-owned output offsets and validates aliases before recording.

Streaming tiles avoid score/probability matrices. Long-key problems with few
query rows split across workgroups and merge bounded partial results. The
measured decode case is faster than public matmul/softmax composition; the
measured prefill and wide-value cases remain slower. See the
[attention contract](benchmarks/tensor-attention-contracts.md),
[benchmark and limits](benchmarks/tensor-attention.md) and runnable
[comparison](examples/bench_tensor_attention.rs).

## Typed array programs

`ComputeRuntime` compiles reusable kernels and owns typed `GpuArray<f32>` and
`GpuArray<u32>` storage. Arithmetic maps are f32; u32 arrays support storage, range updates, exclusive
prefix sums and GPU-resident selection counts. `ComputeProgram` prepares an ordered chain once and
can submit it repeatedly after inputs change.

```rust
use compute_core::{BinaryOp, ComputeRuntime, UnaryOp, gpu_compute::GpuContext};
use std::time::Duration;

let context = GpuContext::new().expect("GPU adapter");
let runtime = ComputeRuntime::new(&context)?;
let input = runtime.upload(&[3.0f32, -4.0])?;
let mut program = runtime.program();
let squared_norm = program.dot(&input, &input)?;
let norm = program.unary(UnaryOp::Sqrt, &squared_norm)?;
let normalized = program.binary(BinaryOp::Divide, &input, &norm)?;

// All kernels and the final copy/map are submitted together. Intermediate
// arrays, including the scalar norm, stay on the GPU.
let ticket = program.submit_read(&normalized)?;
// An event loop can call ticket.try_read()? until it returns Some(values).
let values = ticket.wait(Duration::from_secs(10))?;
// Approximately [0.6, -0.8].
```

Supported operations:

- affine map (`input * scale + offset`);
- add, subtract, multiply, divide, min and max, including a single-element GPU
  array broadcast on either side;
- negate, abs, square, sqrt, reciprocal, exp, log, sin and cos;
- sum and fused dot product with all reduction passes on the GPU;
- six f32 comparisons yielding zero/one u32 masks, with scalar broadcasting;
- exclusive u32 prefix sum with wrapping addition;
- stable f32 compaction by a nonzero u32 mask, with GPU-resident count and zero tail.

`dot(a, b)` evaluates products in registers and reduces directly to workgroup
partials. It no longer allocates an array of products. `dot_into(a, b, output)`
reuses a distinct single-f32 output; both inputs must have equal length. Empty
inputs reset the output to zero on every execution. Its pipeline is compiled
lazily and retained by the runtime. The implementation uses the same graph
lowering and execution path as `FusionGraph::compile_sum`; floating-point
summation order and FMA contraction may differ from separate multiply and sum.

`sum_into(input, output)` likewise reuses a distinct scalar output, including
zeroing it for empty inputs. On Metal, `sum` above 65,536 elements uses a bounded
grid of at most 256 workgroups to reduce intermediate storage and scheduling
overhead. Smaller inputs and other backends keep the previous schedule. Raw
`Reduction` constructors preserve their supplied-kernel scheduling contract;
`record_in_pass` composes all its stages inside a caller-owned compute pass.

### Prefix scans and stable selection

```rust
use compute_core::CompareOp;

let input = runtime.upload(&[10.0f32, 20.0, 30.0, 40.0])?;
let threshold = runtime.upload(&[25.0f32])?;
let mut program = runtime.program();
let keep = program.compare(CompareOp::Greater, &input, &threshold)?;
let prefix = program.exclusive_scan(&keep)?; // [0, 0, 0, 1]
let selected = program.compact(&input, &keep)?;
let total = program.sum(selected.values())?;
let value = program.submit_read(&total)?.wait(Duration::from_secs(10))?;
// value == [70.0]; comparison, scan, selection and reduction use one submission.
// selected.values() has capacity 4: [30.0, 40.0, 0.0, 0.0].
// selected.count() is a one-element GPU u32 array containing 2.
```

`compare` and `compare_into` support `Equal`, `NotEqual`, `Less`, `LessEqual`,
`Greater` and `GreaterEqual`. They compare equal-length f32 arrays or broadcast a
one-element array on either side, producing exact zero/one masks on the GPU.
Update the threshold scalar and resubmit the prepared program to change the
selection. Comparisons follow WGSL semantics for finite inputs; arithmetic
producers must also keep their intermediate f32 values finite. No portable NaN
or infinity comparison contract is promised.

`exclusive_scan` computes `output[i] = input[..i].sum()` modulo 2^32. Compaction
normalizes its mask to zero/one before scanning; every nonzero mask keeps one
value. Neither primitive reads intermediate results on the CPU. A hierarchy of
workgroup scans and prefix additions supports lengths beyond a single dispatch
grid. Prepared programs retain levels, bindings and allocations between runs.
Use `exclusive_scan_into(input, output)` or
`compact_into(input, keep, output, count)` to supply output storage; lengths must
match the input and the count must contain exactly one u32. All output storage
must be distinct from inputs and from the other output, including different
scalar interpretations imported from the same buffer.

`CompactedArray` exposes both capacity and logical count. After the producer
executes, the prefix of `values()` identified by `count()[0]` is stable and the
remaining capacity is zero. The tail is cleared every run, including after a
larger previous selection. Summing the full capacity therefore gives the selected
sum without reading count first. Operations that change zero, such as adding a
constant, must respect the logical count separately. Mutating returned arrays
invalidates the producer's invariant until it runs again. Empty selections have
zero capacity and a GPU count of zero. Scan overflow wraps; selection counts do
not overflow because runtime array lengths are bounded by u32.

### Memory and submission rules

- New arrays are zero-initialized by wgpu without uploading a host zero vector.
- Clones and `prefix(len)` views share storage. `write(array, offset, values)`
  updates contents without changing prepared bindings.
- `_into` variants reuse caller-allocated output arrays. Input/output aliasing,
  length mismatches, foreign runtime arrays, invalid ranges and allocations over
  device limits return `ComputeError` before recording GPU work.
- Binary operations accept equal lengths or one scalar; dot requires equal
  lengths. Empty sum/dot returns zero, empty elementwise output stays empty.
- Programs retain their intermediate allocations. Repeated submissions reuse
  kernels, parameters, bindings and storage; each readback ticket gets separate
  staging memory so overlapping submissions cannot overwrite earlier results.
- Queue writes apply before the next submission, not between program steps.
  Use separate arrays for distinct per-step input values.
- `submit_read` performs no GPU wait. `try_read` polls completion without waiting;
  `wait(timeout)` is a native convenience. A ticket is consumed once. Dropping
  one discards that result and does not block other submissions.
- Arithmetic uses WGSL f32 semantics, including its function domains and
  backend-dependent rounding. No portable NaN, infinity or division-by-zero
  contract is promised. Compare reductions with tolerances against CPU references.

Run the complete centering/normalization example and measured CPU comparison:

```sh
cargo run --release --offline -p compute-core --example array_pipeline
COMPUTE_REQUIRE_GPU=1 cargo test --offline -p compute-core
```

The example reports median host-observed time for GPU compute plus full readback,
upload plus compute plus full readback, and a CPU reference with f64 sum
accumulation and f32 outputs (GPU arithmetic is f32 throughout). Preparation is
outside timing, three warmups precede nine samples, and timing order rotates.
These are workload-specific measurements, not GPU timestamp timings. The strict
GPU environment flag prevents the runtime suite from passing via adapter skips.

## What it owns

- **`Kernel`**: a validated, cached compute pipeline with a declared binding
  layout. Construction is eager and fallible — a broken WGSL source is a
  `KernelError`, not a panic (device error scope around pipeline creation).
- **Buffer helpers**: typed storage/uniform uploads
  (`storage_f32`, `storage_f32_zeroed`, `storage_u32`, `uniform_f32`) and
  typed readbacks (`read_f32`, `read_u32`) through a MAP_READ staging copy
  (storage buffers cannot hold `MAP_READ` in wgpu).
- **Generic kernels** in `shaders/` (`include_str!`-embedded):
  - `scale_add` / `scale_add4` — elementwise affine map
    `output[i] = input[i] * scale + offset`, scalar and vec4 variants
    (the vec4 variant streams 16 bytes per thread and runs ~3x faster
    than scalar on Apple Silicon)
  - `zip_mul` / `zip_mul4` — elementwise product `output[i] = a[i] * b[i]`
  - `block_sum` — grid-strided sum reduction into per-group partials: a
    fixed grid of workgroups strides over arbitrary lengths, so one
    dispatch covers more than the 65535-workgroup limit; chain passes —
    or call the [`reduce_f32`] helper — to fold down to a scalar

## The `WG` anchor convention

Every tunable kernel source declares:

```wgsl
const WG: u32 = 256;
@compute @workgroup_size(WG)
```

The runtime substitutes a tuned power of two before compilation
(`Kernel::with_workgroup_size`, `Kernel::tuned`). Substitution is validated:
non-power-of-two sizes are rejected, and the result is clamped to the device's
per-workgroup limits so a tuner pick never fails pipeline validation on a
tighter backend.

Per-backend tuning follows `gpu_compute::tuned_workgroup_size`: smaller
groups on Metal (tile-based GPUs), the larger default elsewhere. Note the
tuning is a heuristic — for pure streaming elementwise kernels a 256-wide
group measured ~40% faster than 128 on Apple Silicon (M4 Max, see
`examples/bench.rs`), so domain kernels should benchmark their own shape.

## Usage

```rust
use compute_core::gpu_compute::GpuContext;
use compute_core::shaders::SCALE_ADD_WGSL;
use compute_core::{Binding, Kernel, read_f32, storage_f32, storage_f32_zeroed, uniform_f32};

let context = GpuContext::new().expect("GPU adapter");
let device = &context.device;
let queue = &context.queue;

let kernel = Kernel::tuned(
    &context,
    "scale_add",
    SCALE_ADD_WGSL,
    "main",
    &[Binding::Uniform, Binding::StorageRead, Binding::StorageReadWrite],
    128, // metal_size
    256, // default_size
)
.expect("kernel builds");

let input = storage_f32(device, queue, &[1.0f32, 2.0, 3.0, 4.0]);
let output = storage_f32_zeroed(device, queue, 4);
// Params: count(u32), scale(f32), offset(f32), pad — packed as 4 f32.
let mut params = vec![0.0f32; 4];
params[0] = f32::from_le_bytes(4u32.to_le_bytes());
params[1] = 2.0;
params[2] = 0.5;
let params = uniform_f32(device, queue, &params);

kernel.dispatch(device, queue, &[&params, &input, &output], 4);
let result = read_f32(device, queue, &output, 4);
assert_eq!(result, vec![2.5f32, 4.5, 6.5, 8.5]);
```

Binding order is sequential from `binding(0)` in declaration order
(uniforms and storage interleaved as the shader declares them).

A single non-strided dispatch covers at most `65535 * workgroup_size`
invocations (the wgpu per-dimension workgroup-count limit); larger workloads
are the caller's job to chunk (`Kernel::max_dispatch_invocations`).
Grid-strided kernels like `block_sum` sidestep this via
`Kernel::dispatch_groups`, and `reduce_f32` schedules the whole multi-pass
chain for arbitrary input lengths.

## Tests and bench

```sh
cargo test --offline -p compute-core
cargo run --release --offline --example bench -p compute-core
```

GPU-backed tests skip cleanly on machines without an adapter.

Measured on Apple M4 Max (Metal, release): scale_add ~47 GB/s at WG=128 and
~67 GB/s at WG=256; the vec4 variant scale_add4 ~113 GB/s (16 bytes streamed
per thread vs 4); the zip_mul + block_sum dot-product pair ~70–115 GB/s
effective. These are measurements of the original streaming examples. For
reductions, the number of dispatched groups also matters: the later bounded-grid
study retained the scalar shader with a new schedule. A storage
`array<vec4<f32>>` also requires explicit handling of unpadded tails. Benchmark
vector access and dispatch shape against the operation's buffer contract.

## Reusable GPU chains

`ComputeBatch` retains prepared bindings and records all steps into one queue
submission. `Reduction` prepares all sum passes once, with separate uniforms
and intermediate buffers for each pass. Its scalar output stays on the GPU and
can feed another kernel. The synchronous `reduce_f32` convenience function uses
this plan internally, then reads back the scalar.

```rust,ignore
let reduction = Reduction::new(device, queue, &sum, &products, count);
let mut batch = ComputeBatch::new();
batch.push(&multiply, &multiply_bindings, multiply.workgroup_count(count));
batch.push_reduction(&reduction);
// Bind reduction.output() as input to a downstream kernel if needed.
// Update input buffer contents before each submission; reuse this batch.
batch.submit(device, queue);
let total = read_f32(device, queue, reduction.output(), 1)[0];
```

Plans have fixed buffer identities and element counts; rebuild them when those
change. Queue writes before submission apply to the entire batch, so use distinct
uniform buffers for steps with different parameters. Empty sums produce zero;
zero-length typed readbacks return an empty vector. Readback currently uses a
separate copy submission and blocking map. Automatic graph scheduling, buffer
lifetime pooling and kernel fusion are not implemented. The typed runtime above
provides nonblocking readback within the compute submission.

### Local measurement (2026-09-26)

One release run on the local Metal backend, using the example above:

| Elements | GPU resident + full readback | Upload + GPU + full readback | CPU with f64 sums |
| --- | ---: | ---: | ---: |
| 4,097 | 0.334 ms | 0.378 ms | 0.007 ms |
| 1,000,003 | 3.014 ms | 2.783 ms | 1.807 ms |

Maximum absolute difference from the reference was 1.863e-9 and 1.164e-10,
respectively. The small reversal between the two large-array GPU measurements
is run variation, not a benefit from uploading data. This full-readback workload
shows no speedup over the CPU reference. CPU sums use f64 to avoid accumulated
serial-f32 reference error; the GPU uses f32 throughout. Use these numbers as an
example of end-to-end cost, not a hardware-independent performance claim.

### Shared storage and scratch capacity

`GpuArray::view()` exposes a borrowed `gpu_compute::GpuBufferView` for domain
adapters. `GpuBuffer::new(&context, bytes, usage)` creates storage with trusted
context ownership; `ComputeRuntime::import_buffer` checks owner, usage and scalar
length. Context clones share identity; unrelated devices are rejected.

`ScratchPool::new(&runtime, budget_bytes)` retains named allocations. A repeated
`reserve::<f32>("input", len)` reuses sufficient capacity; growth replaces storage
and increments `ScratchArray::generation`. Rebuild prepared bindings on a new
generation. Old plans retain their original buffers. The budget covers capacity
retained by the pool, excluding replaced buffers held by callers or GPU work.

Readback delegates to `gpu_compute::ByteReadback`. Use `record_read` and mark the
ticket `submitted` after submitting a caller-owned encoder. Mapping failures,
cancellation and empty payloads are distinct. Legacy `read_f32`/`read_u32` panic on
transport failure; use their `try_` variants for recoverable errors.

### WGSL contracts and compiled pipeline reuse

`Kernel::new` now derives the selected entry point's workgroup size and buffer
requirements from WGSL. It supports fixed one-dimensional compute entry points;
tuning rejects a textual anchor that does not control the selected entry point.
Wrong entry names, buffer layout kinds and unsupported dimensionality fail before
pipeline execution. `binding_info()` exposes minimum buffer sizes, including one
element for trailing runtime arrays.

Use `Kernel::from_context` or `KernelCache` for checked custom bindings:

```rust,ignore
let mut cache = compute_core::KernelCache::new(&context, 32);
let kernel = cache.get("scale", compute_core::shaders::SCALE_ADD_WGSL, "main", &[
    compute_core::Binding::Uniform,
    compute_core::Binding::StorageRead,
    compute_core::Binding::StorageReadWrite,
])?;
let bindings = kernel.bind(&context, &[
    params.view(0..16)?, input.view(0..input.size())?, output.view(0..output.size())?,
])?;
kernel.record_dispatch(&mut encoder, &bindings, kernel.workgroup_count(count));
```

The checked API rejects foreign devices, wrong usage, unaligned/undersized
ranges, and repeated buffers when a slot is writable. Raw-device constructors
retain existing low-level APIs; `bind` requires tracked context ownership.

`KernelCache` keys exact compiled source, entry and layout; diagnostic labels do
not affect reuse. It evicts the least recently used entry at its configured
capacity. Existing `Arc<Kernel>` handles survive eviction/clear, while failed
compilations leave cached entries intact. `stats()` reports hits, compilation
attempts and evictions. Capacity bounds retained entries rather than total VRAM.

`Readback::wait_mut` allows retry after a timeout without consuming the ticket.
The existing consuming `wait` remains available.

A runnable example measures `compare → compact → sum` with one submission and
only eight bytes of final readback:

```sh
cargo run --release --offline --manifest-path crates/Cargo.toml \
  -p compute-core --example compact_reduce
```

It checks a CPU reference, separates plan preparation, resident execution and
upload-inclusive execution, and rotates timing order. These host timings include
submission and waiting; they are not GPU timestamp measurements.

## Dense matrix multiplication

`MatrixView` gives an existing f32 array an exact row-major shape. `matmul`
records `(m × k) @ (k × n)` and returns a `GpuMatrix`; its `values()` can feed
ordinary array operations, while `view()` can feed another matrix product.

```rust
use compute_core::MatrixView;

let a = runtime.upload(&[1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0])?;
let b = runtime.upload(&[7.0f32, 8.0, 9.0, 10.0, 11.0, 12.0])?;
let mut program = runtime.program();
let product = program.matmul(MatrixView::new(&a, 2, 3)?, MatrixView::new(&b, 3, 2)?)?;
let total = program.sum(product.values())?;
// product has shape 2×2: [58, 64; 139, 154]. No intermediate CPU copy.
let value = program.submit_read(&total)?.wait(Duration::from_secs(10))?;
// value == [415.0]
```

`matmul_into(a, b, output)` writes a matching m×n view without allocating an
output. Matrix dimensions and array length must match exactly, with checked
multiplication and u32 dimension bounds. Input/output aliases, foreign runtime
arrays, mismatched inner dimensions and incorrectly shaped outputs fail before
recording. Shapes with zero m or n produce empty output. A zero k writes zeros
to the entire output on every execution, including reused nonzero storage.

The general kernel stages 32×32 A/B tiles in workgroup memory. A fixed 64-lane workgroup
computes the corresponding output tile; each lane holds sixteen accumulators in
registers. Odd dimensions are padded within shared memory and all global
accesses are bounded. Workgroups stride over flattened tile IDs when the number
of tiles exceeds the dispatch limit. These tile dimensions are fixed and do not
use the generic `WG` tuning anchor. Arithmetic uses f32 fused multiply-add;
inputs and intermediate sums must remain finite. Compare with a tolerance against
a CPU reference because rounding can differ.

On Metal, measured shape ranges select three additional kernels: aligned vec4
tiles, direct small products, or a cooperative reduction across the inner axis.
The cooperative path changes accumulation order. Other backends and unmeasured
shapes retain the general tiled kernel. Exact selection bounds, rejected
candidates and raw benchmark samples are recorded in
[the matrix study](benchmarks/matmul-research.md).

Run the measured baseline comparison:

```sh
cargo run --release --offline -p compute-core --example bench_matmul
```

It uses four warmups and seventeen interleaved samples per shape. Both GPU paths
include submission, completion and full output readback, with inputs resident
and preparation outside timing. The baseline GPU computes one output per thread
without shared tiles. The CPU implementation is single-threaded cache-blocked
Rust with contiguous inner loops; it is not a vendor BLAS baseline. Every output
is checked against an independent f64 reference outside timing. The report
includes maximum absolute errors and does not use GPU timestamps.

## Fused expressions

`FusionGraph` compiles a DAG of f32 arithmetic into one WGSL dispatch. Common
expressions share shader locals, unreachable expressions disappear, and only
final outputs allocate GPU arrays. A prepared kernel can run repeatedly at
new lengths and on new buffers. `compile_cached` uses the bounded `KernelCache`.

```rust,ignore
use compute_core::{BinaryOp, FusionGraph, UnaryOp};

let mut graph = FusionGraph::new(2);
let x = graph.input(0)?;
let bias = graph.input(1)?;
let shifted = graph.binary(BinaryOp::Add, &x, &bias)?;
let squared = graph.unary(UnaryOp::Square, &shifted)?;
let kernel = graph.compile(&context, &[shifted, squared])?;

let input = runtime.upload(&[1.0f32, 2.0, 3.0])?;
let scalar = runtime.upload(&[0.5f32])?;
let mut program = runtime.program();
let outputs = program.fused(&kernel, &[&input, &scalar], input.len())?;
let sum = program.sum(&outputs[1])?;
// One fused dispatch writes both outputs. The reduction consumes the second
// output on the GPU, in the same command submission.
let result = program.submit_read(&sum)?.wait(Duration::from_secs(10))?;
```

`fused_into` reuses caller-owned output arrays. Each input must have the output
length or one broadcast element; this applies to every declared input slot,
including unused slots. Scalar broadcasting works on either side of binary
operations. Explicit output length permits constant-only graphs and zero-length
execution. Multiple outputs must have equal lengths and distinct allocations;
output/input aliases and foreign arrays fail before appending work. Repeated
read-only inputs are allowed. Graph expressions belong to their original graph.
The generated shader is available through `FusedKernel::source()`.

Operations preserve operand order and grouping. Host compilation performs no
arithmetic reassociation or constant folding. WGSL f32 rounding, transcendental
approximations and FMA contraction still apply, so fused and separate kernels
are compared with tolerances. Constants must be finite; callers must keep GPU
inputs and intermediate arithmetic within the supported operation domains.
Compilation checks the device's storage binding limit against live input and
output buffers. The executor uses grid-strided indexing above the dispatch limit.

```sh
cargo run --release --offline --manifest-path crates/Cargo.toml \
  -p compute-core --example bench_fusion
```

This benchmark compares nine existing operations recorded in a shared compute
pass with the same fused expression and a single-loop Rust CPU reference. It
reports resident and upload-inclusive execution, including full output readback,
with compilation and allocation outside the timed loop. There are three warmups
and fifteen samples in rotated order. Intermediate array storage falls from
`8 * len * sizeof(f32)` to zero; both paths retain one final output.

Initial general-tile Metal run (2026-09-27), before shape selection, milliseconds.
This earlier run used two warmups and seven samples:

| Shape `(m, k, n)` | Tiled GPU + readback | Naive GPU + readback | CPU blocked f32 |
| --- | ---: | ---: | ---: |
| 64, 64, 64 | 0.293 | 0.259 | 0.073 |
| 127, 259, 193 | 0.669 | 0.446 | 1.836 |
| 512, 512, 512 | 1.179 | 2.365 | 37.736 |
| 1024, 1024, 1024 | 4.969 | 6.520 | 301.425 |

This run favors shared tiles for the two larger square matrices. The 64×64 case
is faster on CPU, and the rectangular case is faster with the naive GPU kernel.
No automatic crossover policy is inferred from these four shapes. The largest
GPU error against the f64 reference was 3.160e-5; plan preparation took 0.021–0.042
ms with kernels already compiled. These are local host timings subject to runtime
variation, not universal hardware or BLAS performance claims.

### Fusing a map with its sum

`compile_sum` emits a first pass that evaluates the expression and reduces it
inside each workgroup. `fused_sum` folds those partials to a scalar using the
ordinary reduction machinery. The mapped array is never stored. The schedule targets
eight elements per lane on large inputs; partial storage is
approximately `ceil(len / 2048) * 4` bytes, capped by the dispatch limit.

```rust,ignore
let mut graph = compute_core::FusionGraph::new(2);
let value = graph.input(0)?;
let threshold = graph.input(1)?;
let keep = graph.compare(compute_core::CompareOp::Greater, &value, &threshold)?;
let zero = graph.constant(0.0)?;
let selected = graph.select(&keep, &value, &zero)?;
let kernel = graph.compile_sum(&context, &selected)?;
let mut program = runtime.program();
let sum = program.fused_sum(&kernel, &[&values, &threshold_scalar], values.len())?;
let result = program.submit_read(&sum)?.wait(Duration::from_secs(10))?;
```

This conditional sum allocates neither a mask nor a compacted array. Use
`compact` when subsequent work needs the selected sequence itself. Predicates
are typed handles belonging to their graph; all six comparison operators work
in both ordinary fused maps and sums. `select` evaluates both branches, following
WGSL semantics, so each branch must stay within its arithmetic domain.

`fused_sum_into` accepts a distinct, single-element destination. Empty input
actively resets it to zero. Scalar broadcasts and constant-only sums are
supported; logical length must fit u32. Recording retains its uniforms and
buffers so input updates and repeated submissions need no reallocation.
Summation order differs from separate map/reduce execution. Results use f32,
with no compensated-summation guarantee; tolerance should account for scale and
cancellation. A compiled sum has a distinct type and cannot accidentally be used
as an elementwise-output kernel. `compile_sum_cached` shares the existing LRU.

### Scan and stable compaction execution

The scan processes four consecutive values per lane, then scans lane totals
inside the workgroup. Compaction consumes local offsets plus scanned block
offsets directly, combining scatter, count and zero-tail clearing in one final
dispatch. These writes cover disjoint destination ranges, including reuse after
a larger selection. The original public shader sources remain available; the
four-element variant is `SCAN_BLOCKS4_WGSL`.

Matched Metal measurements on M4 Max at 1M/4M elements and 0/50/100% masks show
**1.25–1.43×** improvement including the full compacted output readback, and
**1.38–1.60×** for the complete `compare → compact → sum` pipeline with the same
final readback on both paths. The 4K cases are effectively unchanged, including
one small regression; no small-input speedup is claimed. See
[method, frozen baseline, all results and limitations](benchmarks/scan-compaction-metal.md).
