# Resident WGSL attention contracts

`ComputeProgram::tensor_attention(query, key, value, mask, options)` records
scaled dot-product attention and returns a resident f32 tensor.
`tensor_attention_into(..., output)` writes a caller-provided contiguous
output, including a validated nonzero storage offset. The native
`TensorAttentionBackend` adapter submits this same recorded implementation.

## Shapes and masks

Operands use `[..., heads, sequence, depth]`, with rank-two matrices promoted
to one head. Leading batch dimensions broadcast. Query heads are a positive
multiple of the matching key/value head count; consecutive groups of query
heads share one key/value head. Query/key depth must be equal and positive.
Value depth can differ. When all three operands have rank two, output is
`[queries, value_depth]`; otherwise it retains the broadcast batch and head
axes. Existing tensor layouts supply all strides and offsets, including
permuted and broadcast views, without materializing operands.

Masks broadcast to the conceptual `[..., query_heads, queries, keys]` shape:

- `None` keeps all otherwise visible keys.
- `Keep` accepts a u32 tensor; every nonzero value is true.
- `Additive` accepts finite f32 biases and negative infinity. Negative
  infinity excludes that key before score arithmetic.

`AttentionOptions::scale` defaults to `1/sqrt(depth)` and accepts any finite
explicit value, including zero or negative values. `causal: Some(offset)`
keeps key `j` when `j <= query_index + offset`. Unsigned difference checks
implement this signed mathematical comparison without overflowing at
`i32::MIN` or `i32::MAX`. `Some(0)` gives upper-left causal masking; decoder
alignment uses an explicit `keys - queries` offset when it fits i32.

Fully masked rows and zero-key rows are overwritten with zeros. Empty query,
batch or value-depth dimensions give an empty output. Head counts and
query/key depth remain validated for empty outputs. No dropout is applied.

The shared `AttentionPlan` validates shapes, heads, mask broadcasting and
options. Actual device addresses and dispatch counts are checked separately.
The conceptual score count does not impose a u32 element-count restriction,
because no score tensor is created. The host test records a program whose
conceptual score count exceeds `u32::MAX` while its real operands and output
remain small. That metadata test deliberately does not execute billions of
score calculations.

All input and mask buffers must belong to the runtime. Output storage must
be distinct from every operand and mask, even for views of the same
allocation. All checks, allocations and bindings complete before the prepared
dispatches are appended together. Failed operations leave an existing program usable.

## Kernel and memory behavior

A 64-lane workgroup processes one query row and 64 output channels, streaming
32 keys at a time. The first 32 lanes compute QK scores. Shared reductions
find each tile maximum and sum of shifted exponentials. The online state
updates the running maximum and denominator while retaining the normalized
value result. Query/key scores are recomputed for each 64-channel output
tile. This is an explicit performance tradeoff for arbitrary value depth.

When there are at most 64 query rows across batches and heads, and at least
512 keys, the planner can split the key range into parallel parts. Its part
count is `min(ceil(keys/256), 64, value_budget/output_elements)`, with a
minimum of one. The value budget is 1048576 f32 elements (4 MiB), reduced to
the device's actual buffer/binding limits if necessary. A single part uses
the original streaming path. Other shapes also retain that path.

Each part owns a contiguous original key range of at most
`ceil(keys/parts)` keys. Mask addresses and signed causal comparisons retain
the original logical key positions. The current schedule guarantees every
part starts before the key count; a CPU test checks this invariant at
thresholds, budget limits and `u32::MAX` key count. Part outputs contain
normalized V results and `(maximum, denominator)` summaries. Only the first
64-channel tile writes a part's shared summary; all channel tiles use the
same score/reduction order.

A second GPU dispatch merges parts. It computes the global part maximum,
weights each part by `part_denominator * exp(part_maximum-global_maximum)`,
and forms a bounded normalized value combination. Fully masked parts have
denominator zero and are ignored. Every partial, summary and final output
is overwritten on each execution. No intermediate submission or readback
occurs.

The single-part path has one output allocation, metadata with
`33 + 5*batch_rank` u32 words, a four-byte unused-mask binding and an
eight-byte unused-summary binding. Split execution adds at most 4 MiB of
partial values, at most 32 KiB of `(maximum, denominator)` summaries, and
seven metadata words for merging. Scratch size is proportional to
`rows * parts * value_depth`; no score/probability matrix or expanded operand
is allocated.

For four query rows, 4097 keys and 64 value channels, the planner chooses
17 parts of 241 keys. Partial values occupy 17408 bytes, and summaries occupy
544 bytes: 17952 bytes of workspace. This is a host-plan size calculation;
driver allocations and the final output are separate.

Streaming workgroup storage is 384 bytes: 32 pairs for reduction and 32
exponential weights. Each lane also has a private tile of 32 V values. Merge
workgroups use 512 shared bytes and a private tile of up to 64 partial
values per lane. Private storage placement is determined by the shader
compiler. Workgroup-stride loops cover work counts beyond the portable
65535-group X dimension.

