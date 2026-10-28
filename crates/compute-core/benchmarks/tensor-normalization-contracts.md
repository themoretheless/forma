# WGSL tensor statistics and normalization contracts

Recorded APIs in `ComputeProgram`:

- `tensor_softmax` and `tensor_log_softmax` preserve input shape.
- `tensor_logsumexp(input, axes, keep_dims)` reduces the selected axes.
- `tensor_moments(input, axes, keep_dims)` returns `Moments { mean, variance }`;
  variance is the population variance, divided by the group length.
- `tensor_layer_norm(input, axes, epsilon)` preserves input shape. Gamma and
  beta compose through the existing resident binary operations.
- Each method has an `_into` form. `tensor_moments_into` takes a
  `&Moments<GpuTensor>` with two distinct output allocations.
- Native `TensorStatsBackend` calls submit the same recorded implementations.

Axes can be any unique subset, in any order. Noncontiguous inputs, broadcast
strides and storage offsets are supported. An empty axis list treats every
element as a singleton: softmax is one; log-softmax, variance and normalized
values are zero; mean and logsumexp preserve the input. Elementwise results
preserve empty input shapes. Reduced nonempty outputs reject zero-length
contractions with `EmptyReduction`. Epsilon must be finite and strictly
positive; positive subnormal values are accepted.

Outputs require the exact expected shape and a contiguous layout, with any
validated storage offset. Every output must have a different allocation from
the input and from the other moment output. Ownership, shape and alias checks
finish before any stages are added to the caller's program. All allocations
and bindings are prepared in a temporary program, then appended together.

## Resident calculation

Distribution operations first find each group's maximum, then reduce
`exp(x - maximum)`. Softmax divides by that sum. Log-softmax retains the
shift as `(x - maximum) - log(sum)`, avoiding the loss of small differences
that can result from first adding the large maximum back. Logsumexp adds the
maximum only for its final reduced output.

Moments and normalization first reduce the minimum and maximum. They use
`anchor = min/2 + max/2` and `scale = max(abs(min-anchor), abs(max-anchor))`.
The mean of `t = (x-anchor)/scale` is reduced, followed by the mean of
`(t-mean(t))²`. These terms are bounded: `t` lies in `[-1,1]`, and each squared
centered deviation is at most four. Constant groups bypass the division and
normalize to zero. No uncentered sum or raw squared moment is formed.

The mean is reconstructed as `anchor + scale*mean(t)`. Variance is
`scale*(scale*variance(t))`. Layer normalization stays in scaled coordinates:
for `r = sqrt(epsilon)/scale <= 1`, it evaluates
`(t-mean(t))/sqrt(variance(t)+r²)`; otherwise it uses the reciprocal ratio
`q = scale/sqrt(epsilon)` and evaluates
`(t-mean(t))*q/sqrt(1+variance(t)*q²)`. This remains finite for the supported
finite inputs even when the unnormalized variance exceeds the f32 range.
The square root of the scalar epsilon is calculated on the host once while
recording; tensor data never leaves the GPU.

Large and small scale divisions rescale both operands by a power of two to
avoid reciprocal overflow or subnormal reciprocals. Opposite-sign extreme
values use a halved difference before exponentiation or log-probability
output. An exact integer significand-product guard detects overflowing final
products. Infinity results are written through a u32 storage interpretation:
variance uses `0x7f800000`, and out-of-range log probabilities use `0xff800000`.

Groups containing 2..256 values use one workgroup per group for all five
operations. Each lane keeps its input and transformed value in registers;
shared reductions find extrema, the exponential sum or scaled mean, and the
centered variance. Results are written directly in that dispatch. This path
allocates no summary or partial arrays. More than 65535 independent groups
are covered by a workgroup-stride loop.

Longer reductions use 256 lanes and up to 256 partial groups per output. The
total number of partials is capped at 4096 when there are at most 4096 output
groups; larger numbers of output groups use one partial each. Each partial
occupies two f32 values; each group summary occupies four. Group-stride loops
respect device dispatch limits. Scratch is per-group summaries and partials;
there are no full-input exponential, centered-value or squared-value arrays.
Singleton contractions directly write results without reduction scratch.

Every intermediate remains resident. Repeated submissions overwrite all
partials and summaries, so changing the original buffer contents updates the
results without rebuilding the program.

## Numerical scope

Inputs must be finite. Arithmetic follows f32 accuracy and WGSL underflow
limits, including permitted subnormal flushing. Source input quantization
cannot be reversed. Mathematically out-of-range variances and log
probabilities may produce signed IEEE infinity; normalized values remain
usable without materializing that overflowing variance.

The initial private test applied a relative tolerance to a subnormal
normalized output. On Metal, input `[-1e-20, 1e-20]` with
`epsilon = f32::MAX` (`3.4028235e38`) produced signed zero
(`0x80000000`, `0x00000000`). The independent f64 reference is
`[-5.421010851953291e-40, 5.421010851953291e-40]`. This is below
`f32::MIN_POSITIVE`. The corrected check retains relative tolerance for normal
outputs and permits an absolute `MIN_POSITIVE` underflow floor. No production
shader changed to address that assertion. The first failed output and a
successful diagnostic recording the exact values are preserved alongside the
final test log.

## Verification on Metal, 2026-09-27

```sh
COMPUTE_REQUIRE_GPU=1 cargo test --manifest-path crates/Cargo.toml \
  -p compute-core --test tensor_normalization -- --test-threads=1
cargo test --manifest-path crates/Cargo.toml -p compute-core \
  --test kernels shipped_kernels_validate_with_naga -- --exact
cargo clippy --manifest-path crates/Cargo.toml -p compute-core \
  --lib --test tensor_normalization -- -D warnings
```

All five focused GPU tests passed after the short-group specialization:

1. Shared backend conformance: arbitrary axes, permutations, broadcasting,
   scalar and empty cases, big offsets, opposite f32 extremes, tiny epsilon,
   and a 131077-element reduction tail, checked against an independent f64
   reference.
2. Repeated strided programs at lengths 1, 255, 256, 257, 4096, 4097 and
   131077. Each program computes moments, normalization, softmax, a downstream
   sum, log-softmax and logsumexp. Three changed inputs reuse all bindings;
   distinct input/output offsets and surrounding sentinels are checked.
3. Variance overflow classification around adjacent f32 values surrounding
   `sqrt(f32::MAX)`, large finite ranges and tiny epsilon. Normalized results
   are checked against f64 even when variance overflows.
4. Foreign buffers, aliasing, invalid axes/epsilon, incorrect output shapes
   and noncontiguous outputs. Rejected operations leave the program usable
   and unrelated output storage unchanged.
5. Single groups of 2, 33, 255, 256 and 257 values across every operation;
   65537 groups of two values with varied means and spreads, checked against
   f64 and executed twice with changed inputs. These cover both sides of the
   specialization cutoff and row indexing beyond one dispatch dimension.

All 42 assembled shader sources passed Naga validation. Strict Clippy passed
for the library and new test. The additional diagnostic test passed and
recorded the subnormal values above. Root owns the separate performance
comparison and broad regression qualification; this note makes no speed claim.

Evidence:

- `tensor-normalization-small-metal-tests.txt`: final five-test run with short
  groups specialized.
- `tensor-normalization-metal-tests.txt`: prior four-test run, preserved from
  before the specialization.
- `tensor-normalization-underflow-diagnostic.txt`: exact underflow values.
- `tensor-normalization-initial-failure.txt`: preserved first tool output,
  with provenance explained at its top.
