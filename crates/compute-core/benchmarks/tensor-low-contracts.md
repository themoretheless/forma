# Packed f16/bf16 tensor contracts

Correctness qualification on Metal, 2026-09-27. Performance measurements are
reported separately in [the low-precision study](tensor-low.md).

## Representation and views

`GpuLowTensor` owns shared `GpuArray<u32>` storage, a logical backing length
measured in 16-bit elements, a `LowDtype` and a validated `Layout`. Even elements
occupy the low half of a word, odd elements the high half. `GpuElement` and all
existing four-byte array kernels remain unchanged.

For N backing elements, a fresh runtime allocation uses
`max(4, 4 * ceil(N / 2))` bytes. Imported words may share a larger allocation.
`allocation_bytes()` reports actual bytes. Padding is outside the logical
storage length; layout validation cannot expose it. Shape/address metadata fit
u32, and allocations respect device storage limits.

`upload_low_bits` and native `read_low_bits` preserve raw u16 patterns. Narrow,
permute and broadcast only modify metadata. `tensor_materialize_low` preserves
every payload bit, including signaling NaNs, while copying logical values on
GPU. Native reshape materializes noncontiguous views when necessary.

`from_packed` accepts an exact word count and explicit logical backing length.
`packed_words()` exposes the physical transfer representation.
`write_low_storage_bits()` updates the complete logical backing storage in
physical order, independently of its current view, and keeps prepared bindings.
This host write resets an unused final padding lane to zero.

## GPU conversion

`tensor_cast_to_low` treats f32 input storage as u32 IEEE bits. It rounds to
nearest with ties to even, preserves signed zero and subnormals, and produces
signed infinity on overflow. Casts retain NaN classification; they may
canonicalize the sign/payload. `tensor_cast_to_f32` writes decoded IEEE bits
through a u32 storage binding. Neither codec uses floating-point intermediates.

This avoids relying on packing operations that permit subnormal flushing under
the [WGSL floating-point rules](https://www.w3.org/TR/WGSL/#floating-point-evaluation).
The shared fixture exhaustively validates these codec properties.

Pack/copy dispatches assign one destination word to each invocation. Odd
destination offsets preserve the preceding half; odd ends preserve the following
half. No invocation shares a word with another, so no halfword atomic is needed.
The `_into` methods accept distinct contiguous outputs, including odd offsets.

## Matrix products

`tensor_matmul_low_f32` and `tensor_matmul_low` require matching input dtypes.
Both use shared `MatmulPlan` layout lowering with ordinary f32 tensors: vector
promotion, batch broadcasting, nonnegative strides, empty outputs and zero
contractions follow the same contract.

The shader decodes packed input values while filling 16×16 f32 workgroup tiles
and accumulates in f32. A low output adds one final cast of the completed
product; its intermediate f32 allocation is proportional only to the result.
Whole operands are never expanded to f32. Inputs and accumulated results must
remain finite; final low-format rounding may overflow. Device f32 accumulation
order and fusion can differ, so arithmetic results require suitable tolerance.

Capabilities report `LowStorage::Packed16x2`, `matmul: true` and
`matmul_f32: true` for both formats. This implementation enables no optional
features and does not claim native half arithmetic or Tensor Core instructions.
The native `TensorLowBackend` facade submits GPU work and synchronizes at host
readback. Recorded APIs compose with existing operations in reusable programs.

## Qualification

```sh
COMPUTE_REQUIRE_GPU=1 cargo test --manifest-path crates/Cargo.toml \
  -p compute-core --test tensor_low --test tensor -- --test-threads=1
```

All four low tests passed in 0.50 s, and all ten existing tensor tests passed in
1.76 s after the shared matmul metadata extraction. These are correctness test
durations. Full command output is retained in `tensor-low-metal-tests.txt`.

Coverage includes all 65,536 raw patterns for both formats; strided bit-preserving
copies; exact decode and finite re-encode; every rounding midpoint and adjacent
f32 values for both signs; subnormals, signed zeros, infinities and NaNs;
f32 accumulation cancellation; single output rounding; vectors, batches, empty
products and partial tiles.

Local tests also verify physical allocation bytes, odd sizes through 257,
neighboring halfword sentinels, repeated raw writes, mixed dtypes, foreign owners,
cross-type aliases, and a single-submission cast→strided batched matmul→low
output→f32→sum chain reused with three changed inputs. Its independent CPU
matrix oracle accumulates in f64.

All 38 assembled WGSL sources pass Naga, including the common integer codec
within pack, unpack and matmul. Strict Clippy passes. Browser and other vendor
runtime qualification is separate from this local Metal proof.
