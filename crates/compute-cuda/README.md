# compute-cuda

CUDA implementation of the shared `tensor-core::TensorBackend` and
`TensorIndexBackend`, `TensorReduceBackend`, `TensorScatterBackend` and
`TensorLowBackend`, `TensorLowOpsBackend`, `TensorLowIndexBackend`, `TensorLowScatterBackend`, `TensorStatsBackend` and
`TensorAttentionBackend`, `TensorConvBackend` and `TensorLowConvBackend` contracts.
f32/u32 and native f16/bf16 storage, layouts and
intermediate results stay on the selected NVIDIA device.
Elementwise and reduction kernels use CUDA; matmul uses cuBLAS. There is no CPU
fallback. The existing domain-specific `math-core` CUDA executor is unchanged.

```rust,no_run
use compute_cuda::{CudaRuntime, tensor_core::{MatmulPrecision, Shape, TensorBackend}};

let cuda = CudaRuntime::new()?;
let a = cuda.upload_f32(Shape::new(vec![2, 3])?, &[1., 2., 3., 4., 5., 6.])?;
let b = cuda.upload_f32(Shape::new(vec![3, 1])?, &[2., 1., -1.])?;
let c = cuda.matmul(&a, &b, MatmulPrecision::F32)?;
assert_eq!(cuda.read_f32(&c)?, [1., 7.]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Runtime requirements

### Direct convolution

`TensorConvBackend::conv` accepts channel-first `[N,C,*spatial]` input and
`[O,C/groups,*kernel]` weights for one to three spatial axes. `ConvOptions`
specifies stride, dilation, asymmetric zero padding and groups. The CUDA kernel
reads strided operands directly and accumulates f32; it does not create an
im2col tensor. `TensorLowConvBackend::conv_low_f32` reads native f16/BF16 operands
directly, while `conv_low` adds one final low cast. Bias can be added with
resident broadcasting. This path does not use cuDNN or claim Tensor Core use.
Prepared programs and CUDA Graph recording do not yet expose convolution.
See the [shared contract and qualification](../../docs/qualification/tensor-convolution-2026-09-27.md).

### Libraries and device

- An NVIDIA GPU and a compatible CUDA driver. `OSV_CUDA_DEVICE` selects its ordinal.
- Compatible cuBLAS and NVRTC shared libraries, loadable by the process. The
  bindings target CUDA 12.8. `CudaRuntime::new` requires NVRTC; matmul additionally
  requires cuBLAS. Missing libraries return explicit errors.
  Required symbols are checked before calls, so an older loadable NVRTC or
  cuBLAS library produces a diagnostic instead of a missing-symbol panic.
- `unsafe CudaRuntime::from_ptx` accepts externally compiled kernels when NVRTC
  cannot be shipped. The PTX must implement the exact signatures and safety
  contracts of the combined `CUDA_KERNEL_SOURCE`: [`src/kernels.cu`](src/kernels.cu)
  plus [`src/indexing.cu`](src/indexing.cu), [`src/reductions.cu`](src/reductions.cu)
  [`src/scatter.cu`](src/scatter.cu), [`src/low_precision.cu`](src/low_precision.cu)
  [`src/statistics.cu`](src/statistics.cu), [`src/attention.cu`](src/attention.cu)
  [`src/low_ops.cu`](src/low_ops.cu), [`src/low_indexing.cu`](src/low_indexing.cu)
  [`src/low_scatter.cu`](src/low_scatter.cu),
  [`src/low_statistics.cu`](src/low_statistics.cu) and
  [`src/low_attention.cu`](src/low_attention.cu).
  This is an expert import API,
  not a way to load arbitrary PTX safely.

NVRTC chooses its highest supported virtual architecture no newer than the
device. Compilation disables fast math, contraction and denormal flushing for
the custom kernels. The resulting PTX is loaded through the CUDA driver.
See the [NVRTC manual](https://docs.nvidia.com/cuda/nvrtc/index.html).

Initialization is explicit: callers can inspect `capabilities()` before
running work. An unavailable CUDA backend does not silently select Metal,
WebGPU, MLX or CPU.

## Implemented operations

| Operation | Implementation and limits |
| --- | --- |
| f32/u32 upload/read/update | Ordered stream transfer; update requires unshared contiguous storage |
| Shape, scalar, empty tensors | Shared checked `tensor-core` rules; rank-zero is one scalar |
| Reshape, permute, broadcast, narrow | Shared layouts; permute/broadcast/narrow are allocation-sharing views |
| Materialize and reshape of a strided view | CUDA gather; preserves logical row-major order |
| Nine unary and six binary operations | CUDA kernels; NumPy-style leading broadcast for binary operands |
| Affine | CUDA elementwise kernel |
| f32/u32 sum, product, min, max over arbitrary axes, keepdims | Shared CUDA fold kernels; full reductions use a hierarchy of resident partials |
| Mean over f32 axes | Resident sum then direct division by the logical contraction count |
| Dot | Resident multiply followed by sum; currently allocates product storage |
| Vector, matrix and batched multiplication | cuBLAS GEMM; vectors promote to matrices and remove inserted output axes; leading dimensions broadcast, strided operands materialize on device |
| F32 / TF32 / FP16 / BF16 matmul policy | Explicit compute type; f32 input/output storage; capability and environment checks |
| Ownership and aliases | Foreign runtimes rejected; cloned tensors share storage and prevent in-place update |
| Six f32 and six u32 comparisons | Exact zero/one u32 masks, with leading broadcast; u32 never converts to f32 |
| f32/u32 selection | Three-way broadcast; every nonzero mask selects the true operand |
| f32/u32 axis scan | Inclusive/exclusive, forward/reverse, strided input, hierarchical block carries; u32 addition wraps |
| f32/u32 gather | Resident u32 indices replace the selected axis; invalid reads produce zero and a scalar invalid count |
| f32/u32 stable compaction | Broadcast mask, logical row-major order, fixed capacity, zero tail and resident scalar count |
| f32/u32 scatter | Replace, add, multiply, min, max; broadcast updates, deterministic last-index Replace and resident invalid count |
| Native f16/bf16 storage and casts | Two-byte allocations; raw-bit upload/read/views; GPU RNE casts, including signed zero and subnormals |
| Native low-input matmul | Direct f16/bf16 inputs, f32 accumulation and output; optional single final low-format rounding |
| Native low unary/binary arithmetic | Nine unary/six binary operations; direct u16 loads, f32 evaluation, one final RNE conversion |
| Native low reductions and mean | Direct u16 loads into shared f32 reduction tree; f32 result or one final low cast |
| Native low comparisons and selection | IEEE comparisons through raw-bit classification; bit-preserving selection and broadcast |
| Native low gather and stable compaction | Shared checked traversal, raw u16 movement, resident counts and zero tail |
| Native low prefix sums | Direct u16 loads, f32 block sums and carries, f32 output or one final low cast |
| Native low scatter | All five modes; raw Replace/Min/Max, f32 Add/Multiply accumulation, low or direct f32 result |
| Softmax, log-softmax and logsumexp | Arbitrary axes, shifted exponentials, per-row f64 sums and resident partials |
| Population moments and layer norm | Two centered f64 passes; normalization stays valid when f32 variance overflows |
| Native low statistics and normalization | Direct u16 loads through the same f64 row passes; f32 results or one final low cast |
| f32 scaled dot-product attention | Online 32-key tiles; resident keep/additive masks, signed causal offset, GQA and broadcast batches |
| Native low attention | Direct u16 Q/K/V loads through the same online kernel; f32 result or one final low cast |
| Prepared f32/u32/f16/bf16 programs | Fixed dtype/layout signatures; typed resident scratch and metadata; arithmetic, casts, views, reductions, mean, matmul and typed indexing; caller-owned output reuse |

No data is read back between stages. Ordinary eager operations queue work on
one stream and allocate results. Prepared programs retain their adapter-owned
intermediates and metadata and can write existing outputs. Opt-in CUDA Graph
programs capture a rebuilt schedule with owned slots. Asynchronous readback
tickets, scratch pooling and generated fusion remain open.

### Prepared resident programs

```rust,no_run
use compute_cuda::{CudaRuntime, CudaPrepareOptions};
use compute_cuda::tensor_core::{Shape, TensorBackend, UnaryOp};

