# CUDA source qualification without a GPU

`qualify-nvrtc.sh` compiles the **production source**, using actual NVIDIA NVRTC
inside a disposable Linux container. It does not load the CUDA driver, execute
a kernel, run cuBLAS or validate numerical results.

Requirements: Docker and an existing Linux image with Python 3 and glibc. The
default is `rust:1-bookworm`; `CUDA_NVRTC_IMAGE` can select another local image.
The script uses its immutable image ID and never pulls a replacement image.

```sh
bash crates/compute-cuda/qualification/qualify-nvrtc.sh /tmp/cuda-nvrtc-results
```

The script downloads NVIDIA's [NVRTC 12.8.93 wheel](https://pypi.org/project/nvidia-cuda-nvrtc-cu12/12.8.93/)
for Linux aarch64 or x86_64. SHA256 is pinned independently of the downloaded
package metadata. Only the shared libraries are extracted into the container's
temporary filesystem; no Python package code or setup hooks execute. Nothing
is installed into the host or an existing container.

Inputs are mounted read-only. Only the supplied output directory is writable.
The container is removed at exit. Source concatenation and compiler options
are read from `src/runtime.rs`: the thirteen CUDA files, precise divide/sqrt,
`ftz=false`, and `fmad=false`. The script queries NVRTC's supported architectures
and applies the runtime's highest-supported-target-not-newer-than-capability
policy to the requested capabilities (default 70, 80, 90, 120). These numbers
are **requested compilation targets**, not detected GPUs.

Outputs:

- `report.json`: source hashes, pinned wheel and library hashes, container ID,
  compiler version, supported targets, options, statuses and PTX hashes.
- `compute-*.log`: complete compiler diagnostic logs, including empty logs.
- `compute-*.ptx`: generated PTX, without the API's terminating NUL byte.

Every successful target must expose all 54 production kernel entry points with
the expected scalar parameter widths. The report records those signatures;
pointer validity and kernel semantics still require runtime qualification.
Compilation is useful evidence for CUDA syntax, template instantiation and PTX
generation. Mandatory NVIDIA execution remains the separate
`CUDA_REQUIRED=1 ... --test cuda_tensor` qualification described in the crate
README.

## Current convolution snapshot: 2026-09-27

The [convolution report](nvrtc-12.8.93-linux-aarch64-convolution/report.json)
qualifies all thirteen production source parts for compute_70/80/90/120 with
NVRTC 12.8.93. `conv_f32` and `conv_low` add direct grouped spatial convolution;
the previous 52 entry signatures remain unchanged. Externally supplied modules
used through `from_ptx` must be regenerated to expose the new entries.
Reports and logs are tracked; generated PTX remains local and can be recreated
with the command above. Recorded hashes identify its exact bytes.

## Historical typed-program snapshot: 2026-09-27

The [typed-program report](nvrtc-12.8.93-linux-aarch64-typed-programs/report.json)
qualifies the earlier twelve-part CUDA source. The new `binary_u32` entry
implements wrapping unsigned add/subtract/multiply and unsigned min/max with
the same broadcast layout ABI as f32 binary arithmetic. Division is rejected
by the host API. All 51 preceding entry signatures remain unchanged.

All four requested targets compiled successfully with all **52 entry points**
and their ordered scalar parameter widths checked. Combined source size:
49,262 bytes. SHA256:
`d60ab5d94f748e432c584b0ec88eb2d0cdb54ba9ca47a25aad572f06c9216eda`.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 993,923 | [Log](nvrtc-12.8.93-linux-aarch64-typed-programs/compute-70.log) |
| `compute_80` | Success | 993,923 | [Log](nvrtc-12.8.93-linux-aarch64-typed-programs/compute-80.log) |
| `compute_90` | Success | 993,987 | [Log](nvrtc-12.8.93-linux-aarch64-typed-programs/compute-90.log) |
| `compute_120` | Success | 2,027,934 | [Log](nvrtc-12.8.93-linux-aarch64-typed-programs/compute-120.log) |

