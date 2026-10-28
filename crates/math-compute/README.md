# osv-math-compute

Architecture and crate boundaries: [GPU library design](../../docs/design/gpu-library-architecture.md).

Device backends (wgpu, CUDA, tensor/MLX) for the CPU-only `osv-math` kernels.
`osv-math` declares the `DeviceKernels` port; this crate implements it.
Call `math_compute::install()` once at startup, then every
`math_core::*_accelerated` function reaches the device kernels and falls back
to the CPU reference when a device is unavailable.

```toml
[dependencies]
math-compute = { package = "osv-math-compute", path = "../math-compute", features = ["gpu"] }
```

## Binary64 geometry

On native hosts, `MathGpuSession::tensor()` returns `TensorMathF64`.
Existing session calls for paired distances, distance sums, transformed sums,
bounds, transformed bounds, moments and point-cloud statistics now use software
binary64 on the selected WGSL GPU. Their `MathExecution` reports
`GpuArithmetic::SoftwareBinary64`. Finite coordinates outside f32 range and
local differences below f32 resolution survive upload and computation.
`try_point_cloud_stats_stable` uses the same centered binary64 implementation.

`TensorMathF64` also composes with the native CUDA f64 tensor backend. Bounds,
transforms, distances, centered covariance and raw moments share those recipes;
only final values are read back by the session wrappers. Intermediate products
and sums must fit f64. This is not an arbitrary-range robust statistics API.

Migration remains in progress: nearest/Chamfer, `MathGpuProgram` recording,
legacy CUDA geometry adapters and browser session methods still use their
existing f32 paths. MLX does not yet implement the binary64 tensor contract.
See the [qualification report](../../docs/qualification/software-binary64-2026-09-27.md).

## Resident geometry through WGSL, CUDA and MLX

The optional `tensor` module expresses point-cloud algorithms once through
`tensor-core` contracts. Construct `TensorMath::new(&backend)` with a
`ComputeRuntime`, `CudaRuntime` or `MlxBackend`. Existing `MathGpuSession`
instances expose binary64 `session.tensor()` on their own device and queue.

The explicit legacy `TensorMath` API uses `[N, 3]` points with f32 coordinates. The API provides affine
transforms, paired squared distances and sums, bounds, centered covariance,
raw second moments, nearest neighbors and directed Chamfer summaries.
Results remain native tensors and compose with other domain or backend
operations. Upload/read calls are explicit. Errors retain backend failures;
there is no automatic CPU fallback in this API.

| Feature | Dependencies and use |
| --- | --- |
| Default | Existing f64 CPU math, no tensor or GPU runtime |
| `tensor` | Shared recipes and the dependency-free tensor contract |
| `tensor-wgsl` / `gpu` | WGSL executor and existing specialized GPU sessions |
| `tensor-cuda` | Native CUDA tensor executor |
| `tensor-mlx` | Native MLX executor without WGSL/CUDA dependencies |

`TensorMath::upload_points_f64` explicitly rounds host f64 coordinates to f32
and rejects nonfinite or overflowing conversions. It cannot recover geometry
smaller than f32 resolution at a large coordinate offset. CPU f64 functions and `Acceleration` thresholds remain available.

Nearest-neighbor options limit tiles, result bytes and conservative estimates
of recipe workspace and work before device operations begin. Equal distances use the smaller
original target index. Outputs have shape `[query_count, k]`; missing neighbors
use index `u32::MAX` and distance `f32::MAX`. Distances and intermediate arithmetic
must remain finite. A native evaluation fence completes retained tile outputs
without reading their values on the CPU. MLX then releases unreferenced lazy
dependencies. Workspace accounting covers live logical recipe buffers; it
excludes primitive implementation temporaries/materializations, metadata,
caches, library workspaces and evaluation of caller-built lazy inputs. It does
not bound native peak memory or backend internal work. Browser completion is a
separate asynchronous integration task.

These recipes prioritize common resident composition. WGSL may submit separate
commands, CUDA may enqueue separate kernels and MLX may build a lazy graph.
They do not promise fusion, replay or a speedup over specialized kernels.
The existing fused nearest and point-cloud implementations remain available.

The [executable example](examples/resident_geometry.rs) transforms an uploaded
cloud, computes its covariance and finds two neighbors before reading results:

