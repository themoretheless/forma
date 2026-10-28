# compute-mlx

Native Rust tensor execution through the MLX-C shared library. The adapter builds a lazy MLX graph on an explicit GPU stream and implements `tensor_core::TensorBackend`, `TensorIndexBackend`, `TensorReduceBackend`, `TensorScatterBackend`, `TensorLowBackend`, `TensorLowOpsBackend`, `TensorLowIndexBackend`, `TensorLowScatterBackend`, `TensorLowStatsBackend`, `TensorLowAttentionBackend`, `TensorStatsBackend` and `TensorAttentionBackend`. There is no Python runtime, subprocess execution, or implicit CPU implementation of an unsupported tensor operation.

## Runtime and loading

### Direct convolution

`tensor_core::{TensorConvBackend, TensorLowConvBackend}` provide grouped
channel-first 1D/2D/3D cross-correlation through `conv`, `conv_low_f32` and
`conv_low`. `ConvOptions` selects explicit asymmetric zero padding, stride,
dilation and groups. A shared custom Metal kernel reads strided f32 or native
f16/BF16 input and weights directly and reduces into f32. The low-result method
casts that result once; no complete expanded operand or im2col tensor is built.
The result remains lazy and resident. This requires the existing custom Metal
ABI; reusable MLX programs do not yet expose convolution. See the
[shared contract and qualification](../../docs/qualification/tensor-convolution-2026-09-27.md).

### Libraries and device

The current adapter requires MLX with its **Metal GPU backend**. It was executed on Apple M4 Max with Homebrew MLX-C **0.6.0_4** and MLX **0.32.1**. The binding follows the MLX-C 0.6 ABI. Builds need Rust and the cached `libloading` dependency; the C/C++ SDK is only needed to install or build the external runtime.

`MlxBackend::new_gpu()` loads MLX-C at runtime. Set `COMPUTE_MLX_LIBRARY` to a trusted, ABI-compatible shared library before creating the first backend. Otherwise, macOS discovery checks the Homebrew ARM prefix, `/usr/local/lib`, then the system loader. Missing libraries, missing symbols, absent Metal support, and native errors return `MlxError`. The native module is excluded on `wasm32`; this is not a browser backend.

Every operation receives the backend's explicit stream. Arrays are immutable Rust handles; clones share the native allocation/graph. MLX retains graph inputs after the original Rust handles are dropped. Readback first requests row-contiguous storage and evaluates the graph, so transpose and broadcast views produce their logical row-major values.

```rust
use compute_mlx::MlxBackend;
use tensor_core::{Shape, MatmulPrecision};

let mlx = MlxBackend::new_gpu()?;
let a = mlx.upload_f32(Shape::new(vec![2, 3])?, &[1., 2., 3., 4., 5., 6.])?;
let b = mlx.upload_f32(Shape::new(vec![3, 2])?, &[7., 8., 9., 10., 11., 12.])?;
let c = mlx.matmul(&a, &b, MatmulPrecision::F32)?;
assert_eq!(mlx.read_f32(&c)?, vec![58., 64., 139., 154.]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The [linear example](examples/linear.rs) chains matmul, broadcast bias, ReLU and axis sum, reading only the final two values.

## Coverage

| Operation | Current support |
| --- | --- |
| Upload/read | f32 and u32, scalar and empty shapes |
| Reshape, permute, broadcast, contiguous materialization | arbitrary checked rank; logical row-major readback |
| Unary | all nine canonical tensor-core operations, f32 |
| Binary | all six canonical operations, multidimensional broadcasting, f32 |
| Compare and select | all six comparisons for f32/u32; exact u32 masks; select accepts any nonzero mask and broadcasts all three inputs |
| Axis reductions | sum/product/min/max for f32/u32; f32 mean; multiple checked axes, keep-dims, empty-axis identity |
| Matmul | vectors, matrices and leading batch broadcast; strided inputs, zero inner dimension, F32 policy |
| Scan | f32/u32, one axis, inclusive/exclusive, forward/reverse; u32 wraps modulo 2^32 |
| Gather | f32/u32 values, resident u32 indices of arbitrary rank; invalid indices zero-fill, scalar device count reports invalid indices once |
| Compaction | stable flattened f32/u32 values, broadcast u32 mask, input-sized capacity with zero tail, scalar device count |
| Scatter | f32/u32 replace/add/multiply/min/max, broadcast updates, duplicate reductions, deterministic last-index replace, scalar invalid count |
| f16/bf16 storage and casts | native two-byte elements; exact raw bits through views; device casts with ties-to-even, signed zeros and subnormals |
| f16/bf16 arithmetic and reductions | direct native 16-bit loads, f32 evaluation/partials, exact finite sign/extrema bits, arbitrary axes and optional final low output |
| f16/bf16 masks, indexing and scan | IEEE comparison over every raw pattern; payload-preserving select/gather/compact; direct low-load f32 prefix scans with optional final low rounding |
| f16/bf16 scatter | deterministic raw replace; exact finite extrema; direct low update loads into f32 add/multiply; low or f32 output |
| f16/bf16 matmul | native low-format inputs and f32 accumulation; native same-format output or direct custom Metal f32 output |
| f16/bf16 statistics | direct low loads for softmax/log-softmax/logsumexp, moments and layer norm; f32 result or one final low cast, arbitrary axes |
| f32 statistics | stable softmax/log-softmax/logsumexp, mean and population variance, layer norm over arbitrary axes; positive subnormal epsilon supported |
| f16/bf16 attention | direct low Q/K/V loads, streaming f32 evaluation, f32 output or one final low cast; the same geometry and masks as f32 attention |
| f32 attention | scaled dot-product attention, GQA, batch broadcast and strided operands; keep/additive masks, signed causal offsets, zero for fully masked rows |
| Compiled resident programs | fixed-shape f32/u32/f16/bf16 signatures, views, casts, arithmetic, reductions, indexing with resident counts, statistics, attention and f32/low matmul through the public native compile API |

Shapes, axes, broadcasting, and matrix dimensions use the shared `tensor-core` contracts. Arrays from another backend instance are rejected, even when both instances use the same physical GPU. `AllowTf32`, `AllowF16`, and `AllowBf16` return an explicit unsupported-precision error. F32 storage or a matmul call does not establish that hardware Tensor Cores were used.

### Compiled resident programs

`MlxBackend::program()` builds a fixed-shape graph for native `mlx_compile`. Each value token carries a checked shape and f32, u32, f16 or bf16 dtype. The builder validates shared shape rules, rejects tokens from another builder and rolls back partially recorded compound operations on failure. `compile` consumes the builder and fixes the ordered input/output signatures. Outputs may include input aliases, duplicates or an empty list.

```rust
use compute_mlx::MlxBackend;
use tensor_core::{BinaryOp, Shape, UnaryOp};