The [host qualification report](../../../docs/qualification/tensor-cuda-typed-programs-2026-09-27.md)
records typed planner/executor tests, compiled native fixtures and explicit
unavailable-device results. Source/ABI compilation does not qualify numerical
execution, cuBLAS performance or Tensor Core instruction use. External PTX
modules must now also export `binary_u32`.

## Historical low-attention snapshot: 2026-09-27

The [low-attention report](nvrtc-12.8.93-linux-aarch64-low-attention/report.json)
qualifies the current 48,291-byte combined source. One new `attention_low`
entry reads native u16 Q/K/V through the existing online attention body.
The f32 entry and every other existing exported parameter ABI are unchanged.
All 51 entry points and ordered parameter widths match on all four targets.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 977,874 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-low-attention/compute-70.log) |
| `compute_80` | Success | 977,874 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-attention/compute-80.log) |
| `compute_90` | Success | 977,938 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-attention/compute-90.log) |
| `compute_120` | Success | 1,998,638 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-attention/compute-120.log) |

Current combined source SHA256:
`d8a6cb52807f369f03f23bc5f01f226027bbbed1e6cee38bb4859b5d21699327`.
The new `compute_80` entry contains native u16 loads and f32-to-f64 conversion
without FTZ on the conversion, before Q/K multiplication or V accumulation.
The shared kernel uses f64 scores, exponentials, sums and output scratch.
These source/instruction checks are compilation evidence only. External PTX
imports need `attention_low` with its trailing dtype argument.

The [source audit](low-attention-source-audit.txt) checks all twelve current
source parts, retained PTX hashes and exact parameter signatures against the
report. [Host tests](low-attention-host-tests.txt) compile the shared/native
fixtures and print SKIP for unavailable NVIDIA hardware; the
[required-device log](low-attention-cuda-required.txt) preserves its explicit
failure. No NVIDIA numerical execution or performance is claimed. Earlier
snapshots remain preserved below.

## Historical low-statistics snapshot: 2026-09-27

The [low-statistics report](nvrtc-12.8.93-linux-aarch64-low-statistics/report.json)
qualifies the preceding 46,317-byte combined source. Three new native-u16 loader
entries share the existing partial, emit and moments implementations. Merge
and logsumexp output are reused unchanged; every prior exported entry keeps
its parameter ABI. All 50 entry points and ordered parameter widths match on
all four targets.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 918,633 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-low-statistics/compute-70.log) |
| `compute_80` | Success | 918,633 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-statistics/compute-80.log) |
| `compute_90` | Success | 918,697 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-statistics/compute-90.log) |
| `compute_120` | Success | 1,899,431 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-statistics/compute-120.log) |

Low-statistics combined source SHA256:
`624b0c91e46398b9038dd23e6d4c92fd829a86cdd021b210855576c0fbb07fff`.
All three new `compute_80` bodies contain native u16 loads and `cvt.f64.f32`
without FTZ on the conversion. F16 input conversion uses `cvt.f32.f16` without
FTZ; BF16 uses bit expansion before widening. Generated exponential/reciprocal
library code does contain approximate FTZ instructions; this audit does not
claim the entire kernel is FTZ-free. No NVIDIA execution or speedup follows
from source compilation. External PTX imports need the three new entries.

The [source audit](low-statistics-source-audit.txt) verifies all eleven then-current
source parts, four retained PTX hashes and every parameter signature. The
disposable container exited and was removed.

The [host log](low-statistics-host-tests.txt) records 13 CPU tests and the doctest,
with unavailable native CUDA printed as SKIP. The
[required-device log](low-statistics-cuda-required.txt) records the mandatory
hardware failure on this Mac. Native low-statistics arithmetic fixtures are
compiled but have not run on NVIDIA. Earlier source snapshots remain below.

## Historical low-scatter snapshot: 2026-09-27

The [low-scatter revision report](nvrtc-12.8.93-linux-aarch64-low-scatter/report.json)
qualifies the preceding 43,196-byte combined source. Two new entries specialize
the existing scatter traversal: native low raw Replace/Min/Max and direct
native low updates into f32 output. The existing f32/u32 entry signatures
remain unchanged. All 47 entry points and parameter widths match on all four
targets.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 835,185 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-low-scatter/compute-70.log) |
| `compute_80` | Success | 835,185 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-scatter/compute-80.log) |
| `compute_90` | Success | 835,249 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-scatter/compute-90.log) |
| `compute_120` | Success | 1,754,569 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-scatter/compute-120.log) |