```sh
cargo run --manifest-path crates/Cargo.toml -p osv-math-compute --features tensor-wgsl \
  --example resident_geometry -- wgsl
cargo run --manifest-path crates/Cargo.toml -p osv-math-compute --features tensor-mlx \
  --example resident_geometry -- mlx
cargo run --manifest-path crates/Cargo.toml -p osv-math-compute --features tensor-cuda \
  --example resident_geometry -- cuda
```

The [backend qualifier](../../scripts/qualify-tensor-backends.py) includes the
shared domain fixture and requires the requested hardware to be available.

## Crate layout

The crate root is a thin facade: downstream users still import from
`math_core::{...}`, while implementation is split by domain:

```text
src/
├─ lib.rs              # public facade: module wiring + pub use
├─ types.rs            # V2, V3, M3, ID
├─ error.rs            # Error, Result, ensure()
├─ acceleration.rs     # Cpu/Auto/Gpu/Cuda placement and size heuristics
├─ linalg.rs           # dense vector/matrix helpers, eigen/SVD/solve
├─ local_plane.rs      # top-4 local plane/normal fitting
├─ transform_error.rs  # fused transform-and-distance registration score
├─ bounds.rs           # point-cloud and transformed bounds reductions
├─ moments.rs          # point-cloud centroid/covariance reduction
├─ nearest_neighbor.rs # CPU reference plus GPU/CUDA dispatch
├─ chamfer.rs          # point-cloud Chamfer distance over nearest-neighbor
├─ registration.rs     # rigid transform fitting and ICP
├─ tensor/             # shared resident geometry over tensor backend contracts
├─ gpu/                # portable wgpu backend: Metal/Vulkan/DX12/WebGPU
└─ cuda/               # CUDA driver/PTX backend
```

## `transform_points` — plain CPU math, no GPU/CUDA

`transform_points(points, m, t)` batches `q = M*p + t` over a point set, in
exact f64. There is **no** `transform_points_accelerated`: an earlier version
of this crate had one, but `examples/bench_gpu.rs` measured GPU and CUDA
placements slower than this CPU reference at every size from 1K to 5M points
— even with device buffers reused across calls — because 3 fused
multiply-adds per point is too little work to amortize kernel-launch and
host/device synchronization latency. Existing `Auto` thresholds use those
measurements. The resident tensor API above exposes transforms for composition
with other operations on data that is already on the device.

## Fused transform-and-error scoring

`transformed_squared_distance_pair_sum_accelerated(source, target, m, t,
acceleration)` evaluates the registration/ICP scoring form in one pass:
transform each moving/source point by `q = M*p + t`, compare it to the
corresponding target point and reduce the squared distances. The CPU reference
keeps exact f64 behavior, while explicit `Gpu`/`Cuda` placements run a fused
f32 reduction (`transformed_distance_pair_sum.wgsl` /
`transformed_distance_pair_sum.cu`) and read back only partial sums. This is
preferable to materializing `transform_points(source, m, t)` and then running
a second distance pass when evaluating many registration hypotheses.

`Auto` shares the conservative one-to-one distance placement and stays on CPU:
on the RTX 5090 this fused CPU pass is already faster than materializing the
transformed cloud, while explicit device paths remain upload-bound.

| pairs | cpu fused | auto | gpu | cuda | materialized CPU |
| --- | --- | --- | --- | --- | --- |
| 1,000 | 0.002 ms | 0.002 ms | 0.117 ms | 0.048 ms | 0.005 ms |
| 100,000 | 0.201 ms | 0.199 ms | 3.211 ms | 0.655 ms | 0.504 ms |
| 5,000,000 | 11.397 ms | 11.303 ms | 130.366 ms | 33.581 ms | 28.238 ms |

Use
`cargo run --release -p osv-math-compute --features cuda --example bench_transform_error`
to benchmark your hardware and force `Gpu`/`Cuda` when the surrounding pipeline
already keeps data near a device workload.

## Transformed point-cloud bounds

`transformed_point_bounds_accelerated(points, m, t, acceleration)` computes
the axis-aligned bounds of `M*p+t` directly, without allocating a transformed
point cloud. This is a small but common geometry building block for
registration, culling and scene diagnostics. Explicit `Gpu`/`Cuda` placements
run fused WGSL/CUDA reducers (`transformed_point_bounds.wgsl` /
`transformed_point_bounds.cu`) with grow-only device buffers; `Auto` stays CPU
because repeated measurements did not show a stable device crossover.