let mlx = MlxBackend::new_gpu()?;
let shape = Shape::new(vec![4])?;
let mut graph = mlx.program();
let x = graph.input(shape.clone())?;
let w = graph.input(shape.clone())?;
let product = graph.binary(x, w, BinaryOp::Multiply)?;
let square = graph.unary(product, UnaryOp::Square)?;
let total = graph.sum_axes(square, &[0], false)?;
let program = graph.compile(&[square, total])?;
let x = mlx.upload_f32(shape.clone(), &[1., 2., 3., 4.])?;
let w = mlx.upload_f32(shape, &[2., 2., 2., 2.])?;
let result = program.run(&[&x, &w])?;
assert_eq!(mlx.read_f32(&result[1])?, vec![120.]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The original `input`/`run` interface remains f32-only. Typed programs declare `input_u32(shape)` or `input_low(dtype, shape)` and execute with `run_typed(&[MlxProgramInput])`. `MlxProgramInput::Tensor` accepts f32/u32 arrays; `Low` accepts the checked `MlxLowTensor` wrapper. Results use `MlxProgramOutput::{Tensor, Low}` with checked `as_tensor`, `as_low`, `into_tensor` and `into_low` accessors. Passing a typed graph to the old `run` interface fails before tracing if any declared input or output is not f32.

Every run accepts fresh resident tensors with the exact declared shapes, dtypes and originating backend context. Views may have different strides on different runs. All input signatures are checked before entering the native callback; rejected calls do not trace. Lazy results retain their inputs/context after the program is dropped. Reshape preserves logical row-major values, including when replay inputs are transposed. Empty reductions, zero contractions and empty-batch products keep the shared identities.

The compiled operation set is explicit:

- f32: all canonical unary/binary and comparison operations, select, sum/product/min/max, mean, matmul, statistics/normalization and attention.
- u32: Add/Subtract/Multiply/Min/Max, comparisons, select, sum/product/min/max. Arithmetic wraps modulo 2^32; integer Divide and implicit promotion are rejected.
- f16/bf16: exact device casts, all canonical unary/binary operations, arbitrary-axis f32 accumulation reductions/mean, and direct low-input matmul with f32 output. Low-result variants apply one final cast. Statistics/normalization and attention support direct low inputs with f32 or final low results. Views preserve dtype and raw storage bits.
- All four dtypes: comparisons, selection, axis scans, gather, fixed-capacity compaction and all five scatter modes. Device counts are ordinary u32 graph values; low scans and scatter also offer direct f32 results.

Low arithmetic, reductions and direct matmul share the eager lowering recipes and Metal sources. Each low arithmetic node writes its own rounded low result before later nodes read it; the compiler must retain that boundary. Compiled `matmul_low` uses the direct f32 result followed by one cast, while eager `matmul_low` keeps its native same-low route. These low arithmetic/reduction/matmul nodes do not allocate full f32 input copies; an explicit `cast_to_f32` still creates its requested f32 result. Casts retain their accessor-only ABI requirements; nonempty custom operations additionally require the complete optional Metal kernel API. Indexing, scan and scatter also share their eager recipes; low movement and Replace preserve raw payloads.

`compile_available()` means the complete optional public compile/closure/vector ABI is present. It does not prove that MLX compilation is enabled, a particular operation fused, or allocations are reused. `compile` constructs a native closure; its first valid `run` traces the graph. `trace_count()` counts callback invocations, not GPU executions. The adapter never changes MLX's global compile mode, default device or default stream. External settings such as `MLX_DISABLE_COMPILE`, native device eligibility or changes to MLX's default-device cache key can disable reuse or trigger another trace; the operations themselves keep this backend's explicit GPU stream. The qualified enabled-mode replay traces once, while an isolated disabled-mode process traces on each fresh run and still computes correct results.

Programs avoid rebuilding their Rust/C operation graph after a reusable trace, but create fresh lazy outputs on each run. They do not promise fixed allocations, graph capture of arbitrary Rust closures, asynchronous readback or a compiled-program cache across backend instances. Native fusion may change f32 rounding and accumulation order, so ordinary numerical tolerances apply. Inputs and outputs stay on the device until an explicit read; the diagnostic count observes tracing only.

The C callback executes declarative operations under the existing native-call lock. Raw array/vector guards clean up within that lock; they do not recursively call backend methods or drop ordinary array wrappers. Panics are contained at the C boundary and callback errors become `MlxError`. The native payload owns plain graph data and borrows prepared native handles. The program keeps kernel/constant owners alive until after its native closure is freed; callback destruction never drops wrappers that would reacquire the lock. Lazy output graphs retain their dependencies and stream context. [The initial unit failure](qualification/compiled-initial-failure.txt) identified MLX's null destination placeholders from `array_new`/`closure_new`; validation now checks populated handles after setters. Integration tests were not reached in that initial attempt. [Native focused qualification](qualification/compiled-focused.txt) covers actual C callback error recovery, aliases, changed values/strides and disabled mode; [complete qualification](qualification/compiled-metal.txt) records the full regression. [Public MLX-C implementation](https://github.com/ml-explore/mlx-c/blob/v0.6.0/mlx/c/compile.cpp), [callback ownership](https://github.com/ml-explore/mlx-c/blob/v0.6.0/mlx/c/closure.cpp), [MLX 0.32.1 compilation/cache implementation](https://github.com/ml-explore/mlx/blob/v0.32.1/mlx/compile.cpp).

The [typed enabled run](qualification/compiled-typed-focused.txt), [typed disabled run](qualification/compiled-typed-disabled.txt) and [full 104-test run](qualification/compiled-typed-metal.txt) qualify typed replay and shared eager lowering. The [source archive manifest](qualification/compiled-typed-source-manifest.json) records the exact inputs to qualification. The [first typed run](qualification/compiled-typed-initial-failure.txt) exposed two host-oracle errors: the matmul witness values did not distinguish f32 from low output, and an empty f64 sum produced negative zero. The corrected fixture supplies an exact half-ULP witness and the specified positive-zero contraction identity; bit comparisons and tolerances were retained. A separate tiny-BF16 Multiply diagnostic found a real pre-existing normal-product loss and led to the protected multiplication fix described below. No typed-program timing claim follows from correctness qualification.

#### Compiled indexing and resident counts

The [runnable indexing example](examples/compiled_indexing.rs) records gather →
scan → compaction → scatter, combines invalid-index counters on the GPU, then
replays with changed values, masks and indices. Only the final outputs are read.
Run it with `cargo run --manifest-path crates/Cargo.toml -p compute-mlx --example compiled_indexing`.

`gather`, `gather_u32` and `gather_low` return values and an `invalid_count` token.
`compact`, `compact_u32` and `compact_low` return input-sized storage, a zero tail
and a scalar `count`. These counts compose with comparisons, selection,
arithmetic and subsequent indexing. Shape signatures stay fixed; compaction
changes its resident count rather than resizing the compiled output.

`scan` and `scan_u32` accept an axis and `ScanOptions`. Low scans decode their
inputs directly, accumulate in f32, and optionally round once to the source dtype.
`scatter`, `scatter_u32`, `scatter_low` and `scatter_low_f32` return values and an
invalid count. Replace selects the last valid logical index deterministically.
Low arithmetic scatter creates freshly zeroed atomic lists on every replay.
Compound recording rolls back both values and counts if validation fails.

The [compiled indexing qualification](../../docs/qualification/tensor-mlx-index-programs-2026-09-27.md)
records native checks with compilation enabled and disabled. Reused tracing
does not establish fixed device allocations or a performance improvement.

#### Compiled statistics and attention

See the [compiled statistics and attention qualification](../../docs/qualification/tensor-prepared-statistics-2026-09-27.md) for current checks and hardware limits.

`softmax`, `log_softmax`, `logsumexp`, `moments`, `layer_norm` and `attention`
share declarative lowering with the eager API. Their low variants retain native
f16/BF16 inputs and apply any requested low cast only after the f32 result.
The callback uses raw C operations under the existing native-call lock; prepared
constants and custom kernels remain owned by the compiled program. Invalid
compound operations roll back every recorded node, including both moments outputs.

```rust,no_run
use compute_mlx::MlxBackend;
use tensor_core::{AttentionMask, AttentionOptions, Shape};

let mlx = MlxBackend::new_gpu()?;
let shape = Shape::new(vec![2, 2])?;
let mut graph = mlx.program();
let x = graph.input(shape.clone())?;
let probabilities = graph.softmax(x, &[1])?;
let attended = graph.attention(probabilities, probabilities, x,
    AttentionMask::None, AttentionOptions::default())?;
let normalized = graph.layer_norm(attended, &[1], 1e-5)?;
let moments = graph.moments(attended, &[1], false)?;
let program = graph.compile(&[normalized, moments.mean, moments.variance])?;
let input = mlx.upload_f32(shape, &[1., 2., 3., 4.])?;
let outputs = program.run(&[&input])?;
mlx.read_f32(&outputs[0])?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Resident indexing

Import `tensor_core::TensorIndexBackend` to call its typed methods. Both associated tensor types use `MlxTensor`, with dtype checks at every trait boundary. The inherent reshape, permute, broadcast, materialize, scan and sum methods also support both dtypes.

Gather replaces the chosen axis with the full index shape. Scalar indices remove the axis. The adapter compares indices on the device, replaces invalid ones with zero before native `take`, and zeroes their resulting values. A zero-length source axis bypasses `take` and creates a native zero array. `invalid_count` counts original index elements once, even when output dimensions repeat each index or the result is empty. The older `gather_axis` convenience method still accepts a host index vector and returns an error for an invalid index.

Compaction normalizes the broadcast mask and computes an exclusive u32 prefix sum. For logical element `i`, let `p` be the selected count before it and `c` the total selected count. Selected values write to `p`; rejected values write zero to `c + i - p`. Selected destinations cover `[0,c)` and rejected destinations cover `[c,n)`, each exactly once. Native `put_along_axis` therefore receives unique in-bounds destinations, including all-true and all-false masks. The result retains input-sized capacity, with stable selected order and a zero tail. Both counts and all temporary arrays remain in the lazy device graph; there is no intermediate host read or data-dependent host allocation.

MLX dimensions fit i32; flattened compaction also requires `input.numel() <= i32::MAX`. Gather index counts are bounded by the shared u32 count contract. Empty sums create the native zero identity directly, avoiding an unavailable empty-u32 reduction kernel in MLX 0.32.1 Metal.

### Resident scatter

`TensorScatterBackend` scatters updates along one input axis, using the same index expansion as gather. Updates may broadcast to that shape; the returned values retain the base shape and leave the input unchanged. Invalid indices are ignored and counted once per logical index, including when another dimension makes the result empty.

Add, multiply, min and max use native MLX scatter reductions. The adapter reshapes update slices to MLX's index-first layout, sanitizes invalid indices before native access and gives their updates the operation identity. Integer addition and multiplication wrap modulo 2^32; f32 inputs and intermediates must remain finite, and native reduction order may vary.

Replace uses a separate u32 owner array for the target axis. A native integer scatter-max records the greatest one-based logical index position. The adapter then gathers the winning update slice and selects the original base for positions without an owner. Invalid positions contribute owner zero. This makes duplicate replacement deterministic without relying on native overwrite order or scanning all indices for each output element. Owner positions are sanitized before gather. Empty inputs and empty index tensors bypass native scatter/gather while preserving invalid-count semantics.

No intermediate values are read on the host. Replace additionally requires `indices.numel() <= i32::MAX` because the owner positions and winning updates use a flattened MLX dimension; every mode keeps the shared u32 invalid-count limit.

### Reductions and vector products

`TensorReduceBackend::reduce_f32` and `reduce_u32` accept `ReduceOp::{Sum, Product, Min, Max}`. `mean_axes` accepts f32 tensors. Empty axes preserve the input, including scalar shape and dtype. Integer sum/product wrap modulo 2^32; floating-point reductions follow MLX's parallel evaluation order.

An empty contracted dimension produces zeros for sum and ones for product. Min, max and mean return `TensorError::EmptyReduction` when the output would contain values. If the output itself is empty, every reduction returns that empty shape. The adapter applies these rules before native dispatch, avoiding MLX's different handling of empty extrema. Native zeros/ones remain in the lazy graph.

Matmul follows the shared rank-one promotion contract: vector × vector returns a scalar; matrix × vector and vector × matrix remove their promoted singleton dimension; batch dimensions broadcast. MLX performs the promotion internally and the adapter validates its result against the common shape. Two zero-length vectors produce scalar zero. Empty output shapes create native empty arrays directly: MLX 0.32.1 Metal otherwise crashes on empty-batch vector matmul. Precision remains F32 only.

### Native f16/bf16 tensors

Import `tensor_core::TensorLowBackend` for low-format storage and products. Its associated `MlxLowTensor` is a separate immutable wrapper: an f32/u32 array cannot enter its `HasLowDtype` implementation. `upload_low` and `read_low_bits` exchange u16 bit patterns, including every NaN payload and signed zero. Reshape, transpose, broadcast and materialization preserve those bits. MLX stores two bytes per element, with no paired-u32 padding for odd lengths.

`cast_to_low` accepts an f32 tensor; `cast_to_f32` widens a low tensor. Both operate in the lazy device graph. Casts preserve subnormals and signed zeros, round to nearest with ties to even, overflow to signed infinity, and preserve NaN classification. Native f32→bf16 conversion in MLX 0.32.1 Metal flushes f32 subnormals, so the adapter implements bf16 rounding with native integer view/shift/add/mask operations and reinterprets the resulting u16 storage. This correction does not read intermediate data on the host. Native f16 conversion and both widening conversions passed exhaustive boundary checks.

`matmul_low` requires matching dtypes, consumes native half/bfloat arrays and accumulates in f32. The final output is rounded to the input dtype. Vector, batch broadcast, strided and empty shape rules match the common tensor contract. This native MLX operation is unchanged by the direct-f32 implementation below. `low_precision_support` reports `Native16`, `matmul=true` and `matmul_f32=true` on the qualified runtime. Direct f32 support requires both the dtype's host accessor and the optional custom Metal symbol group; otherwise it returns `UnsupportedLowPrecision`. These operations do not change the older f32 tensor `MatmulPrecision` policies or establish use of specific hardware instructions.

```rust
use compute_mlx::MlxBackend;
use tensor_core::{LowDtype, Shape, TensorLowBackend};

let mlx = MlxBackend::new_gpu()?;
let v = mlx.upload_low(LowDtype::F16, Shape::new(vec![3])?, &[0x3c00, 0x4000, 0x4200])?;
let dot = mlx.matmul_low(&v, &v)?; // 1² + 2² + 3² = 14
assert_eq!(mlx.read_low_bits(&dot)?, vec![0x4b00]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

MLX-C exports half data accessors only when its host compiler supports those C types. The loader treats them as optional to keep existing f32/u32 operations usable. If an accessor is absent, upload/cast entry points for that low dtype return `UnsupportedLowPrecision`, and its matmul capability is false. Both accessors were present in the qualified ARM Homebrew runtime.

### Direct low-input matrix products with f32 output

`matmul_low_f32` uses the shared `MatmulPlan` for vectors, matrices and batch broadcasting, then evaluates custom Metal kernels over the original low allocations. Matrix products use 16×16 output/contraction tiles with 128 threads: four SIMD groups each retain an 8×8 float accumulator. Cooperative native f16/bf16 loads fill shared operand tiles, and bounds checks zero-fill partial tiles. All threads participate in every required barrier.

Native SIMD-group float matrix evaluation flushed BF16 subnormal operands in the qualified standalone probe, including products whose correct f32 result is normal. A tile containing any nonzero BF16 subnormal therefore takes a uniform protected path: publish all current float accumulators, fold that tile with integer exponent-balanced products, then reload the accumulators. Ordinary tiles use `simdgroup_multiply_accumulate`. Tests place tiny products in the first, middle and last partial contraction tiles, in both operand orders, with M/N tails. The probe establishes an observed path limitation; it does not identify which hardware instruction causes the flushing.

Canonical `M==1` or `N==1` uses a separate GEMV/dot kernel. Each 256-thread group partitions K for one completed output and performs a f32 tree reduction. BF16 operands use the same protected multiplication. Both kernels decode original batch/output bases once, then step fixed native strides inside K. This supports transposed and zero-stride views without repeated multidimensional index division in the contraction loop.

Leading batch expansion and vector promotion remain metadata views. A right vector becomes a column through singleton broadcast and transpose; an already-matching view is cloned after validation. No operand materialization is needed. Both paths allocate the actual `4*N`-byte f32 result and a 28-byte parameter buffer. Matrix workgroups declare at most 3,104 bytes of shared storage; GEMV groups declare 1,040 bytes. There are no full f32 operand copies or global partial matrices. Native metadata, compiler spills, runtime allocations and readback staging are outside this tensor-storage count. At most 65,535 groups process checked 64-bit work indices, with final barriers protecting scratch before grid reuse. Zero contraction produces zeros; empty results avoid dispatch after ownership, dtype and shape validation.

The result retains the extra f32 bits lost by widening native low-output GEMM. Accumulation order and fused arithmetic may differ across paths. Inputs and accumulated results must remain finite; genuine subnormal arithmetic follows f32 limits. The ordinary native `matmul_low` path is unchanged. Apple SIMD-group operations do not establish NVIDIA Tensor Core execution.

The [initial tiled source and manifest](qualification/low-matmul-tiled-source-manifest.json), [69-test baseline run](qualification/low-matmul-tiled-metal.txt), [SIMD candidate](qualification/low-matmul-simd-source-manifest.json) and [70-test SIMD run](qualification/low-matmul-simd-full-metal.txt) remain preserved. The [combined source manifest](qualification/low-matmul-source-manifest.json), [13 focused checks](qualification/low-matmul-routed-focused.txt) and [full 72-test run](qualification/low-matmul-routed-metal.txt) qualify the combined path. It additionally checks GEMV/vector-matrix/dot at K65/257/1025 with an independent f64 accumulation bound, BF16 tiny products at middle/tail positions, and 65,537 workgroups on both matrix and vector routes. [Matched wall-time measurements](benchmarks/low-matmul.md) include fresh graph construction, allocation, evaluation and full result readback; GPU-only timing is not claimed. The combined candidate changes addressing, GEMV dispatch and identity-view handling together, so its timing cannot isolate one change's contribution.

### Direct low arithmetic and reductions

`TensorLowOpsBackend` uses the optional MLX-C custom Metal kernel ABI. Kernels read native f16/bf16 allocations directly, including permuted and broadcast strides; they do not materialize a whole f32 copy of either input. Unary/binary arithmetic evaluates in f32 registers and rounds once into the input format. Negate and Abs manipulate raw sign bits. Min/Max use integer ordering of the original finite bits, so BF16 subnormals remain distinct, Min selects negative zero, and Max selects positive zero. BF16 output uses integer ties-to-even rounding; f16 output uses Metal's native round-to-nearest conversion. Multiply uses the same protected product helper as direct matmul: a power-of-two transfer between operands preserves normal results involving a BF16 subnormal input. The [initial eager/compiled diagnostic](qualification/compiled-typed-tiny-multiply.txt) returned zero for these normal products; the [corrected diagnostic](qualification/compiled-typed-tiny-multiply-corrected.txt) retains them. Genuine subnormal arithmetic outputs still follow f32 underflow limits.

`reduce_low_f32` and `mean_low_f32` accept arbitrary checked axes. The first pass loads low storage, decodes each element in registers and accumulates in f32. Long contractions use at most 1,024 f32 partials per output element, followed by a f32 fold. Short contractions need only their f32 output. For `R` outputs and `P = min(ceil(K/4096), 1024)` parts, extra partial storage is `4 * R * P` bytes when `P > 1`, plus small shape/launch metadata. No input-sized conversion buffer is allocated. Mean divides the final sum by the logical contraction count; callers must keep f32 intermediate arithmetic in range. Parallel order and f32 arithmetic underflow limits apply. Integer ordering also preserves exact tiny values and zero signs through both extrema passes.

`reduce_low` and `mean_low` use these f32 results followed by one final device cast into the input dtype. The existing BF16 rounding graph may allocate output-sized integer intermediates. Empty axes widen logical values, and low-result variants round-trip their original finite bits. Empty contractions return sum zero/product one; extrema and mean reject a nonempty result. Empty outputs skip custom dispatch.

The [native probe](qualification/low-ops-native-probe.txt) demonstrates why ordinary low MLX operators are insufficient: native BF16 Negate/Abs and extrema flush tiny values, zero signs in native extrema depend on argument order, and low sums round before a later f32 conversion. The same probe executes direct low-load custom kernels and retains the extra f32 sum bits. Its [standalone C source](qualification/low-ops-native-probe.c) requires the external MLX-C SDK only to reproduce the research.

Custom kernel handles are cached by full source, header, input names and atomic-output mode within the backend context. Compiled names include a source hash and process-unique suffix, preventing reuse of a different source under the same MLX library name. Native graph outputs retain inputs after Rust handles are dropped. Kernels use explicit strides and 64-bit offsets, bounded grids, and uniform workgroup barriers. If the optional custom-kernel symbol group is absent, nonempty arithmetic/reduction dispatch returns an explicit error; there is no hidden full-input cast or CPU fallback. Existing f32/u32 operations remain available. MLX omits custom shape/stride arguments for rank-zero inputs, so the adapter presents scalar operands as singleton broadcast views while retaining scalar output shape; the [initial failure](qualification/low-ops-initial-failure.txt) and [corrected full run](qualification/low-ops-metal.txt) preserve that regression. This implementation has no custom kernel performance claim yet.

### Low-format masks, indexing and prefix sums

`TensorLowIndexBackend` compares matching low dtypes without float conversion. Its six comparison operators classify NaNs and compare integer encodings: both zero signs compare equal, infinities remain ordered, and any NaN makes only NotEqual true. Results are exact u32 zero/one masks. Strided operands and trailing-axis broadcasting are supported.

Select routes raw u16 bits through a custom Metal kernel, preserving every NaN payload, signed zero and subnormal. Every nonzero u32 mask means true. Gather and compaction reuse the existing shape, validity-count and destination planners with this raw select. Native MLX `take` and unique-destination `put_along_axis` then move the low storage directly. [The standalone native probe](qualification/low-index-native-probe.c) checked all 65,536 patterns: both moving operations preserved every bit, but BF16 `where` changed 507 payloads, including subnormals and NaNs. [Raw probe output](qualification/low-index-native-probe.txt).

Gather zero-fills invalid indices and counts each original logical index once, including empty value slices. Compaction keeps selected values in stable logical order, returns the full input-sized capacity with a zero tail, and leaves the scalar count on the device. Its existing flattened i32 dimension limit still applies. No routing operation decodes values through floating-point arithmetic.

`scan_low_f32` supports every valid axis, inclusive/exclusive and forward/reverse traversal. A metadata-only permutation moves the scan axis last. For long axes, a custom kernel directly decodes low values into f32 chunk sums; native f32 exclusive cumsum computes carries from these small totals. A second direct-low kernel computes local f32 prefixes, adds carries and writes the completed f32 output. A view restores the original axis order. For axes of at most 256 elements, only that final kernel is needed. `scan_low` rounds each completed prefix once through the existing final device cast; it never scans low-rounded partials.

The implementation reads long-axis inputs twice and allocates no full f32 input conversion or full intermediate prefix array. With `N` input elements, `R` independent rows and `P = ceil(axis_length / 256)`, visible scan storage is the `4*N`-byte f32 output plus two `4*R*P`-byte totals/carry arrays when `P > 1`; MLX's internal cumsum scratch and readback materialization are separate. Low output additionally needs its final low allocation and any output-sized cast temporaries. Arithmetic order and f32 underflow limits apply; inputs and intermediate sums must remain finite.

Empty tensors bypass kernels after axis validation; a scalar has no valid scan axis. Custom Metal scalar arguments are generated as values, so the unused short-scan carry uses shape `[1]` to retain a pointer type. The [initial compilation failure](qualification/low-index-initial-failure.txt) and [corrected complete qualification](qualification/low-index-metal.txt) record this native ABI regression.

### Low-format scatter

`TensorLowScatterBackend` accepts matching low-format base and updates with the common scatter broadcasting and invalid-count rules. `scatter_low` returns the input format; `scatter_low_f32` returns completed f32 values. Replace reuses the resident greatest-owner planner and raw take/select path, so the last logical index wins deterministically and low output preserves every selected payload. F32 Replace widens the completed low result through the cast contract.

Add, multiply, min and max read native low update storage in custom Metal kernels. A first dispatch builds one linked list per target-axis position in a freshly zeroed atomic u32 allocation. Each valid logical index owns one unique, one-based node token. Atomic exchange inserts that token at its destination's head and records the previous head as its next pointer. Exchange order makes each list acyclic; the dependent fold dispatch starts after all node stores finish. Repeated calls receive fresh zero initialization. No input tensor values are read on the host.

Each output element walks only its own destination's list and directly addresses the broadcast/strided updates. Add and multiply accumulate in f32 registers and round once if low output was requested; they never allocate a full f32 update tensor or a separate f32 result before the low store. Min/Max compare the original finite encodings, preserving BF16 subnormals and selecting negative zero for Min and positive zero for Max. Arithmetic order may vary with atomic insertion order; finite-intermediate and f32 underflow limits apply.

For axis extent `A` and `I` logical indices, the list allocation occupies `4 * 256 * ceil((A + I) / 256)` bytes, including at most 255 padding words. The output occupies two or four bytes per base element, alongside the common index-validity/count graph. Work is proportional to the base size, index count and valid expanded updates; each output avoids scanning unrelated indices. Many updates to one destination still form a serial fold for that output. Tokens use the shared u32 count limit, allocation arithmetic is checked, and padded multidimensional storage respects MLX's per-dimension i32 limit. Replace retains the existing flattened `I <= i32::MAX` restriction. Empty bases or indices bypass list dispatch while keeping device invalid counts correct.

The [native atomic probe](qualification/low-scatter-native-probe.txt) visited all 59,580 valid nodes from 65,539 indices without missing nodes, duplicates, cycles or padding changes. Its [standalone C source](qualification/low-scatter-native-probe.c) exercises zero initialization and lazy handle lifetime. The [complete qualification](qualification/low-scatter-metal.txt) covers every raw replacement pattern, finite extrema and signed zeros, contention, one final rounding, strided/broadcast updates, repeated fresh lists, empty shapes and device chains. No MLX low-scatter performance claim has been measured.

### Direct low-format statistics

`TensorLowStatsBackend` provides softmax, log-softmax, logsumexp, population moments and layer norm from native f16/bf16 storage. The `_f32` methods retain their evaluated result; the low-output methods apply the existing exact device cast once at the end. Inputs are finite, statistical/transcendental comparisons use tolerances, and true result underflow/overflow follows the shared f32 contract.

The implementation shares the low-reduction row planner, tree reduction and final f32 partial fold. A metadata-only permutation places kept axes before contracted axes; the output permutation restores the original logical axes without copying input data. Distribution uses a direct-low maximum pass, a direct-low shifted exponential sum, and a final direct-low emission pass. Log-softmax keeps the maximum shift through its subtraction. Logsumexp emits only row results. No full f32 input or shifted/exponential tensor is allocated.

Moments first find exact bit-ordered extrema, build per-row anchor/scale state, then directly reduce scaled values and centered squared deviations. The anchor is `lo/2 + hi/2`; variance is reconstructed as `scale * (scale * variance_scaled)`. Layer norm stays in scaled coordinates, so a reported infinite variance does not make normalized output unusable. Every pass decodes native low storage in registers; neither centered values nor squares occupy an input-sized buffer.

Rows whose greatest magnitude is below `2^-64` first lift their moment coordinates by `2^64`. Integer exponent/significand operations preserve BF16 subnormals before subtraction and undo the lift with nearest-even rounding. Layer norm keeps the lift until forming a bounded scale-to-sqrt-epsilon ratio. This preserves normal outputs around `1e-17` from tiny BF16 inputs with the smallest positive f32 epsilon, instead of flushing the inputs to zero. Distributions retain the original units. Singleton groups, including selected size-one axes, use exact direct casts for mean/logsumexp and the defined one/zero identities for the other outputs, preserving signed zero and subnormal means.

Let `R` be the number of output rows, `K` the contraction size and `P = min(ceil(K/4096), 1024)`. Distribution graphs have two `R`-element f32 summaries and, when `P > 1`, at most two `R*P` partial arrays. Moment/normalization graphs have four `R`-element summaries, a four-float state per row and, when `P > 1`, at most four `R*P` partial arrays. These are logical allocations before native lifetime reuse, plus the actual f32 result(s), scalar/shape metadata and any final low-cast temporaries. They are not peak RSS figures. Partial storage is bounded per row; there is no full input conversion or transformed-input buffer.

[The complete Metal run](qualification/low-statistics-metal.txt) covers all finite singleton encodings, arbitrary axes/strides/broadcasts, BF16 extrema and tiny rows, all epsilon branches, long 131,077-element contractions, and 65,537 independent rows that reuse bounded grids. Shared conformance passed on the first run. A separate long-strided test initially compared a near-zero mean using only relative tolerance: an absolute error of `1.33e-9` failed that test despite valid f32 accuracy at the input coordinate scale. Only that private assertion changed to a scale-based f32 error bound; [the initial failure](qualification/low-statistics-initial-failure.txt) is retained. Probability and variance checks and all shared tolerances were unchanged. No MLX statistics performance claim has been measured.

### Stable statistics and normalization

`TensorStatsBackend` provides f32 softmax, log-softmax, logsumexp, moments and layer norm over any checked axis set. Inputs must be finite; comparisons use f32 tolerances and underflow limits. Every intermediate stays in the lazy device graph. Shape, dtype, ownership and epsilon validation happens before recording the corresponding operation.

Softmax calls native `mlx_softmax_axes` with precise accumulation. Logsumexp uses the native max-shifted reduction, with adapter guards for the shared empty-shape rules. Log-softmax keeps the shifted logits throughout: `(x - max) - logsumexp(x - max)`. This retains `-log(N)` for equal logits near `1e30`, where subtracting an unshifted logsumexp from the input would cancel it.

Moments center and scale each group before taking its mean or squared deviations. The anchor is `lo/2 + hi/2`; the scale is the larger distance from that anchor to either extremum. Zero scale uses a safe unit divisor. The reduced values are bounded, so native mean never sums huge original inputs. The returned population variance is reconstructed as `scale * (scale * scaled_variance)`. Mathematically unrepresentable variance may become positive infinity.

Layer norm remains in scaled units and does not reconstruct that potentially overflowing variance. It selects between two expressions according to scale relative to `sqrt(epsilon)`, with safe denominators in both branches. `sqrt(epsilon)` is computed from the scalar parameter on the host; it is normal even when epsilon is the smallest positive f32 subnormal. Input values and statistics remain on the GPU. Constant groups normalize to zero. Gamma and beta can be applied through ordinary resident multiply/add operations.

An empty axis list treats each element as a singleton: softmax is one, log-softmax/layer norm/variance are zero, and logsumexp/mean preserve the input. Elementwise empty outputs keep the input shape. Reduced empty contractions return `EmptyReduction` only when their output would contain values. Invalid axes and nonfinite/nonpositive epsilon are rejected even for empty inputs.

### Direct low-format attention

`TensorLowAttentionBackend` reads native f16/bf16 Q, K and V through a custom Metal kernel. `attention_low_f32` keeps f32 evaluation and output; `attention_low` applies one final nearest-even device cast. All three inputs must share a dtype, including for empty outputs. Additive masks remain f32 and keep masks remain u32. Batch/head geometry, GQA, strides, signed causal offsets and empty/fully masked results follow the common attention contract.

The kernel streams 32-key tiles through a 64-thread group. Each group owns one query row and up to 64 value channels. Scores and weights live in 384 bytes of threadgroup storage, while each channel keeps a normalized weighted value and a local value scale. Updating that scale avoids overflowing an unnormalized numerator for constant maximum finite V. A prior tile whose weight becomes zero also releases its scale, so obsolete extreme values cannot erase later ordinary results. Integer power-of-two operand balancing preserves normal products between BF16 subnormals and large finite partners, in either Q/K order. True subnormal arithmetic/results still follow f32 limits.

Leading broadcasts and mask expansion are native metadata views; input strides address their original allocations. The adapter allocates the actual `4*N`-byte f32 output, 52 bytes of launch parameters and a four-byte dummy mask when no mask was supplied. It allocates no full f32 Q/K/V copies, score/probability matrices or global partial arrays. Native metadata, compiler register spills, runtime bookkeeping and readback staging are outside that tensor-storage accounting. Low output adds its final allocation and the existing output-sized cast intermediates.

For `R` query rows, key count `K`, key depth `D` and value depth `Dv`, arithmetic work is proportional to `R*K*(D*ceil(Dv/64) + Dv)`: scores are recomputed for each value-channel tile. The launch uses at most 65,535 groups with grid-stride reuse. This keeps memory bounded, but a small number of query rows limits concurrency. There is no MLX attention speed claim from this implementation.

[The first focused run](qualification/low-attention-initial.txt) passed all four tests, including the shared dense-f64 oracle, strict tiny-product checks, maximum V, dropped early tiles, f32 mask precision, final low rounding, and 4,097-key tails. Additional tests cover strided 263-channel values, GQA, masked first tiles, lazy input lifetime, 65,537 rows and validation before empty shortcuts. [The complete 64-test Metal run](qualification/low-attention-metal.txt), [final focused run](qualification/low-attention-focused.txt) and [source manifest](qualification/low-attention-source-manifest.json) capture the qualified code. The shared power-of-two codec is also exercised by the existing low-statistics suite.

### Resident attention

`TensorAttentionBackend::attention` implements `softmax(scale * QKᵀ + bias) * V`. Rank-two inputs have one implicit head; higher ranks use `[..., heads, sequence, depth]`. Leading batches broadcast, consecutive groups of query heads share K/V heads, and value depth can differ from query/key depth. Keep masks accept any nonzero u32; additive masks preserve finite biases and exclude keys with `-Inf`. `causal: Some(offset)` additionally requires `key <= query + offset`, including negative offsets. Empty key sequences and fully excluded rows produce zero; empty query/batch/value dimensions produce empty tensors. Shape, dtype, context and scalar-policy checks happen before native attention.

The adapter promotes/broadcasts operands and flattens batch axes for MLX-C's rank-four API. Masks and intermediate arrays stay in the lazy GPU graph. A resident validity reduction and final select enforce zero for fully masked rows. Keep masks become additive `0/-Inf`: [the native probe](qualification/attention-native-probe.txt) found that raw MLX 0.32.1 boolean masks return nonzero values for closed rows in its general/full paths, while its general additive path can return NaN. No tensor values are inspected on the host.

MLX's causal mode is lower-right aligned. Without an explicit mask, matching offsets use that mode and at most an O(Lq) row-validity vector; no score-sized mask is created by the adapter. Other causal offsets compose an explicit resident mask. Native MLX may select a fused kernel or its GPU matmul/softmax graph. Arbitrary explicit masks and the general graph can require O(batch × heads × Lq × Lk) memory.

For the pinned MLX 0.32.1 Metal implementation, fused vector attention requires `Lq <= 8`, `Lq <= Lk`, `Lq × GQA-factor <= 32`, and equal D/Dv in `{64,96,128,256}` or `(D,Dv)=(192,128)`. Full attention requires `Lq > 8`, equal D/Dv in `{64,72,80,96,128}`, and either an array/no mask or supported lower-right causal lengths. Unsupported fused shapes remain on the GPU through MLX's general graph. [Pinned routing source](https://github.com/ml-explore/mlx/blob/v0.32.1/mlx/backend/metal/scaled_dot_product_attention.cpp).

MLX scales Q before the dot product. For `abs(scale) > 1`, the adapter builds a GPU dot-then-scale graph so finite Q values cannot overflow solely from pre-scaling. That graph also handles input sizes beyond fused kernels' signed indexing range. Grouped heads use a separate broadcast group axis instead of repeating K/V heads. Inputs, allowed logits and weighted arithmetic follow the shared finite-f32 contract; this is inference attention without dropout. [Pinned native operation](https://github.com/ml-explore/mlx/blob/v0.32.1/mlx/fast.cpp), [MLX-C ABI](https://github.com/ml-explore/mlx-c/blob/v0.6.0/mlx/c/fast.cpp).

Native fused accumulation also overflowed for constant `V=f32::MAX` even though the normalized answer is finite. The adapter now scales V by `2^-ceil(log2(Lk))` and restores that exact power of two after attention. A finite bound before restoration prevents rounding excursions from producing infinity. This factor depends only on key count, so masked or obsolete large values do not change the scale. The shift is at most 31; very small intermediate values still follow f32 underflow limits. [Original failure](qualification/attention-extreme-initial-failure.txt), [complete corrected qualification](qualification/attention-metal.txt). The standalone [native C probe](qualification/attention-native-probe.c) reproduces the mask behavior without the Rust adapter.

## Ownership and errors

The C API normally exits on errors. This adapter installs a process-wide error callback and serializes its native calls, turning native failures into Rust errors. Other MLX-C consumers in the same process must not replace that callback or concurrently alter its global error state. The shared library stays loaded for the process lifetime because MLX owns global workers and streams. Backend and tensor handles are deliberately neither `Send` nor `Sync`. Native arrays and streams are released through RAII; explicit `synchronize()` reports completion errors before destruction.

## Validation and remaining scope

The [current compiled-indexing qualification](../../docs/qualification/tensor-mlx-index-programs-2026-09-27.md) records 138 passing top-level tests (nine unit and 129 integration entries, including one marker-only child entry), all 18 new tests with compilation disabled, and the executed public indexing example.

At the earlier typed-program checkpoint, ninety-eight integration tests passed on the actual Metal runtime, including the shared tensor, indexing, reduction, scatter, low-precision, statistics, attention, low-arithmetic, low-indexing, low-scatter, low-statistics and low-attention conformance suites. They cover all canonical arithmetic/comparison operations, strided views, lazy input lifetime, vector/batched matmul, empty tensors/reductions, u32 sum/product wrapping, invalid ownership/dtypes/shapes, scans with 131,075 elements, reductions with 131,077 elements, device gather and stable compaction. Scatter checks include all five modes, 65,539 duplicate indices, repeated deterministic replacement through strided views, broadcast updates, invalid indices, exact u32 values, and empty/scalar cases. Low-precision checks cover all 65,536 raw patterns for each dtype, every finite value's exact round-trip, every positive rounding boundary and its negative mirror, and strided/vector/batched/partial-tile matmul. Statistics checks include arbitrary/strided/broadcast axes, long groups with 131,077 elements, huge common offsets, ±f32::MAX, overflowing raw variance, and the smallest positive epsilon. Attention checks include native general/vector/full/two-pass shapes, GQA, different value depths, strided broadcast masks, signed causal extremes, all-masked rows, scale-overflow avoidance, constant MAX values, and 4,097-key tails. Low arithmetic checks additionally exhaust finite sign/extrema payloads, exact BF16 subnormals and signed zeros through a 131,077-element f32 partial hierarchy, every reduction-axis subset, scalar/empty/broadcast inputs, deferred input lifetime, and a 65,537-row bounded-grid dispatch. Low indexing checks compare all 65,536 raw patterns under all six IEEE comparisons, preserve every payload through strided gather and compaction, and test all scan modes at 255/256/257, 511/512/513 and 131,077 elements, sparse compaction, device chains, and 65,537 independent rows. Six unit tests verify a missing-library error, recovery after an actual MLX-C reshape error on a CPU stream, checked/padded scatter-list allocation, panic containment with local cleanup, actual C closure callback error/recovery, and rollback after a partially recorded typed operation fails. The ten compiled-program integration entries include one marker-only child entry; the disabled-mode test separately launches that child for three native executions. The nineteen typed-program tests cover all low raw payloads, every cast rounding boundary inside the graph, intermediate low rounding, exact u32 arithmetic/branching, f32 reduction/matmul outputs, changed strides, resource lifetime and signature errors. All nineteen also passed in a separate `MLX_DISABLE_COMPILE=1` process. [Original tensor output](qualification/native-metal.txt), [indexing qualification](qualification/index-metal.txt), [reduction/vector qualification](qualification/reduction-metal.txt), [scatter qualification](qualification/scatter-metal.txt), [low-precision qualification](qualification/low-metal.txt), [statistics qualification](qualification/statistics-metal.txt), [attention qualification](qualification/attention-metal.txt), [low-arithmetic qualification](qualification/low-ops-metal.txt), [low-indexing qualification](qualification/low-index-metal.txt), [low-scatter qualification](qualification/low-scatter-metal.txt), [low-statistics qualification](qualification/low-statistics-metal.txt), [low-attention qualification](qualification/low-attention-metal.txt), [complete low-matmul qualification](qualification/low-matmul-metal.txt), [original f32 compiled-program qualification](qualification/compiled-metal.txt), [complete typed-program qualification](qualification/compiled-typed-metal.txt).

Qualification found two native-runtime edge cases and retained their initial logs: [empty u32 sum](qualification/index-empty-u32-initial-failure.txt) lacked a Metal initializer kernel; [empty-batch vector matmul](qualification/reduction-empty-batch-initial-failure.txt) terminated with SIGSEGV. The adapter handles these mathematical identities with native zeros/ones, and the final suite exercises both fixes.

The [initial bf16 cast failure](qualification/low-bf16-initial-failure.txt) records f32 bits `0x00010000` converting to bf16 zero instead of `0x0001`. The final qualification exercises the device integer correction without weakening the shared cast contract.

```sh
COMPUTE_REQUIRE_MLX=1 cargo test --offline --manifest-path crates/Cargo.toml \
  -p compute-mlx -- --test-threads=1
cargo run --offline --manifest-path crates/Cargo.toml -p compute-mlx --example linear
```

Reduced multiplication policies for f32 storage, slicing, asynchronous readback/cancellation, cross-instance compiled-program caching, autodiff, convolution and training operators are not exposed yet. Linux/CUDA-backed MLX, Windows, and mobile packaging have not been validated by this adapter. A missing MLX installation is an unavailable backend, not a successful skipped computation.

## Primary references

MLX-C provides opaque arrays, devices and streams with explicit ownership. Arrays are lazy; data pointers require evaluation. Its error callback must be overridden to prevent the default exit-on-error behavior. [Official MLX-C overview](https://ml-explore.github.io/mlx-c/build/html/overview.html).

The pinned C API provides the array constructors and accessors used here. The local installed headers, including operation signatures, were checked against native execution. [MLX-C 0.6 array implementation](https://github.com/ml-explore/mlx-c/blob/v0.6.0/mlx/c/array.cpp), [operations API](https://ml-explore.github.io/mlx-c/build/html/ops.html), [stream API](https://ml-explore.github.io/mlx-c/build/html/stream.html). The generated public API pages currently identify an older documentation version, so installed 0.6 headers are authoritative for this binding.

MLX 0.32.1 implements `where` by converting the condition to bool and broadcasting all inputs; `take` inserts the index dimensions, and `put_along_axis` uses native scatter. These semantics underpin the resident indexing composition. [Pinned MLX operation source](https://github.com/ml-explore/mlx/blob/v0.32.1/mlx/ops.cpp).

That source also implements max-shifted softmax/logsumexp and a centered two-pass variance. Native mean sums before scaling, so this adapter supplies bounded centered inputs for moments instead of calling native variance on the original values. The installed Metal softmax implementation and MLX-C axis operation signatures were checked alongside actual-device qualification.

The same operation source promotes matmul inputs and output to one dtype; `addmm` with an f32 addend also converts its whole inputs to f32. Native Metal GEMM loads low-format tiles and uses float accumulators before its output store. [Metal accumulator implementation](https://github.com/ml-explore/mlx/blob/v0.32.1/mlx/backend/metal/kernels/steel/gemm/mma.h). Installed MLX 0.32.1 `gemv.h` likewise defaults to float accumulation for f16/bf16 inputs; raw pointer signatures and `mlx_view` were checked in the installed MLX-C headers.

MLX-C can build without Python and can use an existing system MLX. Its CMake configuration requires C++20; building the native runtime is separate from compiling this Rust adapter. [MLX-C 0.6 build configuration](https://github.com/ml-explore/mlx-c/blob/v0.6.0/CMakeLists.txt). MLX has additional CPU/CUDA build configurations beyond this adapter's tested Metal path. [Official MLX installation documentation](https://ml-explore.github.io/mlx/build/html/install.html).

The native low-reduction dtype behavior is defined by [MLX 0.32.1 ops.cpp](https://github.com/ml-explore/mlx/blob/v0.32.1/mlx/ops.cpp) and [Metal reduce.cpp](https://github.com/ml-explore/mlx/blob/v0.32.1/mlx/backend/metal/reduce.cpp): floating input, accumulator, partial and output types retain the low dtype. The custom-kernel C signatures and exception handling come from [MLX-C 0.6 fast.cpp](https://github.com/ml-explore/mlx-c/blob/v0.6.0/mlx/c/fast.cpp). Shape/stride arguments and generated native pointer types follow the [official custom Metal kernel documentation](https://ml-explore.github.io/mlx/build/html/dev/custom_metal_kernels.html); the installed 0.32.1 headers and native probe verify the exact API used here.