Low-scatter combined source SHA256:
`2bfe38d408e0208dba2ddc484b60a5011b543b541724b9aa8b7b17afae9544da`.
Both new `compute_80` entry bodies contain native u16 loads and 32-bit CAS,
without FTZ instructions. Raw output uses padded native16 storage for aligned
neighbor-preserving CAS; f32 output never rounds through low storage. These
checks establish source/ABI compilation and emitted instructions, not NVIDIA
numerical execution. External PTX imports need both new entries. The ephemeral
container exited and was removed; earlier snapshots remain preserved below.

The [host log](low-scatter-host-tests.txt) records 13 CPU tests and a doctest
passing, with native CUDA explicitly skipped. The
[required-device log](low-scatter-cuda-required.txt) records exit 101 because
the macOS host has no CUDA driver/device. The shared low-scatter fixture and
native stress cases compile but have not executed on NVIDIA.

## Historical low-indexing snapshot: 2026-09-27

The [low-indexing revision report](nvrtc-12.8.93-linux-aarch64-low-indexing/report.json)
qualifies the preceding 39,613-byte combined source. Five new entries implement
raw u16 comparisons, selection, gather, compaction and direct low-input f32
prefix scans. Existing f32/u32 scan specializations compile through the same
load-policy traversal; their exported signatures are unchanged. All 45 entry
points and their parameter widths match on all four targets.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 796,660 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-low-indexing/compute-70.log) |
| `compute_80` | Success | 796,660 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-indexing/compute-80.log) |
| `compute_90` | Success | 796,724 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-indexing/compute-90.log) |
| `compute_120` | Success | 1,533,198 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-indexing/compute-120.log) |

Low-indexing snapshot combined source SHA256:
`b0f42e12a9e1649900ab1a9a80be5e3fa55e38f8e60b214097e7c6a274abeff8`.
The emitted `compute_80` code uses u16 loads in all five new entries and u16
stores for selection/gather/compaction, with no FTZ instruction in these entry
bodies. This establishes generated instructions, not CUDA execution. The native
suite includes the shared low-index fixture and offset/reuse/ownership tests;
it remains hardware-gated. External PTX modules need all five new entries.
The disposable compilation container exited and was removed. Historical
compiler snapshots below are preserved.

The [host test log](low-indexing-host-tests.txt) records 12 CPU tests and the
doctest passing, with CUDA runtime qualification explicitly skipped. The
[mandatory CUDA log](low-indexing-cuda-required.txt) records exit 101 because
the current macOS host has no CUDA driver/device. Neither log is GPU numerical
qualification.

## Historical low-operation snapshot: 2026-09-27

The [low-operation revision report](nvrtc-12.8.93-linux-aarch64-low-ops/report.json)
qualifies the preceding 37,023-byte combined source. Four new entries cover native
u16 unary/binary arithmetic and native low-input axis/full reductions. The same
reduction skeleton also compiles its existing f32/u32 specializations. All 40
entry points and parameter widths match on all four targets; existing entry
signatures remain unchanged.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 708,092 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-low-ops/compute-70.log) |
| `compute_80` | Success | 708,092 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-ops/compute-80.log) |
| `compute_90` | Success | 708,156 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-ops/compute-90.log) |
| `compute_120` | Success | 1,369,193 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-ops/compute-120.log) |

Low-operation snapshot combined source SHA256:
`aa61a982e27ab58ea06c35e1ded7a0c8c172ac8b8926dabace003f4d8c943f2c`.
The f32 extrema implementation now compares ordered IEEE bits, preserving
BF16 subnormal values and choosing -0 for Min / +0 for Max through every
hierarchy level. Sum/product and the reduction traversal remain unchanged.
Source compilation does not establish numerical correctness on NVIDIA. The
native suite includes the shared low-operation fixture plus exact widened
subnormal extrema over a large offset view. External PTX modules need the four
new entries. The disposable compilation container was removed.

## Historical attention snapshot: 2026-09-27