| points | fused cpu | auto | gpu | cuda | materialized CPU |
| --- | --- | --- | --- | --- | --- |
| 10,000 | 0.050 ms | 0.063 ms | 0.237 ms | 0.148 ms | 0.041 ms |
| 100,000 | 0.505 ms | 0.647 ms | 1.695 ms | 0.515 ms | 0.582 ms |
| 1,000,000 | 5.082 ms | 6.788 ms | 15.507 ms | 6.224 ms | 6.032 ms |
| 5,000,000 | 26.306 ms | 35.054 ms | 72.990 ms | 32.331 ms | 31.257 ms |

Use `cargo run --release -p osv-math-compute --features cuda --example bench_transformed_bounds`
to measure your hardware.

`transformed_point_cloud_stats_accelerated(points, m, t, acceleration)` extends
that idea to a full bounds + centroid/covariance summary. Bounds use the fused
transformed-bounds backend above; moments are computed from the source moments
and transformed analytically (`centroid' = M*centroid+t`,
`covariance' = M*covariance*M^T`), so callers avoid allocating a transformed
cloud while still reusing the existing CPU/GPU/CUDA reducers.

On the RTX 5090 this remains an O(n), memory-bound summary, so `Auto` stays
conservative; explicit device paths are available for pipelines that already
want those placements:

| points | cpu | auto | gpu | cuda | materialized CPU |
| --- | --- | --- | --- | --- | --- |
| 10,000 | 0.071 ms | 0.109 ms | 0.667 ms | 0.230 ms | 0.060 ms |
| 100,000 | 0.703 ms | 1.110 ms | 3.856 ms | 1.142 ms | 0.794 ms |
| 1,000,000 | 7.222 ms | 11.683 ms | 34.745 ms | 10.610 ms | 8.443 ms |
| 5,000,000 | 36.579 ms | 59.573 ms | 171.160 ms | 53.474 ms | 41.811 ms |

Use `cargo run --release -p osv-math-compute --features cuda --example bench_transformed_stats`
to compare it with materializing transformed points first.

## GPU / CUDA nearest-neighbor search

`nearest_neighbor_accelerated(queries, targets, acceleration)` finds, for
every query point, the index of its closest point in `targets` and the
squared distance to it. `Acceleration::Cpu` (default) is the exact f64
reference (`nearest_neighbor`), brute force in O(queries × targets);
`Acceleration::Auto` resolves from `queries.len() * targets.len()` using the
measured crossover below; `Acceleration::Gpu` (feature `gpu`) runs a portable
wgpu compute shader in f32, one thread per query scanning every target;
`Acceleration::Cuda` (feature `cuda`, needs the `gpu` feature too) runs a CUDA
driver-API PTX port of the same kernel, falling back to the wgpu shader and
then the CPU reference when a device or kernel is unavailable. Both optional features
require nightly Rust (through `gpu-compute`); regenerate the checked-in PTX
with `npm run build:cuda-kernels`. Device buffers are cached per query/target
capacity (grow-only), so repeated calls at a stable size reuse allocations
instead of recreating them.

Unlike the trivial affine transform above, nearest-neighbor search does
`target_count` work per query, giving the GPU/CUDA placements enough
arithmetic intensity to win. **Measured, not assumed:** `cargo run --release
-p osv-math-compute --features cuda --example bench_gpu`, on an NVIDIA RTX 5090
(CUDA 13.4):

| queries × targets | cpu | gpu | cuda |
| --- | --- | --- | --- |
| 1,000 × 100 | 0.078 ms | 0.260 ms | 0.137 ms |
| 3,000 × 100 | 0.237 ms | 0.238 ms | 0.138 ms |
| 1,000 × 1,000 | 0.750 ms | 0.309 ms | 0.175 ms |
| 3,000 × 1,000 | 2.260 ms | 0.366 ms | 0.185 ms |
| 10,000 × 1,000 | 7.782 ms | 0.493 ms | 0.265 ms |
| 100,000 × 1,000 | 76.676 ms | 2.243 ms | 0.902 ms |
| 100,000 × 5,000 | 384.680 ms | 2.670 ms | 1.177 ms |
| 1,000,000 × 2,000 | 1592.680 ms | 19.780 ms | 8.181 ms |

