# Packed f16/BF16 arithmetic and reduction contracts

The recorded GPU API adds:

- `tensor_unary_low` and `tensor_unary_low_into`.
- `tensor_binary_low` and `tensor_binary_low_into`.
- `tensor_reduce_low_f32` and `tensor_reduce_low_f32_into`.
- `tensor_mean_low_f32` and `tensor_mean_low_f32_into`.
- `tensor_reduce_low` and `tensor_reduce_low_into`.
- `tensor_mean_low` and `tensor_mean_low_into`.

Native `TensorLowOpsBackend` implements the same operations. Its low-output
reduction overrides prepare the reduction and final cast in one program and
submit them together. Every intermediate stays resident. Existing packed
storage, casts, layout views and matmul behavior are retained.

## Elementwise operations

Unary operations keep the input storage dtype. Binary operations require
matching dtypes and use the shared trailing-axis broadcasting rules. The
shared `low_binary_shape` helper checks dtype before broadcasting. Arbitrary
input strides, broadcast strides and offsets are supported without
materializing dense operands.

One invocation owns an entire output u32 word. It calculates each addressed
16-bit lane and preserves neighboring lanes outside the output view, including
odd starting offsets and odd final lengths. Output views must be contiguous
and use a distinct allocation from every input. Ownership, shape, layout and
alias checks finish before shader work is appended.

Each arithmetic load decodes its packed value directly. Ordinary unary and
binary math evaluates in f32 and invokes the existing integer low codec once
for its final round-to-nearest, ties-to-even conversion. Final low results
may overflow to infinity even when the evaluated f32 value is finite.

Negate flips the raw sign bit; Abs clears it. Both preserve finite low
subnormal values exactly. Binary Min/Max compare ordered IEEE integer keys
and select an original finite value. They never evaluate these operands in
floating-point arithmetic. Min selects negative zero from a -0/+0 tie; Max
selects positive zero. The same extrema policy applies to reductions.

Inputs must satisfy the existing unary function domains and finite arithmetic
requirements. NaN/infinite inputs have no shared arithmetic policy. F32
transcendental tolerance and underflow limits apply before low rounding.

## Direct packed reductions

The packed reduction specializes the existing `tensor_reduce.wgsl` traversal
instead of copying its reduction algorithm. It changes the scalar carrier to
u32 containing f32 bits, adds packed-input decode and f32-partial load modes,
and uses the same workgroup hierarchy. Generated-source validation checks
the assembled specialization, in addition to the static shader catalog.

Sum and Product convert their operands to f32 at the arithmetic operation.
Min and Max compare integer IEEE keys through the complete hierarchy. Their
f32 output is written as bits, preserving selected BF16 subnormals regardless
of a device's floating-point flush mode. Mean performs f32 sum/count, with
one division after partial sums have been merged. The sum and intermediate
products must remain in f32 range; ordinary f32 underflow is permitted.

Large contractions use at most 256 parts per output and at most 4096 total
partial f32 values (16 KiB). When there are more than 4096 output groups,
each output uses one workgroup and no partial allocation. Group-stride loops
respect device dispatch limits. The original measured f32 sum path is
unchanged.

F32-result operations produce a final f32 tensor without expanding the packed
input first. Low-result reductions use that reduced f32 result and one final
integer-codec cast. For nonempty axes, the f32 temporary has only the reduced
output shape. Empty axes preserve logical values: f32 outputs directly decode
into the requested final output, while low outputs copy raw logical bits.

Empty outputs remain empty. Empty contracted dimensions produce sum zero and
product one. Min, Max and Mean reject an empty contraction when its output
would be nonempty. All shared axis, duplicate-axis and keep-dimension rules
are preserved.

Prepared programs retain their storage bindings. Replacing input contents
reuses the same arithmetic and reduction dispatches; every partial and output
is overwritten. Multi-stage reductions and their final low cast are prepared
transactionally before appending to the caller's program. Cross-interpretation
aliases, such as low storage and a f32 output backed by the same GPU buffer,
are rejected.

## Verification on Metal, 2026-09-27

```sh
COMPUTE_REQUIRE_GPU=1 cargo test --manifest-path crates/Cargo.toml \
  -p compute-core --test tensor_low_ops -- --test-threads=1
cargo test --manifest-path crates/Cargo.toml -p compute-core --lib \
  packed_reduction_specialization_validates
cargo test --manifest-path crates/Cargo.toml -p compute-core \
  --test kernels shipped_kernels_validate_with_naga -- --exact
cargo clippy --manifest-path crates/Cargo.toml -p compute-core \
  --lib --test tensor_low_ops -- -D warnings
```

Five focused GPU tests passed:

1. Shared backend conformance: all unary and binary operations, broadcast and
   strided inputs, all-axis reductions, f32 accumulation versus final low
   rounding, signed-zero ties, exact extrema and empty/error contracts.
2. Every finite f16 and BF16 raw pattern for Negate/Abs and paired Min/Max,
   with exact bit comparisons. Odd output offsets preserve surrounding
   halfwords. A 131077-element hierarchy preserves selected subnormal extrema
   as exact f32 output bits.
3. Strided two-row reductions at lengths 0, 1, 255, 256, 257, 4096, 4097 and
   131077. All four reducers write both f32 and low outputs; repeated input
   changes preserve sentinels and validate the mean path against f64.
4. A reusable resident square, broadcast add, sum and mean chain over a
   transposed packed tensor, with three sets of changed input values.
5. Dtype, ownership, shape/axis, contiguity, ordinary aliases and low/f32
   buffer aliases. Failed operations leave output storage untouched and the
   program usable.

The static 45-source WGSL catalog and generated packed-reduction source both
passed Naga validation. All-target compilation and strict owned Clippy passed.
No native f16 feature or matrix-hardware acceleration is claimed. Performance
measurements and broader regression are reported separately by the root task.

Evidence: `tensor-low-ops-metal-tests.txt` contains the final five-test run.
