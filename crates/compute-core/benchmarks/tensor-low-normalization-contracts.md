# Packed low statistics: WGSL contracts

`GpuLowTensor` supports recorded softmax, log-softmax, logsumexp, centered
population moments and layer normalization. Every operation has an `_into`
form and two result forms: `_low_f32` retains the evaluated f32 result;
`_low` rounds that completed result once into the input dtype. The native
`TensorLowStatsBackend` adapter submits the same recorded implementations.

## Shared traversal and storage

The four existing statistics traversals are specialized with packed input
loads. No complete f32 copy of the low operand, shifted input or centered
input is allocated. Arbitrary axes, permutations, broadcast strides and
storage offsets follow the checked tensor layout. Output layouts must be
contiguous; low outputs may begin or end on either half of a physical word.
Existing cast writers preserve the neighboring halfword.

The shared planner keeps the existing one-workgroup path for 2–256 contracted
values. Larger contractions use bounded partials: up to 256 parts per row,
with at most 4096 row/part work items except when the number of rows itself
exceeds that budget. Dispatches stride across rows when the device group
limit is reached. The partial buffer has two f32 words per row/part. Low
summaries have eight words per row: four numerical values and a separate
scaling flag. Ordinary f32 statistics retain their four-word summary ABI
and share the integer scaling helpers introduced by this phase. Low kernels
initialize lazily on first use.

Singleton contractions bypass summaries. Mean and logsumexp store decoded
IEEE bits directly, preserving every finite f16/BF16 value, including signed
zero and BF16 subnormals. Other singleton results are the shared identities.
Empty shapes and invalid contractions follow the shared statistics helpers.

## Tiny and large groups

Moments and layer norm first select extrema using integer IEEE ordering.
For groups with maximum absolute value below `2^-64`, input coordinates are
lifted by `2^64` through integer exponent manipulation. This normalizes BF16
subnormals before subtraction, so floating-point input flushing cannot erase
a normal layer-norm result. Each hierarchical stage carries the same row
flag; distribution operations keep their original units.

The lifted mean is restored with integer division by `2^64`, and variance
with division by `2^128`, both rounded to nearest even. Layer normalization
divides `lifted_scale / sqrt(epsilon)` and then applies an integer IEEE
downshift by 64 exponent steps. The integer boundary prevents Metal from
moving the power-of-two factor ahead of the division. Minimum-subnormal
epsilon does not overflow an intermediate; maximum epsilon does not
overflow a lifted epsilon.
Normal results from tiny inputs are checked with relative tolerance.
Underflow of the actual f32 result remains subject to the shared backend
contract. The implementation does not weaken this into a broad absolute
error tolerance.

Large groups retain max-shifted exponentials and midpoint/scale centered
moments. Explicit IEEE infinity encoding covers overflowing variance or
negative log probability. Layer norm remains finite for finite BF16 extreme
inputs even when the reported variance exceeds f32. Epsilon must be finite
and strictly positive, including positive f32 subnormals.

## Recording and validation

Owner, shape, dtype, layout and whole-buffer alias checks run before appending
a prepared operation. Moments also reject aliasing between their two outputs.
Cross-type aliases between packed input storage and f32 output are rejected.
Final casts and all summary passes remain in one reusable program; replay
rewrites every result without host-side intermediate reads.

The focused target is `tensor_low_normalization`. It includes the shared
f64/exact-bit fixture, all recorded output forms with strided changed inputs
and odd output offsets, 65,537 tiny rows, and transactional invalid calls.
The generated small/reduce/finish/output variants are validated individually
by Naga. Independent source review covered integer scaling, ties-to-even
restoration, per-row flag propagation, bounded normalization ratios and
transactional aliases.

## Measured correctness failure and fix

The [initial focused run](tensor-low-normalization-initial-metal-tests.txt)
failed three tests. For the tiny pair `[-BF16(0x0001), BF16(0x0001)]` with
minimum-positive f32 epsilon, layer norm returned `-0` instead of
`-2.4532694666933983e-18`. This expected value is normal f32, so accepting it
as arithmetic underflow would violate the contract. Actual shader
[intermediates](tensor-low-normalization-diagnostic-metal.txt) showed that
lifting, centering and scaled variance were correct; the floating expression
for undoing the lift after division had been reassociated and became zero.
The integer downshift restored `-2.4532696e-18`.

For positive BF16 values with raw bits `[0x7f7f, 0x7f7e, 0x7f7d, 0x7f7b,
0x7f77]`, the same diagnostic measured a finite pair of extrema but an
infinite midpoint and NaN normalization. The backend had reassociated
`lo*0.5 + hi*0.5` into an overflowing intermediate. A
[native f32 probe](tensor-low-normalization-midpoint-native-diagnostic-metal.txt)
confirmed that the existing f32 path had the same defect. Both paths now
halve through integer exponent operations before adding, yielding midpoint
`3.3363623e38` and first normalized value `1.0606601` for this group.
Protective scaling in `scaled_delta` and `half_shift` uses the same helpers,
so compiler reassociation cannot cancel the overflow/underflow protection.
The f32 API, metadata and summary sizes are preserved.

[Final required Metal checks](tensor-low-normalization-metal-tests.txt)
passed all four low-statistics tests and all five existing f32-statistics
tests, including the new shared near-positive/negative-f32-maximum cases.
Generated Naga variants and strict all-target Clippy are checked separately.
Initial shader hooks, common shaders and diagnostic sources are retained in
`tensor-low-normalization-initial-source/`; the native source copies there
come from the retained attention baseline archive.