At the smallest size CPU still wins (fixed per-call overhead dominates); CUDA
crosses over first, already winning by 3,000 × 100 (work = 300K), while wgpu
breaks even a bit later and pulls ahead from around 1M work upward — up to
~200x faster than CPU at 1M queries × 2K targets. `Acceleration::Cpu` remains
the right choice for small batches; pick `Gpu`/`Cuda` once your workload sits
in this regime, and re-run the benchmark for your own sizes/hardware before
relying on the numbers above.

Don't want to hand-tune that threshold yourself?
`Acceleration::recommended_for_nearest_neighbor(query_count, target_count)`
encodes the crossover above as a `queries * targets` "work" threshold (CUDA
from ~200K, wgpu from ~700K, falling back to `Cpu` below that and to whichever
placement is fastest if the multiplication would overflow `usize`).
Passing `Acceleration::Auto` to `nearest_neighbor_accelerated` applies that
recommendation directly. It's a starting point tuned to the RTX 5090 numbers
above, not a guarantee for every device — treat it as a reasonable default,
not a substitute for benchmarking workloads where the choice actually matters.

`nearest_two_accelerated(queries, targets, acceleration)` and
`nearest_four_accelerated(queries, targets, acceleration)` use the same
placement model for exact top-k nearest-neighbor search. Top-2 returns the
best and second-best target for each query, which is useful for ratio tests,
correspondence filtering and robust registration pipelines; top-4 supports
small local-neighborhood workflows without jumping to an approximate index:

```rust
use math_core::{Acceleration, nearest_four_accelerated, nearest_two_accelerated};

let pairs = nearest_two_accelerated(&queries, &targets, Acceleration::Auto);
let best = pairs[0][0];
let second = pairs[0][1];
let neighbors = nearest_four_accelerated(&queries, &targets, Acceleration::Auto);
```

Top-2 has the same O(queries × targets) arithmetic intensity, so it gets
dedicated WGSL/CUDA kernels (`nearest_two.wgsl`, `nearest_two.cu`) instead of
being built from two CPU passes. Device buffers are cached grow-only, matching
the top-1 nearest-neighbor path for repeated stable-size registration and
filtering workloads. Top-4 follows the same model with packed output buffers
(`nearest_four.wgsl`, `nearest_four.cu`). Measured with `cargo run --release -p osv-math-compute --features
cuda --example bench_nearest_two` on the RTX 5090:

| queries × targets | work | cpu | auto | gpu | cuda |
| --- | --- | --- | --- | --- | --- |
| 256 × 512 | 131K | 0.132 ms | 0.134 ms | 0.220 ms | 0.081 ms |
| 1,024 × 1,024 | 1.0M | 1.106 ms | 0.128 ms | 0.262 ms | 0.204 ms |
| 4,096 × 4,096 | 16.8M | 17.723 ms | 0.319 ms | 0.825 ms | 0.318 ms |
| 16,384 × 8,192 | 134M | 143.374 ms | 0.807 ms | 1.417 ms | 0.788 ms |

Top-4 benchmark (`cargo run --release -p osv-math-compute --features cuda --example
bench_nearest_four`) on the same host:

| queries × targets | work | cpu | auto | gpu | cuda | auto speedup |
| --- | --- | --- | --- | --- | --- | --- |
| 256 × 512 | 131K | 0.182 ms | 0.183 ms | 0.197 ms | 0.086 ms | 1.00x |
| 1,024 × 1,024 | 1.0M | 1.512 ms | 0.157 ms | 0.274 ms | 0.137 ms | 9.62x |
| 4,096 × 4,096 | 16.8M | 25.104 ms | 0.414 ms | 0.858 ms | 0.399 ms | 60.61x |
| 16,384 × 8,192 | 134M | 198.779 ms | 1.042 ms | 1.813 ms | 1.131 ms | 190.71x |

`local_point_planes(queries, support, acceleration)` builds on top-4: it finds
the four nearest support points for every query and fits a tiny PCA plane to
that neighborhood, returning a deterministic normal, offset and RMS distance.
The expensive neighborhood search uses `Auto`/CUDA/wgpu; the 4-point plane fit
stays on CPU. This is useful for local surface normals and small-neighborhood
geometry filters.

Measured with `cargo run --release -p osv-math-compute --features cuda --example
bench_local_planes`:

| queries × support | work | cpu | auto | gpu | cuda | auto speedup |
| --- | --- | --- | --- | --- | --- | --- |
| 256 × 512 | 131K | 0.255 ms | 0.254 ms | 0.278 ms | 0.189 ms | 1.01x |
| 1,024 × 1,024 | 1.0M | 1.907 ms | 0.467 ms | 0.676 ms | 0.451 ms | 4.09x |
| 4,096 × 4,096 | 16.8M | 27.622 ms | 1.981 ms | 2.413 ms | 1.851 ms | 13.94x |
| 16,384 × 8,192 | 134M | 215.305 ms | 6.975 ms | 8.065 ms | 6.705 ms | 30.87x |

For runtime diagnostics, `cargo run -p osv-math-compute --features cuda --example
backend_report` prints the placement labels, portable wgpu backend report
(`metal`, `vulkan`, `dx12`, or `webgpu`) and native CUDA device report when
available.

## Chamfer distance

`directed_chamfer_distance(queries, targets, acceleration)` computes the mean
nearest-neighbor squared distance from one point cloud into another.
`chamfer_distance(a, b, acceleration)` runs both directions and returns the
directed summaries plus symmetric mean/RMS and Hausdorff worst-case values.
`directed_hausdorff_distance` / `hausdorff_distance` expose the worst-case
metric directly:

```rust
use math_core::{Acceleration, chamfer_distance};

let score = chamfer_distance(&cloud_a, &cloud_b, Acceleration::Auto)?;
println!("symmetric RMS = {}", score.symmetric_rms_distance);
println!("Hausdorff = {}", score.hausdorff_distance);
```

This is a higher-level geometry metric over nearest-neighbor work. Device
placements use fused directed-Chamfer reduction kernels, so they read back one
sum/max pair per workgroup instead of one nearest-neighbor result per query.
The wgpu shader uses the shared backend tuning (`128`-wide workgroups on Metal,
`256` elsewhere).
Measured with `cargo run --release -p osv-math-compute --features cuda --example
bench_chamfer` on the RTX 5090:

| cloud sizes | cpu | auto | gpu | cuda |
| --- | --- | --- | --- | --- |
| 1,000 × 1,000 | 1.486 ms | 0.274 ms | 0.537 ms | 0.280 ms |
| 5,000 × 2,000 | 15.087 ms | 0.632 ms | 1.307 ms | 0.592 ms |
| 20,000 × 5,000 | 150.253 ms | 1.719 ms | 3.960 ms | 1.679 ms |
| 100,000 × 10,000 | 1526.633 ms | 7.169 ms | 14.744 ms | 6.848 ms |

## Pairwise squared distances

`squared_distance_pairs(a, b)` computes one-to-one squared Euclidean distances
for equal-length point arrays. `squared_distance_pairs_accelerated(a, b,
acceleration)` adds explicit `Gpu`/`Cuda` offload through the same portable
wgpu and CUDA-driver layers as nearest-neighbor. Device buffers are cached
grow-only per thread for stable-size repeated calls.
`squared_distance_pair_sum_accelerated` uses a device-side partial reduction
for scalar RMSE/loss-style consumers, reading only partial sums back; that
reduction path also reuses grow-only device buffers.

This kernel is intentionally conservative in `Auto`: unlike nearest-neighbor,
it is only O(pair_count) with a few FLOPs per pair, so on discrete GPUs the
copy/readback cost dominates. Measured with `cargo run --release -p osv-math-compute
--features cuda --example bench_distance_pairs` on the RTX 5090, explicit
device execution is correct but slower; `Auto` therefore stays on CPU to avoid
regressing callers:

| pairs | cpu | auto | gpu | cuda |
| --- | --- | --- | --- | --- |
| 10,000 | 0.009 ms | 0.008 ms | 0.322 ms | 0.098 ms |
| 100,000 | 0.130 ms | 0.068 ms | 3.224 ms | 0.859 ms |
| 250,000 | 0.322 ms | 0.347 ms | 8.167 ms | 2.035 ms |
| 1,000,000 | 1.933 ms | 2.126 ms | 28.189 ms | 8.822 ms |
| 3,000,000 | 6.184 ms | 6.143 ms | 85.045 ms | 24.921 ms |

Scalar sum reduction avoids reading all distances but still has to upload both
input point arrays, so `Auto` remains CPU there too:

| pairs | cpu sum | auto sum | gpu sum | cuda sum |
| --- | --- | --- | --- | --- |
| 10,000 | 0.014 ms | 0.013 ms | 0.321 ms | 0.101 ms |
| 100,000 | 0.140 ms | 0.130 ms | 3.236 ms | 0.765 ms |
| 250,000 | 0.499 ms | 0.500 ms | 7.807 ms | 2.198 ms |
| 1,000,000 | 3.809 ms | 2.809 ms | 29.933 ms | 7.079 ms |
| 3,000,000 | 8.414 ms | 8.885 ms | 79.100 ms | 21.326 ms |

## Point-cloud bounds

`point_bounds(points)` computes exact CPU axis-aligned bounds, center and
extent for a point cloud. `point_bounds_accelerated(points, acceleration)`
adds fused wgpu/CUDA reductions with grow-only buffers and the same
Metal/default workgroup tuning as Chamfer. Because this is O(points) with very
little arithmetic per point, `Auto` intentionally keeps the CPU reference;
explicit `Gpu`/`Cuda` remain available for integrated-GPU/low-copy
experiments or for validating backend behavior.

Measured with `cargo run --release -p osv-math-compute --features cuda --example
bench_point_bounds` on the RTX 5090:

| points | cpu | auto | gpu | cuda |
| --- | --- | --- | --- | --- |
| 10,000 | 0.053 ms | 0.041 ms | 0.356 ms | 0.216 ms |
| 100,000 | 0.422 ms | 0.401 ms | 1.856 ms | 0.670 ms |
| 1,000,000 | 4.188 ms | 4.155 ms | 15.795 ms | 5.097 ms |
| 5,000,000 | 21.135 ms | 21.265 ms | 77.053 ms | 25.854 ms |

## Point-cloud moments

`point_centroid(points)` returns the equally weighted center without computing
squared coordinates. `weighted_point_centroid(points, weights)` accepts one finite,
nonnegative mass per point; at least one mass must be positive. Zero masses are
ignored, and normalization permits totals larger than `f64::MAX`. All point
coordinates must be finite, including zero-mass points. These are discrete point
centers, not volume centers inferred from mesh vertices.

CPU moments use compensated sums and centered covariance to preserve small
spreads at large coordinate offsets. Unrepresentable moments return
`point_moments_overflow`; center-only callers should use `point_centroid`.
Explicit GPU/CUDA moment paths still use f32 raw moments and do not provide the
same numerical stability. The benchmark numbers below predate this CPU stability
change and must be remeasured before making performance comparisons.

`point_moments(points)` computes the centroid, mean outer product and central
covariance matrix for a finite point cloud. `point_principal_axes(points,
acceleration)` builds on that covariance to return PCA variances and unit axes
for orientation/normal workflows. `point_fit_plane(points, acceleration)` uses
the smallest PCA variance/axis as a least-squares plane normal and reports the
RMS orthogonal distance. `point_moments_accelerated(points, acceleration)`
adds fused wgpu/CUDA reductions with grow-only buffers and Metal/default
workgroup tuning. Like bounds, this standalone reducer is mostly memory
movement on discrete GPUs, so `Auto` stays on the exact CPU path; explicit
`Gpu`/`Cuda` remain useful for integrated-GPU experiments and backend
validation.

Measured with `cargo run --release -p osv-math-compute --features cuda --example
bench_point_moments` on the RTX 5090:

| points | cpu | auto | gpu | cuda |
| --- | --- | --- | --- | --- |
| 10,000 | 0.035 ms | 0.035 ms | 0.293 ms | 0.160 ms |
| 100,000 | 0.332 ms | 0.338 ms | 1.793 ms | 0.601 ms |
| 1,000,000 | 3.593 ms | 3.508 ms | 15.199 ms | 5.023 ms |
| 5,000,000 | 18.004 ms | 18.056 ms | 73.931 ms | 24.893 ms |

Plane fitting adds only a 3x3 eigensolve after moments, so it follows the same
placement profile (`cargo run --release -p osv-math-compute --features cuda --example
bench_point_plane`):

| points | cpu | auto | gpu | cuda |
| --- | --- | --- | --- | --- |
| 10,000 | 0.032 ms | 0.032 ms | 0.233 ms | 0.113 ms |
| 100,000 | 0.321 ms | 0.324 ms | 1.702 ms | 0.491 ms |
| 1,000,000 | 3.229 ms | 3.181 ms | 15.745 ms | 4.915 ms |
| 5,000,000 | 17.044 ms | 17.581 ms | 77.347 ms | 23.471 ms |

