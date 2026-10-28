# Packed low tensor indexing contracts

`ComputeProgram` records `tensor_compare_low`, `tensor_select_low`,
`tensor_gather_low`, `tensor_compact_low`, `tensor_scan_low_f32` and
`tensor_scan_low`. Each has an `_into` form. The native `ComputeRuntime`
implements `tensor_core::TensorLowIndexBackend` using these recorded operations.
All intermediate values and counts remain on the GPU.

## Storage and numerical rules

- F16 and BF16 use two logical values per u32 word. Compare and Select require
  matching low dtypes and use the shared broadcast validation.
- Compare decodes to IEEE bits using the integer codec, then compares ordered
  integer keys. Subnormal comparisons are exact, either signed zero is equal,
  infinities are ordered, and a NaN makes only NotEqual true.
- Select, Gather and Compact copy raw halfwords. Every NaN payload, infinity,
  signed zero and subnormal is preserved. Gather writes positive zero for an
  invalid index. Its GPU scalar counts each logical invalid index once,
  including when its output slices are empty.
- Compact returns capacity `[input.numel()]`, preserves logical row-major order,
  writes a GPU scalar count and zeros the remaining capacity on every execution.
- Scan directly decodes packed strided input into the first f32 prefix pass.
  Its f32 block totals, recursive prefixes and carry-add passes reuse the typed
  scan hierarchy. No full f32 input conversion is allocated. Scanning another
  axis may require a full f32 *result* in axis-last order before the output copy.
  Low output rounds each completed f32 prefix once using the existing RN codec.
  Inputs and intermediate f32 sums must stay finite; parallel summation order
  and f32 underflow apply. Final low output may overflow to infinity.

## Traversal and writes

The scalar Select and Gather shaders expose their common logical-value loader.
Packed specializations reuse those loaders with halfword reads and a shared
word writer. One invocation owns each physical output word, preserving any
halfword outside a contiguous output view.

Compaction reuses the existing normalized mask prefix plan. Its scatter uses
u32 atomic compare-exchange to update one halfword, since neighboring selected
values may be handled by different invocations. Every selected slot and zero
slot has one logical writer; compare-exchange preserves the other halfword.
This also preserves neighbors outside an odd-offset output view.

Host Select/Gather/Compare metadata builders are shared with f32/u32 indexing.
The scan planner accepts a first-pass loader hook; later hierarchy levels keep
using the ordinary f32 kernels. All five generated specializations have Naga
validation in `tensor::low::index_sources::tests`.

## Recording and validation

Inputs and outputs must belong to the runtime. Outputs must match the shape
and dtype and be contiguous; offsets are supported. Output allocation aliases
are rejected, including low/f32/u32 reinterpretations and count aliases.
Multi-pass operations build a temporary program and append only after all
validation and allocation succeeds. Programs can run again after input/mask/
index buffer updates; output tails and counts are recomputed.

## Verification

Focused target: `tensor_low_index`. It invokes the shared exhaustive raw-bit
and f64-prefix fixture, and adds recorded replay, odd-halfword sentinels,
131,077-element compaction/scan, 255/256/257 scan boundaries, cross-type aliases,
foreign owners, output layouts and invalid-recording rollback. Existing
`tensor_index` checks the shared traversal and hierarchy after specialization.
No performance claim is made for these new paths.

Metal qualification on 2026-09-27: all five new tests and all five existing
indexing tests passed with `COMPUTE_REQUIRE_GPU=1` and one test thread. The raw
log is [tensor-low-index-metal-tests.txt](tensor-low-index-metal-tests.txt).
All nine CPU library tests passed, including Naga validation of the five packed
index specializations and the existing typed specializations. Strict all-target
Clippy passed. Independent source review found no actionable issue.