The [attention revision report](nvrtc-12.8.93-linux-aarch64-attention/report.json)
qualifies the preceding 33,640-byte combined source. Its new `attention_f32` entry
implements online 32-key softmax/value tiles with f64 accumulation, GQA and
resident masks. All 36 entry points and scalar parameter widths match on all
four targets, including the signed causal-offset parameter's 32-bit ABI.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 598,462 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-attention/compute-70.log) |
| `compute_80` | Success | 598,462 | [Empty log](nvrtc-12.8.93-linux-aarch64-attention/compute-80.log) |
| `compute_90` | Success | 598,526 | [Empty log](nvrtc-12.8.93-linux-aarch64-attention/compute-90.log) |
| `compute_120` | Success | 1,165,047 | [Empty log](nvrtc-12.8.93-linux-aarch64-attention/compute-120.log) |

Attention snapshot combined source SHA256:
`0068632b243d7d60f54f94f4bf21652a0637b4fd4415e2064e9896f6f8a592d7`.
The generated attention PTX contains synchronized warp shuffles, f64 multiply/
division and final f32 conversion. Compilation verifies the emitted kernel ABI,
not NVIDIA execution, numerical conformance or performance. No GPU was requested;
the disposable container exited and was removed. External PTX modules require
the new entry.

## Historical statistics snapshot: 2026-09-27

The [statistics revision report](nvrtc-12.8.93-linux-aarch64-statistics/report.json)
qualifies the preceding 27,896-byte combined source. Five new entry points cover
row/chunk partial reductions, merges, normalization, logsumexp and moments.
All 35 kernel entry points and parameter widths match on all four targets.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 545,175 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-statistics/compute-70.log) |
| `compute_80` | Success | 545,175 | [Empty log](nvrtc-12.8.93-linux-aarch64-statistics/compute-80.log) |
| `compute_90` | Success | 545,239 | [Empty log](nvrtc-12.8.93-linux-aarch64-statistics/compute-90.log) |
| `compute_120` | Success | 1,072,419 | [Empty log](nvrtc-12.8.93-linux-aarch64-statistics/compute-120.log) |

Statistics snapshot combined source SHA256:
`e9ccb56638013e9d7e60d0266206e6d225498578abf802143e72c2ca7bbe5cb3`.
The `compute_80` statistics PTX includes `sub.rn.f64`, `mul.rn.f64`,
`div.rn.f64`, `sqrt.rn.f64` and final `cvt.rn.f32.f64` conversions. This confirms
emitted wider intermediate arithmetic; its NVIDIA numerical behavior and
performance remain unqualified. The mandatory device suite now includes the
shared statistics reference plus offset-row and resident-composition fixtures.
External PTX modules must be rebuilt with the new entries.

## Historical low-precision snapshot: 2026-09-27

The [low-precision revision report](nvrtc-12.8.93-linux-aarch64-low-precision/report.json)
qualifies the preceding 22,393-byte combined source, adding native u16 copy and
f32 ↔ f16/bf16 casts. Every target compiled with all 30 entry points and exact
parameter widths. No toolkit headers or extra packages were needed.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 454,841 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-low-precision/compute-70.log) |
| `compute_80` | Success | 454,841 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-precision/compute-80.log) |
| `compute_90` | Success | 454,905 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-precision/compute-90.log) |
| `compute_120` | Success | 933,831 | [Empty log](nvrtc-12.8.93-linux-aarch64-low-precision/compute-120.log) |

Low-precision snapshot combined source SHA256:
`27905752227d9807469d6435d7f76a2226092c8af4ea73c8d906d343ab900e0b`.
Inspection of `compute_80` PTX confirms two-byte `ld.global.u16` /
`st.global.u16` storage accesses and the explicit F16 `cvt.rn.f16.f32` /
`cvt.f32.f16` instructions. BF16 uses integer rounding/bit expansion. These
checks establish emitted code; numerical rounding, allocation behavior and
direct native-input cuBLAS results still require the mandatory NVIDIA test.
No cuBLAS operation is executed by this compiler qualification. External PTX
modules must include the three new entry points.

## Historical scatter snapshot: 2026-09-27

