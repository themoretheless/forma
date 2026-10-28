# Packed low scatter contracts

`ComputeProgram` records `tensor_scatter_low` and `tensor_scatter_low_f32`, with
`_into` variants for caller-owned output and invalid-count tensors. The native
`ComputeRuntime` implements `TensorLowScatterBackend` through these programs.
Base and updates must share their low dtype. Updates broadcast to the shared
scatter expansion shape. Invalid indices are ignored and the GPU scalar counts
each logical index once, even when other dimensions make output slices empty.

## Storage and arithmetic

Both storage variants specialize the existing scatter shader traversal. They
read update halfwords directly from packed storage; no full f32 updates tensor
is created. The common host helpers build the same index, update and destination
metadata and prepare the same deterministic Replace owners.

- Low Replace starts from a raw copy of the base and replaces each chosen
  halfword using u32 compare-exchange. The greatest logical row-major index wins
  duplicates. Every raw payload is preserved, including NaNs and signed zero.
- Low Min/Max also use packed halfword compare-exchange. They choose between the
  original finite low values using decoded IEEE integer ordering. Subnormal
  values remain exact, Min chooses -0 and Max chooses +0.
- Low Add/Multiply first decode the base into a result-shaped f32 accumulator.
  Kernels fold directly loaded low updates into it using f32 compare-exchange.
  After all updates, one final cast rounds each completed destination to low.
- F32 output uses a result-shaped f32 accumulator for every operation. Replace
  decodes the chosen update directly; finite values, infinities and signed zero
  widen exactly, while NaN classification is preserved. Min/Max compare IEEE
  integer keys and store the selected f32 bits. Add/Multiply use f32 arithmetic;
  the result is never first rounded to low storage.

Add/Multiply inputs and intermediates must remain finite. Parallel order and
f32 underflow apply; final low conversion may overflow to infinity. Highly
contended destinations can require repeated compare-exchange attempts. Measured
performance, contention limits and the comparison with f32 expansion are recorded
in the [matched benchmark](tensor-low-scatter.md).

## Recording and reuse

Every output starts from the current base on each execution. Replace owner
scratch is cleared and invalid counts are reset each time. Empty/no-index and
all-invalid cases retain the base. Output offsets, including odd halfword
offsets, are supported; neighboring halfwords are preserved atomically.

Shapes, dtype, runtime ownership and contiguous output layouts are checked.
Aliases are rejected across the low/f32/u32 interpretations of an allocation,
including output and invalid-count aliases. Preparation is transactional: the
new dispatches are appended only after every validation/allocation succeeds.

## Verification

`tests/tensor_low_scatter.rs` includes shared conformance plus recorded replay,
65,539 duplicate strided updates, output/count sentinels, raw NaN payloads,
exact tiny extrema and signed zeros, final-rounding versus f32-result checks,
and rejected ownership/dtype/layout/alias cases. The generated packed and f32
specializations are separately checked by Naga. Existing `tensor_scatter`
regression tests cover the extracted shared owner and metadata helpers.

Metal qualification on 2026-09-27: five new tests and all four existing scatter
tests passed with `COMPUTE_REQUIRE_GPU=1` and one test thread. The new target also
runs a recorded scatter → gather → f32 scan → sum chain after changing all three
input buffers. The raw log is
[tensor-low-scatter-metal-tests.txt](tensor-low-scatter-metal-tests.txt).
Both generated shaders passed Naga validation; strict all-target Clippy passed.
Independent source review found no actionable issue. The correctness suite
passed on the first real-GPU run; the later performance change is described below.

## Packed extrema early exit

The packed Min/Max path now returns when an atomic snapshot already dominates
the update. This snapshot is a valid linearization point: concurrent updates to
the same halfword move monotonically in the same integer IEEE order, and writes
to its neighbor cannot change that dominance. Improving updates still retry CAS
with the refreshed complete word. Required -0/+0 sign changes are not skipped.

The original shader, fingerprints and benchmark output remain in the
`tensor-low-scatter-initial-*` artifacts. After this change, both generated
specializations passed Naga and all five focused Metal tests passed; see
[tensor-low-scatter-extrema-metal-tests.txt](tensor-low-scatter-extrema-metal-tests.txt).
Existing coverage includes 65,539 contended tiny/zero-tie extrema, neighboring
halfwords and replay. Add/Multiply and f32 accumulator code are unchanged; the
initial hot Add case remains an approximately 5 ms contention bottleneck.
Matched performance measurements are tracked separately from this correctness
qualification.