`point_cloud_stats(points)` returns bounds and moments together.
`point_cloud_stats_accelerated(points, acceleration)` fuses both summaries
into one device pass/upload. That fused CUDA path finally amortizes the
fixed overhead for large clouds, so `Auto` selects CUDA from 500K points when
the `cuda` feature is built and otherwise keeps CPU:

| points | fused cpu | fused auto | fused gpu | fused cuda | separate cpu | separate cuda |
| --- | --- | --- | --- | --- | --- | --- |
| 10,000 | 0.059 ms | 0.058 ms | 0.331 ms | 0.170 ms | 0.070 ms | 0.231 ms |
| 100,000 | 0.586 ms | 0.580 ms | 1.757 ms | 0.571 ms | 0.705 ms | 1.093 ms |
| 1,000,000 | 6.298 ms | 5.059 ms | 15.156 ms | 4.928 ms | 7.659 ms | 10.152 ms |
| 5,000,000 | 32.023 ms | 23.940 ms | 74.866 ms | 23.759 ms | 38.781 ms | 47.066 ms |

## ICP / rigid point-cloud registration

`rigid_transform(source, target)` computes the least-squares rigid transform
for known correspondences (Kabsch/SVD). `icp_register(source, target, options)`
adds iterative closest-point registration on top: each iteration transforms
the source, finds closest target correspondences with
`nearest_neighbor_accelerated`, rejects optional distance outliers, fits the
next rigid delta, scores the post-fit residual with the fused transform-error
reduction, and composes the final `RigidTransform`.

This is where the nearest-neighbor kernel becomes a higher-level primitive:
`IcpOptions::default()` uses `Acceleration::Auto`, so large ICP batches select
CUDA/wgpu automatically when available and otherwise stay on the CPU reference.
Measured with `cargo run --release -p osv-math-compute --features cuda --example
icp_registration` on the same RTX 5090:

| points | work/iteration | cpu | auto/cuda | iterations |
| --- | --- | --- | --- | --- |
| 1,000 | 1,000,000 | 1.565 ms | 0.329 ms | 2 |
| 5,000 | 25,000,000 | 37.791 ms | 0.948 ms | 2 |
| 20,000 | 400,000,000 | 614.513 ms | 3.547 ms | 2 |

## License

MIT — see repository root `LICENSE`.

### Recorded GPU pipeline

`MathGpuSession::try_*` returns explicit errors and a `MathExecution<T>` report
(backend and f32 arithmetic). Existing `Option` convenience methods retain CPU
fallback compatibility. To keep intermediate results on the device:

```rust,ignore
let context = gpu_compute::GpuContext::new().expect("GPU");
let runtime = compute_core::ComputeRuntime::new(&context)?;
let session = math_core::gpu::MathGpuSession::new(&context);
let points = runtime.upload(&[0.0f32, 0.0, 0.0, 1.0, 2.0, 3.0])?;
let mut plan = session.program(&runtime)?;
let moved = plan.transform(
    math_core::gpu::PointCloudView::new(&points)?, math_core::ID, [1.0, 0.0, 0.0],
)?;
let distances = plan.squared_distances(
    math_core::gpu::PointCloudView::new(&points)?,
    math_core::gpu::PointCloudView::new(&moved)?,
)?;
let sum = plan.sum(&distances)?;
let mut encoder = context.device.create_command_encoder(&Default::default());
plan.record(&mut encoder);
let mut read = runtime.record_read(&mut encoder, &sum)?;
read.submitted(context.queue.submit([encoder.finish()]));
assert_eq!(read.wait(std::time::Duration::from_secs(5))?, [2.0]);
```

Packed xyz f32 layout and context ownership are checked. Plans can be recorded
again; their buffers remain alive through bind groups. Queue writes update input
data without rebuilding the plan. Recorded operations use f32 arithmetic and
expect finite device data; recording validates layouts and ownership without
reading coordinates back.

#### Nearest neighbors and fused statistics

`plan.nearest_neighbors(queries, targets)` returns `GpuNearestNeighbors` with
public `indices: GpuArray<u32>` and `squared_distances: GpuArray<f32>` fields.
The outputs can feed `plan.sum`, generic compute, or another custom shader in the
same encoder. Equal distances pick the first target. Empty queries produce empty
arrays; empty targets produce `u32::MAX` indices and `f32::MAX` distances. Distances
that overflow f32 also keep the sentinel. The search is exhaustive, with
O(query_count * target_count) work.

