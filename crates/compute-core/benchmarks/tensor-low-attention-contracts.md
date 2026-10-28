# Packed low attention: WGSL contracts

`tensor_attention_low_f32` evaluates scaled QK, masked online softmax and PV
from packed f16/BF16 inputs directly. `tensor_attention_low` rounds only the
completed f32 output into Q's dtype. Both expose recorded `_into` variants;
the native `TensorLowAttentionBackend` adapter submits the same recording,
including the final cast in a single program.

## Shared planning and bounded storage

The original f32 attention planner now accepts checked input buffers/layouts
and a kernel catalog. Packed attention specializes only Q/K/V loads in the
existing streaming shader. It shares the f32 partial merge, geometry checks,
layout broadcasting, masks, causal alignment, grouped query heads and key
partition policy. Separate lazily compiled f16 and BF16 pipelines make dtype
a compile-time constant; metadata uses the same batch descriptors as f32.

The workgroup streams 32 keys for one query and 64 output channels. Each
additional 64-channel value tile repeats QK work. For at most 64 output rows
and at least 512 keys, the planner may split keys into at most 64 parts,
subject to the existing four-MiB partial-value budget and device limits.
Normalized partial values and their maxima/denominators remain on GPU.
Fully masked partials have zero denominators and are excluded by the merge.
The shaders rewrite all output and partial state on every program execution.

No complete expanded f32 Q/K/V operand or score/probability matrix is
allocated. F32 partials, bounded workgroup tiles and actual result buffers
are allowed. Low output uses an actual f32 result followed by the existing
integer nearest-even cast. Masks remain f32/u32 and are never rounded through
the input dtype. Additive negative infinity excludes a key before dot/value
arithmetic; keep masks use nonzero truth.

The f16 pipeline reserves one KiB for a 256-word query cache. Queries with
depth at most 256 decode once per row/key-part/value-tile; deeper queries
load directly. An added workgroup barrier publishes the cache before use.
BF16 keeps its cache-free source without either cost. Global workspace is
unchanged.

## Arithmetic

Q/K/V dtype equality is checked before geometry and empty-result shortcuts.
Normal arithmetic uses decoded f32 values. Normal finite f16 values use
`unpack2x16float` on the selected 16 bits with a zero upper half. Half
subnormals, signed zero and special encodings retain the exact integer
decoder, because portable unpack semantics permit intermediate subnormal
flushing. Finite nonzero f16 values are normal after conversion to f32, so
the f16 product has no BF16-specific scaling branch.

For a nonzero BF16 subnormal Q or K operand, the product lifts that operand
by 64 exponent steps and lowers its partner by the same amount, using
integer IEEE transformations before the
multiply. This preserves normal logits from tiny-times-large inputs in
either operand order. A true subnormal intermediate or final result remains
subject to backend f32 underflow limits.

The exact power-of-two operations introduced for statistics are extracted
into `float_power2.wgsl` and shared by attention. Integer boundaries prevent
Metal from cancelling protective scaling under fast math. The attention
value ratios now normalize subnormal inputs with integer exponent shifts;
score differences halve through the same exact helper. The online normalized
value accumulator and historical-scale reset continue to protect constant
large V and score changes that discard an earlier tile.

Attention requires finite operands, representable allowed f32 dot products
and logits, and finite normalized results, as specified by the common API.
There is no CPU fallback or implicit whole-input conversion.

## Validation and replay

All output ownership, dtype, shape, contiguous-layout and whole-buffer alias
checks precede appending the prepared operation. Cross-type output aliases
with packed inputs or masks are rejected. Output storage offsets are
honored; final low casts preserve adjacent halves of odd physical words.
Foreign masks are checked even when the result is empty. Failed operations
append no passes, including failures before a final low cast.