The [scatter revision report](nvrtc-12.8.93-linux-aarch64-scatter/report.json)
qualifies the preceding 20,232-byte combined source. It adds a fresh f32 copy,
axis-owner election and typed scatter folds to the preceding reduction snapshot.
All four targets compiled with all 27 entry points and their exact parameter
widths checked.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 430,307 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-scatter/compute-70.log) |
| `compute_80` | Success | 430,307 | [Empty log](nvrtc-12.8.93-linux-aarch64-scatter/compute-80.log) |
| `compute_90` | Success | 430,371 | [Empty log](nvrtc-12.8.93-linux-aarch64-scatter/compute-90.log) |
| `compute_120` | Success | 865,781 | [Empty log](nvrtc-12.8.93-linux-aarch64-scatter/compute-120.log) |

Scatter snapshot combined source SHA256:
`a528ded6eca381d746304f566a04800260ceec6c9e8a479ca3ebc1a20adadf76`.
Inspection of `compute_80`'s f32 scatter PTX confirms `atom.global.cas.b32`
with `add.rn.f32` and `mul.rn.f32`, without float atomic add or `ftz` arithmetic.
This verifies emitted instructions, not execution or numerical outcomes. The
mandatory native test includes a contended exact-subnormal addition fixture.
New entry points require regenerating external modules used through `from_ptx`.

## Historical reductions snapshot: 2026-09-27

The [reduction revision report](nvrtc-12.8.93-linux-aarch64-reductions/report.json)
qualifies the preceding 15,948-byte combined source, including typed sum/product/
min/max kernels and empty-product fill. Mean uses the existing unary kernel's
new direct-division mode. Every target compiled successfully and exposes all
23 expected entry points with matching parameter widths.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 369,015 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64-reductions/compute-70.log) |
| `compute_80` | Success | 369,015 | [Empty log](nvrtc-12.8.93-linux-aarch64-reductions/compute-80.log) |
| `compute_90` | Success | 369,015 | [Empty log](nvrtc-12.8.93-linux-aarch64-reductions/compute-90.log) |
| `compute_120` | Success | 618,464 | [Empty log](nvrtc-12.8.93-linux-aarch64-reductions/compute-120.log) |

Reduction snapshot combined source SHA256:
`3b674b4ccc3638975a39fb8cd8fab7a26457943c38de11eefc8e55b54eaafc25`.
The changed reduction entry signatures require regenerating any external PTX
used through `from_ptx`. Vector matmul promotion uses the Rust/cuBLAS path;
NVRTC source compilation does not qualify cuBLAS execution.

## Historical indexing snapshot: 2026-09-27

[Initial NVRTC 12.8.93 report](nvrtc-12.8.93-linux-aarch64/report.json) preserves
successful compilation of the preceding 13,767-byte source on Linux aarch64, for all
four requested targets. All 19 entry points and expected parameter widths are
present in each PTX module.

| Target | NVRTC status | PTX bytes | Compiler diagnostics |
| --- | --- | ---: | --- |
| `compute_70` | Success | 297,211 | [Deprecated target warning](nvrtc-12.8.93-linux-aarch64/compute-70.log) |
| `compute_80` | Success | 297,211 | [Empty log](nvrtc-12.8.93-linux-aarch64/compute-80.log) |
| `compute_90` | Success | 297,211 | [Empty log](nvrtc-12.8.93-linux-aarch64/compute-90.log) |
| `compute_120` | Success | 474,711 | [Empty log](nvrtc-12.8.93-linux-aarch64/compute-120.log) |

The combined source SHA256 is
`31b606bb9d6004e675af2eab5ac4ea363a0a151ad75b3eb826b9dfe27c7156c3`.
The report includes per-file source hashes, compiler-library hashes and all
PTX fingerprints. Generated PTX remains available locally and is ignored by
Git; the small report and complete logs are preserved in source control.

The container used the already cached official `rust:1-bookworm` image,
Python 3.11.2 and a 43.1 MB pinned NVIDIA wheel. Its writable temporary filesystem
was discarded at exit. No GPU was present or requested. This changes the
qualification status from “Rust compilation only” to “Rust and actual NVRTC
source compilation”; CUDA execution, cuBLAS and numerical correctness on
NVIDIA remain unqualified.