Let `m` and `l` be the previous maximum and shifted denominator. For a tile,
the kernel computes `m' = max(m, tile_max)`, previous weight
`a = l*exp(m-m')`, tile weights `e_j = exp(score_j-m')`, and
`l' = a + sum(e_j)`. Value accumulation uses normalized weights before
summation. Each output lane also tracks the largest participating absolute
V value and expresses its accumulator in those scaled coordinates. The
result is bounded by that observed magnitude; clamping its scaled value to
`[-1,1]` corrects rounding outside this mathematical bound.

When `a` becomes zero, the obsolete V scale and accumulator are discarded.
The old-scale division is skipped entirely. This matters when a newly
dominant key tile has small values after an earlier tile contained huge
ones. Empty or fully masked tiles do not read V or change the state.

All tensor data stays on the GPU. Prepared programs can be submitted again
after changing Q/K/V or mask contents through `ComputeRuntime::write`.
Each invocation recomputes its local online state and overwrites its output.

## Numerical limits and corrected finding

Q, K and V must be finite. Dot products and allowed scaled/biased logits must
remain representable in f32. Additive NaN and positive infinity are outside
the shared contract. Reduction order and underflow follow backend f32
limits. A halved score difference prevents overflow when subtracting
opposite finite extremes; exponentials below f32 range contribute zero.

Value normalization permits a finite normalized result even when a raw
unnormalized PV numerator would overflow. Tests include constant positive
and negative `f32::MAX`, cancellation of `1e30` values, and a new live tile
containing `1e-30` after an expired tile containing `f32::MAX`.

The initial extreme-V test exposed a real bug: a constant `f32::MAX` result
became zero on Metal. Floating power-of-two rescaling did not protect that
case. The corrected helper changes exponent bits before division, keeping
reciprocals in normal range and preventing cancellation of the protective
floating factors. The targeted retest and full suite pass with this change.
The exact compiler transformation behind the original failure was not
measured. Independent review also identified the obsolete-scale division
after its weight became zero; the explicit reset and skip address it.

Positive and negative minimum-subnormal V inputs are checked to produce
finite results. Flushing those values to zero is allowed under the stated
underflow contract.

## Verification on Metal, 2026-09-27

```sh
COMPUTE_REQUIRE_GPU=1 cargo test --manifest-path crates/Cargo.toml \
  -p compute-core --test tensor_attention -- --test-threads=1
cargo test --manifest-path crates/Cargo.toml -p compute-core \
  --test kernels shipped_kernels_validate_with_naga -- --exact
cargo clippy --manifest-path crates/Cargo.toml -p compute-core \
  --lib --test tensor_attention -- -D warnings
cargo test --manifest-path crates/Cargo.toml -p compute-core --lib \
  split_key_schedule_bounds_workspace_and_respects_cutoffs
```

Six focused GPU tests passed after adding split-key execution:

1. Shared backend conformance against an independent dense f64 reference:
   rank-two and 5D broadcasting, grouped query heads, all mask forms,
   strided views, causal offset extremes, scale options, empty cases,
   invalid contracts, 4097-key tails and strengthened extreme-V fixtures.
2. Repeated programs over physical strided Q/K/V and masks, with key lengths
   0, 1, 31, 32, 33 and 257, value depths 1, 63, 64, 65 and 129, output
   sentinels, changed masks including fully excluded rows, and a resident
   attention-to-sum composition.
3. A 131077-key single query with 65 value channels, 65537 independent query
   workgroups, and recording a conceptual score shape above u32 capacity.
4. Extreme finite V, cancellation, opposite score extremes, expired large
   value scales, zero replacement values and minimum-subnormal inputs.
5. Ownership, mask broadcasting, invalid scales, noncontiguous outputs,
   output aliases and failure-before-recording behavior.
6. Key counts 511/512/513/4097 around the split threshold, multiple value
   tiles, masked prefix/suffix parts and fully masked replays, output offsets,
   signed causal bounds across parts, and expired large values followed by
   tiny values. The CPU planner test also verifies the 4 MiB value and 32 KiB
   summary bounds and nonempty partition starts.

All 44 assembled shader sources passed Naga; strict Clippy passed for the
library and attention test. Performance measurements and full-repository
regression are maintained separately by the root task. This contract note
makes no speed claim.

Evidence:

- `tensor-attention-split-metal-tests.txt`: final six-test run with split-key
  execution.
- `tensor-attention-metal-tests.txt`: preserved five-test run before split
  execution.
- `tensor-attention-initial-failure.txt`: preserved initial four-pass,
  one-failure run before the exponent-scaling correction.
- `tensor-attention-extreme-retest.txt`: successful targeted extreme-V retest
  after exponent scaling, before the final additional reset/subnormal cases.