let cuda = CudaRuntime::new()?;
let mut input = cuda.upload_f32(Shape::new(vec![3])?, &[1., 2., 3.])?;
let mut builder = cuda.program();
let x = builder.input(input.layout().clone())?;
let shifted = builder.affine(x, 2., 1.)?;
let y = builder.unary(shifted, UnaryOp::Square)?;
let sum = builder.sum_axes(y, &[0], false)?;
let mut program = builder.prepare(&[y, sum], CudaPrepareOptions {
    max_scratch_bytes: 1 << 20,
    max_metadata_bytes: 1 << 16,
})?;
let mut output = cuda.upload_f32(Shape::new(vec![3])?, &[0.; 3])?;
let mut total = cuda.upload_f32(Shape::new(vec![])?, &[0.])?;
for values in [[1., 2., 3.], [-1., 0., 1.]] {
    cuda.write_f32(&mut input, &values)?;
    program.run_into(&[&input], &mut [&mut output, &mut total])?;
}
program.synchronize()?;
assert_eq!(cuda.read_f32(&output)?, [1., 1., 9.]);
assert_eq!(cuda.read_f32(&total)?, [11.]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Mixed signatures use `input_u32`, `input_low` and typed transport. This example
keeps BF16 values in native two-byte storage and reduces them directly to f32:

```rust,no_run
use compute_cuda::{CudaPrepareOptions, CudaRuntime};
use compute_cuda::tensor_core::{
    BinaryOp, CompareOp, LowDtype, ReduceOp, Shape,
    TensorBackend, TensorIndexBackend, TensorLowBackend,
};

let cuda = CudaRuntime::new()?;
let shape = Shape::new(vec![3])?;
let mut x = cuda.upload_f32(shape.clone(), &[1., 2., 3.])?;
let weights = cuda.upload_low(LowDtype::Bf16, shape.clone(), &[0x3f80, 0x4000, 0x4040])?;
let mask = cuda.upload_u32(shape.clone(), &[1, 0, 7])?;
let mut builder = cuda.program();
let xv = builder.input(x.layout().clone())?;
let wv = builder.input_low(LowDtype::Bf16, weights.layout().clone())?;
let mv = builder.input_u32(mask.layout().clone())?;
let rounded = builder.cast_to_low(xv, LowDtype::Bf16)?;
let product = builder.binary_low(rounded, wv, BinaryOp::Multiply)?;
let selected = builder.select_low(mv, product, wv)?;
let total = builder.reduce_low_f32(selected, ReduceOp::Sum, &[0], false)?;
let greater = builder.compare_low(selected, wv, CompareOp::Greater)?;
let mut program = builder.prepare(&[selected, total, greater], CudaPrepareOptions {
    max_scratch_bytes: 1 << 20,
    max_metadata_bytes: 1 << 16,
})?;
let mut low_output = cuda.upload_low(LowDtype::Bf16, shape.clone(), &[0; 3])?;
let mut sum_output = cuda.upload_f32(Shape::new(vec![])?, &[0.])?;
let mut comparison_output = cuda.upload_u32(shape, &[0; 3])?;
for values in [[1., 2., 3.], [-1., 0., 1.]] {
    cuda.write_f32(&mut x, &values)?;
    program.run_typed_into(
        &[(&x).into(), (&weights).into(), (&mask).into()],
        &mut [(&mut low_output).into(), (&mut sum_output).into(),
              (&mut comparison_output).into()],
    )?;
}
program.synchronize()?;
assert_eq!(cuda.read_low_bits(&low_output)?, [0xbf80, 0x4000, 0x4040]);
assert_eq!(cuda.read_f32(&sum_output)?, [4.]);
assert_eq!(cuda.read_u32(&comparison_output)?, [0, 0, 0]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`CudaProgramBuilder` supports these fixed-signature operations:

| Storage | Recorded operations |
| --- | --- |
| f32 | Nine unary/six binary operations, affine, four reductions, mean, explicit-policy vector/batched matmul |
| u32 | Add/subtract/multiply with wrapping arithmetic, min/max, four reductions; division is rejected |
| f16/bf16 | Nine unary/six binary operations, direct low-input reductions/mean and matmul with f32 or low output |
| All four dtypes | Materialize, reshape, permute, broadcast, comparisons/selection, axis scan, gather, stable compaction and all five scatter modes |
| f32 ↔ low | Explicit `cast_to_low` / `cast_to_f32` nodes |

Ordinary `input`, `unary`, `binary`, `reduce`, `mean` and `matmul` retain their
f32-only contracts. Typed methods check the node dtype; binary low operations,
selection and matmul require matching low formats. Views retain dtype, offsets
and strides. Copies and low selection preserve raw payloads. Strided reshape
and matmul materialize in the original storage format.

Each low unary/binary node evaluates in f32 and rounds once to its own low
result; later nodes consume that rounded result. Low reductions decode native
u16 loads directly into an f32 hierarchy, and low GEMM passes native16 inputs
to cuBLAS with f32 accumulation/output. Neither needs whole-input f32 copies.
`reduce_low_f32`, `mean_low_f32` and `matmul_low_f32` retain that f32 result;
the versions without `_f32` add one final low cast. These operations preserve
the eager numerical contracts, including finite subnormal extrema and signed
zero rules; source compilation does not establish NVIDIA numerical behavior.

#### Resident indexing and count composition

`scan` / `scan_u32` keep their accumulator dtype. `scan_low_f32` loads native
u16 directly into an f32 hierarchy, and `scan_low` adds one final low cast.
Every scan supports arbitrary axes and forward/reverse, inclusive/exclusive
traversal. `gather`, `compact` and `scatter` have `_u32` and `_low` variants;
`scatter_low_f32` retains the completed f32 result for every scatter mode.

Gather/scatter return `Gathered`/`Scattered<CudaValue, CudaValue>` with a scalar
u32 `invalid_count`; compaction returns `Compacted` with a scalar u32 `count`.
Those values can feed later recorded nodes without a readback. Invalid indices
are counted once per logical index, even when another dimension makes the
values output empty. Gather zero-fills invalid reads. Stable compaction retains
fixed `[input.numel()]` capacity and a zero tail. Replace scatter selects the
last logical index on duplicate destinations.

This pipeline uses both resident counts as indices and reuses all outputs when
the mask changes:

```rust,no_run
use compute_cuda::{CudaPrepareOptions, CudaRuntime};
use compute_cuda::tensor_core::{
    ScanOptions, ScatterOp, Shape, TensorBackend, TensorIndexBackend,
};

let cuda = CudaRuntime::new()?;
let shape = Shape::new(vec![4])?;
let x = cuda.upload_f32(shape.clone(), &[10., 20., 30., 40.])?;
let indices = cuda.upload_u32(shape.clone(), &[3, 1, 99, 0])?;
let mut mask = cuda.upload_u32(shape, &[1, 1, 0, 1])?;
let mut builder = cuda.program();
let xv = builder.input(x.layout().clone())?;
let iv = builder.input_u32(indices.layout().clone())?;
let mv = builder.input_u32(mask.layout().clone())?;
let gathered = builder.gather(xv, iv, 0)?;
let prefix = builder.scan(gathered.values, 0, ScanOptions::default())?;
let packed = builder.compact(prefix, mv)?;
let picked = builder.gather(packed.values, gathered.invalid_count, 0)?;
let scattered = builder.scatter(xv, packed.count, picked.values, ScatterOp::Replace, 0)?;
let mut program = builder.prepare(
    &[scattered.values, packed.values, packed.count, gathered.invalid_count],
    CudaPrepareOptions::default(),
)?;
let mut outputs = program.run_typed(&[(&x).into(), (&indices).into(), (&mask).into()])?;
cuda.write_u32(&mut mask, &[0, 1, 0, 0])?;
program.run_typed_into(
    &[(&x).into(), (&indices).into(), (&mask).into()],
    &mut outputs.iter_mut().map(|o| o.as_output_mut()).collect::<Vec<_>>(),
)?;
program.synchronize()?;
assert_eq!(cuda.read_f32(outputs[0].as_f32()?)?, [10., 0., 30., 40.]);
assert_eq!(cuda.read_f32(outputs[1].as_f32()?)?, [60., 0., 0., 0.]);
assert_eq!(cuda.read_u32(outputs[2].as_u32()?)?, [1]);
assert_eq!(cuda.read_u32(outputs[3].as_u32()?)?, [1]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Every replay clears invalid counters, compaction capacity/count and Replace
owner tables before dependent passes, and initializes scatter destinations
from the current base. Low gather/compact and raw Replace preserve payloads.
Low Min/Max preserve finite subnormals and signed-zero ties. Low Add/Multiply
scatter accumulates directly loaded u16 updates in an f32 result, then rounds
once for low output. Raw low scatter uses a padded internal allocation so its
32-bit CAS never crosses an odd native16 tail; terminal output copies retain
the normal unpadded logical layout.

Input dtype, shape, strides and offset are fixed. Each run may supply new
allocations and values with that signature. `run_typed_into` checks every
runtime owner, dtype, layout, span and alias before enqueueing: outputs must
have the declared dtype/shape, dense zero-offset layout and uniquely owned
storage, independent of inputs and other outputs. Read-only input aliases are
allowed. Invalid bindings and current precision-policy rejection leave outputs
untouched and allow a corrected retry. Every requested matmul policy is checked
again before each run, including empty or K=0 operations. Native low matmul
also rechecks format/hardware/cuBLAS eligibility. `NVIDIA_TF32_OVERRIDE=0` cannot
silently retain a previously permitted TF32 plan.

`CudaProgramInput`, `CudaProgramOutput` and `CudaProgramOutputMut` distinguish
f32, u32 and `CudaLowTensor`; the low wrapper retains its F16/BF16 dtype.
`run_typed` allocates fresh independent outputs, whose checked `as_f32`,
`as_u32` and `as_low` accessors expose the matching tensor type. The legacy
`run`/`run_into` accept only f32 input **and output** signatures, while allowing
typed intermediate nodes. They reject other signatures before enqueueing.

`run_typed_into` and its f32 wrapper perform no adapter device-tensor allocation,
metadata upload, NVRTC compilation or cuBLAS initialization. They still make
individual kernel/cuBLAS calls and allocate ordinary host argument bookkeeping.
cuBLAS internal memory behavior is outside this guarantee. Duplicate requested
values and terminal inputs/views are copied to separate outputs, so later runs
leave earlier results unchanged. Empty sum/product and K=0 matrix results
explicitly write their identities on every replay.

Preparation expands reduction/scan hierarchies, normalized masks, prefixes,
count/owner storage, materializations, terminal copies and GEMM geometry before
checking the complete scratch/metadata budget. Scratch uses
four bytes per f32/u32 element and two per f16/bf16 element; empty allocations
reserve one element of their own dtype. Low reduction partials/results are f32,
and final low casts reserve their own output. Active raw low scatter scratch
is padded to complete u32 words; these physical bytes are included in the
budget. Metadata includes each uploaded u64 allocation's sentinel. No liveness
pooling is applied. `stats()` reports kernel launches, GEMM calls, explicit
asynchronous `memset_calls` and adapter scratch/metadata bytes. Memset commands
are counted separately from kernels and exclude preparation-time clearing.
Caller outputs, runtime resources and opaque cuBLAS internal allocations are
excluded; these counts are not measured peak memory. Eager and prepared paths
share checked launch descriptors and their kernel/cuBLAS argument implementation.

A prepared program borrows its runtime, owns scratch/metadata, and requires
`&mut self` for replay. It retains no caller tensor clones between runs. An
enqueue failure, enqueue panic or a failure observed by `program.synchronize()` permanently
poisons the program. Outputs can be partially modified after such a failure;
recovery requires a new program and appropriate runtime recovery. There is no
rollback of GPU work.

The builder has no narrow node, generated fusion,
asynchronous readback or scratch pooling.
Externally created narrow views can still supply fixed input layouts.

Host planning, type checks and source review are qualified separately from
native execution. See the [prepared indexing qualification](../../docs/qualification/tensor-cuda-index-programs-2026-09-27.md),
the [typed prepared-program qualification](../../docs/qualification/tensor-cuda-typed-programs-2026-09-27.md)
and the [earlier f32 qualification](../../docs/qualification/tensor-cuda-programs-2026-09-27.md).
The examples compile as `no_run` doctests; NVIDIA execution, performance and
Tensor Core instruction use remain unverified on this Apple host.

### CUDA Graph programs

`CudaPreparedProgram::capture_owned` consumes a prepared program, rebuilds its
logical views against dense private input slots, then captures that schedule.
It supports the same typed operations, including indexing counts, statistics,
attention and cuBLAS GEMM. NVIDIA execution is still unqualified on this host;
see the [graph qualification](../../docs/qualification/tensor-cuda-graphs-2026-09-27.md).

```rust,no_run
use compute_cuda::{CudaRuntime, CudaPrepareOptions, CudaCaptureOptions};
use compute_cuda::tensor_core::{Layout, Shape, TensorBackend, UnaryOp};

let cuda = CudaRuntime::new()?;
let shape = Shape::new(vec![4])?;
let mut builder = cuda.program();
let x = builder.input(Layout::contiguous(shape.clone())?)?;
let square = builder.unary(x, UnaryOp::Square)?;
let total = builder.sum_axes(square, &[0], false)?;
let prepared = builder.prepare(&[square, total], CudaPrepareOptions::default())?;
let mut graph = prepared.capture_owned(CudaCaptureOptions::default())?;
let input = cuda.upload_f32(shape, &[1., 2., 3., 4.])?;
let result = graph.run(&[&input])?;
assert_eq!(cuda.read_f32(&result[1])?, vec![30.]);
graph.synchronize()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`run`/`run_into` require f32 input/output signatures. `run_typed` and
`run_typed_into` use the existing typed transport and exact external layouts.
All bindings, alias restrictions and current precision policies are validated
before any work is enqueued. The first transfer materializes each current
resident input into a dense slot, the graph runs, and the last transfer writes
independent caller-owned outputs. Slots are never returned as clonable tensors.
Input/output copying remains part of each replay's cost.

The graph retains a private nonblocking stream in the same primary CUDA context.
A fresh Rust context wrapper disables slice event tracking only for private
allocations; the original runtime keeps its tracking and stream unchanged.
Two preallocated events order input copies → graph → output copies. Copies use
the caller's ordinary stream access guards. Custom nonprimary contexts are
rejected. `CudaRuntime::new_with_stream(CudaStreamMode::NonBlocking)` separately
selects an explicit ordinary runtime stream; `new()` retains its previous default.

Input view provenance preserves permute/reshape/broadcast semantics after dense
rebasing. Only required remap materializations are inserted; ordinary prepared
execution keeps its original plan. No allocation is sized by a sparse external
input's physical span. Shape, storage and resource budgets are checked before
new buffers are created. Graph-owned cuBLAS handles have a checked 256-byte-aligned
workspace, configured after their stream, with host alpha/beta constants.

`CudaCaptureOptions` contains preparation budgets, `max_owned_bytes` and optional
`cublas_workspace_bytes`. The total includes dense slots and empty sentinels,
scratch padding, captured and transfer metadata, and workspace alignment padding.
Driver graph/module/library bookkeeping is excluded. `stats()` reports these
bytes and the additional input/output copy launches; these are allocation
contracts, not measured peak memory. `run_typed_into` creates no adapter device
buffers or events, uploads no metadata, and performs one graph launch.

Warmup and graph upload finish before capture preparation returns. The program
owns modules, all captured buffers, metadata and library resources until replay
completion. Drop synchronizes both streams and destroys graph handles first.
If completion cannot be established after a driver error, it retains the private
owners rather than free potentially referenced storage. Enqueue/synchronize
failures poison replay; validation failures leave the graph reusable. Raw graph
access is serialized, and the wrapper is neither `Send` nor `Sync`.

The [standalone graph example](examples/graph.rs) replays changed resident input.
Graph capture does not establish a speedup or Tensor Core instruction selection.
Native capture/replay, failure recovery and Compute Sanitizer remain required
qualification on NVIDIA hardware.

### Reductions and vectors

Import `tensor_core::TensorReduceBackend` for `reduce_f32`, `reduce_u32` and
`mean_axes`. `ReduceOp` selects sum, product, min or max. Empty axes preserve the
input's logical values for every operation. An empty contraction yields zero
for sum and one for product; min, max and mean return `EmptyReduction` when a
zero-length contracted axis would produce a nonempty result. Empty outputs are
valid for every operation. u32 sum and product wrap modulo 2^32.

Full reductions retain a bounded grid and recursively fold device partials.
Partial-axis reductions assign a block to each output. Padding lanes use the
operation's identity, so positive minima and negative maxima are preserved.
Mean uses the logical contraction count, including broadcast dimensions, and
performs direct f32 division on the device. Parallel f32 arithmetic and count
conversion can round; inputs and intermediate values must remain in the
operation's representable domain.

Matmul accepts operands of rank at least one. Two vectors return a scalar,
matrix-vector products remove the trailing singleton output axis, and
vector-matrix products remove the inserted row axis. Leading batches broadcast
for either vector case. Zero-length vector dot products return scalar zero;
scalar operands remain invalid. Precision policy and cuBLAS dimension limits
are unchanged.

### Reusable statistics and attention

See the [CUDA/MLX statistics and attention qualification](../../docs/qualification/tensor-prepared-statistics-2026-09-27.md) for current checks and hardware limits.

The builder records `softmax`, `log_softmax`, `logsumexp`, population `moments`,
`layer_norm` and `attention`. Low inputs expose `_low_f32` results and `_low`
results with one final cast. Attention keeps f32 additive/u32 keep masks,
grouped query heads, broadcast batches and signed causal offsets.

CUDA retains f64 row summaries/partials and the attention accumulator during
preparation. All physical bytes count against the scratch limit; f64 is internal
kernel state in prepared programs. Public eager f64 tensors use the separate
`TensorF64Backend` contract below. Singleton and zero-key results are
initialized on every replay. The ordinary eager API uses the same launch helpers.

```rust,no_run
use compute_cuda::{CudaPrepareOptions, CudaRuntime};
use tensor_core::{AttentionMask, AttentionOptions, Layout, Shape, TensorBackend};

let cuda = CudaRuntime::new()?;
let shape = Shape::new(vec![2, 2])?;
let mut graph = cuda.program();
let x = graph.input(Layout::contiguous(shape.clone())?)?;
let probabilities = graph.softmax(x, &[1])?;
let attended = graph.attention(probabilities, probabilities, x,
    AttentionMask::None, AttentionOptions::default())?;
let normalized = graph.layer_norm(attended, &[1], 1e-5)?;
let moments = graph.moments(attended, &[1], false)?;
let mut program = graph.prepare(&[normalized, moments.mean, moments.variance],
    CudaPrepareOptions::default())?;
let input = cuda.upload_f32(shape, &[1., 2., 3., 4.])?;
let outputs = program.run(&[&input])?;
program.synchronize()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Stable statistics and normalization

Import `tensor_core::TensorStatsBackend` for `softmax`, `log_softmax`,
`logsumexp`, `moments` and `layer_norm`. Axis lists may be unsorted and can cover
any subset of dimensions. Strided, offset and broadcast views are read directly.
Normalized outputs keep the input shape; moments/logsumexp support `keep_dims`.

Softmax subtracts each group's maximum before `expf`, then sums those bounded
exponentials in f64. Log-softmax computes `(x - maximum) - log(sum)` so a large
common offset cannot erase the log normalization. Logsumexp adds the maximum
back only for the reduced result. Extremely negative log probabilities may
round to negative infinity; softmax probabilities follow f32 underflow limits.

For moments and layer norm, a resident first value anchors each group. The first
pass sums `double(x) - anchor`; the second sums squared deviations about that
centered mean. f64 can represent the differences, squares and sums of every
finite f32 input over the backend's addressable count range. This avoids both
raw squared-moment cancellation and intermediate f32 overflow. Variance uses
population division by N. Layer norm divides in f64 before rounding its output,
so an overflowing reported f32 variance does not destroy normalized values.
The FP64 cost varies by NVIDIA architecture and is currently unmeasured.

Non-singleton operations use two partial-reduction/merge pairs and one output
pass. Scratch contains two f64 values per row and bounded row/chunk partials;
there are no input-sized expanded exponential or deviation arrays. All passes
share the retained CUDA stream, with no host reads between them.

An empty axis list means singleton groups: softmax returns ones, log-softmax,
variance and layer norm return zeros, and mean/logsumexp retain the input values.
Empty normalized outputs are valid; nonempty reduced outputs reject empty
contractions. Epsilon must be finite and strictly positive, including positive
subnormals. Constant groups normalize to zero. Inputs must be finite; NaN and
infinity input semantics are outside the shared contract. Gamma/beta compose
through existing resident binary operations.

### Native low statistics and normalization

Import `tensor_core::TensorLowStatsBackend` for `softmax_low_f32`,
`log_softmax_low_f32`, `logsumexp_low_f32`, `moments_low_f32` and
`layer_norm_low_f32`. The corresponding methods without `_f32` cast each result
once to the input's low dtype. Mean and variance retain separate outputs.

Native u16 values decode in registers and widen directly to f64. Three loader
specializations share the original statistics traversal, centered arithmetic,
chunk planner, merge and final output logic. No complete f32 copy of the input,
shifted values or deviations is allocated. Scratch holds two f64 summaries per
row and bounded row/chunk partials; f32 result allocations and their final low
casts are explicit. The f64 cost remains unmeasured on NVIDIA.

The f32 statistics shape, axis, finite-input and epsilon contracts apply.
Singleton mean/logsumexp decode directly into the actual result, preserving
finite subnormals and signed zero; their low outputs retain the same finite
bits. Singleton softmax is one; log-softmax, variance and layer norm are zero.
Final low results may overflow or underflow even when the retained f32 result
is finite. Finite BF16 subnormal inputs widen before centered arithmetic, so
layer norm can retain normal-sized results even for tiny input deviations.

### Online attention

Import `tensor_core::{TensorAttentionBackend, AttentionMask, AttentionOptions}`.
`attention` accepts `[L, D]` matrices or `[..., H, L, D]` tensors. Leading
batches broadcast; each consecutive group of query heads shares a K/V head.
Strided, transposed, offset and broadcast inputs are addressed directly.
Masks stay resident and broadcast to `[..., Hq, Lq, Lk]`: `Keep` accepts any
nonzero u32; `Additive` adds finite f32 bias or excludes a key with negative
infinity. `causal: Some(offset)` permits key j when `j <= i + offset`, including
negative offsets. Fully masked rows and empty key sequences produce zero.
Empty query, batch or value-depth dimensions produce empty outputs. Heads and
query/key depth must remain positive. Scale defaults to `1/sqrt(D)`; any finite
explicit scale, including zero or negative values, is valid.

One 256-thread block owns each query row. Eight warps compute its 32-key tile's
dot products once, then every value component reuses the shared weights. The
running maximum, exponential denominator and weighted output are updated online,
following the tiled recurrence described in [FlashAttention](https://arxiv.org/abs/2205.14135).
No Lq×Lk score/probability allocation is made. Temporary global storage is one
f64 value per output component (eight bytes); shared state is fixed-size.
The arbitrary value-depth loop reuses those same scores, including Dv > 256.
This implementation does not use Tensor Cores or claim FlashAttention's speed.

Dot products, exponential weights, denominators and weighted values use f64;
the final output rounds to f32. Wider exponentials avoid prematurely losing
a tiny weight before multiplying a large V. The portable contract still requires
finite inputs and representable f32 allowed logits/weighted arithmetic; NaN and
positive-infinite additive masks are outside it. CUDA FP64/transcendental costs
and attention execution remain unmeasured without NVIDIA hardware.

### Native low attention

Import `tensor_core::TensorLowAttentionBackend` for `attention_low_f32` and
`attention_low`. Q/K/V must have the same dtype, including when the result is
empty. The shared `low_attention_plan` validates that contract before any
allocation. Keep masks remain u32 and additive masks remain f32, preserving
mask biases independently of the Q/K/V format.

A native-u16 loader specializes the existing 32-key tiled kernel. Each Q/K/V
value decodes directly into a register and widens to f64 before multiplication
or weighted accumulation. This preserves a normal product formed by a BF16
subnormal operand and a large finite partner, in either Q/K order. No complete
Q/K/V conversion or score/probability matrix is allocated. The existing
output-sized f64 scratch, bounded row grid, GQA mapping and mask handling are
shared with f32 attention.

The `_f32` result retains the evaluated output; the low result rounds it once
to Q's dtype. Geometry, finite input/logit/result domain, causal offsets,
empty/fully masked rows and underflow limits follow the shared f32 attention
contract. Constant finite V is accumulated in f64, preventing an overflowing
unnormalized f32 numerator. This path does not use Tensor Cores and has no
NVIDIA execution or performance qualification on this host.

### Resident indexing

`CudaTensor<T = f32>` preserves the original f32 API and uses the same checked
layout and allocation owner for u32. The indexing extension imports
`tensor_core::TensorIndexBackend`; `ScanOptions::default()` is an inclusive
forward scan. Reverse scan changes traversal direction while keeping output
coordinates in their original order.

Scans use work-efficient 256-lane Blelloch blocks. Each block writes a chunk
total; recursive scans compute chunk offsets, then a parallel pass adds the
offsets to the output. This handles long axes and multiple strided rows without
a single thread scanning the entire input. Compaction normalizes the mask to
zero/one, scans those flags, and scatters selected values into fresh zeroed
storage. These algorithms have not yet been timed or executed on NVIDIA here.

Gather output shape is `input[..axis] + indices.shape + input[axis+1..]`.
Each invalid logical index contributes once to `invalid_count`, including when
another dimension makes the output empty. Invalid indices are checked before
any input address is accessed. Scalar indices remove the gathered axis.

Both gather `invalid_count` and compaction `count` have scalar shape `[]` and
stay on the device. Compaction output shape is `[input.numel()]`; only its prefix
is selected data. The mask must broadcast to the input without enlarging it.
Gather index count and compaction input count must fit u32, so neither count
can silently wrap. Scan arithmetic itself deliberately wraps for u32.

### Resident scatter

Import `tensor_core::TensorScatterBackend` for `scatter_f32` and `scatter_u32`.
The result contains a fresh copy of the base with the same shape and a scalar
`invalid_count`. Updates broadcast to the corresponding gather output shape.
Inputs and their views remain unchanged, including when the base and updates
share storage. Invalid indices are ignored and counted once per logical index,
even when a separate zero dimension makes the output empty. The number of
logical indices must fit u32.

`ScatterOp::Replace` chooses the greatest logical position in the index tensor
when indices repeat. A first pass elects this position with integer atomic max;
only its update writes each destination in the second pass. The owner buffer
uses one u32 per target-axis element. Add, multiply, min and max include the
base value and all valid updates. u32 add and multiply wrap modulo 2^32.

The f32 folds use integer compare-and-swap loops around precise float arithmetic.
This includes add: CUDA's global-memory float atomic add flushes subnormal inputs
and results, while the custom kernels specify `ftz=false`. See NVIDIA's
[PTX atomic-instruction semantics](https://docs.nvidia.com/cuda/archive/12.8.0/parallel-thread-execution/index.html#parallel-synchronization-and-communication-instructions-atom).
These folds require finite inputs and intermediates; their accumulation order
can vary. An exact subnormal fixture is included in the mandatory NVIDIA test,
but has not been executed on this development host.

The passes visit base elements, indices and logical updates directly; no
per-output scan through all indices is used. Highly repeated destinations
serialize atomic updates and can require CAS retries. No scatter performance
claim is made before NVIDIA measurements.

### Native f16/bf16 storage

Import `tensor_core::TensorLowBackend`. `CudaLowTensor` contains a genuine
`CudaSlice<u16>` tagged with `LowDtype::F16` or `LowDtype::Bf16`; no f32 shadow
allocation backs it. `storage_bytes()` reports the shared allocation size:
two bytes per element, including odd counts. Empty tensors use a two-byte
sentinel. Views share the allocation; `narrow_low` provides checked offset views.

`upload_low` and `read_low_bits` transfer raw u16 values unchanged. Materializing
strided views copies their bits on the GPU, preserving NaN payloads and signed
zero. `cast_to_low` / `cast_to_f32` execute on the GPU, use round-to-nearest,
ties-to-even, preserve representable subnormals and signed zero, and overflow
to signed infinity. Casts retain NaN classification, without promising payloads.
F16 uses PTX conversion instructions; BF16 uses integer rounding and exact bit
expansion. No CUDA toolkit headers are needed at runtime. BF16 storage and casts
also work on devices too old for BF16 GEMM.

`matmul_low_f32` sends the native two-byte input pointers directly to
`cublasGemmEx` with `CUDA_R_16F` or `CUDA_R_16BF`, `CUBLAS_COMPUTE_32F` and f32
output. Strided inputs materialize in their original two-byte dtype. Inputs
must have the same dtype; vector, batch and empty rules match the f32 API.
`matmul_low` uses that same f32 result and one GPU cast to the input dtype,
so the low output is rounded once after accumulation. It allocates a temporary
f32 **result**, and never expands either whole input to f32. These combinations
are specified by the [cuBLAS GEMM type table](https://docs.nvidia.com/cuda/archive/12.8.0/cublas/index.html#cublasgemmex).

`low_precision_support` reports `LowStorage::Native16` and product availability
per dtype: this implementation requires cuBLAS and compute capability 5.0 for
F16, or 8.0 for BF16. Unsupported products return an explicit error. A supported
product permits Tensor Core use where hardware and cuBLAS choose it; no specific
instruction is guaranteed. The existing f32 `MatmulPrecision::Allow*` policies
remain independent: they permit lower multiplication precision for f32 storage.

The existing `math-core` CUDA path separately covers nearest-neighbor variants,
distances, sums, transformed error, Chamfer, bounds, moments and raw point-cloud
statistics. Its synchronous host finalization and recorded WGSL domain plans
have not been migrated into this crate. Stable covariance remains a WGSL API.

### Native low-precision arithmetic and reductions

Import `tensor_core::TensorLowOpsBackend` for `unary_low`, `binary_low`,
`reduce_low_f32`, `mean_low_f32`, `reduce_low` and `mean_low`. Binary operands
must have the same low dtype; ownership, dtype and broadcast checks precede
allocation. Unary/binary outputs retain that dtype. Arithmetic evaluates in f32
and rounds once to native u16 storage; transcendental tolerances and valid
function domains follow f32. A finite f32 result can overflow the low format.

Negate and Abs edit the raw sign bit, preserving finite subnormals and signed
zero exactly. Binary Min/Max select an original u16 value through ordered IEEE
bits. Min chooses -0 over +0 and Max chooses +0 over -0. Reductions use this
same bit ordering on decoded f32 values and every subsequent partial pass,
so BF16 subnormal extrema survive without floating-point comparison/arithmetic.
The shared f32 reduction kernel also uses these signed-zero extrema rules.

Low reductions reuse the ordinary axis traversal, block tree and hierarchy.
A load helper decodes each native u16 element into a register. Full reductions
write bounded f32 partials, then reuse the existing f32 reducer. Partial-axis
reductions write the requested output directly.
There is no whole-input f32 conversion temporary. Empty-axis f32 results are
the requested decoded output itself. Sum/product accumulate in f32; mean
divides that sum by the logical contraction count. Inputs and intermediate
arithmetic must remain in f32 range; arithmetic underflow is permitted.

`reduce_low_f32` and `mean_low_f32` preserve their f32 results. `reduce_low` and
`mean_low` use those same paths followed by exactly one GPU cast. Empty outputs
stay empty, empty contractions return zero for sum and one for product, and
min/max/mean reject empty contractions with nonempty output. Empty axes retain
logical values. No data is read back between stages.

### Native low-precision indexing

Import `tensor_core::TensorLowIndexBackend` for `compare_low`, `select_low`,
`gather_low`, `compact_low`, `scan_low_f32` and `scan_low`. Compare and select
reject mixed low dtypes before allocating. Comparison handles every raw low
pattern with integer classification: signed zeros compare equal, infinities
are ordered, and either NaN makes only NotEqual true. This also preserves exact
ordering for BF16 subnormals without floating-point arithmetic.

Select, gather and compact reuse the typed traversal with native u16 loads and
stores. They preserve NaN payloads, signed zeros and subnormal bits. Every
nonzero u32 mask is true. Gather counts each invalid logical index once, even
with repeated output copies or an empty output; invalid elements become +0.
Compaction preserves logical order in a fixed `[input.numel()]` capacity, with
a +0 tail and a resident u32 scalar count. Views may be strided or offset.

Scan decodes low values only at the first block's register load. Its block
tree, recursive totals and carry addition all use f32. `scan_low_f32` returns
those completed prefixes; `scan_low` adds exactly one final low-format cast.
There is no full-input f32 conversion temporary or host intermediate. All four
inclusive/exclusive and forward/reverse modes preserve the input coordinate
order. Empty tensors remain empty after axis validation; scalars have no scan
axis. Inputs and intermediate sums must stay finite in f32; parallel ordering
and permitted arithmetic underflow follow the shared scan contract.

### Native low-precision scatter

Import `tensor_core::TensorLowScatterBackend` for `scatter_low` and
`scatter_low_f32`. Base and updates must have the same low dtype. Axis/index
expansion, broadcast updates and resident invalid counts match typed scatter.
Each invalid logical index is counted once even when the output is empty.
Inputs stay unchanged, including when base and updates share storage.

For low output, Replace preserves every raw payload and selects the greatest
logical index on duplicates. Finite Min/Max compare ordered raw bits, preserving
subnormals and choosing -0 for Min / +0 for Max. A fresh native u16 base copy is
padded to an even physical element count. A 32-bit CAS updates one halfword
while preserving its neighbor, including the odd logical tail. This uses no
floating-point atomic instruction or newer half-atomic capability.

Add/Multiply decode the base into the result-shaped f32 accumulator and read
native u16 updates directly inside an f32 CAS loop. A low result is rounded once
after all updates; no full f32 update tensor is created. `scatter_low_f32`
retains the f32 result for every mode, and therefore never widens an already
low-rounded accumulation. Its Replace follows cast semantics for NaNs; Min/Max
use ordered f32 bits for exact widened subnormals and signed-zero ties.
Arithmetic inputs and intermediate values must stay finite; parallel order and
underflow follow the f32 contract. No CUDA execution or speedup is claimed here.

## Precision policy and Tensor Cores

| Policy | cuBLAS compute type | Minimum compute capability checked here |
| --- | --- | --- |
| `F32` | `CUBLAS_COMPUTE_32F_PEDANTIC` | No Tensor Core requirement |
| `AllowTf32` | `CUBLAS_COMPUTE_32F_FAST_TF32` | 8.0 |
| `AllowF16` | `CUBLAS_COMPUTE_32F_FAST_16F` | 7.0 |
| `AllowBf16` | `CUBLAS_COMPUTE_32F_FAST_16BF` | 8.0 |

Strict F32 does not silently downcast inputs. The other policies permit
reduced-precision multiplication with f32 output. They permit Tensor Cores;
cuBLAS still chooses the actual kernel. Instruction-level profiling on the
target device is needed to prove Tensor Core execution. Unsupported policies
return an error. `NVIDIA_TF32_OVERRIDE=0` also rejects the requested TF32 policy.
These choices follow the [cuBLAS compute-type contract](https://docs.nvidia.com/cuda/archive/12.8.0/cublas/index.html#cublascomputetype-t).

FP16 permits a narrower exponent range than f32; BF16 and TF32 lose mantissa
bits. Cancellation, large values and underflow can magnify errors. The caller
owns the decision to allow a lower precision. No universal relative error bound
is promised. Summation uses f32 and a parallel order, so CPU f64 references need
appropriate tolerances. Finite inputs and valid mathematical domains follow
the shared contract; matching NaN payloads across devices is not promised.

Each nonempty cuBLAS matrix dimension must fit `i32`; checked shapes and batch
offsets must fit `usize`. Logical byte size uses the actual scalar width and must
fit `usize`, including zero-stride broadcast views. A low view can fit the two-byte
limit while widening it to f32 correctly returns an overflow error.
Empty and inner-dimension-zero matmul returns a
correctly shaped zero result without a GEMM launch.

## Qualification

CPU-only builds exercise the policy, row-major GEMM mapping, broadcast batch
offsets, strided scan/gather address mapping and logical size limits. The CUDA
integration test runs the same `tensor-core::conformance::{check_backend,
check_index_backend, check_reduce_backend, check_scatter_backend, check_low_backend,
check_low_ops_backend, check_low_index_backend, check_low_scatter_backend,
check_stats_backend, check_low_stats_backend, check_attention_backend,
check_low_attention_backend}`
suites as the other backends, then adds large/odd
reductions, offset views, signed zero, resident updates, alias rejection,
foreign ownership, strict-F32 mantissa preservation and bounded error fixtures
for each hardware-supported precision mode. Indexing fixtures include deep f32
and wrapping u32 scans, every traversal mode, exact u32 extremes, strided views,
broadcast masks, a million gather indices, repeated mask updates and zero tails.
Reduction fixtures cover all axis subsets, keepdims, empty identities/errors,
strided inputs, wrapping integers and hierarchical tails. Matmul fixtures cover
vector promotion, batched vectors, zero contraction and scalar rejection.
Scatter fixtures cover every operation, strided views, broadcast updates,
empty shapes, duplicate winners across blocks, wrapping integers, unchanged
aliases and resident scatter/gather/scan composition. CUDA-specific fixtures add
repeated contended f32 folds, exact subnormal addition and foreign ownership.
Low-precision fixtures cover all 65,536 raw patterns, exact finite round trips,
every rounding boundary and its adjacent f32 values, NaN classification, strided
copies, mixed-dtype errors and low/f32 output distinctions. CUDA adds actual
allocation byte counts, odd grid-stride tails, two-byte GEMM offsets and widening
overflow checks. These numerical fixtures require the native hardware test.
Statistics fixtures cover prime contraction tails, arbitrary axes, large common
offsets, tiny positive epsilon, overflowing variance and extreme finite inputs.
CUDA adds thousands of transposed offset rows with exact mean/variance checks
and a resident normalization → softmax → sum chain, plus foreign-owner rejection.
Attention fixtures exercise GQA/batch broadcast, masks, causal offsets, empty
rows and resident composition. CUDA adds 67-key/35-depth/263-value-depth tails,
repeated V updates, bounded-grid row reuse and foreign inputs/masks. Metadata
addressing is checked by CPU tests; CUDA arithmetic remains hardware-gated.
Low-operation fixtures check bit-exact signs/extrema, arithmetic after one
rounding, broadcast/strided layouts, all reductions and f32/low result
distinctions. CUDA additionally checks a 131,077-element offset view through
multiple reduction levels, exact widened subnormal extrema, resident
Abs→Min→low output, unchanged backing bits and foreign inputs/mixed dtypes.
Low-indexing fixtures cover IEEE comparisons, raw-bit movement and all prefix
scan modes. CUDA adds nonzero native16 offsets through a 131,077-element scan
hierarchy, transposed raw-payload compaction with changing broadcast masks,
once-per-index invalid counts and foreign-owner rejection.
Low scatter adds 65,539 repeated indices, strided raw base/updates, odd output
padding, input/update aliases and exact contended tiny-value accumulation.
The shared fixture checks every operation and low/f32 result precision.
Low-statistics fixtures check singleton identities, strided/broadcast axes,
shifted exponentials, tiny BF16 deviations and extreme finite variance. CUDA
adds 2,053 transposed offset groups, repeated execution, unchanged source bits,
resident probability reductions, exact signed-zero/subnormal singleton results
and foreign-owner rejection.
Low attention adds offset/transposed Q/K/V and masks, completely excluded first
tiles, 67-key/35-depth/263-value tails, direct tiny-times-large Q/K products,
constant maximum V, bounded-grid reuse and dtype rejection before empty output.
The shared low-attention fixture also checks GQA/broadcasts and final low rounding.

```sh
cargo test --manifest-path crates/Cargo.toml -p compute-cuda -- --nocapture
CUDA_REQUIRED=1 cargo test --manifest-path crates/Cargo.toml -p compute-cuda -- --test-threads=1 --nocapture
```

`COMPUTE_REQUIRE_CUDA=1` is an equivalent hardware gate.
Without `CUDA_REQUIRED=1`, unavailable hardware is printed as **SKIP**; a green
CPU-only run is not CUDA qualification. With the flag, absence of the driver,
device or NVRTC fails the test. Kernel compilation errors and incorrect results
always fail. Unsupported reduced-precision policies are explicitly checked and
reported; their absence is not proof of Tensor Core execution.

Current development host is Apple silicon macOS with no CUDA device. Rust
compilation and CPU contract tests pass here. Actual NVRTC 12.8.93 compiled the
production kernels in an isolated Linux aarch64 container for compute_70/80/90/120;
all 52 entry points and their parameter widths were checked in the
[typed-program compiler snapshot](qualification/nvrtc-12.8.93-linux-aarch64-typed-programs/report.json). See the
[compiler qualification](qualification/README.md) for source hashes, complete
logs and reproduction. CUDA execution, cuBLAS results, numerical checks on
NVIDIA and performance measurements still require the mandatory hardware run
above. No CUDA speedup is claimed.


## Native binary64 tensors

`TensorF64Backend` exposes `CudaTensor<f64>` with double-precision unary/binary
arithmetic, reductions and mean, views, matrix products and explicit evaluation.
`f64_support()` reports `Float64Support::Native`. A missing CUDA/cuBLAS library
returns an error; this path does not convert to f32 or run arithmetic on the CPU.

The cuBLAS path uses `CUDA_R_64F` for both operands/output and double alpha/beta,
with `CUBLAS_COMPUTE_64F_PEDANTIC`. This follows the
[cuBLAS 12.8 compute/scale type contract](https://docs.nvidia.com/cuda/archive/12.8.0/cublas/index.html#cublasgemmex).
The shared GEMM planner validates eight-byte output extents, and typed launch
validation rejects mismatched output/compute modes. No Tensor Core speedup is
claimed for this path. Parallel reductions and transcendental functions do not
promise bit-identical CPU results. Finite intermediates and valid unary domains
are required; sum and sum-based mean can overflow even for finite inputs.

```rust,no_run
use compute_cuda::CudaRuntime;
use tensor_core::{BinaryOp, Shape, TensorF64Backend};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let runtime = CudaRuntime::new()?;
let a = runtime.upload_f64(Shape::new(vec![2])?, &[100_000_000., 100_000_001.])?;
let origin = runtime.upload_f64(Shape::new(vec![])?, &[100_000_000.])?;
let delta = runtime.binary_f64(BinaryOp::Subtract, &a, &origin)?;
assert_eq!(runtime.read_f64(&delta)?, [0., 1.]);
# Ok(()) }
```

`math_core::tensor::TensorMathF64` composes this contract for resident transforms,
bounds, pair distances and centered covariance without narrowing coordinates.
F64 stable statistics, attention, convolution and prepared/graph APIs remain
open. WGSL/MLX binary64 arithmetic remains unsupported.

Run the required hardware fixtures with:

```sh
COMPUTE_REQUIRE_CUDA=1 cargo test --manifest-path crates/Cargo.toml -p compute-cuda --test float64 -- --nocapture
COMPUTE_REQUIRE_CUDA=1 cargo test --manifest-path crates/Cargo.toml -p osv-math-compute --features tensor-cuda --test tensor_f64 -- --nocapture
```

The [qualification report](../../docs/qualification/tensor-float64-2026-09-27.md)
records NVRTC/host evidence separately from the missing NVIDIA execution proof.


### Binary64 indexing, scans and scatter

`TensorF64IndexBackend` provides comparisons, selection with u32 masks, prefix
sums, gather and stable compaction. `TensorF64ScatterBackend` adds Replace, Add,
Multiply, Min and Max. All reuse the ordinary shape, broadcast, offset, count
and ownership contracts. Gather/scatter return resident invalid-index counts;
compaction returns full capacity with a selected prefix and resident count.

The f64 specializations use the existing traversal kernels and scan tree;
every scan total and carry is double precision. Scatter folds use 64-bit CAS,
with double arithmetic and bitwise retry termination. Replace elects the last
logical index deterministically. Arithmetic fold order may vary; inputs and
intermediates must be finite. Min/max preserve `-0 < +0`. Copy, select, gather,
compact and Replace preserve raw binary64 payloads, including NaN payloads.

These APIs currently use eager CUDA execution; they do not add f64 slots to
prepared programs or CUDA Graph. The
[qualification](../../docs/qualification/tensor-float64-indexing-2026-09-27.md)
contains the required-device command and distinguishes compile proof from GPU
execution. Numerical qualification still requires NVIDIA hardware.