`plan.point_cloud_stats(points)` returns `GpuPointCloudStats { values, samples }`.
Bounds and raw moments are reduced in multiple GPU passes; centroid and
population covariance are finalized on the GPU. Empty clouds are rejected.
The packed `values` array has 24 f32 elements, with public associated constants:

| Constant | Range | Values |
| --- | --- | --- |
| `MIN` | 0..3 | Minimum x, y, z |
| `MAX` | 3..6 | Maximum x, y, z |
| `SUM` | 6..9 | Coordinate sums x, y, z |
| `OUTER_SUM` | 9..15 | Product sums xx, xy, xz, yy, yz, zz |
| `CENTROID` | 15..18 | Mean x, y, z |
| `COVARIANCE` | 18..24 | Population covariance xx, xy, xz, yy, yz, zz |

Covariance uses `E[pp] - E[p]E[p]` in f32, so large coordinate offsets can lose
precision. Input coordinates and intermediate sums/products must remain finite
within f32 range; the recorded plan cannot inspect GPU input values on the host.
The synchronous adapter retains its existing final f64 CPU fold.
Neither recorded operation submits commands or reads intermediates back.

`nearest_neighbors_into` and `point_cloud_stats_into` accept caller-owned output
arrays, including scratch-pool allocations. Output lengths, device identity and
allocation aliasing are checked before adding dispatches. Repeated `record`
calls reuse bindings and retained intermediate buffers. A rejected operation
leaves the prior plan intact.

#### Centered GPU covariance

`plan.point_cloud_stats_stable(points)` and `point_cloud_stats_stable_into`
preserve that packed layout and the original bounds, raw product sums and
centroid. They add a second traversal that reduces residuals `d = p - centroid`
and computes `E[dd] - E[d]E[d]`. The correction matters when the centroid itself
rounds to f32. This avoids cancellation between large raw second moments: for
points at `1_000_000 ± 1`, the expected variance is one even though the raw
f32 products cannot retain that unit difference.

`MathGpuSession::try_point_cloud_stats_stable(points)` exposes the same plan as
a synchronous operation and returns the existing `PointCloudStats` structure.
It caches kernels, uploads the input, and reads only the final 24 scalars.
Repeated resident workloads should keep a recorded plan to reuse allocations.
The existing accelerated/default selection and CUDA paths keep their behavior.

The extra traversal has a cost; this is an explicit precision option. All
arithmetic remains f32, and tiny negative covariance eigenvalues can still
result from rounding. Input conversion loses unit differences at magnitudes
above `2^24`; centering cannot recover those differences. Coordinates, raw
sums/products and centered intermediate sums/products must remain finite.
Raw product fields keep their original meaning and precision, so recomputing
covariance from `OUTER_SUM` discards the benefit of the centered result.

On Apple M4 Max, a million-point cloud translated by `10^6` had maximum
covariance error `146,124` with raw moments and `1.23e-7` with centered reduction.
The retained vector implementation took 1.68–1.90× the legacy GPU time across
three paired cases; it was 2.27–2.65× faster than the first centered prototype.
See [method, costs, raw samples and limitations](benchmarks/stable-stats-metal.md).

#### Measured nearest-neighbor dispatch

Recorded and synchronous nearest-neighbor calls share a cooperative GPU kernel:
one 64-lane workgroup scans targets for each query, then reduces distance/index
pairs while preserving first-index ties. The original scalar WGSL remains the
reference and fallback. `NearestNeighborAlgorithm::for_shape` exposes selection:
Metal, 512..8192 targets and 256..(2 * targets) queries. Other shapes and backends
use the original path. CPU/CUDA placement is unchanged.

On the measured Apple M4 Max, actual recorded latency including both full
readbacks improved **2.40× / 2.71× / 1.41×** at 256×512, 4096×4096 and
16384×8192 points. The synchronous API now submits its kernel and both readbacks
together; it improved **1.85× / 2.37× / 1.30×** against the old three-submission
transport. CPU remains faster for the smallest case. See
[method, limitations, medians and p90](benchmarks/nearest-neighbor-metal.md),
including the initial noisy run and the longer production confirmation.