The focused `tensor_low_attention` target includes the shared f64/reference
fixture plus strided Q/K/V and masks, key boundaries 0/1/33/512/513/4097,
value widths crossing 64, all four recorded result forms, changing inputs,
changed and fully disabled masks on replay, a resident downstream sum,
output sentinels and cross-type/empty validation. Depth 255/256/257,
zero depth strides, odd packed offsets and 65,537 query rows cover load
boundaries and dispatch grid reuse. Both generated dtype shaders are
validated separately by Naga. The [matched benchmark report](tensor-low-attention.md)
records measured speed and storage tradeoffs.

## Focused evidence and memory accounting

The [first required Metal run](tensor-low-attention-initial-metal-tests.txt) passed
all three new low-attention tests plus six existing attention, four packed
statistics and five f32 statistics tests: 18 passed, zero failed. The exact
[initial source hashes](tensor-low-attention-initial-wgsl-source-fingerprints.json) include
`float_power2.wgsl` and every affected shared include. Its functions were
extracted from the corrected statistics implementation with identifier
renaming; their integer arithmetic and rounding rules are unchanged.

For the matched benchmark's decode shape Hq=4, Hkv=2, Lq=1, Lk=4097,
D=Dv=64, both paths choose 17 key parts. Each allocates 17,408 bytes of
normalized partial values and 544 bytes of max/denominator state: 17,952
bytes total. The other four benchmark shapes use one part, so they allocate
no partial-value buffer and retain the eight-byte dummy state binding.
Both paths also have metadata and a four-byte unused-mask sentinel. These
are planner allocation counts, not total driver memory or a measured peak.

The baseline's additional f32 operand storage is exactly four times the sum
of Q/K/V element counts. The direct path removes those three operand buffers;
result buffers and partial scheduling are otherwise identical. Low storage
still includes the existing four-byte word alignment for odd element counts.
The benchmark checks exact quarter-valued inputs against dense f64 attention,
with identical result readbacks and separate GPU-pass and host timings.
Normal BF16 subnormal-times-large behavior is qualified by the shared strict
fixture, separately from the benchmark's ordinary-valued performance inputs.

## Decoder optimization evidence

The cache-free per-dtype candidate removes the dynamic dtype branch and
excludes zero from BF16 subnormal protection. A later query-cache candidate
added 256 decoded workgroup words to both dtypes, but its measured BF16
regressions led to removing the shared version of that cache. Its source overlay, both raw timing runs and depth/grid
coverage remain preserved. The f32 baseline was unchanged in these trials.

The guarded f16 candidate changes only normal f16 conversion. Its
[required Metal tests](tensor-low-attention-guarded-f16-metal-tests.txt) passed
17 tests: 13 library tests and four low-attention integration tests, with zero
failures. Its [source hashes](tensor-low-attention-guarded-f16-wgsl-source-fingerprints.json)
remain a historical snapshot. The library probe calls the actual generated
production query loader and compares all 63,488 finite f16
encodings against an independent f64 oracle, with nine neighboring halfwords,
both parities and nonzero offsets: 1,142,784 exact f32 bit comparisons.
Signed zeros are checked exactly. Special neighboring words include infinities
and NaNs, so native conversion cannot accidentally use an adjacent payload.

The [candidate source overlay](tensor-low-attention-guarded-f16-source-fingerprints.json)
is based on the per-dtype snapshot. The initial, specialized and query-cache
snapshots and paired benchmark logs are retained separately; none are replaced
by later candidates.

The retained final implementation layers the tested bounded cache onto the
guarded decoder for F16 only. Its [required Metal run](tensor-low-attention-f16-cache-metal-tests.txt)
passed 17 tests (13 library and four integration), with zero failures,
including the exhaustive loader, depth boundaries, zero strides and grid
reuse. The [final source hashes](tensor-low-attention-wgsl-source-fingerprints.json)
identify the retained source. The [source overlay](tensor-low-attention-f16-cache-source-fingerprints.json)
changes only the F16 generation branch; BF16 and the f32 baseline remain
unchanged. Both final timing runs and the earlier candidates are compared in
the [benchmark report](tensor-low-attention.md). Small timing changes in the
unchanged BF16 path do not establish an effect from the F16-only cache.
