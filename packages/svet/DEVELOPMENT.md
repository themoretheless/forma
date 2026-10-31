# Svet development and qualification

The objective is to develop Svet across its complete engine and application
surface. A passing unit suite is evidence for covered cases, not proof that
all features are complete or that performance claims hold on real hardware.

## Qualification gates

- Reactive graph: dynamic dependencies, diamonds, batching, exceptions, nested
  ownership, cleanup, cancellation, slot reuse, equality, and cycle behavior.
  Require regressions plus deterministic model comparisons and graph telemetry.
- DOM and components: keyed updates, branch teardown, event lifetime, forms,
  focus preservation, portals, async boundaries, accessibility, and virtualization.
  Require DOM regressions and real-browser interaction checks.
- Compiler: JSX and Forma syntax, diagnostics, source locations, nested expressions,
  imports, and generated module integration. Require positive and negative fixtures
  and execution of compiled output.
- SSR and resumption: escaping, serialization, request isolation, concurrent async
  work, resource lifetime, event restoration, and parity with client rendering.
  Require independent concurrent requests and browser resumption checks.
- GPU and WASM: allocator reuse, dirty spans, device failure, alignment, numerical
  correctness, and ownership. Require Rust tests, JS integration tests, and hardware
  measurements separating allocation, CPU work, transfers, and GPU execution.
- Distribution: documented public exports, clean consumer installation, type
  declarations, production build, examples, compatibility, and repeatable CI.
  Require a separate consumer build and native/browser checks where applicable.
- Performance: representative end-to-end workloads, repeated baseline/candidate
  measurements, retained raw results, and documented variance. Synthetic baseline
  implementations do not establish superiority over actual frameworks.

## Verified progress in this working session

- Resource cancellation and replacement lifecycle: regressions cover explicit
  abort, disabled source, owner disposal, and overlapping Suspense requests.
- Effect handle lifetime: regressions cover stale handles, owner teardown,
  slot reuse, linked effects, and cleanup counts.
- Dynamic tracking: removed execution shortcuts that inferred static dependencies
  from previous reads. Regressions cover new dependencies, branch switches,
  nested computed sources, retry after a computed throws, and a deterministic
  batched diamond reference model.
- `npm run build` in this package builds the Rust WASM core, embeds it, checks
  TypeScript, and builds the demo. It does not establish consumer packaging or
  real-browser/hardware qualification.

## Remaining work

All qualification gates above remain open beyond the explicitly covered cases.
Nested effect ownership and cleanup on exceptions now have regression coverage:
top-level effects own nested effects and `onCleanup`; failed initialization and
throwing disposal cleanup release graph nodes. Resumption now has regressions for container-local IDs (including detached trees
and duplicate IDs), root annotations, owner/explicit teardown, replacement, and
malformed snapshots. SSR cleanup on exceptions and live form serialization have
round-trip regressions. Next audit compiler/DOM integration and implement genuine
async SSR context propagation, together with the remaining reactive graph cases. Keep
regressions for each reproduced defect.

## Latest local verification (2026-10-31)

- `npm --prefix packages/svet test`: 234 tests pass in 16 files. jsdom reports its
  existing unimplemented canvas context warnings; this is not GPU qualification.
- `npm --prefix packages/svet run build`: Rust WASM release, TypeScript, and Vite
  demo production build pass.
- `cargo test --manifest-path packages/svet/crates/svet-wasm/Cargo.toml`: 13 Rust
  unit tests pass; no Rust doc tests exist.
- `git diff --check`: passes.

Tracking every execution replaces unsafe shortcuts. No runtime speedup or absence
of a performance regression is claimed without controlled workload measurements.

## Async SSR qualification

The Node-only `svet-engine/server` entry now uses AsyncLocalStorage to select a
request-local WASM core across awaits. Requests have ownership roots, release
resources and reset graph registries on completion, error, and cancellation via
AbortSignal. The browser entry retains no static Node dependency.

Regression coverage includes genuinely interleaved requests finishing in reverse
order, effects created after await, nested async and synchronous isolation,
resource cancellation, rejected requests, pre-aborted requests, late work after
teardown, and async render/snapshot/client resumption with escaped content. A
separate real Node subprocess imports public package exports without a bundler or
DOM globals and runs concurrent reactive requests. Type-only import checking is
enabled so native Node type stripping does not attempt to load erased interfaces.

`runWithIsolatedContext` itself remains synchronous. Node tests do not qualify
edge runtimes, independent owner/DI propagation inside async component subtrees,
real browser interactions, or hardware behavior. Full async component rendering and broader compiler/DOM integration remain open;
installed distribution qualification is described below.

## JSX compiler qualification

Replaced preceding-text and brace-counting heuristics with Babel TSX/JSX AST
parsing and MagicString insertions. The compiler now handles adjacent expression
containers, regex/comment braces, literal text resembling calls, object styles,
optional calls, template substitutions, typed callbacks, computed method names,
and nested JSX. It emits high-resolution source maps and positional diagnostics.
The original string API remains available; a dedicated compiler subpath is added.

Tests execute transformed JSX through TypeScript's JSX lowering into the actual
Svet DOM runtime, then verify text, attribute, style and click updates. Source-map
positions are checked by decoding the map with trace-mapping. Native Node package
import remains covered. The Vite production demo build exercises the plugin.

Local browser smoke check: the demo loaded, Count changed 1 -> 2, Atomic Batch
changed Count to 7 and multiplier to 12 (diamond result 98), and editing the title
updated the heading. This is a bounded interaction check, not full browser or
GPU qualification. Synthetic-framework superiority text was removed from the demo.

An intermediate build retained unused compiler initialization (170.84 kB JS,
61.15 kB gzip). Marking compiler code as free of module side effects while retaining
server initialization reduced the same demo to 133.75 kB JS / 49.66 kB gzip,
compared with the earlier 133.73 kB / 49.66 kB build. After the demo caption edit,
the final build is 133.78 kB / 49.66 kB. These are bundle sizes, not
runtime performance results.

Compiler diagnostics and source maps for Forma .ui, full syntax fixture coverage,
DOM event/model teardown, production-browser checks and consumer packaging remain
open alongside the other qualification gates.

## DOM lifecycle and reset qualification

Reproduced and fixed retained model listeners, retained delegated callbacks,
cross-scope callback cleanup, document-global registration, logical currentTarget,
stopImmediatePropagation, and open-shadow event paths/retargeting. Delegated
registrations are reference-counted per owner document. The listener is removed
when the final binding is disposed; tests inspect native registration counts.

Mount replacement releases the previous root, stale unmount handles are inert,
failed view construction releases prepared resources while retaining the previous
view, and parent owner disposal unmounts nested mounts. Reactive style objects
remove obsolete managed properties, including custom CSS properties and changes
between string, object and null values.

Graph generations protect reused WASM IDs after reset. Regressions cover stale
signals, computed values, effect handles, reads, updater callbacks, and resetting
inside an active computed evaluation without restoring old observer edges.

The browser fixture at `tests/browser/dom-lifecycle.html` was
run in the local in-app browser: all 5 native checks passed (model teardown, mount
replacement, event methods/currentTarget, open Shadow DOM, multiple documents).
The fixture can be rerun using the package dev server.
This is not a cross-browser matrix. Closed shadow roots, adoption between
documents, SVG upgrade binding lifetimes, reactive input type changes and other
DOM edge cases still require qualification. Package distribution and all other
remaining gates stay open.

## Installed package qualification

Public exports now point to compiled ESM JavaScript and declarations, including a
reactivity subpath. Package emission rewrites runtime `.ts` imports to `.js` and
includes inline source content in source maps. Explicit archive allowlisting
excludes source demo, tests, scripts, and Rust build output. Package build and
`prepack` require TypeScript only on the authoring side; installed runtime needs
neither TypeScript, Vite nor Rust.

The consumer verifier packs the actual archive, installs it outside this repository,
and checks all public entrypoints with regular Node (no stripping flags), shared
runtime identity, interleaved SSR, generated Forma imports, strict declaration and
JSX checks with `skipLibCheck: false`, a rejecting type fixture, and an external
JSX/Forma Vite build without source aliases. Compiler declarations use a structural
Vite adapter so a runtime consumer does not need Vite's declarations. Forma code
now imports the real package name by default; the source demo opts into its alias.

The external built consumer was also opened in the local browser. Its JSX Count
button changed 1 -> 2 and its independently rendered Forma card changed 1 -> 2.
The retained consumer fixture is under the temporary `svet-consumer-LOncCp` folder;
repeat `npm run test:package -- --keep` to create a new inspectable consumer.
Automated results are saved to `dist/package-verification.json` on each run.

A Svet job has been added to the repository CI workflow using its existing pinned
actions. It builds WASM/package/demo before running engine tests, Rust tests and
the installed-consumer gate. This is configuration, not a verified GitHub CI run.
The full local suite and standard-install consumer gate used macOS arm64 / Node
22.23.4. An additional consumer-only run passed on Node 26.13.0 with install scripts
disabled; it is not full-suite qualification for that version. Other platforms
and the broader Node version range remain unqualified. This session did not publish a package or create a release.

Remaining work includes other DOM edge cases, Forma compiler diagnostics and maps,
GPU numerical/device qualification, async subtree owner propagation, broader
browser/platform coverage and controlled performance measurements.

## GPU arena and numerical qualification (2026-10-31)

Strict buffer mocks reproduced null-buffer uploads, missing full reuploads after
growth/device changes, incorrect aligned byte accounting, simulated transfers
without a device, writes through disposed tensors, freeing reused IDs after reset,
unchecked lengths/indices, global GPU configuration bleed, and leaked scene
producer effects. These are fixed with per-core state, validated lifetimes,
allocated/resized buffers, full reuploads, and producer disposal.

Dirty minima are normalized from signed WASM i32 returns to unsigned values, so
an empty dirty set no longer becomes a negative upload range. Failed queue writes
retain dirty ranges for retry. Rust allocation failure now returns u32::MAX,
checked before allocating a graph node; a Rust regression proves existing arena
contents survive overflow/cap rejection. The Rust suite now has 5 tests.

Numerical regressions compare SIMD chunks and scalar tails against explicitly
rounded float32 multiply/add. Additional regressions cover signed zero, NaN,
float32-equivalent updates, overlapping writes, context boundaries, owner cleanup
and unsupported hardware initialization. The synchronous compute compatibility
entry no longer silently accepts a shader without a fallback: it reports that
hardware WGSL was not implemented at that qualification stage. The asynchronous
hardware entry described below now closes that execution gap.

The native browser fixture `tests/browser/gpu-uploads.html` passed 5/5 checks using
WebGPU buffers and mapped readback: full data preservation after arena growth,
DUMA 16-byte partial upload, UMA 4-byte dirty window, correct readback values, and
no repeated clean uploads. The validation error scope returned no errors. This
qualifies those upload paths in the local browser; no physical zero-copy claim,
performance advantage, device-loss recovery, shader execution, or cross-vendor
qualification follows from it.

The subsequent compute qualification below adds WGSL dispatch and device-loss
invalidation/replacement. Render buffer/texture cleanup, broader allocator and
numerical cases, and controlled GPU measurements remain open.

## WGSL execution and cancellation qualification (2026-10-31)

Added `dispatchGpuComputeKernelAsync`: descriptor/input snapshots, storage bindings,
shader diagnostics, compute pipeline and command submission, mapped readback and
one tracked output write after validation. It reports backend and operation-local
logical upload/readback sizes. An explicit async host fallback uses private copies
when GPU compute is unavailable. Hardware failures do not silently invoke it.

Output revisions prevent older dispatches or readback after tracked host writes
from overwriting newer state. Invalid descriptors and pre-aborted calls do not
supersede a valid pending operation. Caller descriptor mutation cannot redirect
publication. Cancellation covers AbortSignal, owners, graph reset, device changes
and loss; temporary resources are destroyed before a running job rejects.

Device validation scopes are serialized with readback per device. Queue tails are
removed when drained. A shared loss hub uses weak state references, deduplicates
subscriptions and unregisters finished configurations. Its Node subscription is
created outside the request AsyncLocalStorage store. A request integration test
checks detached subscription context, one shared subscription, non-overlapping
scopes, result isolation and resource cleanup across concurrent requests.

Initialization retains its captured owner after await and owns devices it creates.
Late or superseded devices are destroyed; explicitly supplied devices remain
caller-owned. Reconfiguration retains only one current cleanup registration.

New mocked-device regressions cover publication, snapshots, descriptors, float32
readback, cancellation before/after submission and while queued, shader/pipeline/
allocation/validation failures, late initialization, loss and replacement, context
reset, asynchronous fallback failure, arena growth during mapping and ownership.

The native browser fixture `tests/browser/gpu-compute.html` passed 5/5 checks:
two-input WGSL with a seven-element tail, 84 submitted upload bytes / 28 logical
readback bytes, one reactive publication, invalid shader rejection without
fallback/mutation, and device loss followed by successful replacement computation.
An initial fixture attempt reused an already-consumed adapter for a second device;
the backend rejected it. The fixture now requests a fresh adapter for that device.
The replacement check was rerun successfully. This was a test-harness assumption,
not evidence that a device can always be recreated from the same adapter.

This does not establish shader throughput, GPU-vendor/platform parity, resident
GPU-only graphs, concurrent rendering scope compatibility, pipeline/buffer caching,
or automatic adapter-retry policies. These and earlier open gates remain work.

Final checks after these changes: 149 TypeScript tests in 15 files, the full
WASM/package/demo build, the Rust/integration gate, and all 9 installed-consumer
checks pass locally. Both native browser fixtures were rerun against the final
implementation: WGSL compute 5/5 and arena uploads 5/5. CI execution remains
unverified; these are local results, not a published release or performance claim.

## Canvas3D resource lifetime (2026-10-31)

Eight strict-device regressions cover owner teardown, padded uint16 index uploads,
device replacement without a mesh mutation, partial initialization cleanup,
detachment, device loss/replacement, reactive reset, and retry after initialization
failure. Vertex/index buffers, the retained depth texture and canvas configuration
are released together. Context-local renderer notifications avoid adding reactive
graph nodes and unregister on owner teardown or reset.

A canvas bound to WebGPU cannot acquire a 2D context. When the device disappears,
Canvas3D reports `unavailable` if the browser rejects 2D acquisition; it no longer
claims a successful fallback. A separate-canvas adaptive viewport, geometry/index
replacement, resize, and real-browser render-lifetime qualification remain open.
These eight tests use strict mocks, not a hardware driver.

## Adaptive Viewport3D (2026-10-31)

Added a stable div wrapper with separate WebGPU and Canvas2D contexts; legacy
Canvas3D/Scene3D return types remain unchanged. Initial 2D, GPU upgrade, loss to
2D, replacement, initialization-failure fallback and orbit event teardown have
unit coverage. Forma has an explicit Viewport3D tag, and the demo opts in.

The native browser fixture `tests/browser/gpu-render.html` passed 5/5 on this
machine: populated initial 2D frame, GPU triangle submission without validation
errors, device-loss recovery to populated 2D with all three GPU allocations
released, replacement on the original canvas, and teardown of all allocations.
The initial fixture passed a structural device wrapper to canvas.configure;
the browser correctly rejected it as not a native GPUDevice. The corrected
fixture instruments actual native device allocation methods. Screenshot:
`svet-adaptive-viewport.png` in the session visualization directory.

GPU pixel correctness, geometry/index replacement, resize, independent scale
picking parity, and broader browser/driver qualification remain open. The previous
separate-canvas viewport item is now implemented within the above tested scope.

## Reactive viewport dimensions (2026-10-31)

Width/height now accept numbers or reactive accessors. Both canvas drawing buffers,
CSS dimensions, wrapper width and depth attachment dimensions update together.
Old render resources are released on resize; ordinary transforms reuse allocations.
Forma dimension expressions update the viewport and camera aspect. Unit regressions
cover batched resize, unchanged-size resource reuse, initial and later invalid sizes,
recovery to a valid size, stable Forma nodes and scaled 2D picking after resize.

The updated native browser fixture passed 7/7: previous five lifecycle checks plus
GPU triangle center color and resized GPU center color/dimensions with destruction
of old allocations. GPU pixels are sampled synchronously via drawImage into a 2D
snapshot before browser presentation. Screenshot: `svet-viewport-resize.png` in the
session visualization directory. This is bounded pixel evidence, not full-frame,
depth occlusion, clipping, multi-browser or performance qualification.

Resize currently rebuilds the pipeline and all per-renderer GPU allocations.
Geometry/index replacement, GPU/2D projection/picking agreement, attachment-only
resize optimization and broader pixel/depth validation remain open.

## Reactive mesh replacement and geometry validation (2026-10-31)

Canvas3DProps.mesh accepts a mesh or accessor. Replacement releases old render
allocations, rebuilds capacity/topology, drops old vertex dependencies and uses
the new mesh for picking. Tests replace an indexed triangle with a larger line
mesh and back, verify old transforms no longer redraw, and check replacement
vertex picking. Mesh lifetime remains caller-owned; annotation edges/faces are
independent. Direct index mutation is not a reactive topology update API.

createMesh3D rejects malformed xyz/triangle triples, non-finite float32 positions,
and invalid/out-of-range uint16 indices before creating reactive mesh nodes.
Regression coverage includes the exact 65535 valid / 65536 rejected boundary.

The native render fixture passed 8/8 with a larger replacement indexed mesh,
GPU center pixel checks, old-buffer destruction and no validation errors, then
loss/fallback/replacement/teardown. Screenshot: `svet-mesh-replacement.png` in the
session visualization directory. Final TypeScript suite: 171 tests in 16 files.
Remaining work includes GPU/2D projection and picking agreement, clipping/depth
pixel cases, uint32 topology, mutable index signaling and allocation optimization.

## Viewport projection and vertex visibility (2026-10-31)

Removed the 0.65 scale from Canvas2D projection and Rust picker, matching the
unchanged WGSL/WebGPU viewport mapping. Returned pick distance uses the same
projection. Rust validates finite query values, positive viewport sizes and
nonnegative radius; it excludes non-finite, nonpositive-w and frustum-clipped
vertices. Canvas2D applies the same visibility rule to vertex markers.

Regressions cover an off-center vertex at (180,60), rejection at its former
incorrect (152,74) position, clipped/behind-camera points, small positive w,
invalid queries, marker coordinates and Rust/JS distance agreement. Current
checks: 173 TypeScript tests and 7 Rust tests. The native fixture passed 9/9,
including an off-center 2D marker pixel, GPU coverage near the triangle corner,
and event-driven picking on both visible surfaces. Screenshot:
`svet-projection-picking.png` in the session visualization directory.

This does not qualify full 2D polygon/segment clipping, hidden-face vertex
occlusion, whole-frame raster equivalence or cross-browser parity. In particular,
2D edges/faces still need homogeneous clipping when crossing clip planes.

## Native homogeneous 2D primitive clipping (2026-10-31)

Rust owns segment interval clipping and convex polygon clipping across all six
WebGPU homogeneous planes. An internal WASM scratch result is projected/transported
by Canvas2D. Results are copied to ordinary screen points before the next scratch
call; marker tensor views are reacquired after potential WASM memory growth.
Clipped faces use their resulting average depth for sorting. Negative/out-of-range
annotation indices and non-finite or unprojectable degenerate primitives emit no path.
The GPU path now returns before constructing unused CPU screen points.

Tests cover near/side-plane segment endpoints, clipped and wholly invisible convex
faces, invalid indices, scratch clearing, arena bounds, six explicit segment planes,
reversal, and 500 deterministic segments against interior reference samples.
Current suite: 175 TypeScript tests in 16 files and 10 Rust tests.

The browser fixture passed 10/10: a quad straddling the near plane produces no
2D pixel on the rejected side and retains its visible half; corresponding GPU
samples show the same coverage with no validation errors. Existing lifecycle,
resize, geometry replacement, picking and pixel checks also pass. Screenshot:
`svet-homogeneous-clipping.png` in the session visualization directory.

This covers convex quad annotations and segments, not arbitrary non-convex faces,
zero-w apex degeneracies, per-pixel depth sorting equivalence, face-occluded vertex
picking or full-frame/cross-browser qualification. Boundary raster rules and more
numerical cases remain open. Scratch clipping allocates native temporary vectors;
no throughput improvement has been benchmarked.

## Explicit uint32 mesh indices (2026-10-31)

createMesh3D now accepts `indexFormat: 'uint16' | 'uint32'` with uint16 as the
compatible default. Input validation checks integer/range constraints before
constructing reactive nodes. Mesh3D.indices is a Uint16Array/Uint32Array union;
render upload uses its actual byte width and setIndexBuffer uses the matching
format. Reactive mesh replacement can switch formats in either direction.

Regressions cover exact upload of index 65536, format switches and invalid uint32
values/runtime formats. Current TypeScript suite: 178 tests in 16 files. Rust
computation and its 10-test suite are unchanged. Installed-consumer fixtures now
exercise the uint32 constructor and check its public declaration.

The native browser fixture passed 10/10 after replacing its growth mesh with
65,537 vertices and indices [0,1,65536]. Its interior GPU pixel would disappear if
the high index were truncated to zero; validation, device loss/replacement and
resource cleanup also pass. Screenshot: `svet-uint32-indices.png` in the session
visualization directory. Larger-scene throughput, mutable topology signaling,
primitive/depth edge cases and face-occluded picking remain open.

## Reactive in-place topology updates (2026-10-31)

Mesh3D.updateIndices copies and validates new indices before committing topology.
The indices getter tracks a lifecycle-guarded signal; renderers detect its new
snapshot and recreate buffers/pipeline as needed. Omitted format retains the last
successful choice. Empty indexed topology draws zero indices, while undefined
selects nonindexed lines. This fixes the old empty-array path that drew all vertices.

Tests cover buffer growth, uint16/uint32 switches, input copying, invalid-update
preservation, empty/nonindexed transitions, batched read tracking, disposal/reset/
owner teardown and foreign-context rejection. Current suite: 184 TypeScript tests
in 16 files. Installed-consumer runtime checks exercise index updates too.

The native browser fixture passed 11/11: updating a uint32 mesh to [] clears its
GPU triangle pixel and restoring [0,1,65536] restores the pixel on the same mesh,
with old allocations destroyed and no validation errors. Screenshot:
`svet-topology-updates.png` in the session visualization directory.

Topology changes currently rebuild all renderer GPU allocations rather than only
the index buffer. Caller annotations and vertex picking do not filter by indexed
membership. Occluded picking, buffer reuse, numerical/raster edge cases and broader
browser/driver/performance qualification remain open.

## Reusing topology GPU resources (2026-10-31)

Renderer initialization, pipeline topology selection and index uploads now have
separate lifetimes. Index snapshots upload only when changed. Updates within
capacity reuse the index buffer; growth creates one larger index allocation and
destroys its predecessor. Vertex/depth resources remain live. Changing uint16 to
uint32 reuses capacity where sufficient; only triangle/line mode changes require
a new pipeline. Device changes, mesh replacement, resize, teardown and synchronous
failure still perform full resource cleanup.

Current suite: 187 TypeScript tests. Regressions verify capacity reuse/growth,
unchanged indices skipping repeat upload on transforms, zero-index restoration,
mode changes preserving vertex/depth allocations, and upload failure cleanup with
retry from the current snapshot. These update the old full-rebuild expectations.

The native fixture passed 11/11 with counters on actual GPUDevice allocation and
pipeline methods: empty/restore updates make no new buffer/texture/pipeline;
growth makes one buffer, destroys one old index buffer, preserves the pipeline
and still renders the expected interior pixel. Loss/replacement/teardown checks
continue to pass. Screenshot: `svet-index-buffer-reuse.png` in the session
visualization directory. This proves resource reuse in the exercised cases, not
frame-time or throughput improvement. Resize/mesh replacement reuse, asynchronous
render validation, large-scene limits and occluded picking remain open.

## Depth-only viewport resize (2026-10-31)

Resize now retains vertex/index buffers, pipeline and canvas configuration while
replacing the depth texture/view at the new dimensions. A new attachment becomes
current only after successful view creation; the previous texture is then destroyed.
On a synchronous texture/view failure, both old and partially created resources are
cleaned and the adaptive viewport falls back to 2D. A later draw retries initialization.

Current suite: 189 TypeScript tests. Resize regressions assert geometry/pipeline
reuse, old-depth destruction, new attachment sizes, failure cleanup and recovery.
The native browser fixture passed 11/11, including grow and shrink with exactly one
new depth texture each, unchanged geometry/pipeline counts, old attachment destruction,
correct GPU pixels and no validation errors. Screenshot:
`svet-depth-only-resize.png` in the session visualization directory.

This is resource-lifetime evidence, not a frame-time benchmark. Mesh-replacement
reuse, asynchronous render error scopes/limits and face-occluded picking remain
open. Synchronous failure tests use strict mocks; the native device checks cover
successful resize, not forced native allocation failure.

## Asynchronous render validation (2026-10-31)

Render operations now open/pop a validation scope synchronously per frame and
observe its result asynchronously. An error releases GPU render resources, records
DOM diagnostic attributes and draws an independent 2D frame. Reactive updates
retry normal initialization. Scope handlers attach outside the request async
context and refer weakly to the lifecycle target. Teardown/reset disable that
target, and resource epochs invalidate pending results. Newer validated success
supersedes older failures, while merely pending newer frames cannot hide errors.
A device without queue.submit cannot claim a successfully submitted GPU frame.

Current suite: 198 TypeScript tests in 16 files. Added regressions for async failure
and recovery, out-of-order frame/device results, late disposal/reset responses,
rejected promises, balanced scope popping on synchronous errors, missing submit,
and a stream of pending frames with an unresolved failure.

The native fixture passed 12/12. Its device injects a real invalid pipeline vertex
stride; validation failure destroys allocations and restores a populated 2D frame.
Disabling the fault and updating topology recovers both a passed scope and a GPU
triangle pixel, then teardown destroys the recovered allocations. Screenshot:
`svet-render-validation.png` in the session visualization directory.

The status is validation completion, not presentation/queue timing. Out-of-memory
and internal error scopes, proactive device limit checks, pending-scope throughput,
mesh replacement resource reuse and face-occluded picking remain open.

## Render device limit preflight (2026-10-31)

Canvas rendering checks exposed maxTextureDimension2D and maxBufferSize before
creating a GPU context, buffer, texture or pipeline. Checks use actual padded
allocation sizes (minimum vertex 64 bytes and index 16 bytes), not logical data
length alone. Over-limit resize/topology cleans prior resources, records the
failure and uses the independent 2D surface; a fitting update can recover.

Current suite: 202 TypeScript tests in 16 files. Regressions cover exact texture
limit acceptance, rejection before any allocation, oversized vertex buffers,
index growth beyond maxBufferSize, over-limit resize cleanup and recovery.

The native browser fixture passed 13/13: it reads the actual device texture limit,
requests limit+1, confirms no additional GPU allocations and populated 2D pixels,
then resizes to a fitting size and verifies both a passed scope and GPU triangle
pixel. Screenshot: `svet-device-limits.png` in the session visualization directory.

This is preflight for the two exposed render limits, not all WebGPU limits or a
physical memory guarantee. OOM/internal error scopes, queue/backpressure behavior,
mesh replacement reuse and face-occluded picking remain open.

## Render OOM/internal error scopes (2026-10-31)

Frames now push validation, out-of-memory and internal scopes and pop each in
reverse order within the same synchronous draw. Group observation reports success
only once all filters return null, but acts on the first error/rejection immediately.
Handlers retain only a weak lifecycle target. Partial setup closes all scopes
successfully opened before failure. Existing epoch/disposal and newer-success
protection apply to every filter. Reference: W3C WebGPU section 22.3 Error Scopes,
https://www.w3.org/TR/webgpu/#error-scopes (checked 2026-10-31).

Current suite: 206 TypeScript tests in 16 files. New regressions cover OOM/internal
errors with other results still pending, all-filter success in different order,
and partial setup cleanup. Prior stale-frame, reset and scope rejection tests now
run through grouped three-filter results.

The native browser fixture passed 14/14. Native GPUDevice accepts all three scopes;
the test injects GPUOutOfMemoryError/GPUInternalError objects into native pop results,
checks destruction of real allocations and 2D fallback, and recovers normal GPU
rendering after fault removal. This is injected result handling, not actual GPU
memory exhaustion, an internal hardware fault or a driver crash qualification.
Screenshot: `svet-all-error-scopes.png` in the session visualization directory.

Compute dispatch still uses its separate validation scope; extending its error
filter coverage, bounded pending-frame work, mesh replacement reuse and occluded
picking remain open. Three-scope per-frame overhead is not benchmarked.

## Compute OOM/internal error scopes (2026-10-31)

Async compute now captures all three GPU error filters. Cleanup pops the complete
group synchronously before awaiting any result; Promise.all gates publication,
rejects on any filter failure and attaches handlers to all pending results. A
partial push failure still closes earlier scopes. The per-device queue continues
to serialize transactions; cancellation still waits for started resource cleanup.

Current suite: 210 TypeScript tests in 16 files. New tests cover OOM/internal
rejection without output mutation/fallback, following-operation recovery, partial
setup cleanup and readback remaining private while one filter is still pending.
Existing shared-device, concurrent SSR and cancellation tests now expect three
nested scopes per transaction rather than one.

The native compute fixture passed 6/6: real WGSL/readback, shader rejection,
injected GPUOutOfMemoryError/GPUInternalError results preserving output and freeing
all native temporary buffers, successful retry, and loss/replacement recovery.
Screenshot: `svet-compute-error-scopes.png` in the session visualization directory.
Injected results are not real physical OOM or hardware/internal failure testing.

Pending-work backpressure, mixed rendering/compute scheduling qualification,
mesh replacement reuse and face-occluded picking remain open. No throughput claim
is made for the added error scopes.

## Rust indexed surface occlusion picking (2026-10-31)

Added context-local uint32 index scratch and a native visible-vertex picker. Rust
clips triangle occluders before perspective division, builds projected triangle
fans and tests candidate depths with barycentric interpolation. Hidden candidates
are excluded before the existing score, allowing a nearby visible vertex to win.
JS transports indices and reacquires tensor views after possible WASM memory growth.
Standalone picking defaults to indexed occlusion; none is explicit through-surface
selection. Viewport defaults to indexed on WebGPU and none on 2D wireframe, with
pickOcclusion overriding either mode. Triangle membership does not restrict candidates.

Current checks: 214 TypeScript tests and 12 Rust tests. Cases include hidden center
vertex, nearby visible candidate, self/coplanar vertices, triangle winding, near-plane
clipping, different homogeneous w, uint32 indices above 65535, topology changes and
viewport overrides. Direct x/w division also avoids reciprocal overflow for tiny
positive w in native viewport projection.

The native browser render fixture passed 15/15: a GPU triangle pixel is present,
its hidden center vertex is not picked, explicit none selects through it, removing
indices exposes the vertex, and restoring indices hides it again. Screenshot:
`svet-occlusion-picking.png` in the session visualization directory.

This is same-mesh geometric occlusion with 1e-6 NDC depth bias and barycentric
numerical tolerances, not bit-exact GPU raster/depth visibility, annotation-face or
cross-mesh occlusion. No spatial acceleration is present; complexity and large-scene
latency remain to be measured. Zero-w clipping degeneracies and edge coverage remain
open alongside scheduling/backpressure and broader platform qualification.

## Mesh replacement GPU capacity reuse (2026-10-31)

Removed unconditional renderer teardown on mesh identity changes. Vertex capacity
is tracked separately and grows only when required. Index snapshots/capacity keep
their existing independent lifetime; depth texture and compatible pipeline remain
live. New geometry always uploads current vertex data; nonindexed draws use the
current vertex count. A smaller mesh retains peak GPU capacity until cleanup.

Current suite: 217 TypeScript tests. Regressions cover fitting replacements with
format changes, growth with retained depth, shrink using current draw counts,
old dependency detachment, and failed vertex growth cleanup/retry.

Native browser fixture passed 15/15: replacing with a 65,537-vertex uint32 mesh
creates only one vertex allocation and destroys its predecessor; returning to a
small mesh keeps live buffers/depth/pipeline and retains the expected pixel. Device
loss/replacement, validation and teardown checks continue to pass. Screenshot:
`svet-mesh-buffer-reuse.png` in the session visualization directory.

No frame-time/throughput gain is claimed. Peak-capacity retention policy, redundant
vertex uploads, bounded pending-frame work and picking acceleration remain open,
as do bit-exact depth/raster behavior and broader platform qualification.

## Coalesced frames and per-viewport GPU backpressure (2026-10-31)

Added renderMode sync/animation-frame to Canvas3DProps and Forma. Initial drawing
stays immediate; deferred effects track mesh/vertices/topology/dimensions while a
single request flag retains the latest state. requestAnimationFrame drawing uses
the captured WASM core without adding graph dependencies. Each scheduled viewport
holds one GPU submission credit until queue completion and all error scopes pass.
Queue rejection after successful validation still fails the frame. Scope failure
shows 2D and releases resources, but GPU retry retains a drain token until the
submitted work completes. Late results
cannot release another device/frame's credit. Teardown/reset cancel callbacks.
The demo opts in; sync remains the default. Missing RAF uses sync, and scheduled
GPU rendering without queue completion support uses 2D.

Current suite: 227 TypeScript tests. New cases cover burst coalescing, both completion
requirements, latest vertex upload, CPU resize scheduling, late disposal/reset,
queue rejection/retry, device replacement, missing completion support, Forma
mount/unmount and invalid modes.

The native scheduling fixture passed 5/5: 100 updates produce 101 synchronous
submissions versus 2 scheduled submissions (initial plus latest). A controlled
completion gate holds the initial scheduled frame and prevents extra submissions;
release uploads exact latest vertices. All buffers are destroyed on teardown and
no later submissions appear. An injected validation failure falls back to 2D but
retains queue backpressure until the controlled completion gate releases, then
recovers on the latest state. Screenshot: `svet-frame-scheduling.png` in the session
visualization directory. One CPU update-loop observation was 1.90 ms sync and
0.40 ms scheduled, one burst in fixed order with an injected gate. These are not
randomized/warmed distributions, actual slow-GPU measurements, GPU frame times or
end-to-end throughput evidence.

The credit is per viewport. Global multi-viewport/compute scheduling, maximum
physical queue load, long-running stress, fair benchmarks and coherent presented-
frame picking still require qualification. Signal/model computations are not
coalesced by this rendering mode.

## Picking coherent with deferred draw/submission snapshots (2026-10-31)

Viewport callbacks now use copied clip vertices and indices from the last drawn
Canvas2D frame or submitted GPU frame, not the newer live model waiting for RAF or
queue credit. A fourth callback argument exposes VertexPickFrame3D mesh identity,
draw id, width/height and backend; the first three numeric arguments are unchanged.
Snapshots survive source mesh disposal, replace only on drawing and clear on
teardown/reset/unavailable surfaces. Invalid dimension updates retain the unchanged
frame. Synchronous callback invocation restores the captured WASM core.

Rust receives context-local vertex/index scratch and reuses native occlusion math
through an isolated temporary graph with no reactive nodes. Live arena contents,
length and node counts remain unchanged. Native scratch buffers retain capacity.
Snapshot copying happens only when onVertexPick exists; no draw-copy performance
claim is made.

Current checks: 234 TypeScript tests and 13 Rust tests. Regressions cover pending
CPU/GPU vertex changes, replaced/disposed mesh identities, delayed index occlusion,
frame metadata, invalid dimensions, failed device-change redraw, isolated callback context and unchanged live
arena/nodes. Rust directly checks scratch ownership and live-geometry preservation.

The native scheduling fixture passed 6/6. With completion held, changing a vertex
leaves old-position picking active. After the next GPU submission, the old position
stops hitting and the new position returns a newer frame id and expected mesh.
Screenshot: `svet-frame-picking.png` in the session visualization directory.

This is draw/submission coherence, not compositor presentation acknowledgment or
bit-exact raster/depth visibility. Per-frame snapshot copies, per-click transport,
large-scene memory/latency, spatial acceleration and stronger presentation-aware
selection remain open. Standalone pickNearestVertex3D still queries live geometry.

### Bounded hardware compute admission

Per-device admission now caps unfinished hardware compute calls at 32 across all
reactive contexts. Rejection precedes tensor snapshot copies and output revision
reservation, so overload cannot supersede accepted output. Credits survive queued
cancellation until closures drain, and release after validation/execution failure.
Separate devices have independent capacity; host fallback behavior is unchanged.
This limits queued entry count, not aggregate tensor bytes or runtime duration.

Validation: 238 TypeScript tests in 16 files pass, including four new regressions
for overload/output preservation, cancelled retained entries, failure recovery and
shared-device isolated contexts with independent-device progress. Package ESM/types
build and full TypeScript checking pass. GPU mocks verify admission behavior; this
change does not claim a new native GPU throughput or memory benchmark.

### Shared compute snapshot byte budget

Hardware admission now reserves both input snapshot bytes and private output-copy
bytes before any tensor data copy. The default budget is 256 MiB per device;
configureGpuSubsystem accepts maxPendingComputeBytes (nonnegative safe integer,
zero allowed). Explicit updates are shared across contexts using that device;
omission preserves the setting. Reducing it never evicts accepted work. Credits
remain reserved for cancelled queued closures until drainage, matching the
32-entry admission limit. Validation/execution failures return both credits.

Validation: 248 TypeScript tests in 16 files pass; full TypeScript checking and
package ESM/type compilation pass. Ten added cases cover exact byte boundary,
output preservation, shared configuration and reduction, invalid settings,
zero/missing-device handling, failure recovery and queued cancellation retention.
The budget excludes live WASM arenas, GPU allocations, mapped readback/result
copies, JS overhead and host fallback execution. No process-wide memory ceiling
or native GPU performance improvement is claimed.

### Scene-producer revision safety

Reproduced ten stale-publication failures: pending hardware compute and async host
fallback could overwrite a newer local/world transform, camera projection, mesh
MVP matrix or transformed vertex tensor. Native scene producers notified reactive
dependents but did not increment the tensor revision used by async publication.
All five producer paths now revise the tensor before notifying dependents. Parent
transform, camera and source-vertex updates supersede pending output consistently
with explicit tensor.write/setAt and host-kernel operations.

Validation: the ten regressions failed before the fix (compute resolved instead
of rejecting), then pass while preserving the exact newer tensor contents and
releasing temporary GPU buffers. The full suite passes 258 TypeScript tests in
16 files; ESM/type compilation and TypeScript checking pass. Hardware scheduling
is exercised with the deferred GPU mock, not a new native-device qualification.
Raw borrowed-view mutations and direct raw WASM calls remain unversioned.

### Scene dependency validation and descriptor capture

Parent transforms and mesh transform/camera dependencies are now checked via
untracked live views before allocating child signals or arena tensors. Foreign,
disposed or reset-invalidated dependencies reject at construction without leaking
new child nodes. The constructors snapshot descriptor fields, so editing the
caller's parent/transform/camera fields cannot redirect a later reactive update.
This is shallow descriptor capture; original dependency signals stay reactive.

Validation: five new regression cases reproduced leaked nodes and silent parent
redirection before the fix; a sixth checks retained camera identity and continued
reactivity after descriptor mutation. All 264 TypeScript tests in 16 files pass,
as do package ESM/type compilation and full TypeScript checking. The checks cover
dependency preflight, not rollback from arbitrary allocator or raw WASM failures.

### Transactional ArrayLike tensor writes

Three regressions reproduced partial unnotified writes on source getter failure,
silent lost writes after source-triggered WASM memory growth, and writes through
stale views after source-triggered disposal/allocation reuse. Non-typed ArrayLike
sources now convert into a private Float32Array before touching live storage.
The destination view and lifetime are reacquired afterwards. Length is captured
once, conversion is limited to the portion that fits, and typed-array sources
retain their existing overlap-safe TypedArray.set path.

Five added cases cover these failures, bounded conversion, and a failed write
while GPU readback is pending (accepted output revision and data remain intact).
User getter side effects are not rolled back. This does not change arbitrary
raw borrowed-view writes or direct WASM calls into transactional operations.

Validation: all 269 TypeScript tests in 16 files, full TypeScript checking and
nine isolated installed-package gates pass on Node 22.23.4, darwin arm64.
The three original failure cases failed before the fix. No new native GPU
performance or memory qualification is claimed by these host-write regressions.

### Tensor constructor source lifetime

Initial ArrayLike conversion now completes before any native arena/node allocation.
A generation check after length access and conversion rejects reentrant graph reset
without creating a stale node in the replacement generation. Initial values are
snapshotted for typed arrays too: a source borrowed from WASM must survive memory
growth caused by the destination arena allocation. Reads after construction still
return live zero-copy views; initialization pays a temporary float32 snapshot.

Four regressions failed before the fix: source exceptions leaked a node, source
memory.grow lost values, and reset during length/element access leaked or returned
invalid objects. A fifth test copies a 1M-element borrowed WASM view, confirms the
native allocation actually changes the memory buffer, and verifies both endpoints
and the original source. All 274 TypeScript tests in 16 files pass, as do full
TypeScript checking and package ESM/type compilation. This validates source
conversion and generation isolation, not rollback from arbitrary native traps.

### Compute queue observability

GpuDeviceTelemetry now exposes pendingComputeCount, pendingComputeSnapshotBytes,
maxPendingComputeCount and maxPendingComputeBytes. Values describe the configured
physical-device queue across reactive contexts, including running work and retained
cancelled queue entries. Snapshots are fresh plain objects, not reactive signals;
saved telemetry does not change. Device replacement selects the new device's
counters and budget; no-device contexts report zero occupancy and default limits.
Existing explicitly recorded devtools snapshots receive these fields automatically.

Four new cases verify shared occupancy, cancellation retention and drain-to-zero,
old snapshot immutability, device switch/budget selection, validation/execution
failure recovery and exclusion of host fallback/arena transfer traffic. All 278
TypeScript tests in 16 files pass. Installed-package runtime and strict typing
fixtures now check all four public telemetry fields. Byte occupancy describes
reserved logical snapshot storage, not JS heap or GPU-driver residency.

### Atomic synchronous host compute

The synchronous dispatch compatibility entry now snapshots descriptors, inputs and
initial output. Kernels run on private arrays and publish via tracked tensor.write
only after synchronous success and generation/owner/output-revision checks. Failed
callbacks preserve live output and the revision of earlier accepted async work.
Reentrant writes, disposal/reuse, owner disposal and reset cannot be overwritten.
Returned thenables reject synchronously; their late rejection is observed and
retained working arrays never alias live WASM output. This adds temporary copies
and changes callback input mutations to private working mutations. Other explicit
user side effects are not rolled back.

Five regressions failed before the fix, covering partial writes, memory growth,
recycled output, reentrant writes and async callback misuse. Three additional cases
cover owner disposal, reset/replacement and failed-sync-versus-pending-async output.
The installed-package runtime fixture now checks sync failure atomicity and a
successful sync fallback in addition to its async fallback gate.

Validation: all 286 TypeScript tests in 16 files, full TypeScript checking,
ESM/type compilation and nine isolated installed-package gates pass on Node
22.23.4, darwin arm64. These are host callback publication guarantees, not
new hardware GPU performance or driver qualification.

### Typed compute admission errors

Count and snapshot-byte admission failures now use exported GpuComputeCapacityError.
The reason discriminates count versus snapshot-bytes, requestedSnapshotBytes is
null for pre-inspection count failures, and queue holds a frozen numeric-only
rejection-time snapshot. No device, tensor or mutable live telemetry references
are stored in the error. GpuComputeQueueTelemetry is exported and shared by the
error and existing device telemetry to avoid diverging field contracts.

Five added tests cover exact demand/occupancy, immutable historical snapshots,
count-limit pre-inspection behavior and non-capacity classification of pipeline,
descriptor and fallback failures. All 291 TypeScript tests in 16 files pass.
Installed-package runtime checks now execute byte rejection on a device stub that
throws if any GPU allocation/compilation starts, asserting actual class identity,
reason and requested size; strict consumer typing checks class narrowing.
Capacity rejection is not a validation certificate or an automatic retry scheduler.

Validation: full TypeScript checking, ESM/type compilation and all nine isolated
installed-package gates pass on Node 22.23.4, darwin arm64, alongside the 291-test
suite. These checks qualify admission/error handling, not physical GPU allocation
failure or driver-level performance.

### Manual disposal owner-registration retention

Reproduced owner retention: 1000 manually disposed tensors left 1000 cleanup
closures, and 100 transform/camera/mesh replacement cycles left 900 closures.
Tensor, effect and computed manual disposal now unregisters its live-owner cleanup
before releasing storage/entries. Tensor disposal clears cached memory views, and
tensor construction checks captured owner liveness after source conversion.
Failed effect initialization also unregisters its parent callback through disposal.

Effect cleanup now detaches its disposable list before invocation. Otherwise a
cleanup that disposes a sibling resource can shift the live array and run twice
once manual disposal unregisters callbacks. Root teardown retains reverse cleanup
order and stale/public-handle lifetime isolation.

Six added regressions cover 1000 tensor cycles, 100 scene replacements, 1000
computed/effect cycles, 100 initialization failures, owner disposal during tensor
construction and single-invocation sibling cleanup. All 297 TypeScript tests in
16 files pass, as do full TypeScript checking and ESM/type compilation. Assertions
measure retained registration/child-scope counts and live graph nodes; they are
not a process-wide heap/driver memory benchmark.

### Complete scene ownership

Transform and perspective-camera owner cleanup previously released matrix tensors
and effect computations but left their three/four ordinary parameter signals live.
A full scene left seven nodes after root teardown, and effect-owned reconstruction
added seven leaked nodes on every rerun. Both constructors now register complete,
idempotent aggregate disposal, unregistering that callback on manual disposal.
Borrowed parent transforms and cameras are preserved; dependency ownership does
not become recursive. Resources created outside an owner still require disposal.

Five new regressions failed before the fix: transform/camera/full-scene teardown,
borrowed dependency preservation and stable node counts across 20 scene reruns.
All 302 TypeScript tests in 16 files pass, along with full TypeScript checking
and ESM/type compilation. The installed-package runtime fixture now builds a full
scene under a root and verifies its live node count returns to zero on teardown.
These are node/lifetime guarantees, not a general process heap residency bound.

### Native validated perspective cameras and atomic projection changes

Added Rust's shared pure perspective matrix builder and a WASM validation ABI.
Both validation and publication use the exact float32 coefficients, reject invalid
frusta/nonfinite or degenerate coefficients, and preserve destination/dirty state
on invalid raw matrix writes. Depth translation now uses near*(far/(near-far))
instead of the overflowing near*far intermediate; a 1e20..1e30 range stays finite.

The TS camera constructor validates before node allocation. Parameter set/update
wrappers validate before publication, preserving the last valid parameter, matrix
and mesh data on failure. New setProjection(PerspectiveCamera3DOptions) validates
a complete candidate and batches native parameter publication, allowing coupled
near/far changes without an invalid intermediate configuration. Its options type
is exported. Lifetime checks cover disposal during an updater and grouped updates.

Validation: 323 TS tests in 17 files, 16 Rust tests, the three root Node/WASM
integration checks and the complete WASM/types/package/demo build pass. Twenty-one
new TS cases cover invalid domains, float32 underflow/overflow/collapsed depth,
no-allocation failure, set/update preservation, grouped single publication,
large-range finite output and reentrant disposal. Three Rust cases cover the
native validator, unchanged invalid-write state and large-depth endpoint mapping.
Embedded WASM is 49,659 bytes. Installed-package fixtures exercise the real native
validation, atomic API and exported option type. Finite matrix validation does not
certify arbitrary scene conditioning, raster visibility or native GPU performance.

### Reentrant signal/computed lifetime checks

Reproduced four failures when user callbacks dispose their own node and allocate
a replacement at exactly the same native ID. Signal updater/equals callbacks
could still update the stale cached value and notify the replacement's consumers.
Computed evaluation/equals callbacks could cache their returned value and commit
old dependencies/state onto the replacement slot. Signal writes now recheck
both disposal and generation after callbacks. Computed evaluation rechecks after
compute/equals, skips commits after disposal, restores enclosing observer state,
and does not register the recycled ID as an outer observer dependency.

Six new cases exercise actual ID reuse for updater/equals/evaluation, outer
observer independence and throwing disposed computations. Four cases failed before
the fix. All 329 TypeScript tests in 17 files pass, along with full TypeScript
checking and package ESM/type compilation. Installed-package runtime checks also
exercise reentrant signal and computed disposal against the embedded native graph.
These changes preserve lifetime isolation; they do not define transaction rollback
for arbitrary other user callback side effects.

### In-flight effect disposal and pooled-shell isolation

Two regressions reproduced a live callback returning into a pooled shell already
reused by a replacement, and disposal from old cleanup causing double cleanup plus
an unwanted new callback run. Running depth now prevents shell recycling until
all active invocations exit. Disposed effects skip dependency commits; pooled
entries clear callback, owner and dependency references before reuse. Cleanup is
detached before user invocation, and returned late cleanup executes immediately.

Cleanup registration passed to a callback is bound to its invocation version and
generation, so late registration cannot overwrite a later run or replacement.
Manual effect disposal marks its owner disposed; effect/computed construction in
that dead owner rejects before allocation. A new root can establish an independent
lifetime when explicitly desired.

Five added tests cover self-disposal/replacement, disposal from previous cleanup,
late registration across reruns and ID reuse, throwing late cleanup and rejection
of orphan child construction. All 334 TypeScript tests in 17 files pass, as do full
TypeScript checking and package ESM/type compilation. Installed-package runtime
fixtures also verify self-disposal, immediate returned cleanup and an independent
replacement subscriber against the embedded WASM graph.

### Native browser integration refresh

Re-ran the actual browser WebGPU fixtures after the reactive lifetime and camera
changes. Render passed 17/17 (including two new cases), compute 8/8 (including two
new cases), and frame scheduling/picking 6/6. These used browser-branded GPUDevice,
WGSL submission/readback and GPUCanvasContext, rather than the unit GPU mocks.

New render cases sample real perspective-camera pixels before/after atomic aspect
and near/far updates, verify invalid updates preserve the matrix and pixels, and
assert no geometry/depth/pipeline allocations are recreated. Expected pixel ranges
account for the shader's depth shading. Automatic root teardown, with no manual
mesh/transform/camera disposals, returns live node count to baseline and destroys
all recorded GPU allocations. New compute cases reject a zero-byte budget before
any buffer allocation, assert typed error demand/output preservation, then execute
WGSL at the exact 84-byte snapshot boundary and observe admission drain to zero.

Screenshots: svet-native-camera-validation.png and svet-native-compute-admission.png
in the session visualization directory. The scheduling run retained its six checks,
including submitted-frame picking; its CPU loop timings are not throughput/FPS
benchmarks. OOM/internal error cases inject scope results, not physical exhaustion
or driver failure. GPU model/backend-wide parity and automated browser CI remain
unqualified. Temporary browser tabs and the test-only Vite server were cleaned up.

### Queued effect lifetime snapshots

Three regressions reproduced a queued callback deleting a peer and creating a
replacement at exactly its native ID/pool shell: scalar, batch and tensor flushes
ran the replacement twice (initial run plus the stale queued ID). Each effect
creation now advances a shell lifetime counter, and all multi-effect flush paths
capture record identities/lifetimes before any user callback. Invocation requires
matching registry identity, lifetime, generation and dirty state. Two temporary
arrays retain identities/counters for a multi-effect flush; single-effect direct
paths are unchanged. No global throughput improvement is claimed.

Five added cases cover all three affected paths, genuine replacement writes during
the same flush and graph reset/new-generation slot reuse. The three primary cases
failed before the fix. All 339 TypeScript tests in 17 files, package ESM/type
compilation and full TypeScript checking pass. Installed-package runtime fixtures
now also assert exact reused-ID queue isolation and legitimate later notification
against the embedded native WASM graph.

### Bounded self-write stabilization

Effects that write dependencies they already read now request another invocation
after the current callback returns, rather than recursively entering the same
callback. Current direct dependencies are checked in JS; computed ancestry is
queried from the native Rust graph. Rust's reachability walk uses reusable stack
and epoch-marked scratch, handles cycles and epoch rollover, and leaves graph
versions/states unchanged. Scratch retains peak capacity; this is not a global
memory or speed claim. Writes unrelated to the active reads do not request reruns.

Each invocation allows 100 synchronous rounds with normal resource/cleanup turnover.
Initial effect construction drains notifications to existing subscribers. Exceeding
the per-effect or native flush-round budget throws exported ReactiveStabilizationError
(effectId is null for the flush budget). Native pending dirty work is inspected
without consuming it at the flush limit and can be drained after correcting the
condition. Existing writes are not rolled back; initial failed effects are disposed.
The limit is a work budget, not proof of an infinite cycle, and can reject slower
convergent sequences. Propagation between different direct subscribers still uses
existing synchronous paths; this is not a complete global nonrecursive scheduler.

Validation: 351 TS tests in 18 files, 18 Rust tests, all three root Node/WASM checks,
complete WASM/types/package/demo build and nine isolated installed-package gates
pass. Twelve new TS cases cover initial/primed/computed/tensor self-writes, callback
depth one, cleanup/resource turnover, typed limits, recovery, mutual queued cycles,
initial notifications, unrelated-output exclusion and graph reset. Two Rust cases
verify graph reachability/cycles/disposal and stable scratch pointers/state/epochs.
Installed consumers execute computed self-writes and typed initial-cycle failure.
Embedded WASM is 50,768 bytes.

Re-ran native browser fixtures on this engine: render 17/17, compute 8/8 and scheduling
6/6. Proof: svet-stabilization-native-compute.png in the session visualization
directory. OOM/internal cases still inject scope errors; CPU burst timings are not
GPU throughput measurements. Temporary tabs and the test server were cleaned up.

### Native pending-queue lifetimes and batch-born effects

Three Rust regressions reproduced stale native pending IDs executing an unscheduled
replacement, returning signal/computed replacement slots as effects, and emitting
a legitimately scheduled replacement twice/in the old job's position. Native nodes
now carry a slot lifetime, and pending entries store ID+lifetime. Flush accepts only
live queued effects with matching tokens; stale entries cannot clear a new job's
in_queue flag. Lifetime wrap purges obsolete entries for that slot only. Pending
counts use the same identity and queue state without consuming native work. The
public WASM flush buffer remains uint32 IDs. Metadata adds native storage rather
than constituting a memory/speed optimization.

Public-API regressions then exposed new effects losing writes when created inside
a batch and the old single-subscriber cache ignoring newly batch-created peers.
The unlinked optimization now applies only outside a batch; native-linked one-read
effects invalidate the prior JS direct-subscriber cache. Real writes coalesce once
at batch end, including replacements, while retaining actual native schedule order.

Validation: 355 TS tests in 18 files and 23 Rust tests pass. Added four TS cases
(three failed before their fixes) and five Rust cases (three failed before the
native fix), including forced counter wrap, nonconsuming counts, real queue order,
same-batch replacement writes and future peer notification. The three root
Node/WASM integration checks and complete WASM/types/package/demo build pass.
Embedded WASM is 51,230 bytes. Installed-package fixtures exercise stale pending
replacement, coalesced new jobs, accurate dirty-job counts and later peer updates.

### Computed cycle diagnostics and failure recovery

Reproduced stack overflow from a direct computed cycle and observers permanently
missing updates after a computed exception. Added per-context evaluation stacks
and per-entry evaluation guards. ReactiveCycleError carries a frozen closed numeric
node-ID trace; identities are entry-based, so disposed/reused slots and isolated
contexts do not cause false cycles. Stacks/flags unwind on throw, disposal and reset.
Synchronous peek and comparator recursion are included, not asynchronous Promise
graphs or arbitrarily deep acyclic evaluation.

Computed host failure flags force reads to retry even when native scheduling state
is clean. Failed native branches remain invalidatable on a later source write,
and failed reads retain reached observer dependencies before rethrowing. This
preserves cached values without freezing an invalid result into the fast path.
A multi-observer regression additionally showed the first exception abandoning
other already-drained native jobs. Multi-effect dispatch now completes remaining
valid dirty captured lifetimes, collecting and then propagating the first failure;
removed/replaced entries still cannot run. Writes/side effects are not rolled back.

Validation: 372 TS tests in 19 files pass, including 17 new cases. Three regressions
failed before their fixes: direct cycle overflow, ordinary observer recovery and
shared-computed multi-observer recovery. Further cases cover exact closed traces,
immutability, peek/equals recursion, caught/uncaught layered failures, automatic
cycle repair, ID reuse, context isolation/reset, first-error identity, thrown
undefined and queue replacement after an exception. Root Node/WASM checks pass
(including the 23-test Rust suite), as does the full WASM/types/package/demo build.
Installed-package fixtures exercise typed cycle identity, trace typing and recovery
of two existing observers through an indirect notification queue. WASM remains
51,230 bytes; no new native GPU performance qualification is claimed.

### Bounded expired native queue metadata

Four Rust regressions demonstrated pending tokens growing with create/write/dispose
churn despite constant live slot counts. The queue now compacts after writes and
disposal when entry length exceeds 2*active_native_nodes+64. Stable retain keeps
only live matching queued effect lifetimes; it does not clear a replacement's flags
or drop clean-but-scheduled jobs that may be dirtied again. The threshold amortizes
scans and also handles a shrinking population without requiring a future write.
Vector allocation retains peak capacity; this is not an OS/process memory ceiling.

Added native physical-entry count ABI and pendingEffectCount/pendingEffectEntryCount
to getGraphTelemetry (also carried by devtools snapshots). Dirty-job counts exclude
expired/clean entries; physical entry count exposes metadata awaiting collection.
Telemetry is a fresh numeric snapshot and excludes JS-drained/running callbacks.
The devtools header no longer incorrectly claims constant-time snapshot capture.

Validation: 374 TS tests in 20 files and 27 Rust tests pass. Four new Rust cases
failed before the fix, then verify 10,000 churn cycles with bounded entries/stable
warmed capacity, survivor ordering/flags, clean queued jobs and population-drop
collection. Two new TS/WASM cases verify 5,000 cycles under an open batch, stable
warmed linear-memory byte length for that fixture, preserved real notifications
and telemetry. Root Node/WASM checks and full WASM/types/package/demo build pass.
Installed-package fixtures exercise 1,000-cycle churn, queue telemetry and strict
counter types. Embedded WASM is 51,723 bytes. These are bounded metadata/workload
checks, not a general JS heap, driver residency or throughput benchmark.

### Native topology rollover and cursor discovery audit

Four forced-rollover Rust regressions reproduced ancient cache stamps hiding new
subscribers, running detached effects and dirtying signals that reused cached effect
or computed slots. All topology increments now share a rollover-safe helper. On
wrap it reserves epoch zero as invalid, clears terminal stamps/selectors and cached
downstream ID lists, then rebuilds caches on demand. Normal duplicate/no-op links
and ordinary warm-cache operation remain valid. This is forced-boundary testing,
not a benchmark that executes billions of mutations.

Auditing other topology mutations found ordinary raw native cursor bugs: pruning
and replacement failed to invalidate caches, reordered reads lost forward edges,
and invalid/self reads or disposed observer frames could panic on slice bounds.
Frames now carry native slot lifetimes and recursive begins are balanced no-op
frames. Existing tail dependencies are swapped into the read position rather than
removed/re-added; actual edge changes advance topology. Source disposal preserves
active read prefixes and adjusts cursors; explicit source replacement resets the
cursor. Stale frame completion cannot mark a replacement observer clean. Counter
wrap invalidates ancient frames for that slot. Stable and reordered reads retain
source/subscriber/stack allocations and topology-cache stamps.

Validation: 379 TS/WASM tests in 21 files and 45 Rust tests pass. Added 18 Rust cases,
14 of which reproduced failures before their fixes (four rollover, seven original
cursor, two source-mutation and one recursive-begin cases). Five new tests run the
actual embedded WASM ABI for prune/replace, reorder/invalid reads, ID reuse, source
disposal and balanced pause/invalid/recursive scopes. The root Node/WASM checks and
full WASM/types/package/demo build pass; embedded WASM is 53,560 bytes. Installed
package fixtures exercise native cursor operations in a separate WASM instance.
No test-only counter mutation API was added to production, and no new native GPU
or throughput qualification is claimed.


### Host source lifetime tracking

Public scalar, computed and GPU tensor disposal now invalidates staged source
reads in every active observer, including paused ancestors. A later tracked read
of the replacement explicitly restores that observer's dependency; nested or
untracked reads cannot restore an ancestor's retired dependency. Commit compacts
the staged dependency prefix before linking it to native storage.

A separate source-retirement revision forces the next commit of inactive observers
to rebind even when recycled numeric IDs match their previous dependency tuple.
This conservatively adds one rebind after source disposal; ordinary reads do not
allocate lifetime tokens. Revision rollover invalidates observer stamps. Numeric
IDs remain reusable slots, not public lifetime identities.

Validation: 388 TS/WASM tests pass in 22 files, including nine source-lifetime
regressions for scalar/computed/tensor reuse, nested observers, inactive observers,
fresh rereads, untracked reads and overflow dependency storage. The installed
package consumer exercises active and inactive source-slot reuse.

The full WASM/types/package/demo build, all nine installed-package checks and
three root Node checks (including the 45 Rust tests) pass for this change.
Browser GPU rendering was not requalified in this host-tracking change.


### Reset across synchronous host scopes

`createRoot`, `runWithOwnerScope`, captured-owner reentry, `untrack` and `batch`
now restore their saved context only when its generation still matches. Reset
inside a callback must not reinstall a retired owner/observer, decrement the new
graph's batch depth or end its native transaction. Callback return values and
exceptions retain normal finally semantics.

Validation: 394 TS/WASM tests pass in 22 files. Six added reset cases cover nested
roots, untracked effect execution, owner scopes with return/throw paths and nested
batches followed by fresh transactions. The full WASM/types/package/demo build
passes. Installed-package fixtures also check owner reset and fresh batching in
an isolated server context. This does not claim automatic cleanup of arbitrary
external resources held by old user scopes after an explicit graph reset.


### Failed ownership-scope initialization

A synchronous exception escaping `createRoot` or `runWithOwnerScope` now disposes
the newly created scope before rethrowing the original value. Owned effects,
computed nodes, tensors and registered cleanups are released immediately; failed
child scopes detach from their surviving parent. Cleanup exceptions remain
suppressed by scope teardown, so they cannot replace the initialization failure.
A captured disposer remains idempotent. Successfully returned scopes retain the
existing ownership semantics; captured-owner reentry does not own or tear down
its caller's existing scope on failure.

Validation: 399 TS/WASM tests pass in 22 files, including five new regressions for
resource cleanup, child detachment, thrown undefined, disposer idempotence and
reset followed by failure without harming fresh resources. The full
WASM/types/package/demo build passes. The installed consumer fixture checks failed
root cleanup against live graph-node counts and subsequent signal writes.


### Disposed-owner preflight

`runWithOwnerScope` now rejects a disposed parent before invoking user code or
attaching a child. This closes a bypass of the existing effect/computed
construction guards. `configureGpuSubsystem` similarly rejects a disposed owner
before changing upload architecture, device ownership, cancellation state or the
shared per-device compute snapshot budget. Independent `createRoot` lifetimes
retain their existing semantics.

Validation: 401 TS/WASM tests pass in 22 files. Both added regressions failed
before these guards: child scopes could create orphan effects, and GPU
configuration could change architecture/budget after owner disposal. The full
WASM/types/package/demo build passes. Installed consumers exercise both guards
and verify unchanged GPU telemetry. No native GPU execution claim is added.


### GPU configuration field preflight

Configuration fields are now snapshotted once before publishing changes. Unknown
memory architectures are rejected before updating shared snapshot budgets or
unregistering owner cleanup. Throwing option getters leave previously configured
state intact unless those getters themselves mutate it. Reset, owner disposal or
reentrant configuration/initialization during option reads invalidate the outer
attempt, preserving the newer state rather than overwriting it.

Validation: 406 TS/WASM tests pass in 22 files. Five added cases cover invalid
architecture, throwing getters without partial budget publication, reset during
reads, reentrant configuration and single-read field semantics. The first three
reproduced failures before the fix. Full WASM/types/package/demo build passes;
installed consumer fixtures reject invalid architecture and throwing getters
without changing telemetry. This preflight is not a rollback guarantee for
arbitrary exceptions thrown by device teardown methods or device-property getters.


### Hardware initialization owner admission

`initWebGpuHardware` rejects a disposed owner before incrementing the initialization
version or requesting an adapter. An invalid caller therefore cannot supersede a
valid pending initializer. Existing cancellation checks after adapter discovery,
metadata discovery and device allocation remain in place.

Validation: 410 TS/WASM tests pass in 22 files. Four new cases cover admission
without adapter requests, preservation of an existing pending initializer, and
owner disposal during adapter/metadata waits without device requests. Full
WASM/types/package/demo build passes. The installed Node consumer also verifies
disposed-owner initialization rejection. Adapter/device behavior in these unit
cases uses explicit deferred test doubles; no new hardware qualification is claimed.


### Temporary hardware-device cleanup

Initialization now clears its temporary device reference before invoking
`destroy()` and suppresses teardown exceptions on cancellation/fallback paths.
Successful configuration transfers ownership to the GPU state and clears the
temporary reference. A rejected or already-lost device is released once;
cleanup failure cannot replace initialization cancellation or prevent the
existing no-device fallback behavior.

Validation: 413 TS/WASM tests pass in 22 files. Three new cases cover a late
device whose destroy throws, an invalid device whose teardown throws and an
already-lost returned device released exactly once. The first two reproduced
failures before the fix. Full WASM/types/package/demo build passes. The installed
Node consumer simulates invalid-device cleanup with a throwing destroy method.
These are lifecycle tests with device doubles, not hardware execution evidence.


### Iterative propagation across different effects

Scalar writes inside a running effect now schedule downstream effects in the
native queue, first linking an unlinked unique subscriber when needed. The
current callback commits before those subscribers execute. Queue draining marks
itself as running, and nested explicit flush/batch completion defers to the outer
drain. Top-level writes and effect construction still drain synchronously before
returning. Explicit nested effect construction and arbitrary user recursion are
outside this propagation guarantee.

The former global 100-round cap is replaced with a per-effect-lifetime queued-run
budget. A lifetime that has executed 100 times and is dirty again produces the
existing typed queued stabilization error before the next native drain. Pending
work remains queued for explicit recovery. Settled saturated lifetimes do not
block unrelated downstream work. Pooled entries with new lifetimes have separate
counts. Counts and saturated entries are local to a drain, so a long acyclic
cascade uses temporary storage proportional to observed effect entries; no
throughput or hard process-memory ceiling is claimed.

Validation: 419 TS/WASM tests pass in 22 files. Six added cases cover direct and
linked 1500-effect chains, unique-subscriber mutual cycles with pending recovery,
nested batch/explicit-flush callback order, a 300-link tensor chain, and a
100-execution settled lifetime followed by an unrelated 150-link tail. The first
three reproduced stack overflow, false round-budget rejection and missing pending
recovery work before the fixes. Installed consumers execute a 400-link chain and
verify callback depth one. The full WASM/types/package/demo build and three root
Node checks (including 45 Rust tests) pass.

Browser requalification on the current source: native WebGPU render 17/17,
WGSL compute 8/8 and frame scheduling 6/6 pass. This includes pixels, validation,
device loss/replacement, allocation teardown, native projection/picking,
readback/publication and queued-frame picking. Evidence screenshot:
`/Users/themoretheless/.codex/visualizations/2026/10/28/01a182c3-ab85-7fd3-bbf5-0662a407a419/svet-queued-cascades-webgpu.png`.
These results supersede the earlier 100-flush-round and direct-recursion boundary
in the historical stabilization entry above.


### Batch body and observer error precedence

`batch` now preserves the exact exception from its body while still ending the
native batch and attempting the queued flush. A later observer/stabilization error
cannot mask that primary body failure, including undefined/null/zero. If the body
succeeds, flushing failures still propagate normally. Writes remain committed;
this is notification batching, not rollback. Secondary flush failures are
suppressed when a body failure is already propagating. Surviving callbacks in the
current native queue retain existing execution semantics, and stabilization-limit
pending work remains available for explicit recovery.

Validation: 427 TS/WASM tests pass in 23 files. Eight new cases cover exact body
error identity with four thrown values, normal observer-error propagation, nested
batch precedence, pending recovery after a suppressed stabilization error and
successful nested return values. Five reproduced masking before the fix. Full
WASM/types/package/demo build passes. Installed consumers exercise body/observer
failure precedence followed by a successful batch. Browser hardware suites were
not rerun for this error-precedence-only change.


### Iterative owner-tree disposal

Owner-tree teardown now uses explicit traversal frames instead of recursively
calling `disposeOwnerScope` for every child. It preserves depth-first child
insertion order, marks ancestors disposed before callbacks, processes each
scope's cleanup list in reverse order, and detaches completed children. Later
siblings are not marked disposed before their turn. Cleanup failure and reentrant
sibling disposal retain the previous best-effort/idempotent behavior. Traversal
uses temporary storage proportional to tree depth; user cleanup callbacks that
explicitly recurse are not transformed by this implementation.

Validation: 432 TS/WASM tests pass in 24 files. Five added cases cover a
12000-level tree with leaf computed/effect/tensor resources, cleanup order,
reentrant sibling disposal, ancestor/sibling lifetime observations after a throwing
cleanup and a 12000-level tree removed before effect rerun. The first deep-tree
case reproduced stack overflow before the fix. The full WASM/types/package/demo
build passes. Installed consumers construct/dispose 12000 contextual scopes using
the existing captured-owner adapter and verify every cleanup ran. Browser GPU
suites were not rerun for this teardown-only change.


### Resource-owned state disposal

`Resource.dispose()` permanently cancels a resource, disposes its source observer
and three internal state signals, and unregisters its cleanup from a live owner.
Owner teardown uses the same idempotent path. Source-observer initialization
failure now releases already allocated state and removes its owner registration;
construction under a disposed owner is rejected before allocation. Refetch checks
owner/generation/disposal before evaluating user source code. Mutating a disposed
resource is a no-op, and retained scalar accessors preserve final snapshots.
`abort()` remains reusable and differs from permanent disposal.

Disposal releases internal nodes in finally blocks even if loading observers or
source cleanup fail. Loading completion decrements Suspense in a finally block so
a throwing loading observer cannot strand its pending token. Explicit graph reset
invalidates later refetch/publication; it does not add a global reset callback or
promise automatic cancellation of fetchers outside disposed owners.

Validation: 439 TS/WASM tests pass in 24 files. Seven new cases cover pending
owner teardown, manual cleanup/unregistration, failed source initialization,
disposed-owner admission, refetch after reset/disposal, a throwing external loading
observer and abort-versus-dispose reuse. Five reproduced missing lifetime behavior
before the change. Full WASM/types/package/demo build passes. Installed package
fixtures exercise the new disposal method, exact graph-node recovery, stopped
source subscriptions and its strict exported Resource type. Browser GPU suites
were not rerun for this resource-lifecycle change.


### Boundary-owned state and branch teardown

`Suspense` and `ErrorBoundary` now construct private child owner scopes and
register their internal scalar state for disposal. ErrorBoundary explicitly
clears its independent content/fallback root when that owner scope is removed.
Constructor failure disposes partially created state/resources; internal signal
cleanup is registered before DOM access, including failure in Node without DOM.
Retry state writes are batched so recovery from an async resource failure mounts
the replacement content once. Retained reset callbacks only write disposed
signals after teardown and cannot recreate a branch.

Validation: 448 TS/WASM tests pass in 25 files. Nine new DOM cases cover Suspense
pending teardown, main/fallback ErrorBoundary branch teardown, constructor failures
in both boundaries, one-mount async retry, disposed-owner admission and retained
reset after unmount. The initial six cases reproduced leaked signals, surviving
branch subscriptions or duplicate retry mounts before the fix. Full
WASM/types/package/demo build passes. Installed Node consumers verify that failed
DOM access in both components restores exact graph node counts. These components
still require an enclosing mount/root for successful owned lifetime teardown;
this change does not add a standalone fragment disposer or rerun native GPU tests.


### Router lifetime and reentrant navigation

Routers now track disposal, owner/generation validity and navigation attempt
versions. Invalid navigation is rejected before route matching, guard execution or
history publication. A guard that disposes the router, resets its graph or starts
a newer navigation invalidates the older attempt. Guard iteration uses a snapshot
and skips removed guards; newly registered guards wait for another transition.
Manual disposal unregisters owner cleanup, clears guards and removes the listener
from the browser window captured during construction. Invalid beforeEach
registration is a no-op. Initial construction rejects disposed owners before
allocating signals and rechecks validity after option/route reads.

RouterView now captures its router in a private child owner scope and registers
active route-root cleanup. Unmount stops surviving route callbacks and releases
owned tensors; mutating the caller's descriptor does not switch the router.

Validation: 457 TS/WASM tests pass in 26 files. Nine added cases cover disposed
navigation, manual owner-registration churn, disposal/reentry from guards,
disposed-owner construction, active route unmount, graph reset, deferred guard
registration and descriptor mutation. The initial six reproduced failures before
the fix. Full WASM/types/package/demo build passes. Installed Node consumers
exercise superseding navigation, idempotent disposal and exact graph-node recovery.
This change does not add history rollback after browser popstate guard rejection,
async guards or automatic listener removal on standalone graph reset; call
router.dispose or dispose its owner to remove the listener.


### Link browser navigation semantics

Link no longer prevents every click. It only intercepts unmodified primary
clicks on same-origin HTTP(S) anchors with a router, a self/default target and no
download attribute. Existing prevention, modifier keys, external origins/schemes,
standalone links and non-self targets preserve browser defaults. URL resolution
uses the bound anchor's document/base URI; same-origin destinations reach the
router as pathname/search/hash. Added typed target/download/rel props. Document
base targets and delegated nested-child currentTarget semantics are respected.
Unsafe URL sanitization still replaces blocked schemes with a nonexecuting href
and prevents activation.

Validation: 480 TS/WASM tests pass in 27 files. Twenty-three new DOM cases cover
six modifier/button combinations, three external schemes/origins, standalone and
prevented clicks, target/download handling, same-origin absolute normalization,
three unsafe schemes, base target/URL, self/download=false/rel, query-only URLs and
nested child clicks. The initial 15 reproduced failures before the fix, including
cross-origin history errors. Full WASM/types/package/demo build passes. Strict
installed-package consumers verify new Link prop types. These DOM tests observe
default-prevention and router calls while suppressing actual external navigation;
they do not claim OS-level tab/download behavior or native browser requalification.


### Portal construction and teardown

Portal now owns a private child scope and registers external-container removal
before user child rendering or target insertion. Owner/generation checks follow
mount/child descriptor reads, content rendering, child insertion and target
attachment. On construction failure, a direct container-removal attempt covers
synchronous insertion hooks that attach only after owner cleanup has already run;
then the scope releases remaining resources. Cleanup failures do not replace the
original construction failure. Context inheritance remains intact and children
are captured with one descriptor read.

Validation: 491 TS/WASM tests pass in 28 files. Eleven new DOM cases cover partial
resource cleanup with a surviving caller, partial append failure, owner disposal
before/after attachment, reset with fresh graph state, successful context/teardown,
invalidating descriptor reads, nested portal failure, disposed-owner admission,
one-read children and late insertion after completed cleanup. Five initial cases
and the late-insertion follow-up reproduced failures before their fixes. Full
WASM/types/package/demo build passes. Installed Node consumers also verify failed
DOM access leaves graph counts unchanged. Successful standalone fragments still
need an enclosing mount/root; arbitrary asynchronous DOM reinsertion or failing
native removal hooks are outside this synchronous construction guarantee.


### Resource request result freshness and abort reentry

Refetch now always returns a Promise; source-reader exceptions reject it rather
than escaping synchronously. Load versions are assigned before abort callbacks,
and active controllers are detached before invoking user listeners. An abort
listener's newer request supersedes the older load before another fetcher can
start. Startup is within the load's try/finally and freshness is checked after
loading notifications, before publication, before result return and before error
forwarding. Obsolete successful fetches resolve undefined. Explicit abort and
source-disable completion only settle loading if their version remains current.

Validation: 500 TS/WASM tests pass in 28 files. Nine added cases cover canceled
result promises after abort/disposal/reset, supersession while replacement is
pending, Promise rejection from source reads, loading-observer cancellation,
abort-listener reentry without a third request, retained replacement loading and
error-observer recovery without stale ErrorBoundary forwarding. All nine
reproduced failures before their respective fixes. Full WASM/types/package/demo
build passes. Installed consumers exercise canceled Promise results and rejected
source-reader promises. Already published values are not rolled back; arbitrary
fetcher work remains cooperatively cancellable through its AbortSignal, and these
checks do not establish cancellation of external work that ignores that signal.


### Canonical router navigation URLs

Router initialization and navigation now normalize internal HTTP(S) URLs before
matching, guards and history publication. Relative/query/hash references resolve
against the last committed full router URL, not a reconstructed query object.
Same-origin absolute URLs become pathname/search/hash; invalid URLs, other schemes
or origins are rejected before navigation side effects. Invalid initial URLs
throw before graph-state allocation. The retained full URL preserves duplicate
query keys and encoding. Redirects normalize too, so an already canonical target
does not cause a redundant guard pass. SSR uses a synthetic .invalid origin for
local URL resolution and never fetches it.

Validation: 517 TS/WASM tests pass in 29 files. Seventeen new cases cover six
relative/query/hash/empty forms, same-origin absolute dynamic matching, five
external/invalid URLs, raw query preservation, canonical guard redirects, invalid
guard redirect recovery and two invalid initial URLs. The initial thirteen
reproduced divergent history/signals or history exceptions before the fix. Full
WASM/types/package/demo build passes. Installed Node consumers verify relative
normalization, query/hash publication and external URL rejection. This does not
change the low-level match(pathname) contract or claim browser history-position
rollback for blocked popstate navigation; that remains separate work.


### Route segment encoding and parameter properties

Matching now splits first and decodes static pattern/path segments once, aligning
Unicode/literal patterns with normalized percent-encoded URL paths. Invalid
percent/UTF-8 segments keep their raw text. Encoded slashes cannot introduce a new
path separator. Dynamic detection still checks the raw pattern, so an encoded
leading colon stays literal. Parameters are defined as enumerable, writable own
data properties; reserved Object property names no longer disappear or invoke an
inherited setter. The ordinary object prototype is retained.

Validation: 534 TS/WASM tests pass in 30 files. Seventeen added cases cover eight
static Unicode/encoded/reserved-character forms, three special parameter names,
one-decode dynamic/slash handling, two malformed forms, literal encoded colon,
static double-decode rejection and RouterView Unicode rendering. Nine initial
cases reproduced failures before the fix. Full WASM/types/package/demo build
passes. Installed Node consumers verify encoded static matching and the
__proto__ parameter's own-property/prototype behavior. Matching retains its
existing case sensitivity, wildcard and empty-segment policy; no route-ranking,
optional-segment or browser-history rollback feature is claimed here.


### Owned view-node insertion rollback

RouterView and ErrorBoundary now share a private node-insertion helper. It checks
the captured view owner/generation and active branch owner before/after insertion,
then returns the complete committed node list. Failed partial insertion removes
candidate nodes even if a synchronous hook inserted them after owner cleanup.
Root/scope failure handling releases branch resources. Invalid view lifetime is
also checked before ErrorBoundary invokes fallback user code.

Validation: 540 TS/WASM tests pass in 31 files. Six added cases cover partial
RouterView insertion with later recovery and stopped subscriptions, partial
ErrorBoundary content followed by fallback, owner unmount during component
rendering, late insertion after completed cleanup, RouterView reset with fresh
replacement state and ErrorBoundary reset without calling stale fallback code.
The initial four cases and the stale-fallback follow-up reproduced failures before
the fixes. Full WASM/types/package/demo build passes. Candidate rollback does not
restore an already removed previous branch or roll back committed navigation /
browser history, and arbitrary failing removal hooks remain outside its
best-effort cleanup guarantee. Native GPU/browser suites were not rerun for this
DOM ownership change.


### Suspense construction lifetime checks

Suspense now captures its private view owner/generation and checks lifetime after
descriptor reads, rendering, child insertion, visibility updates and fragment
attachment. Content/fallback descriptors are read once. Construction failure
removes both wrappers from their actual parents, covering synchronous attachment
after completed owner cleanup. Invalid content cannot proceed into fallback user
code after unmount or reset. Existing owner-scope cleanup releases state and
subscriptions.

Validation: 546 TS/WASM tests pass in 32 files. Six new cases cover owner disposal
in content, reset preserving fresh state, one-read descriptors, invalidating
fallback getters, late wrapper attachment and visibility-setter failure followed
by successful resource retry. The first five reproduced engine failures before
the fix. The last required correcting the test double to jsdom's actual
CSSStyleProperties prototype and qualifies loading/pending recovery after an
injected setter error. Full WASM/types/package/demo build passes. The installed
package gates retain DOM-less constructor/state-cleanup coverage. This is DOM
unit qualification; native browser/GPU tests were not rerun for this change.


### Show branch commit and retry

Show now owns a private view scope and registers branch cleanup before observer
initialization. Clearing a branch detaches its disposer/node list and resets cached
truthiness before teardown. Cached activeState/lastTruthyVal are committed only
after complete successful branch insertion; a failed attempt no longer suppresses
another notified update of the same truthiness. Owner/generation checks reject
invalid condition reads/rendered output. Reactive when getters are captured once
per invocation while retaining dependency tracking.

The shared view-lifetime/insertion helpers moved from advanced.ts into internal
DOM-module exports, reused by Show, RouterView and ErrorBoundary. They are not
added to the package root API. Show supplies its mutation-counter callback so
normal insertion accounting is retained and rollback removals are counted.

Validation: 553 TS/WASM tests pass in 33 files. Seven new cases cover failed truthy
and fallback retries, partial insertion cleanup/retry, unmount during rendering,
successful non-keyed DOM identity, tracked one-read when getters and graph reset
preserving replacement state. The first four reproduced failures before the fix.
Full WASM/types/package/demo build passes, and installed-package gates exercise
the rebuilt dependency graph and external browser bundle. This change does not
add automatic retry without a notified dependency update, restore previously
removed branches or qualify native browser/GPU execution anew.


### Writes during paused host read tracking

Running effect entries now retain their staged cursor when entering untrack,
computed evaluation or nested effect execution. Write invalidation examines live
read prefixes of every executing effect in that context's observer stack, using
the active cursor for the current entry and saved cursors for paused entries.
Retired dependency slots are skipped. This restores sequential self-write
stabilization and typed limits for untracked writes, and lets nested callbacks
invalidate an outer effect before its first native binding commits. The cursor is
reset when pooling entries; ordinary reads do not allocate new tracking tokens.

Validation: 565 TS/WASM tests pass in 34 files. Twelve new cases cover scalar,
primed, computed and tensor self-writes, nested computed/effect writes, no tracking
from untracked reads, retired ID reuse, bounded nonstabilizing writes, overflow
dependency storage, current-invocation pruning and paused computed dependency
paths. Seven initial cases reproduced lost updates or missing stabilization errors
before the fix. Full WASM/types/package/demo build and installed-package gates
pass; installed consumers verify untracked self-write stabilization. Invalidation
work is proportional to executing effects and their already-read prefixes; this
is not a throughput claim or proof for arbitrary side effects inside computations
before a parent has staged its read. Native browser/GPU suites were not rerun.


### For prepare/commit recovery and row-owned index signals

For now has a private view scope, captured lifetime checks, input-value snapshots
and key uniqueness preflight. New rows are staged outside lookup tables; existing
row indexes/signals remain unchanged while rendering new rows. Old rows are kept
live until DOM insertion/movement succeeds. Only then are indexes, lookups and the
committed record array published and removed rows disposed. Before that commit,
errors dispose provisional rows and restore old DOM order with original scopes
and nodes. Invalid owner/generation paths discard both staged and old resources
without reviving an unmounted view. Owner cleanup detaches state before disposal.
The existing integer table, double-buffered arrays, small-move paths and native
Rust/WASM LIS calculation remain in use.

Lazy index signals are now explicitly owned by their row disposer, including row
render failure. A retained unused index accessor returns its final index after
removal without allocating another signal. Late valid allocation selects the
original WASM core. Duplicate keys fail before renderer invocation; source-list
mutations during callbacks do not change the prepared value snapshot.

Validation: 577 TS/WASM tests pass in 35 files. Twelve new cases cover failed row
preparation, unchanged existing indexes, index-signal disposal, retained lazy
accessors, duplicate update keys, insertion rollback, unmount during preparation,
string-key staging, existing-row move rollback, list snapshots, reset with fresh
state and duplicate initial keys. The initial seven reproduced failures before
the fix. Full WASM/types/package/demo build and nine installed-package gates pass.
An edit-script boundary mistake was caught by the first build and corrected before
successful validation. Tests cover single-node rows; fragment/range reconciliation
remains separate unqualified work. Rollback is best effort if native DOM removal
or restoration hooks themselves fail; post-commit teardown errors do not undo
already published state. No throughput or process-memory ceiling is claimed.


### Keyed fragment row ranges

For fragment rows now keep start/end comments instead of the consumed empty
DocumentFragment. Range traversal discovers the current live sibling nodes before
movement/removal, so nested Show/For updates remain part of their keyed row.
Empty fragments retain a stable position. Provisional/movement snapshots survive
synchronous owner cleanup long enough to remove late insertions and restore
partially moved ranges. Successful moves and completed teardown clear snapshots.
Single-node rows retain their original marker-free insertion path and integer/
map lookup and native LIS logic remain row-based.

Validation: 587 TS/WASM tests pass in 36 files. Ten new cases cover grouped
reorder/deletion and identity, empty groups, dynamic Show and nested For content,
new-range insertion rollback, partial existing-range move rollback, reactive
indexes, late insertion after owner cleanup and physical move counts for both
single/grouped rows. The initial five reproduced failures before the fix. A swap
uses one insertion for a single-node row and four for a two-node fragment plus
boundaries; this is operation counting, not a latency/throughput benchmark.
Full WASM/types/package/demo build and installed-package gates pass.

Native browser fixture tests/browser/for-fragments.html passes 7/7: dynamic groups,
keyed identity, row resource removal, failed preparation, retry, partial insertion
rollback and owner teardown to zero reactive nodes. Screenshot:
`/Users/themoretheless/.codex/visualizations/2026/10/28/01a182c3-ab85-7fd3-bbf5-0662a407a419/svet-keyed-fragment-rows.png`.
This supersedes the earlier single-node-only For qualification. Native LIS still
minimizes logical row movement rather than weighted physical node cost; arbitrary
external corruption of range boundaries or failing restoration/removal hooks is
outside the best-effort rollback guarantee. No new SSR/resumption or GPU hardware
qualification is claimed.


### Current values for retained keyed rows

For.children now receives a third lazy accessor current(), leaving the initial
item argument compatible. Successful reconciliation publishes replacement items
and indexes; failed preparation leaves committed row values intact. Item signals
share row lifetime and function-valued items remain data. Forma clones local
property descriptors and binds each loop variable to current(), keeping nested
outer locals, actions and child prop bridges live without remounting rows.

Validation: 598 TS/WASM tests in 37 files pass; eleven new cases cover same-key
replacement, reorder/index publication, failed preparation, retained accessors,
function items, pure moves, positional primitives and Forma text/actions/nested
loops/child props. Full WASM/types/package/demo build passes. Native browser
fixture tests/browser/for-current-item.html passes 7/7, including input value,
focus and selection preservation on same-key replacement and zero nodes after
teardown. Focus preservation across physical reorder is not claimed. Initial
items remain snapshots; mutable deep properties are not automatically tracked.

All nine isolated installed-package gates also pass, including the strict typed
current() consumer fixture. git diff --check passes.


### Reactive row indexes independent of callback arity

Removed the Function.length heuristic for For index accessors. Default second
parameters and rest parameters report arity below two while still consuming the
provided index accessor; previously reordered rows kept their original numeric
indexes. Every row now receives the same lazy accessor, allocating an index
signal only when it is actually read and preserving row-owned disposal.

Both new regressions failed with B1A0 instead of B0A1 before the change. They now
pass while retaining keyed element identity and tearing down to zero graph nodes.
All 600 TS/WASM tests in 37 files and the full WASM/types/package/demo build pass.
Nine isolated installed-package gates and git diff --check also pass. No native
browser requalification was performed for this callback-arity change.


### Mount teardown survives DOM errors

Unmount now releases its root in finally even when container.replaceChildren()
throws. The handle is invalidated and detached from the container registry before
DOM removal, so repeated stale handles cannot clear a later replacement. Mount
also covers old-app teardown inside the prepared-root failure handler: replacing
an app whose DOM removal throws releases both the old and provisional new roots.
DOM/replacement failures remain primary if resource teardown also fails.

Three regression cases cover failing unmount, failing replacement, and a stale
handle after successful replacement. The first two reproduced leaked graph nodes
before the fix. All 603 TS/WASM tests across 38 files and the full build pass.
Failed native removal may leave inert DOM in the container; this change guarantees
resource disposal, not recovery from an arbitrary throwing DOM implementation.
All nine installed-package gates pass. The first typecheck caught two test
callbacks returning numbers; they were corrected to return void, and the full
build plus focused regressions passed afterward. git diff --check passes.


### Mount validates caller and prepared-root lifetimes

Mount captures both calling-owner and prepared-root lifetimes before rendering.
It rejects graph reset or owner disposal after rendering, after old-app teardown,
and after DOM commit. A pre-insertion snapshot retains fragment children even
when insertion consumes the fragment; failed or invalidated commits remove their
own prepared nodes still attached to the container and dispose the prepared root.
Fresh signals created after graph reset remain valid and are not disposed through
stale numeric IDs. Removal rollback remains best effort if native removal throws.

Five additional cases cover reset during rendering, calling-owner disposal,
reset with late fragment insertion, reset from old-app cleanup, and native
insertion that attaches nodes before throwing. The first three were reproduced
failing before implementation. All 608 TS/WASM tests across 38 files pass.
Full WASM/types/package/demo build, nine isolated installed-package gates and
git diff --check pass. These DOM fault-injection cases were qualified in jsdom;
no new native-browser or GPU qualification is claimed.


### Reentrant mount attempts

Each container has an object identity for its newest mount attempt. Prepared views
validate that identity after rendering, old-view teardown and native insertion.
A newer same-container mount makes the interrupted attempt throw a superseded
error, dispose its prepared root, and avoid publishing a stale unmount handle.
Attempts in different containers remain independent. Failed newer attempts also
supersede older attempts; a prepared older view is never implicitly resumed.

Three new cases cover a mount from old-view cleanup, a mount after native
insertion, and independent containers. The first two failed before the fix.
All 611 TS/WASM tests in 38 files pass. This guards lifecycle/registry publication;
an arbitrary monkeypatched DOM method that overwrites newer DOM after a nested
commit is not repaired by the attempt identity alone. Existing insertion/removal
rollback remains best effort. No new native-browser qualification is claimed.
Full WASM/types/package/demo build, nine installed-package gates and
git diff --check pass.


### Reactive input types and live model conversion

h now binds type through bindProp before other properties, supporting reactive
type accessors. A type binding reapplies registered model synchronizers after
updating the native attribute. This refresh is untracked so changing a model does
not subscribe or rerun the type binding. No extra native signals are allocated.
Model output reads the live input type; input/change listeners filter by current
type rather than retaining the initial checkbox/number classification. Numeric
empty values retain the existing conversion to zero. Cleanup releases both event
listeners and the weakly keyed model synchronizer registration.

Five cases cover text/checkbox/number transitions, unchanged model values during
type changes, external native type changes, static conversion compatibility,
dependency isolation and checkbox event filtering. The initial three reproduced
failures before the fix. All 616 TS/WASM tests across 39 files pass. External type
mutations affect event conversion immediately; automatic model-to-DOM refresh
requires a bound type update or a model update. Radio groups, multiple-select
arrays and IME composition policies are separate unsupported/unqualified scope.
Full WASM/types/package/demo build, nine isolated installed-package gates and
git diff --check pass. These cases use jsdom; no new native-browser proof is
claimed for type transitions.


### Radio model groups

Radio models now synchronize checked by comparing the non-null model's string
value with the option value. They never overwrite option values. Only checked
change events publish the option string; input and unchecked change events are
ignored. Reactive/static value bindings notify radio model synchronizers after
applying a new option value, without subscribing the value binding to the model.
This makes model/value prop order independent for radio construction. Existing
checkbox, text and numeric paths remain covered by the full suite.

Four new regressions initially failed; all 620 TS/WASM tests in 40 files now pass.
Full WASM/types/package/demo build, nine installed-package gates and diff checks
pass. Native browser fixture tests/browser/model-inputs.html passes 9/9: radio
selection, native group click, model update, option update, type transition,
checkbox click, numeric conversion, detached listener cleanup and zero graph
nodes. This additionally qualifies the previous reactive input-type work in a
native browser. Radio group support supersedes the earlier unqualified note;
multiple-select arrays and IME composition policies remain separate work.
Screenshot: svet-input-models.png in the task visualization directory.


### Select model initialization, arrays and option mutations

Select construction reapplies its model after child options are attached, fixing
initial native fallback to the first option. Multiple selects compare array entry
strings with every option and publish selected strings in DOM order on change;
non-array models mean no multiple selection. A per-select owner-scoped native
MutationObserver handles structural changes, implicit option text, explicit value
and multiple attributes at the mutation checkpoint. Callback reads are untracked,
bound to the original core, and guarded by owner/generation/active state. Disposal
disconnects the observer and removes model handlers. No extra signals are added.

Seven new tests cover initialization, array updates/events, keyed dynamic options,
implicit/explicit option values, queued teardown, reactive multiple and reset.
Four initially reproduced failures. All 627 TS/WASM tests in 41 files pass; full
WASM/types/package/demo build, nine installed-package gates and diff checks pass.
Native browser fixture tests/browser/model-select.html passes 7/7, including
native selection behavior, dynamic options and zero-node teardown. Screenshot:
svet-select-models.png in the task visualization directory. This supersedes the
earlier multiple-select unsupported note. Browsers without MutationObserver lack
automatic option refresh; external changes to option.selected alone are not model
writes until a change event. IME composition remains separate unqualified scope.


### Composition-aware text models

Input/textarea model bindings now retain a composing flag for text/search/tel/url/
email/password inputs and textarea. Intermediate input events are ignored while
composing or when marked isComposing. Model synchronization still reads its source
to preserve dependencies, but defers DOM writes during active text composition.
Compositionend clears the flag and publishes the current field value once; a
following equal input uses normal signal equality. Completed user text wins over
external writes during composition. If the type changed to non-text, composition
completion clears state without publishing stale text. Both new direct listeners
are removed on disposal, including an unfinished composition.

Six tests cover input and textarea, intermediate events, external writes, final
publication, isComposing without start, non-text transition and retained detached
nodes. Four reproduced failures before implementation. All 633 TS/WASM tests in
42 files pass. This supersedes the earlier entirely-unqualified composition scope
for these event semantics. Actual OS IME candidate windows, platform event ordering
and mobile keyboard integration remain unqualified; synthetic composition events
are not evidence of those native behaviors.
Full WASM/types/package/demo build, nine isolated installed-package gates and
git diff --check pass.


### Cross-realm child node recognition

DOM insertion now keeps the local instanceof Node fast path and uses the native
Node.hasChildNodes brand check for foreign/adopted nodes. Looking up the node's
owner-document constructor is insufficient after adoption changes the document
without changing the node prototype. Static and reactive child insertion and SVG
child detection share this internal check. SVG Element narrowing uses nodeType
once the value is known to be a native Node. Node-shaped plain objects continue
through text conversion. No public API or extra reactive allocation was added.

Five tests cover foreign identity/native direct listeners, reactive replacements,
already adopted prototypes, SVG title conversion and plain object rejection.
The first four reproduced failures. All 638 TS/WASM tests in 43 files pass, as do
the full WASM/types/package/demo build, nine installed-package gates and diff
checks. Native browser fixture tests/browser/dom-realms.html passes 7/7 using a
real same-origin iframe; screenshot svet-dom-realms.png. Existing ambiguous SVG
upgrade still creates a new SVG node. Delegated-handler document migration and
dynamic fragment reconciliation remain separate scope; cross-origin access is
not enabled or claimed.


### Reactive consumed-fragment ranges

insertChild now represents accessor-returned DocumentFragments using a start
comment and the existing dynamic end marker. Replacement traverses the current
live sibling range before removal, so nested Show/For additions are removed too.
An empty fragment keeps a position and a repeated identical consumed fragment
keeps its current range. Text-to-text fast updates remain unchanged. Range
boundary discovery completes before removal; missing boundaries fail without
removing unrelated siblings. Cross-realm fragments use the shared native-node
recognizer. Fragment insertion/removal increments DOM operation telemetry.

Seven regressions cover group-to-text, text-to-group, group replacement, empty
ranges, repeated identities, nested dynamic views, foreign fragments and corrupt
boundaries. Five of the initial six failed before implementation. All 645 TS/WASM
tests in 44 files pass. This adds DOM range reconciliation, not ownership transfer
for preconstructed nodes/views. Arrays returned by reactive accessors, failures
inside native DOM insertion/removal and synchronous lifecycle reentry remain
separate unqualified work. No new native-browser qualification is claimed.
Full WASM/types/package/demo build, nine installed-package gates and
git diff --check pass.


### Dynamic child commit recovery and lifetime checks

Reactive child updates now capture parent/effect owner and graph generation,
checking before/after user evaluation and DOM commit operations. Node/fragment/text
transitions share staged insertion: candidate nodes are inserted before old nodes
are removed; current metadata publishes only after success. Candidate origins and
old live ranges are snapshotted for best-effort rollback. Consumed candidate
fragments regain their original children after a recoverable failure and can retry.
Plain single-node origins also restore when possible. Text-to-text updates retain
node identity and the direct nodeValue path, with lifetime validation.

Rollback checks lifetime too: unmount during restoration stops further restoration
and removes both old and provisional late nodes rather than reviving a dead view.
Arbitrary failing native restore/remove hooks remain best effort; this does not
transfer ownership of preconstructed node resources or add reactive array support.
The previous unqualified native-failure/lifecycle-reentry scope is superseded only
for these tested cases, not arbitrary hostile DOM implementations.

Six regression cases initially failed: partial insertion, partial old-range
removal, accessor unmount/reset, late insertion after unmount and unmount during
rollback. All 651 TS/WASM tests in 45 files pass. Full WASM/types/package/demo
build, nine isolated installed-package gates and diff checks pass. Native browser
fixture tests/browser/dynamic-children.html passes 7/7, including fragment range
replacement, candidate retry and zero-node teardown. Screenshot:
svet-dynamic-child-recovery.png in the task visualization directory. No latency
or throughput improvement is claimed for the additional validation.


### Mixed arrays from reactive child accessors

Reactive children now recognize arrays as grouped output rather than coercing them
to comma-separated strings. An explicit stack snapshots and flattens nested arrays,
tracks active ancestry to reject cycles, omits empty/boolean values and detects
duplicate native node identities. Native nodes are collected without staging moves,
so committed ranges remain intact until insertion. Nested accessors receive their
own normal reactive child binding in a detached holder. Array groups share range
replacement and best-effort rollback with fragments. Candidate origins restore in
reverse order so adjacent nodes can recover their original sibling anchors. Array
identity does not suppress reevaluation; consumed fragment identity still does.

Seven tests cover mixed arrays/identity, reorder and text/fragment transitions,
same-array reevaluation, nested accessors/disposal, cycles/duplicates, failed moves
and 12,000 levels of nesting without stack overflow. Six reproduced failures before
the change. All 658 TS/WASM tests in 46 files pass. This is grouped replacement,
not minimal/keyed reconciliation. Preconstructed resources retain their original
owners; DOM rollback does not revive nested effect scopes disposed before a parent
rerun. Snapshotting does not undo arbitrary user renderer side effects. Static
non-reactive array insertion retains its existing recursive implementation.
No new native-browser or throughput qualification is claimed.
Full WASM/types/package/demo build, nine installed-package gates and
git diff --check pass.


### Transactional reactive content ownership

Each reactive child render now prepares inside an independent candidate root.
The committed content root is retained across parent observer reruns and failed
preparation/DOM commits. A successful commit publishes the new content disposer
before releasing the old root; parent teardown clears that disposer before calling
it. A cleanup-triggered unmount therefore disposes the new content without reviving
old DOM. createRoot failure cleanup releases provisional effects immediately.
Returning the identical native node or consumed fragment discards the provisional
root and preserves the original content root and its nested bindings. Text-to-text
updates keep the DOM fast path while still replacing their render resource scope.

Six tests cover failed preparation/provisional cleanup, failed array insertion
with live old bindings and retry, retained fragment/node bindings, successful text
scope release and cleanup-triggered unmount. Four of the initial five reproduced
failures. All 664 TS/WASM tests in 47 files pass. This supersedes the earlier
committed-render observer-loss limitation on DOM rollback. Preconstructed external
resources keep their owners; ownership transfer of reused nodes across different
array/group renders and arbitrary renderer side effects remain separate scope.
Each candidate root is JS ownership state, not an extra WASM graph node; no heap
or performance improvement is claimed. No native-browser requalification this turn.
Full WASM/types/package/demo build, nine installed-package gates and
git diff --check pass.


### Iterative static child array preflight

Extracted the iterative array traversal shared by dynamic normalization and static
insertChild arrays. Static insertion now flattens the complete array tree before
constructing child bindings or appending nodes. Cycles throw a specific error
before any leading child is committed; acyclic repeated subarrays remain valid.
Empty/boolean semantics, native node identity and nested binding ownership remain
unchanged. Mutations made by an accessor after preflight do not extend the current
prepared child list. This supersedes the earlier recursive-static-path limitation.

Six tests cover 12,000 levels, cycles before leading DOM and binding allocation,
repeated subarrays, renderer mutation and native/binding lifetime. Four initially
reproduced failures. All 670 TS/WASM tests in 48 files pass. Traversal uses temporary
stack/set/output arrays proportional to the visited tree; no constant-space or
throughput claim is made. Arbitrary custom getter/iterator side effects and native
insertion failures do not gain a static-view transaction from flattening alone.
No new browser qualification was performed for this algorithmic change.
Full WASM/types/package/demo build, nine installed-package gates and
git diff --check pass.


### Explicit ARIA and data boolean tokens

applyStaticAttribute now stringifies boolean values specifically for aria-* and
data-* names before the native presence-boolean path. False states remain explicit
and true states no longer serialize as empty tokens. This shared path covers
static/reactive DOM, SVG and DOM-backed SSR serialization. Null/undefined removal
and ordinary native disabled presence/removal remain unchanged. No ARIA role/state
validation or assistive-technology behavior is inferred from attribute correctness.

Six regressions initially failed: static ARIA, dataset, reactive null/false/true,
native disabled compatibility, SVG and SSR. All 676 TS/WASM tests across 49 files
pass. No new native-browser or screen-reader qualification was performed.
Full WASM/types/package/demo build, nine installed-package gates and
git diff --check pass.


### Enumerated HTML attribute boolean props

contenteditable, draggable and spellcheck now share token-valued boolean handling
with ARIA/data attributes, using case-insensitive attribute-name classification.
Translate maps boolean props to yes/no. Explicit strings remain unchanged and
reactive null/undefined still removes the attribute. Native presence booleans are
not reclassified. The shared static-attribute path covers DOM bindings and SSR.

All six new regressions initially failed: three token-valued names, translate,
reactive removal and SSR/native-boolean compatibility. All 682 TS/WASM tests in
50 files pass. Tests inspect tokens and the draggable reflected property in
jsdom; editing, spellchecker UI and browser translation behavior are not newly
qualified. No platform-wide accessibility or editing parity is claimed.
Full WASM/types/package/demo build, nine installed-package gates and
git diff --check pass.


### Concurrent action state and failure finalization

createServerAction now counts active calls instead of finishing pending on the
first settlement. Per-call object identity selects the latest-started invocation
for error publication, with comparison after rollback to handle reentrant calls.
Startup pending/error writes are batched; optimisticUpdate is inside try/finally,
so synchronous preparation failure cannot strand pending or skip error state.
RPC and rollback failures become an AggregateError with the RPC failure as cause.
Error-publication observer failures and final pending observer failures cannot
replace an existing action rejection. Unprintable rejection values normalize to
an Error retaining their cause. Successful calls still surface final observer
errors through their promise if there is no primary failure.

Nine tests cover overlap, old/new failure order, optimistic startup, aggregate
rollback errors, rollback/pending observer reentry, error observer failure and
unprintable rejection. All initial seven reproduced failures. A test cleanup
initially treated EffectHandle as callable; it was corrected to dispose() before
successful validation. All 691 TS/WASM tests in 51 files pass. RPCs are not canceled
or serialized; overlapping optimistic mutations still require caller-owned
rollback coordination. Action disposal/owner-reset cancellation is separate scope.
No actual network integration or new native-browser qualification is claimed.
Full WASM/types/package/demo build, nine installed-package gates and
git diff --check pass.


### Server action owner lifetime and cooperative cancellation

ServerAction exposes idempotent dispose() and an exported ServerActionDisposedError.
Construction captures owner/core/generation and rejects a disposed owner before
signal allocation. Owner teardown disposes the action; manual disposal unregisters
its cleanup, clears request ownership before abort callbacks, rejects all active
cancellation promises, sends AbortSignal to each RPC and releases pending/error.
The RPC signature accepts a second AbortSignal while existing one-argument
callbacks remain assignable. A cancellation race settles callers even for an
uncooperative pending RPC; late RPC rejections remain observed. This cannot undo
server side effects or force transport resource reclamation if RPC ignores abort.

Invocation checks lifetime before option reads, after startup publication/getters/
optimistic work, and after completion. Invalid requests cannot run rollback or
publish errors into disposed state. State publication and synchronous callbacks
reenter the captured core. Raw reset is detected at those boundaries, without a
global reset listener; a never-settling standalone RPC still needs explicit dispose
for immediate cancellation. Retained signals keep snapshots and cannot reuse fresh
slots through stale writes. This supersedes the previous action-disposal scope.

Eleven lifecycle cases include owner/manual cleanup, all-request abort, uncooperative
RPC, late rejection, recycled slots, pending/optimistic/getter/abort callback
reentry, reset and disposed-owner construction. All initial eight failed before
implementation. All 702 TS/WASM tests in 52 files pass; full build passes. Installed
consumer fixtures now cover runtime cancellation/error export and strict typed
AbortSignal/ServerAction/dispose usage. No actual network or browser qualification
is claimed for cancellation; optimistic mutation cleanup remains application-owned.

Callback getter snapshots retain their original options receiver; an additional
compatibility case checks both optimisticUpdate and RPC this binding. Raw reset
may freeze retained signal snapshots at pre-reset values; stale state is never
written through into the fresh graph. Full build, installed-package runtime/type
fixtures and diff checks were rerun after the receiver compatibility adjustment.


### Native fetch HTTP cancellation qualification

Added scripts/check-action-http.mjs using Node's native test runner, built ESM
public APIs, fetch and a real HTTP server bound only to 127.0.0.1 on an ephemeral
port. Three scenarios verify explicit disposal before response headers, owner
teardown while response.json waits on an incomplete body, and two overlapping
response bodies keeping pending true until both settle. Both abort cases await
the server-side response close event and verify writableEnded is false, proving
transport closure rather than only racing the action promise. Tests also check
zero graph nodes and clean all responses/listening sockets. Per-test deadlines
bound failures; no external network or real backend is used.

New test:actions command builds the package and runs these Node-only cases. The
Svet CI job invokes it explicitly; the script name avoids accidental Vitest
collection. All three HTTP tests pass locally on Node 22.23.4/macOS arm64, and all
702 TS/WASM tests in 52 files still pass. No engine behavior was changed by this
qualification step. CI execution on Ubuntu and browser fetch remain unverified;
configuring a CI command is not evidence that a remote CI run passed. This narrows
the prior entirely-unqualified transport cancellation boundary to the tested native
Node/loopback scope. It does not establish remote server side-effect rollback.
Nine isolated installed-package gates and git diff --check also pass.


### Browser fetch HTTP cancellation qualification

Added a dedicated serve-action-browser.mjs Vite harness bound to 127.0.0.1:5189.
Test-only endpoints hold responses before headers or after a partial JSON body,
report close/writableEnded state, finish overlap responses and forget per-run IDs.
The harness has a bounded 256-entry fixture map, destroys unfinished responses on
cleanup and closes its HTTP server on shutdown. It does not modify the ordinary
application Vite configuration or add runtime endpoints to the published engine.
New dev:actions command starts this manual browser fixture from a source checkout.

Real IAB browser checks in tests/browser/server-action-http.html pass 7/7: typed
cancellation/AbortSignal, unfinished response closure before headers, owner teardown
while reading JSON plus transport closure, zero-node owner cleanup, overlapping
response pending/final state and rejected calls after disposal. Checks use server
stats after abort rather than treating a raced promise as transport proof. Per-run
requests are forgotten in finally; the temporary browser tab and local server were
closed after verification. Screenshot: svet-browser-action-http.png in the task
visualization directory. This extends Node-only transport evidence to the tested
browser/loopback path, not other browser engines, remote server rollback, CI browser
automation or a latency benchmark. No engine behavior changes were required.
Nine isolated installed-package gates and git diff --check pass after the harness
and dev command additions. Port 5189 was verified free after stopping the harness.


### Resumption model and attribute parity

resumeContainer no longer duplicates an input-only string model implementation.
It delegates model binding to the common DOM bindProp model path, gaining checked
booleans, radio option preservation, numeric values, multiple-select arrays and
mutation observation, text composition and full owner cleanup. Both annotated and
explicit-table resumption use this same path. A shared internal applyAttributeValue
helper applies raw attribute semantics and notifies model synchronizers for type
or radio value changes. Normal bindProp type/value also uses that helper. It is
not exported at the package root and does not interpret special model/ref/children
props when used for an attr binding. Existing resumption name/URL filtering remains
before attribute application; native property handling and boolean/null semantics
now match the DOM attribute writer rather than always writing String(value).

Nine regression cases cover checkbox, numeric input, radio groups, multiple select
with inserted options, composition/disposal, disabled/token/null attributes,
reactive type/model interaction, literal special attr names and retained unsafe
name/URL filtering. Seven of the initial eight failed before the change. All 711
TS/WASM tests in 53 files pass. Composition events use jsdom; this step does not
qualify real IME behavior or a full browser SSR/resumption roundtrip. It also does
not add pre-resumption dirty-input preservation or rollback of arbitrary failed
resumption construction. No component rerender is introduced.
Full WASM/types/package/demo build, nine installed-package gates and
git diff --check pass.


### Node SSR to native browser resumption roundtrip

Added serve-resumption-browser.mjs and dev:resume. The dedicated loopback Vite
harness renders the built ESM package in Node with a JSDOM document, temporarily
installs/restores only synchronous rendering globals, and returns HTML plus the
serialized signal snapshot over HTTP. The view registers signal cleanup and a
final isolated-core node-count check; diagnostics also report the restored main
core count. No application runtime endpoints or engine behavior were changed.

The actual IAB browser parses that server HTML and resumeContainer attaches to
existing nodes. tests/browser/resumption-roundtrip.html passes 8/8: SSR cleanup,
escaped input values/identity, checkbox and native/ARIA booleans, numeric input,
radio values/group, inserted multiple-select option, type/model interaction,
re-resumption with stable graph count/stale-handle rejection, and disposal with
queued observer cleanup. Both server graphs and final client graph report zero
nodes. Screenshot: svet-node-ssr-resumption.png in the task visualization directory.

This supersedes the prior unverified SSR/browser boundary for this synchronous
Node/JSDOM-to-tested-browser form path. It is not browser-side renderToString used
as a substitute for server rendering. Async concurrent SSR, other browser engines,
real IME, progressive dirty-input preservation and deployed backend behavior are
not qualified by this fixture. It remains a manual browser check, not a remote CI
run or a performance benchmark.
Nine installed-package gates and git diff --check pass. The temporary tab and
Node harness were closed, and port 5189 was verified free after the run.


### Model construction owns rollback and lifetime guards

Extracted model setup into a private runWithOwnerScope child. Cleanup is registered
before the initial observer/synchronization/listener operations. Initial reads and
writes and each native registration validate owner/generation; a catch path repeats
best-effort cleanup to remove registrations attached after reentrant owner teardown.
A caught initialization failure disposes only its child scope/effect, preserving a
live caller owner. Callback activity is disabled before cleanup, and native cleanup
errors do not mask the original setup failure. Failed native removal may physically
retain an inactive listener/observer; this is not a native API infallibility claim.

Common type synchronizers and model events check their captured lifetime, removing
stale registrations without replaying old model values after raw reset. Selection
observers use the same guard and core reentry. No new WASM graph nodes are added;
one JS owner scope per model is added. Successful unowned bindings still require
an enclosing mount/root if automatic teardown is needed. No heap/performance win
is inferred from safety checks.

Seven new regressions all failed before implementation: partial listener setup,
late registration after owner teardown, initial value-setter disposal/reset, stale
type refresh, partially installed MutationObserver and native cleanup errors.
All 718 TS/WASM tests in 54 files pass, and full WASM/types/package/demo build passes.
The existing real Node SSR -> browser fixture was rerun against this code: 8/8
pass with zero server/client graph nodes after cleanup. Screenshot:
svet-model-construction-ssr.png in the task visualization directory. New fault
injection cases use jsdom; native browser proof covers ordinary SSR/model paths,
not monkeypatched native API faults or real IME candidate-window interaction.
Nine installed-package gates and git diff --check pass. The repeated browser tab
and Node SSR harness were closed; port 5189 is free.


### Resumption construction, runtime binding lifetime and reentry

resumeContainer captures caller/core/generation before parsing and rejects disposed
callers without replacing live state. After syntax validation each container gets
an object attempt identity. A stop controller is registered with the caller and
container before any initial binding writes, making pending construction disposable.
It marks stopped and detaches its registry/caller registration before root cleanup,
so cleanup reentry cannot erase a newer root. Root construction, binding entry/exit,
text conversions/DOM writes and raw attribute application validate caller/root and
attempt identity. A failed runtime lifetime check also stops its restored scope;
fresh post-reset native slots are not disposed through stale signal handles.

Newer independently owned attempts supersede older continuations. A nested attempt
owned by the scope it replaces is still canceled with that owner; there is no
implicit ownership escape. Snapshot syntax validation happens before supersession
or old cleanup. Already disposed previous resumptions are not revived after a valid
new construction fails, and arbitrary native setter side effects are not rolled
back. Successful resource cleanup removes the manual caller callback as well.

Eight cases cover disposed callers preserving existing bindings, initial/reset
and runtime/reset attributes, pending manual disposal, caller teardown during model
registration, independent setter reentry, cleanup reentry and nested-owner lifetime.
All initial six reproduced failures; the independent-reentry case explicitly uses
an independent root, preserving native ownership semantics. All 726 TS/WASM tests
in 55 files and the full build pass. Node SSR -> native browser form roundtrip was
rerun against this code and passes 8/8 with zero-node cleanup. Screenshot:
svet-resumption-lifetime.png in the task visualization directory. Fault injection
uses jsdom; this is not a DOM transaction or all-browser/native-fault qualification.
Nine installed-package gates and git diff --check pass. The temporary browser tab
and Node SSR harness were closed; port 5189 was verified free.


### Iterative SSR DOM serialization

serializeDomNodeToHtml now traverses using explicit node/close-tag frames and
output chunks, eliminating recursive calls through each child. Fragment order,
internal-comment omission, safe user comments, native void output and live form
serialization retain their prior semantics. Textarea first-LF preservation remains
explicit. Children are snapshotted per visited container and pushed in reverse so
output remains depth-first/in-order. Unsupported node types remain omitted.

Four cases cover real JSDOM chains of 4,000 elements, renderToString over a 3,000
chain with request cleanup, mixed fragments/comments and 1,000 wide siblings plus
live input/textarea/void elements. Both deep cases reproduced stack overflow before
the change; all four pass now. All 730 TS/WASM tests in 56 files and the full build
pass. Depth qualification is serialization of already constructed DOM, not HTML
parser acceptance of arbitrary depth. Temporary frames/chunks scale with visited
nodes/output; no constant-space, throughput or browser-wide limit is claimed.
No native-browser roundtrip was rerun specifically for this algorithm change.
Nine installed-package gates and git diff --check also pass.


### SSR request publication lifetime and graph retirement

renderToString captures the original caller and isolated render owner lifetimes,
checking after view/DOM/state serialization and after cleanup for caller validity.
Its finally resets only the isolated request core, so bare signals created without
owner cleanup cannot remain active after return/failure. Retained signal getters
are snapshots; the caller's live graph remains usable. Own-request reset during
teardown after HTML/JSON strings are complete is allowed, matching async teardown;
it does not invalidate already copied output. An initially over-strict cleanup
reset test was revised accordingly rather than rejecting this valid behavior.

The Node async renderer captures caller/request lifetimes before awaiting the view,
snapshots node/signals metadata once, guards serialization and rechecks caller
validity after request cleanup. A caller-owner cleanup cancels the request race
through a private AbortController while forwarding an explicit host abort signal;
listener/caller registrations unlink on completion. Pending user work may continue,
but request resources/graph close and late work cannot use the closed context.
Preflight failures remain promise rejections. The generic sync/async isolation
helpers' policies are unchanged; these guards belong to SSR render publication.

Nineteen cases cover view/DOM/JSON resets, disposed callers, await/cleanup teardown,
bare signal retirement, caller-graph preservation, primary errors, successful own
teardown resets, metadata reads and pending caller/host cancellation. Ten actual
defect cases failed before implementation; remaining cases qualify compatibility
and cancellation. Request-core construction failures reject asynchronously and
unlink the temporary caller cancellation hook. All 749 TS/WASM tests in 57 files and the full build pass. The
existing Node SSR -> real browser resumption fixture was rerun: 8/8 pass, screenshot
svet-ssr-request-lifetime.png. Installed Node consumer checks additionally exercise
bare sync signal retirement and caller-owned async cancellation without a DOM.
No remote backend or all-browser/native-fault coverage is inferred from these tests.

### Guarded browser history traversal and redirects

The previous popstate handler ran guards after the browser had already moved, but
left the visited URL/position in place when a guard rejected. Guard redirects also
changed only reactive state, leaving the original destination in the address bar.
Five of seven initial real-JSDOM history regressions failed before the change.

The router now lazily stamps its initial physical entry and pushed entries with a
reserved chain/index record. Rejected or throwing transitions return to the last
accepted position with the full relative history.go delta; the compensating event
skips guards. Programmatic navigate returns false while compensation is pending.
Guard redirects replace the visited entry and retain its neighbors. A nested
successful navigation commits from the actual visited slot and supersedes rollback.
Native compensation failure cannot mask a primary guard exception. Disposal clears
history-chain bookkeeping alongside the listener, guards and reactive state.

Unknown external entries are not assigned a guessed distance: rejection repairs
the URL in place, preserves host state and does not push/truncate forward history.
A subsequent push starts a new segment. Plain host object state fields survive
stamping; other state values use the reserved __svet_router_state__ envelope.
The metadata key __svet_router_history__ belongs to the router. One authoritative
router per window and intact metadata are required for positional recovery; host
code must not copy metadata into independently created history entries.

Twelve tests in router-history.test.ts cover denied Back/Forward/multi-entry moves,
redirect slot persistence, nested navigation, host object/primitive state, pending
compensation admission, external segments and primary-error precedence. The full
suite passes 761 TS/WASM tests in 58 files. The native browser fixture
/tests/browser/router-history.html passes 8/8 using real History API traversals;
proof: svet-router-history.png. Run it via ordinary Svet Vite in a disposable tab.
Concurrent user traversals, cross-document navigation, all-browser behavior,
scroll/focus restoration and failure-proof native History methods remain unqualified.

### Finite overlapping History API traversal qualification

A separate continuation tested the existing compensation algorithm before changing
it. Six bursts [-1,-1], [-1,-2], [-2,-1], [-1,1], [-2,1,-1] and [-1,-1,1,1]
all settled correctly in JSDOM and in the actual browser. No engine defect was
reproduced, so the runtime algorithm was retained. Each scenario starts from an
accepted middle entry with forward history available, denies guards, observes an
event-quiet interval, checks unchanged history length plus URL/signal agreement,
then confirms that a fresh navigate call is admitted and commits its URL.

The quiet interval is 40 ms in JSDOM and 80 ms in the browser, bounded by explicit
timeouts. This is a finite stress regression, not a proof of all future event
interleavings or of arbitrary user/browser task scheduling. The browser fixture
now reports 14/14; screenshot svet-router-history-bursts.png. router-history.test.ts
has 18 cases; the complete suite passes 767 tests in 58 files. No new Rust/WASM,
GPU, transport or cross-document behavior is inferred from these history checks.

### Native nullable FileList model binding

File inputs previously inherited text value/input conversion: a non-null FileList
was stringified and assigned to the forbidden nonempty value property, and a
native selection's change event did not publish files. Seven of eight initial
regressions failed. The binding now uses FileList|null, change events and the native
files setter. null clears with an empty value; input/cancel do not double-publish.
The common binding covers reactive types and resumed fields. Invalid non-FileList
values retain native validation errors; a failed construction retires the provisional
binding scope. Owner teardown during a files getter cannot publish a late selection.
SSR skips file value serialization entirely, including not invoking the value getter.

model-files.test.ts adds ten cases. JSDOM supplies real empty native FileLists;
only the clearing-path getter/setter observation is simulated. The real-browser
fixture /tests/browser/model-files.html uses nonempty DataTransfer-generated
FileLists with in-memory Files: multiple selection publication, native files setter,
null reset, type switching, JSON-null resumption, SSR omission and disposal all pass
8/8. Screenshot: svet-file-models.png. The full suite passes 777 tests in 59 files.
System picker interaction, disk selection, uploads, all-browser behavior and binary
JSON serialization are not qualified. FileList/File data is client-only; snapshots
must start nullable rather than attempting to restore files from JSON objects.

### Native form reset publication and owner-safe coordination

Ordinary reset did not publish model values: native reset does not emit input/change,
and the reset event occurs before the default action. Nine of eleven initial tests
failed. A shared reset coordinator now observes document/form capture events and
queues one microtask per event, respecting final defaultPrevented. It snapshots all
associated model values before writes and batches publications per captured core.
Checked radio candidates take precedence over unchecked peers for a shared signal;
no selected default produces null, and file reset also publishes null. Composition
ends only for successful reset. Values follow native HTML defaults, not an implicit
copy of initial signal values.

Weak per-element model records and reference-counted document/form listeners avoid
an enumerable global control registry. Existing form association is registered at
binding time; h('form') connects the initially assembled controls, covering detached
Svet-created forms. Connected external form-associated controls are found through
the native elements getter, which also avoids named controls shadowing form.elements.
Owner cleanup unlinks registrations; queued work checks generation/owner state and
stops stale registrations. Native listener registration reentry that disposes the
owner cannot retain a late document lease.

A follow-up test reproduced an option MutationObserver restoring old model state
before the reset snapshot. Pending reset counts now defer that observer until reset
publication; canceled reset still permits the observer to synchronize afterward.
Another follow-up fixed stale reset registrations not detaching on invalidation.

model-reset.test.ts contains 17 cases including atomic observation, cancellation,
IME flags, radio/select defaults, nullable files, external controls, detached forms,
queued observers, graph reset and listener teardown/reentry. All 794 tests in 60
files pass. The real browser /tests/browser/model-reset.html passes 8/8, including
native reset-button activation and clearing a nonempty DataTransfer FileList.
Screenshot: svet-form-reset.png. Composition events are synthetic, not OS IME input.
Arbitrary later native insertion/moves of models into detached forms, Shadow DOM
routing, custom form-associated elements and all-browser behavior remain unqualified.

### Owned reset areas and public ShadowRoot mounting

Eight of nine initial model-reset-roots regressions failed: initial-control-only
registration missed late native insertion into empty detached Svet forms, transfers
between forms, closed-shadow enclosing forms and later native forms. Every live
h('form') now owns a reset lease independent of initial controls. mount owns leases
on its stable container and initial tree root. Leases capture lifetime, register
cleanup before native operations, and release even after owner-disposal reentry
returns a late lease. Failed registrations remove provisional cleanup hooks. The
old initial-control scanning adapter was removed. No mutation observer or native
DOM prototype patch is required for these owned reset areas.

mount now publicly accepts HTMLElement|ShadowRoot. model-reset-roots.test.ts has
13 cases: late append/moves, closed roots, direct mounting/replacement, cancellation,
array-publication deduplication and listener lifetime/failure tests. The installed
strict consumer also compiles direct closed-ShadowRoot mounting.

Exposing ShadowRoot mounting revealed a separate event gap: document-only delegation
cannot see closed-tree controls or noncomposed events. Five of eight initial event
cases failed. Delegated bindings now retain an owner-inherited local shadow mount
area alongside their document fallback. Each local listener traverses only to its
boundary, handles its own area, and document fallback checks the actual route to
avoid duplicate bindings. This requires no Event-object dispatch cache, so synchronous
redispatch/reuse still works. Native methods/currentTarget/target facade behavior
is retained. Local/document registration is staged with cleanup before native calls;
registration failure or disposal reentry cannot leave an active provisional binding.

shadow-mount.test.ts has 13 cases including Show/For late children, Portal to the
document, open-tree retargeting, slot fallback, noncomposed input, Event reuse/move,
stopPropagation, retained-node teardown and local registration failure/reentry.
All 820 tests in 62 files pass. Native browser fixture
/tests/browser/model-reset-roots.html passes 16/16; proof: svet-shadow-mount-reset.png.
The fixture exercises closed roots, late forms, dynamic events, slot/Portal fallback,
noncomposed input, native host propagation and full graph retirement.

This supersedes the previous reset qualification gap for owned areas. Completely
unmanaged detached trees, prebound unowned subtrees, cross-document adoption,
individual node/Portal moves into another closed tree outside their inherited area,
and all-browser behavior remain unqualified. No broad native-event parity, heap,
throughput or cross-document guarantee is inferred from these finite tests.

### DOM callback generation and synchronous core ownership

Retained event handlers previously checked only binding.active, and direct listeners
had neither generation checks nor guarded registration. Eight event-lifetime tests
all failed before implementation: graph reset still ran direct/delegated callbacks,
old peers ran after a reset within dispatch, direct setup under a disposed owner
registered callbacks, callbacks used the dispatching core instead of their own,
and native direct registration failure/disposal reentry left late callbacks/hooks.

All on* bindings now capture owner/core/generation before setup. Both direct and
delegated callbacks check that lifetime before user code, lazily unlink stale
bindings, and enter the captured core synchronously with restoration afterward.
Native this/currentTarget/event facade behavior is retained. Direct listener cleanup
is registered before native registration, repeated after late attachment, and
unlinked on setup failure. A failed native remove leaves an inactive callback.
New bindings after reset keep their own generation even on the same retained node.

All 828 tests in 63 files pass. The real-browser fixture
/tests/browser/event-lifetime.html passes 8/8: ordinary native callbacks, reset and
fresh-state preservation, same-dispatch peer invalidation/new registration, explicit
independent-core dispatch, closed-shadow callbacks, direct registration reentry and
final graph retirement. Proof: svet-event-lifetime.png. Native fault reentry is
simulated on the specific test element, not a claim about arbitrary native failures.

The checks guard entry and synchronous execution; already-running user code and
async continuations are not interrupted or automatically canceled. Graph reset is
not immediate global DOM teardown: stale callbacks detach when encountered, while
owner disposal/unmount releases listeners directly. No cross-document adoption,
transport, all-browser or performance qualification follows from these checks.

### Style construction and reactive snapshot lifetime

The ordinary nested-style replacement hypothesis did not reproduce: effect reruns
already retire their previous child scopes, and the new bounded-shape regression
passed before changes. Five of six initial tests instead found lifecycle defects:
static partial construction leaked a nested effect after a later coercion failed;
getters/resolvers/coercion/native setters could invalidate an owner or graph and
still publish returned CSS or later properties.

Style binding construction now runs in a private owner scope. Each reactive
application additionally has a provisional scope, so a failed update retires newly
created nested effects without killing the outer subscription; a subsequent valid
snapshot recovers and clears partially managed keys. All value/property effects
capture the binding/run/property lifetime and check it around user reads/coercion
and native writes. Object keys are snapshotted before guarded per-key value reads.
This avoids Object.entries continuing later getters after owner invalidation.
String-style metadata/native item reads are guarded too.

Three follow-ups reproduced and fixed provisional effects surviving a failed
reactive update, later getters running after disposal, and a matching native CSS
read returning success after it invalidated the owner. These are distinct from
ordinary shape replacement, which was already bounded and remains so.

style-lifetime.test.ts adds nine cases. All 837 tests in 64 files pass. The native
browser fixture /tests/browser/style-lifetime.html passes 8/8 against real CSSStyle
operations, covering graph/node count, failure/recovery, getter/coercion/reset,
resolver invalidation, native setter reentry and final graph retirement. Screenshot:
svet-style-lifetime.png. Element-local native hooks simulate fault/reentry paths.
Private JS scopes add ownership bookkeeping; no heap/throughput improvement is
claimed. Existing style/property effects retain their normal WASM node costs.
Already-written CSS is not rolled back on failure, and arbitrary user cleanup
exceptions/native-fault combinations or all-browser behavior remain unqualified.

### Attribute/property resolver and coercion lifetime

All nine initial attribute-lifetime regressions failed: stale resolver/coercion
output reached native HTML properties or namespace/boolean attributes; matching
native reads could return success after owner teardown; native writes did not
reject completion after invalidation; failed static-object/reactive coercion left
temporary effects active in a surviving caller.

bindAttribute now captures caller/run lifetime before resolver evaluation and uses
the guard throughout applyStaticAttribute/applyAttributeValue. Checks surround
coercion, native reads/writes/removals and type/radio-value model synchronization.
Reactive snapshots get provisional scopes; static object coercion also gets one,
while static primitive values avoid extra scope allocation. Failed work retires its
temporary subscriptions and retains the outer reactive subscription for recovery.
Boolean tokens, namespace names, live value/checked properties and URL/inline-event
filtering remain unchanged.

The first full run found one integration regression: the generic helper guard masked
resumption's richer superseded error with a disposed-owner error during independent
reentrant resumption. The existing regression was kept; resumeContainer now passes
its own publication guard into the shared writer, preserving token/registry safety
and error precedence. All existing resumption tests pass alongside the new cases.

All 846 tests in 65 files pass. Native fixture
/tests/browser/attribute-lifetime.html passes 8/8 with real HTML/SVG operations,
getter/coercion/reset/setter reentry, failure recovery and graph retirement.
Proof: svet-attribute-lifetime.png. Fault paths use element-local native hooks.
As with styles, already-completed native writes and arbitrary user side effects
are not rolled back. No heap/throughput, all-browser or arbitrary native-fault
combination guarantee follows from these finite checks.

### Cleanup core isolation and caller dependency isolation

The initial primary-error masking hypothesis was refuted: scope teardown already
continues past callback failures and preserves an original construction exception.
That behavior is retained and explicitly tested. Seven of eight initial regressions
instead exposed retained root/effect disposers invoking cleanup in the dispatching
core, scope/effect cleanup reads becoming dependencies of a caller effect, and reset
inside root cleanup invalidating the calling core's live graph.

Public root disposers now select their captured core. Public effect disposal also
selects its creation core and runs untracked. Iterative owner teardown, effect owner
cleanup and returned effect cleanup are untracked, covering failure and rerun paths
without changing depth-first/reverse-callback ordering or exception policy. Captured
core selection restores the previous core even when a returned cleanup throws.
Existing untrack write semantics still dirty already-read caller dependencies.

cleanup-context.test.ts has 11 cases: core selection/restoration, root/effect/failed
child-scope dependency isolation, error-policy compatibility, cleanup writes, and
reset in root/returned-effect cleanup preserving the calling graph. All 857 tests
in 66 files pass, including the existing 12,000-scope iterative teardown tests.
All three real native HTTP action tests pass, including abort during body streaming.
The installed Node consumer additionally checks independent-core disposal and
cleanup-read isolation without Vite or a DOM.

This is synchronous core/tracking isolation, not execution under a reestablished
owner context, automatic cancellation of async cleanup continuations, or a claim
about arbitrary user resource creation during teardown. No heap/performance or
remote backend/CI qualification follows from these bounded tests.

### Foreign signal/computed callback and cascade execution

Eleven of twelve initial reactive-core-context regressions failed. Captured native
IDs belonged to the right graph, but an updater/comparator/effect could execute JS
with the foreign caller selected. Cascaded flushEffects could drain the wrong graph,
computed evaluation/peek used the wrong core, callback reads polluted caller effect
dependencies, and reset in a callback invalidated the calling graph.

Signal writes now detect foreign entry before user callbacks and execute in their
creation core. Computed reads/peek enter their core before evaluation/comparison or
tracking a paused caller. Both foreign caller and paused callee tracking are paused
around entry; actual effect/computed runs still collect their own dependencies.
Context restores after callback failure; foreign callback writes still invalidate
already-read dependencies. Same-core updater/comparator tracking is preserved.
Stale handles keep their cached/no-op behavior before any core lookup. Cached
unobserved computed reads and peek return early without unnecessary core switching.

reactive-core-context.test.ts has 14 cases: updater/comparator selection, single and
queued subscribers, cascades, read/peek comparison, reset in callbacks, caller read
isolation, exceptions/restoration and cross-core nested writes. All 871 tests in 67
files pass. All three native HTTP action tests pass. Installed Node consumer gates
add independent-core updates, effects/cleanup, computed reads/peek and live caller
state checks, without Vite or DOM.

A controlled local Node v22.23.4 arm64 A/B microbenchmark used current ESM with the
foreign-entry checks removed as control, seven alternating rounds of 1,000,000
operations each plus 100,000 warmup operations per sample. Baseline/candidate median
milliseconds: write-only 16.635/19.354 (+16.35%, about +2.72 ns/op), one-observer writes
55.601/53.503, cached computed 6.925/6.860, cached peek 8.989/6.873. These latter
measurements are observations, not claimed framework speedups. The write-only cost
is retained for correct context selection. No closure is added on the same-core
write path. AsyncLocalStorage/server imports and end-to-end latency were not profiled.
The temporary A/B artifacts include script, source copies, checksums and samples;
they are local evidence, not a historical release comparison or production gate.

Cross-graph reads are snapshots, not an automatic reactive bridge. Async callback
continuations, all-browser behavior, arbitrary graph composition and broader
throughput/heap/performance qualification remain outside this finite proof.

### Source constructor metadata before native allocation

Eight of eleven initial source-construction tests failed. Signal and computed
allocated a native ID before reading options.equals: a throwing getter leaked the
constructor node, reset followed by fresh signal allocation could collide with the
unfinished constructor's registry write, and computed owner teardown could occur
before cleanup registration. Invalid comparator/body values were accepted until
later use despite the typed callable contract.

Both constructors now read comparator metadata once and validate it before native
allocation. They recheck the captured graph generation after reading metadata;
computed also rechecks its owner, and validates the body before invoking option
getters. Non-function supplied comparators/body throw TypeError. Nullish comparator
fallback remains Object.is. Existing primary getter errors propagate unchanged,
even if the getter reset the graph and created fresh state before throwing.

source-construction.test.ts has 12 cases, including normal comparison/metadata-read
count and intentional independent lifetime of bare signals after caller-root
teardown. All 883 runtime tests in 68 files pass. The installed Node consumer adds
throwing/invalid metadata without allocation, reset/fresh-slot preservation,
reactive notification from the fresh replacement signal, and invalid computed body.
Two test-only typing issues discovered by the full build were fixed with explicit
getter return/generic annotations; the runtime constructor contract was unchanged.

Basic signals remain independent sources; this does not make them owner-disposed
or roll back arbitrary user allocations/side effects performed inside option
getters. Native allocation failure, all runtime object shapes and broader resource
transaction semantics remain unqualified by these finite constructor checks.

### Multiple effect cleanup registrations and late registrar lifetime

Inspection of effect construction found that registerCleanup overwrote its previous
callback and a returned cleanup overwrote it again. Nine of ten initial regressions
failed. Effect bodies now reject non-functions before owner/native-node allocation;
the invalid-body test counts native allocator calls through a forwarding real-WASM
wrapper rather than treating a final zero node count as proof of no allocation.
Cleanup arguments are validated before changing registrations.

Each invocation lazily creates a callback list. Registered and returned cleanups
are drained once in reverse registration order using an iterative detached list,
including 10,000 callbacks without recursive composition. All callbacks finish even
when several throw; the first encountered error remains primary, including undefined.
Late registrars from disposed/older invocations execute their new callback immediately
without replacing a newer invocation's cleanup. Reentrant late registration remains
immediate and does not mutate the detached list being drained.

The first full run exposed caller dependency leakage when late cleanup read a
caller-core source. A shared captured-cleanup helper now pauses both contexts for
foreign cleanup and is also used by public root/effect disposal. Captured teardown
still works if the caller's request resolver rejects access to its closed context.
This extends the previous core-isolation checks to caller-core reads, preserving
ordinary cleanup writes and prior exception behavior.

Two follow-up regressions reproduced queued effects permanently missing later writes
after cleanup failure, both for direct and computed dependencies. The error path
now rearms the effect and actual native computed ancestors, marking cached computed
values for retry. Per-edge ancestry checks reject stale JS numeric-slot dependencies
so a fresh computed at a retired slot is untouched. The original cleanup exception
also survives failure of the native repair query. This repair is conservative and
error-only; it can force later computed evaluation and does not promise recovery
from arbitrary native failures or rollback of cleanup side effects.

The new effect-cleanup-registration file has 14 cases; cleanup-context has three
additional cases. All 900 tests in 69 files pass. All three native HTTP action tests
pass. Installed Node consumer checks additive/reverse cleanup, late registrar core
and read isolation, invalid effect bodies and recovery through a dirty computed.
The existing 12,000-owner teardown and prior lifetime/slot-reuse checks still pass.
No async-cleanup completion, heap/throughput or full native-fault qualification is
inferred from these bounded tests.

### Context Provider metadata and child publication lifetime

Eight of ten initial context-provider-lifetime regressions failed. Provider evaluated
value before its child scope existed, read children metadata repeatedly, and could
invoke children or move output after owner disposal/reset. Provisional effects from
a throwing value getter stayed attached to a surviving caller.

Provider now captures caller lifetime before metadata, snapshots value and children
inside protected scopes, installs the provided value before children metadata, and
checks caller/provider validity through document-fragment creation, child rendering
and insertion. Getter/factory execution remains once per invocation. Nested context
shadowing/default lookup and failed child resource retirement remain compatible.
Replacement effects/signals after reset survive teardown of the rejected old scope.

Two follow-up regressions found that a guard only after insertChild was too late:
after the first array insertion invalidated its owner, a second native move still
happened; static text coercion could append its result after teardown. The shared
insertChild now snapshots arrays before mutation and checks its caller guard between
entries and around static native/coercion operations. Provider, Portal, Suspense and
owned branch rendering forward their existing guards to preserve richer publication
errors. Dynamic insertion retains its existing range recovery/ownership policy.

Two additional regressions ensure value-getter effects keep their parent context
on later runs. Construction and provided-value scopes use separate context maps,
so installing a value cannot change metadata observers retrospectively. This adds
JavaScript scope bookkeeping; no performance or heap improvement is claimed.

context-provider-lifetime has 13 cases and static-child-lifetime has one. All 914
tests in 71 files pass. The real-browser fixture
/tests/browser/context-provider-lifetime.html passes 11/11; screenshot
svet-context-provider-parent-lifetime.png. It includes actual native fragment insertion
reentry and verifies that the second caller-owned node remains unmoved. Installed
Node consumer checks metadata failure cleanup and disposed-caller preflight before
DOM instantiation; this is not a full headless Provider rendering qualification.

Already completed native operations and arbitrary user side effects are not rolled
back. Async subtree context, arbitrary native-fault combinations and whole-app
performance/heap behavior remain unqualified. useContext semantics outside the active
owner scope were not changed by this work.

### Delegated events during engine-driven cross-document adoption

Six of seven initial event-adoption cases failed: mounting, Portal publication,
new dynamic/keyed children and static subtree moves retained only source-document
listeners. Synchronous dispatch inside native insertion also missed the handler.
An insertion failure leaving the node at its source was already compatible.

A weak per-node route registry now stages shared destination-document leases before
an adopting native write. Afterward it reconciles leases with actual node placement.
This is integrated into static/dynamic insertion, mount, Portal and owned/keyed
branch commits. Consumed fragment descendants are collected before insertion.
Ordinary same-document commits skip the descendant walk. Each pending adoption pins
its destination independently, so nested native insertion hooks cannot release an
outer transaction's listener early. Ownership and generation checks protect setup;
late leases after registration teardown are released before publication.

Follow-up regressions exposed three additional defects: dynamic native-write rollback
restored placement without routing, nested adoption released an outer pending route,
and failed physical listener removal caused duplicate dispatch after a return move.
Rollback now stages the source route too. Retired listener entries become inert even
if native removal fails, and their registry identity retires before native removal
can reenter registration. Native insertion/registration failures retain their primary
exception; completion of already performed native writes is not generally undone.

All 927 tests in 72 files pass, including 13 event-adoption regressions. Full build,
nine isolated installed-package gates and three native action HTTP checks pass.
The actual-browser fixture /tests/browser/event-adoption.html passes 10/10 with
same-origin iframe documents, nested synchronous native hooks, partial-insertion
rollback, a deliberately retained retired native listener and owner teardown.
Screenshot: svet-event-adoption-final.png. Package gates qualify installation and
exports; browser behavior is supported by the separate native fixture.

This qualifies adoption performed by these Svet commits. Raw native adoption/moves
outside the engine, already-adopted nodes with stale document leases, and document
routing hidden inside arbitrary closed shadow subtrees remain open. Existing direct
listeners stay on their native node. No global DOM/prototype patches, per-node native
event bridges or asynchronous observer delay were added. Cross-document traversal,
route maps and lease bookkeeping add work; no throughput or heap improvement is
claimed. Cross-origin access and async context propagation remain separate scopes.

### Model reset routing during document adoption

Five of seven initial model-adoption cases failed: Portal and static subtree adoption
into a foreign native form did not move model reset routing, including closed shadow
roots and reset dispatched synchronously inside native insertion. Canceled reset and
owner teardown before queued publication were compatible. Another regression found
that removal of an old root's shared callback could erase a new registration created
by native removal reentry.

The adoption registry now carries both delegated-event and model-reset routes.
withDocumentAdoption replaces the event-only helper at every existing commit/recovery
site. Model routes stage the destination document and physical tree root before the
write, then reconcile to the actual control document, form and root. Pending stages
pin roots independently; failed setup releases its provisional leases. Registration
teardown releases a native lease that arrives after cleanup. Model entries leave weak
reset lookup before their leases are released. Shared reset roots now have distinct
listener identities per retained entry and become inert when retired, so native
removal reentry cannot remove a newer registration.

All 937 tests in 73 files pass, including ten model-adoption regressions. Full build,
nine installed-package checks and three native HTTP action checks pass. The real
browser /tests/browser/model-adoption.html passes 8/8: same-origin iframe forms,
closed shadow roots, immediate reset inside insertion, mixed event/model routing,
cancellation and teardown. Screenshot: svet-model-adoption.png. The existing event
adoption browser fixture was rerun against this shared implementation and passes
10/10; screenshot: svet-model-adoption-event-regression.png.

This qualifies model controls discovered in subtrees whose document adoption is
performed by Svet commits. Same-document raw native moves, stale routes after
external adoptNode, arbitrary hidden shadow descendants and cross-origin access
remain open. Model values retain existing post-default-action microtask semantics;
reset is not made synchronous. Additional root/route maps and shared listener leases
are JavaScript bookkeeping, with no extra reactive WASM nodes. No heap or throughput
improvement is claimed; full application performance remains unqualified.

### Independent event dispatchers and measured adoption traversal

A native-browser A/B probe exposed duplicate calls when two separately compiled
Svet engines used one document. Each engine had its own document listener map but
read the same node binding sets. All four initial event-instances regressions failed,
including mixed nested bubbling, additive bindings on one node and listener counts.
Filtering callbacks by engine would still violate child-before-parent order across
engines, so compatible copies now share a versioned target-owned dispatcher registry.
Its reference counts and retirement identity coordinate across copies, while each
binding invokes its own captured lifetime/core callback. Last-owner teardown removes
the registry. Older bundles lacking the protocol remain unqualified.

The adoption descendant walk now uses firstChild/nextSibling instead of requesting
and iterating a NodeList for each visited node. It preserves traversal order, guards,
iterative depth handling, route snapshots and staged commit/recovery semantics. The
same-document fast path still skips the walk. This avoids the NodeList/iterator path;
no heap-size reduction has been measured.

scripts/prepare-adoption-bench.mjs creates independent baseline/candidate JS snapshots
with identical engine sources except the traversal. It records every source hash,
Node/TypeScript/platform identity and a unique snapshot directory. Unique paths are
required because development-server node_modules resources may be cached immutably.
The final candidate hashes were checked against every current engine source file;
dom.ts is the only differing baseline/candidate file. The initial probe that failed
handler checks was rejected as a performance result; it led to the dispatcher fix.

Native Chrome 156 on this macOS arm64 host, nine alternating measured rounds after
two warm-up rounds, median elapsed milliseconds per sample:

| Scenario | Native moves/sample | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: | ---: |
| Same-document, 1500 text spans | 240 | 14.3 | 14.4 | +0.7% |
| Cross-document, 1500 text spans | 24 | 15.8 | 8.0 | -49.4% |
| Cross-document, depth 1500 | 24 | 9.3 | 5.4 | -41.9% |
| Cross-document, 400 event/model rows | 24 | 18.4 | 14.7 | -20.1% |

Timings include native insertion but exclude construction, placement/handler/model
checks and teardown. Both variants pass those checks and retire all reactive nodes.
The earlier exploratory run also improved all three cross-document scenarios, but
its 24-move ordinary-insertion sample was too short (1.4 vs 1.5 ms) for a useful control;
the final control increased its sample length tenfold. Both raw results are retained:
svet-adoption-ab-exploratory.json and svet-adoption-ab-final.json. Final screenshot:
svet-adoption-ab-final.png. This is a bounded local A/B, not a whole-app throughput,
heap, cross-browser or peer-framework qualification.

All 941 tests in 74 files pass; full build, nine isolated installed-package gates and
three native HTTP action checks pass. Browser event-instances passes 7/7 against the
independently compiled snapshots (svet-event-instances.png). Existing event adoption
10/10 and model adoption 8/8 were rerun against current source after the optimization;
screenshots: svet-instance-adoption-regression.png and svet-instance-model-regression.png.
Physical cross-realm module execution and legacy-bundle interoperation are not proven
by the two-copy same-realm test. Async context and raw unmanaged DOM relocation remain
separate open scopes.

### h and ref construction ownership

Nine of ten initial h-construction regressions failed. h copied all prop metadata
before establishing a protected construction lifetime, component factories could
publish after disposal, and caught metadata/component/ref failures left effects
attached to a surviving caller. A failed ref could also leave earlier event/model
bindings live on its retained element. Ref disposal before child insertion was
already caught by the shared insertion guard.

h now preflights its caller, enters a private construction owner, snapshots enumerable
own props with guarded key/descriptor/getter phases, and checks lifetime through
native creation, prop binding, child insertion, select synchronization and component
output. Snapshots preserve symbols, own __proto__ data, omitted non-enumerable/inherited
props, getter receivers and explicit child overrides. Public ref binding has its own
protected scope so failure cleanup also applies outside h. Ref return values remain
ignored; callbacks can register teardown through onCleanup.

Seventeen h-construction cases cover these paths, native creation teardown, old/new
reactive generations, prop ownKeys reentry, child-coercion failure and tracking
compatibility. Metadata still participates in caller tracking; component bodies
remain untracked. Owned effects/bindings retire on failure; independently owned bare
signals preserve their existing lifetime contract. Failed construction preserves
its primary exception and does not retire independent replacement graph lifetimes.

All 959 tests in 75 files pass. Full build, nine isolated installed-package gates and
three native HTTP action checks pass. Installed Node checks run metadata/component
failure cleanup and disposed-caller preflight before DOM instantiation; they do not
qualify full headless element rendering. The actual browser fixture
/tests/browser/h-construction.html passes 11/11, including an actual custom-element
constructor that disposes the caller. Screenshot: svet-h-construction-ancestors.png.

The extra construction/ref scopes allocate JavaScript owner/context bookkeeping,
without adding reactive WASM nodes. No construction-throughput or heap-size claim
is made. Completed DOM writes and arbitrary callback side effects are not generally
rolled back. Async components/refs, direct Fragment construction outside protected
callers and unmanaged native relocation remain separate open scopes.

A follow-up h sibling-cleanup regression and a core owner-teardown regression exposed
execution under a stopping ancestor before a later child is physically disposed.
A shared internal liveness check now follows ancestors during active teardown,
covering view/event/model guards, effect invocation, computed reads/construction,
child-scope creation and captured-owner reentry. Outside teardown it retains a direct
owner fast path, because engine disposal drains all descendants synchronously.
Physical sibling visitation flags, depth-first order, reverse per-scope cleanup and
12,000-level iterative teardown remain compatible. The browser's eleventh case and
installed Node consumer also exercise suppressed descendant execution. This does not
pre-mark all siblings as disposed or change independent-root/bare-signal lifetimes.

Native event adoption 10/10 and model adoption 8/8 were also rerun against the final
construction/liveness changes; screenshots: svet-h-event-regression.png and
svet-h-model-regression.png. The h ancestor-cleanup regression additionally verifies
that a later model's native input event cannot overwrite its independent source.

### Bound event owner context and tracking isolation

Ten of eleven initial event-context cases failed. Direct and delegated handlers
entered the captured core but inherited the dispatching owner's context (or none).
Their subscriptions could become unowned or belong to a different transient caller,
and their reads could subscribe an effect that synchronously dispatched the event.
Invocation under a closed ambient resolver was already compatible.

The common invoke path now runs untracked in the engine's caller/callee cores and
reenters the binding's captured owner. It reuses the bound owner without allocating
a new owner scope for every event. useContext, onCleanup, created observers and new
DOM therefore use the view's inherited context/lifetime. The former runCleanupInCore
helper was generalized in name/export to runUntrackedInCore; its core/tracking and
closed-ambient-resolver behavior is reused by both disposal and events. Captured-owner
reentry restores previous owner state on success/error and preserves reset-generation
rules. Native this/currentTarget and the event facade remain intact.

Fourteen event-context cases include nested direct/delegated dispatch, callback errors,
foreign-core caller tracking restoration, closed-resolver compatibility and a new
node created inside a closed-shadow event. Event-created nodes inherit their bound
mount area and stop responding after mount disposal. Explicit effects created by a
callback still track their own dependencies. Native callback exceptions propagate
normally; resources created before an exception remain view-owned until teardown,
without introducing a per-event transaction or accumulating empty per-event scopes.

All 973 tests in 76 files pass. Full build, nine isolated installed-package gates
and three native HTTP action checks pass. Installed Node exercises the direct-handler
entry path with its native EventTarget (no HTMLElement/browser qualification): bound
context, observer ownership and untracked dispatch are checked. The real-browser
/tests/browser/event-context.html passes 11/11, including native window error reporting,
a closed shadow tree, callback-owned cleanup and two reactive cores. Screenshot:
svet-event-context.png. The existing event-lifetime browser fixture passes 8/8 against
this implementation; screenshot: svet-event-context-lifetime-regression.png.

This is synchronous propagation. Async continuations after await, cross-engine-copy
signal tracking, legacy bundle interoperation and unmanaged DOM movement remain open.
No event throughput or heap-size improvement is claimed. The wrapper adds JavaScript
invocation bookkeeping but no reactive WASM nodes, owner scope per dispatch or extra
native listeners. Unowned bindings still require an explicit managed lifetime.

### Direct Fragment construction and effect rerun retirement

All eight initial fragment-construction cases failed. Direct Fragment created native
DOM before caller preflight, read defined children metadata twice, leaked getter-owned
effects after caught failures, and could accept output invalidated by native creation
or metadata reset. h(Fragment, ...) already provided an outer construction boundary,
but the public direct call did not. This resolves the direct-Fragment open scope in
the earlier h construction milestone.

Fragment now preflights its caller, creates a private owner, checks native creation,
snapshots children once and forwards its combined lifetime guard through insertion.
Provisional effects/cleanup hooks retire on failure and preserve the primary exception,
even if a cleanup hook throws. Existing-node output is rejected before stale metadata
can move it; independently created replacement graph lifetimes survive old teardown.
Reactive accessor children retain their observer semantics after fragment consumption.

A further check exposed reentry into a later old descendant while an effect's earlier
child was being cleaned up. The native queue already coalesced some old effect work,
but explicit computed reads and captured-owner creation could still execute there.
A temporary weak retirement marker now covers the effect owner during child/resource
cleanup. The shared liveness predicate checks that marker during active teardown;
outside teardown the direct-owner fast path remains. Empty effect-owner cleanup
returns immediately. Marker removal and teardown-depth restoration are protected by
finally, including nested cleanup/disposal. Physical child visitation flags, order,
independent roots and permanent disposal semantics are preserved.

Nine Fragment cases and a new core owner-teardown case cover the construction and
rerun behavior. Disabling only the temporary-marker check reproduced both follow-up
failures; restoring it passed. The counterfactual source edit was restored before
full verification. All 983 tests in 77 files pass, including existing 12,000-level
owner teardown cases. Full build, nine isolated installed-package gates and three
native HTTP action checks pass. Installed Node checks root/JSX Fragment alias identity,
disposed-caller preflight without DOM, and the core rerun barrier; it does not render
native fragments headlessly.

The native /tests/browser/fragment-construction.html fixture passes 9/9, including
consumed reactive fragments, native creation reentry, primary error preservation,
computed suppression during rerun, reset and replacement-state preservation.
Screenshot: svet-fragment-construction-rerun.png. The event-context browser fixture
also passes 11/11 against this final implementation; screenshot:
svet-fragment-event-regression.png.

The additional owner/context scope and temporary weak marker are JavaScript lifetime
bookkeeping without extra reactive WASM nodes for Fragment construction itself.
No heap or throughput improvement is claimed. Async continuation propagation,
unmanaged DOM movement and arbitrary native-fault/side-effect rollback remain open.

### Explicit captured context for awaited continuations

captureReactiveContext is now a public root/reactivity API with CapturedReactiveContext
and ReactiveContextDisposedError. Capture requires a live owner. The handle snapshots
owner/core/generation, registers a single cleanup hook and exposes active, generic run
and idempotent dispose. run validates entry before and after ambient-core resolution,
then uses untracked captured-owner reentry. Provider lookup, created observers, DOM
routing and cleanup therefore belong to the captured view, with caller state restored
on success/error. It does not allocate an owner scope or reactive WASM node per run.

The owner cleanup hook permanently invalidates an old capture before an effect body
is reused, so old continuations cannot revive on later effect invocations. Liveness
also rejects captures during ancestor/rerun retirement and after graph reset. The
second entry check rejects ownership/generation changes caused by ambient resolver
reentry before user code can touch replacement state. Manual disposal releases the
snapshot and unregisters its hook without disposing the view or its existing effects.
Closed ambient request resolvers do not prevent a separately live capture from running;
a capture owned by a closed SSR request is inactive and rejects entry.

Nineteen captured-context cases cover actual awaited reentry, independent parallel
providers completed in reverse order, owner disposal while awaiting, effect rerun,
reset, explicit disposal, errors/return values, untracked caller
reads with tracked owned observers, nested restoration, foreign cores, resolver
reentry and closed-resolver compatibility. A callback returning a promise explicitly
proves immediate context restoration rather than implicit propagation after await.
Resources created before callback failure remain view-owned until view teardown.

All 1002 tests in 78 files pass. Full build, nine installed-package gates and three
native HTTP action checks pass. Real installed Node verifies public awaited reentry,
resource ownership and typed stale rejection. It also captures inside an isolated
async SSR request and confirms invalidation when the request closes. Strict consumer
checks verify generic return inference, CapturedReactiveContext, readonly active and
the literal error code. The native /tests/browser/captured-context.html fixture passes
10/10, including capture from a real Svet event, awaited node creation in a closed
shadow root, reverse parallel completion, cancellation by owner disposal and two
reactive cores. Screenshot: svet-captured-context.png.

This is an explicit synchronous continuation boundary, without global Promise/DOM
patches or owner retention across await. Automatic async component rendering,
transparent Promise context propagation, fetch cancellation, latest-request ordering
and cross-copy reactive tracking remain separate open scopes. Heap size and callback
throughput have not been measured for the added snapshot/hook bookkeeping. Bare signals
and independent roots retain their existing lifetime contracts.

### Capture cleanup scalability and destructive reentry qualification

Three additional captured-context checks verify owner disposal during run with an
explicitly independent root, disposal of a foreign dispatching owner before subsequent
signal reads, and graph reset during run with replacement effects. These passed the
original implementation. Scaling measurements then found a different problem:
each active capture occupied an owner.disposables entry, and each manual release
searched/spliced that array. Large release batches therefore had quadratic cost.

Captures now share one owner cleanup group. A single active capture uses a direct
slot, concurrent captures promote to a Set, and the last manual release detaches the
group hook. A recorded array slot has an identity check/fallback search if another
resource removed an earlier cleanup entry. Empty groups are weakly cached by owner;
there is no global strong owner cache. Bulk owner cleanup clears snapshots without
per-token Set mutation. It retires the group before draining and clears it in finally,
so a reused effect owner gets fresh lifetimes and old handles never revive. User
cleanup order and lifetime visibility remain compatible.

The bounded-registration regression initially observed 2000 hooks instead of one.
It now verifies one shared hook, no per-token array search, complete release and fresh
recapture. Further cases cover user callback ordering, stale slot fallback, partial
release/additions and bulk invalidation of 256 captures over effect reruns.
All 1009 tests in 78 files pass (26 captured-context cases). Full build, nine isolated
installed-package gates and three native HTTP action checks pass. Installed Node also
checks a 1024-capture group and complete hook detachment. The browser continuation
fixture passes 11/11 including 1024 grouped captures and a 256-capture rerun; screenshot:
svet-captured-context-groups.png.

scripts/bench-captured-context.mjs compares two compiled ESM package directories:

```
node packages/svet/scripts/bench-captured-context.mjs --baseline /absolute/baseline --candidate /absolute/candidate --output /absolute/result.json
```

The local Node 22.23.4 macOS arm64 run uses two warm-ups and seven alternating measured
rounds per scenario. Construction, explicit release and owner stop are timed separately;
correctness checks and index generation are outside timing. Every sample confirms
inactive handles, empty owner hooks, preserved live-owner status for manual release
and zero reactive graph nodes. Only reactivity.js differs between compiled variants;
all candidate compiled-module hashes were checked against the current package build.

Final median milliseconds (capture time is a separate interval from release):

| Scenario | Baseline | Grouped candidate |
| --- | ---: | ---: |
| 20000 explicit releases, creation order | 257.746 | 1.685 |
| 20000 explicit releases, fixed shuffled order | 30.507 | 2.471 |
| Creation of 20000 captures, owner-stop case | 3.952 | 5.712 |
| Owner stop with 20000 pending captures | 0.229 | 0.173 |
| 50000 sequential capture/dispose cycles | 10.374 | 11.997 |

The first Set-only grouping improved batch release but regressed the sequential control
from 11.212 to 15.752 ms; the single-slot path reduced that overhead. The final control
still costs about 16% more in this local run, and creation can cost more. This is a
scalability tradeoff, not a universal throughput or heap improvement. GC/timing variation
is visible across rounds; raw samples are retained. There is one cached JS group per
owner that has used capture, with no additional reactive WASM node.

Artifacts: svet-capture-release-baseline.json, svet-capture-release-ab.json (initial
Set version), svet-capture-release-control.json, svet-capture-release-final.json.
Each A/B record includes compiled module hashes, environment and all timed samples.
Async propagation/rendering, transport cancellation, cross-copy tracking and full-app
performance/heap qualification remain open.

The optional manual GC smoke check is reproducible after package build with
node --expose-gc packages/svet/scripts/check-captured-context-gc.mjs. On this Node
runtime it retained 4000 disposed capture handles while all 1000 weakly referenced
owner roots were collected (zero graph nodes). Output: svet-capture-group-gc.json.
This checks that the weak group cache/disposed handles do not pin those roots under
forced Node GC; it is not a heap-size, browser-GC or long-running application proof.

### Capture allocation refinement (2026-10-31)

Replaced the per-capture `isActive` closure with a shared state predicate. The
public own enumerable properties and detachable `run`/`dispose` methods remain
covered by a regression test. Group registration and cleanup semantics are unchanged.

A CPU profile pointed to capture construction and GC rather than group lookup.
An unprofiled A/B compared the preceding grouped implementation with this refinement
on Node 22.23.4 / darwin arm64 (two warmups, seven alternating measured rounds).
The 50000 sequential capture/dispose median was 16.035 -> 14.010 ms, about 13%
lower in this run. Bulk results were mixed: 20000 shuffled total 12.657 -> 10.406 ms,
while 1000 shuffled total 0.245 -> 0.361 ms. These are local microbenchmarks with
visible GC variation, not a full-engine speedup or proof that the earlier overhead
against the original per-token cleanup implementation is eliminated.

Raw records, environment, all samples and compiled-module hashes are retained in
`svet-capture-allocation-refine.json`; only reactivity.js differs between variants.
The full regression suite passes 1010 tests in 78 files.

### Resource retirement during ancestor cleanup (2026-10-31)

Resource liveness now uses the shared logical-owner predicate, including ancestor
teardown and effect-rerun retirement. Previously a cleanup in an earlier sibling
could call a later sibling's retained resource before its raw disposed flag was set:
refetch reread the source and started a second fetch, and mutate published new data.
Two regressions reproduce this for root stop and effect rerun (before fix: reads=2,
fetches=2, mutated snapshot; after: reads=1, fetches=1, initial snapshot). The pending
signal is aborted by normal disposal and eventual results remain ignored; graph
telemetry returns to zero. Full suite: 1012 tests / 78 files.

This closes reentrant resource publication during teardown. AbortSignal remains
cooperative: a fetcher that ignores it can keep doing external work, and a refetch
Promise awaiting that fetcher need not settle until it does.

### Router retirement during ancestor cleanup (2026-10-31)

Router construction and runtime liveness now use the shared logical-owner check.
A later sibling can still have disposed=false while an ancestor is stopping; its
router must already be inactive. The same applies while a containing effect drains
its previous invocation's descendants.

Two regression cases exercise retained navigate, guard registration and popstate
from an earlier sibling cleanup during root stop and effect rerun. Both failed
before the change because navigate returned true. Afterward it returns false,
neither existing nor newly submitted guards run, retained path stays /a, and no
pushState, replaceState or history.go occurs. The native event URL (/b in the
fixture) is left untouched by this retired router. All reactive nodes are released.
These cases use jsdom; they do not newly qualify browser history traversal timing.
Full suite: 1014 tests in 78 files.

### Resource cancellation settles public waits (2026-10-31)

Resource requests now race a cancellation completion against their existing async
worker. Abort settles cancellation before dispatching synchronous abort listeners,
with both active slots detached first; reentrant replacements keep their own slots.
The worker retains the direct await of the fetcher's Promise, preserving data and
loading publication ordering for consumers awaiting that original Promise. The race
observes late worker rejection even after cancellation has already won.

Five regression cases previously remained unsettled after explicit abort, dispose,
owner stop, superseding refetch and disabling the source. They now resolve undefined
without resolving the underlying fetchers, confirm aborted signals, preserve current
replacement loading/data, and reject obsolete fetchers afterward without publishing
errors. Full suite: 1019 tests / 78 files. The installed native Node consumer now
also awaits cancellation before letting the original fetchers complete.

This supersedes the earlier note about explicit cancellation waits depending on
fetcher settlement. Cancellation of external work remains cooperative. Raw reactive
graph reset still invalidates results without a proactive per-resource notification;
it does not newly gain immediate wait settlement or request abortion.

### Resource reset cancellation after storage retirement (2026-10-31)

Added an internal, unregisterable one-shot reset-finalizer subscription, separate
from the existing permanent GPU pre-storage-reset hook. Pending resource requests
register a finalizer and remove it on normal settlement, cancellation or disposal.
Reset increments generation and clears native/JS storage first, then detaches the
finalizer batch and invokes it under the reset core. Failures do not skip the rest.
New requests register in a new batch; recursive resets drain only that new batch.

Resource finalizers cancel public waits and abort their fetcher signals. Four new
regressions cover an uncooperative pending refetch, abort-listener replacement
state, foreign-core isolation, and recursive reset with remaining outer requests.
The replacement signals survive old resource disposal and remain writable. Native
installed-package coverage also checks reset cancellation and replacement state.
Full suite: 1023 tests / 78 files.

This supersedes the earlier raw-reset cancellation limitation. Signal accessors
from the old generation retain their frozen snapshots (loading may remain true);
they are not new-generation live state. External work cancellation remains
cooperative. Active requests are registered until settlement, explicit teardown
or reset; this is not a qualification of browser GC or overall heap usage.

### Resource callbacks and async commits retain their WASM core (2026-10-31)

Refetch now reads its source and starts its fetcher under the resource core, with
both ambient and suspended resource-core observers paused. Abort/dispose dispatch
listeners under that same guard. Async success/error publication and finalization
explicitly reenter the owned core; batching therefore covers all state writes in
the graph that owns those signals. Reentry rechecks request liveness before commit.
Error normalization also rechecks after user-controlled string conversion.

Four regression cases failed before this change: caller effects gained fetcher
dependencies; a suspended resource-core observer gained dependencies through a
foreign call; and async success/error each emitted an intermediate loading=true
snapshot before their final snapshot. All now pass, alongside prior cancellation,
reset and recovery cases. Installed native Node coverage checks owned fetcher and
abort-listener core identity plus atomic async success publication from another
core. The change does not install automatic async callback context propagation,
transfer callback ownership, or qualify tracking across independent package copies.

Validation for resource core isolation: 1027 tests / 78 files, full WASM/TypeScript/
package/demo build, and all nine installed-package consumer checks passed.

### Resource construction rejects reentrant invalidation (2026-10-31)

The initialValue getter is read once before state allocation, followed by a
logical-owner/generation check. The same check runs after source observer startup
inside the existing failure cleanup guard. Initial source or synchronous fetcher
execution can stop its owner or reset the graph; construction now rejects that
invalidated result and cleans partial state instead of returning a stale resource.
Ordinary fetcher failures remain resource errors, preserving the existing API.

Six cases cover options/source/fetcher invalidation under reset and owner stop.
They verify source/fetcher invocation counts, one option read, zero retired nodes,
and a writable replacement signal surviving disposal of the old root. Before the
fix the reset cases and source/fetcher owner-stop cases returned stale resources;
options owner-stop reached a later effect-construction error. Installed native Node
coverage also checks an initialValue-triggered reset with no source execution and
exactly the independent replacement node remaining. Full suite: 1033 / 78 files.

### Server actions cancel pending calls at graph reset (2026-10-31)

An action registers one reset finalizer when its first call starts, unregisters
when the last call settles, and removes the hook on explicit/owner disposal. The
existing typed disposal path aborts and rejects every pending call, including RPCs
that ignore AbortSignal, after old native/JS graph storage has been cleared.

Three regressions reproduced missing reset cancellation before this change. They
now verify overlapping uncooperative RPC cancellation, no late optimistic rollback,
a replacement action/signal created by an abort listener surviving old-owner
cleanup, and another WASM core continuing its active request. Full suite: 1036 tests
in 78 files. The installed Node consumer also checks typed reset cancellation.
Native loopback HTTP coverage now includes reset before response headers and checks
that fetch abort closes the still-unfinished response; total HTTP cases: four.

This supersedes the earlier standalone-action raw-reset limitation. Retained old
signal accessors still freeze their pre-reset snapshots, including pending=true.
External server-side effects remain subject to cooperative transport/application
cancellation; reset does not run obsolete optimistic rollbacks into new graph state.

### Server actions reject work during ancestor retirement (2026-10-31)

Construction and invocation liveness now use logical owner retirement, covering
root teardown and effect-rerun cleanup before the walker reaches a later sibling.
Previously an earlier sibling cleanup could read both callback options, apply an
optimistic update and start an RPC through that sibling's retained action. Both
new regression cases reproduced counts [2,1,1]; they now assert [0,0,0] and typed
ServerActionDisposedError rejection. Cleanup releases all graph nodes. This closes
the pre-start side-effect window rather than merely discarding RPC completion.

Validation: 1038 tests / 78 files, full build, four native loopback HTTP cases,
and all nine installed-package consumer checks passed.

### Server action callbacks pause caller and callee tracking (2026-10-31)

Replaced core-only callback switching with runUntrackedInCore for action disposal,
option reads, optimistic update, RPC startup, rollback and error normalization.
A guarded callback helper rechecks lifetime after entering the captured core.
Receiver binding for options callbacks is retained. Six newly reproduced cases
previously subscribed both a suspended owned-core observer and a foreign caller;
these now remain at one invocation after both source signals change. A seventh
case covers user-controlled error string conversion under the same isolation.

Two more regressions reproduced disposal/reset during error normalization followed
by obsolete rollback. A liveness check after normalization now rejects with the
typed disposal error before rollback, preserving independent fresh graph state.
All 1047 tests in 78 files pass. Installed native Node coverage exercises nested
caller/action cores and confirms no extra observer invocations. Automatic async
callback context propagation, third-core tracking and cross-package-copy tracking
are not newly qualified by these two-core tests.

### RPC then getter isolation with native Promise adoption (2026-10-31)

Promise.race setup now runs under the action-core untracked guard. Native adoption
can synchronously read a returned thenable's then getter; previously it ran after
leaving the guarded RPC callback and collected reads into both suspended callers.
The reproduced counts were outer=2, inner=4, getter=4 after changing two sources;
the regression now requires 1/1/1 and owned-core identity. Native adoption, getter
read counts, receiver handling and microtask scheduling are retained.

Five added cases cover getter isolation, observation of an already-rejected Promise
returned by an RPC that synchronously disposed the action, plus throwing/dispose/
reset getters and their rollback/lifetime rules. The race guard intentionally does
not reject early on stale lifetime: it must still observe such returned Promises.
Full suite: 1052 tests / 78 files. Installed Node coverage now exercises the then
getter through the nested-core action regression.

A separate compiled Node diagnostic returned value=7, getterUsesOwnedCore=true,
microtaskMethodUsesOwnedCore=false for a custom thenable owned by a nondefault core.
The later custom then method is still native Promise microtask work and is an open
context-propagation boundary. This change does not claim full thenable execution
isolation or replace the standard Promise resolution algorithm.

### RPC then-method and nested adoption context (2026-10-31)

RPC results now pass through private protocol adapters with fresh null-prototype
proxy targets. A then getter is read on its original object in the action core;
its callable result is invoked asynchronously there with the original receiver.
Resolution arguments get adapters lazily, so nested thenables retain the same core
without recursively walking the chain. Native Promise resolution still controls
first settlement, reject/throw precedence and scheduling of each then-method job.
Terminal adapters are unwrapped before returning, preserving result identity.
Fresh targets also support frozen user thenables without proxy invariant failures.

Four failing method cases reproduced wrong nondefault-core identity and reset
behavior. Added coverage verifies first resolve winning over reject and throw,
receiver identity, getter read once, async invocation, method throw/dispose/reset,
plain nonthenable result identity (three getter reads, matching observed prior
native behavior), and a 1500-link frozen chain with one getter/method call per link
and no recursive stack growth. Full suite: 1058 tests / 78 files. Installed Node
coverage checks a frozen thenable and nested method in a nondefault core.

This supersedes the preceding microtaskMethodUsesOwnedCore=false diagnostic.
Adapters add JS allocations and can add an adoption job for native Promises; this
is a correctness change with no action-throughput or application-speed claim.
Exact Promise constructor/species instrumentation counts are not a new API
guarantee. User callback code after its own await still needs explicit continuation
capture; these adapters do not propagate owners or patch global Promise behavior.

### Lazy reverse mapping for RPC adoption (2026-10-31)

Reverse mapping now records only terminal object adapters, at the point their then
property is found noncallable. Promise/thenable adapters need no reverse entry;
primitive results and rejection paths allocate no reverse WeakMap. The terminal
map is created lazily, including when a self-resolving thenable changes into a
nonthenable object. A regression verifies that transition, terminal identity and
four observable getter reads. Existing frozen/1500-link/lifetime cases remain.

Reproduce the local benchmark using immutable ESM package snapshots:

    node packages/svet/scripts/bench-server-actions.mjs --baseline BASELINE_DIR --candidate CANDIDATE_DIR --calls 50000 --output RESULT.json

Each scenario uses two warmups and seven alternating measured rounds. Action
creation/disposal and final correctness/graph checks are outside timing; sequential
calls include the real action implementation and await/result handling. Errors
include the loop's expected-failure identity branch. Compiled module hashes and
all samples are recorded. Only resumable.js differs in this A/B. No forced GC.

Node 22.23.4, darwin arm64, median milliseconds per 50000 calls:

| Result | Eager reverse map | Lazy terminal map |
| --- | ---: | ---: |
| Native Promise, primitive | 41.591 | 40.451 |
| Native Promise, object | 47.676 | 46.846 |
| Two nested thenables | 46.778 | 44.986 |
| Rejection with rollback | 83.774 | 84.186 |

The small local changes (-2.7%, -1.7%, -3.8%, +0.5%) are subject to runtime/GC
variation; this does not establish a universal throughput improvement. The A/B
compares eager vs lazy reverse mapping, not protected adoption vs the older
unprotected action implementation, and excludes transport/application latency.
The short preliminary CPU profile included 18 GC samples and 38 action samples;
it motivated reducing allocations but is not an allocation-byte measurement.
Raw artifact: svet-action-adoption-lazy-ab.json.

Lazy reverse-map validation: 1059 tests / 78 files, full WASM/TypeScript/package/
demo build, four native HTTP checks and nine installed-consumer checks passed.
All measured candidate JS hashes matched the current compiled package.

### GPU logical retirement and scalar conversion safety (2026-10-31)

GPU execution/construction liveness now uses the shared logical-owner predicate.
Retained tensor methods reject reads/writes once ancestor teardown or effect-rerun
cleanup begins, before physical tensor cleanup. Configuration, tensor/camera
construction and device initialization reject a retired ambient owner. Tensor
length and camera option reads now have owner preflight; configuration already
preflights before options. Existing physical cleanup/unregistration remains intact.
Compute owner checks also use logical retirement, including async publication.

Three regressions reproduced tensor access during root/effect retirement and
configuration/tensor construction during an earlier sibling cleanup. They now
confirm unchanged borrowed data, no input/option getter reads, no adapter request,
unchanged external GPU configuration/budget, and zero reactive nodes. These use
fake device/adapter surfaces; no new physical GPU execution is qualified.

Separately, setAt now converts the scalar before reacquiring its live WASM view.
The previous order let valueOf dispose/reallocate the tensor and then write into
replacement storage: a compiled Node diagnostic observed 99 instead of 17. WASM
memory growth during conversion silently lost a write (7 remained instead of 9).
Four regressions reproduce dispose/owner-stop/reset invalidation and memory growth;
they now preserve replacement data and publish the grown-memory write once. The
installed native consumer also checks scalar replacement safety. Borrowed views
remain caller-managed; these guards protect API methods, not raw view mutations.

Full suite: 1066 tests / 78 files. One first full run timed out only in the existing
comparison benchmark's 5000ms limit (1065 passed); an unchanged repeat passed all
1066 in 6.13 seconds. No benchmark thresholds/timeouts were changed.

### Compute preparation binds lifetime before option reads (2026-10-31)

Both compute entries capture core/generation/owner and preflight before inspecting
options. Shared metadata snapshotting preserves own enumerable data (including
symbols and __proto__), checks after ownKeys/descriptor/getter access, and reads
inputs once. Inherited/nonenumerable inputs remain supported. Input iteration
checks each yielded item and completion; invalidation closes the iterator before
another item is requested. Async preparation keeps the originally captured owner
through admission/validation and publication rather than recapturing it later.

The previous capture-after-spread order adopted replacement graph state after
reset in an option getter: compiled native diagnostics reported calls=1 and
replacementValue=99 for both sync/async entries (replacement started at 17). Four
initial regressions reproduce this and double input-getter reads. Eight more
cases cover disposed-owner metadata preflight, iterator closure on reset, and
inherited/nonenumerable input compatibility. Full suite: 1078 / 78 files. Installed
Node coverage also checks reset getter rejection, no later output getter/fallback
execution and preserved replacement data in both entries. Existing admission,
compute and renderer tests remain the evidence boundary; no new physical WebGPU
qualification or dispatch throughput improvement is claimed.

### Host fallback core, ownership and tracking (2026-10-31)

Synchronous fallback invocation pauses tracking under its dispatch core while
retaining the captured owner. Input snapshot collection remains outside that guard
so explicitly supplied input tensors still drive enclosing effects. Deferred async
fallback reenters the captured core and owner before invoking its synchronous
prefix. Async publication uses untracked core entry with request liveness rechecked
inside the guard. Callback-owned effects/cleanup therefore retire with the original
dispatch owner rather than becoming unowned work in the ambient default core.

Two regressions reproduced incidental signal dependencies (two invocations instead
of one) and a deferred nondefault-core callback failing tensor access with
GPU tensor belongs to a different reactive context. They now verify incidental
reads are untracked, input writes still recompute, correct core/tensor access,
owned effect updates and root teardown, and zero remaining graph nodes. Full suite:
1080 tests / 78 files. The installed native consumer also exercises a nondefault
core fallback and checks effect/cleanup ownership after completion and root stop.

This qualifies host fallback entry/publication, not new WGSL hardware behavior or
throughput. It does not carry the owner/core through arbitrary user awaits or
claim tracking suppression in every third suspended context. Returned async work
continues to need cooperative cancellation and explicit continuation capture.

### Transactional transform/camera initialization (2026-10-31)

A shared scene-construction cleanup stack retains signals, tensors and the producer
effect until successful publication. On failure it disposes partial resources in
reverse order, continues after disposal errors, and preserves the original failure.
Transform/camera success keeps existing explicit/owner disposal behavior. Transform
metadata now uses guarded enumerable snapshotting (shared with compute preparation),
binding generation/owner before getters and stopping between protocol operations.
The parent matrix handle is captured once as a borrowed dependency. Camera option
reads also check between each field and precede all allocations.

Transform tuple iteration/coercion finishes before native matrix writes. Explicit
float32 conversion preserves the native input representation; owner/generation and
local/world/parent tensor storage are rechecked before writes. This prevents reset
or disposal from turning numeric conversion into a write to reused storage.

Four initial cases failed before the change: a standalone failed tuple initializer
left five graph nodes, reset in transform metadata returned a new-generation scene,
and camera stop/reset still invoked later getters. They now release/reject early.
Two additional cases protect a 32-float replacement tensor during numeric reset
and ordinary parameter signal release when initial tuple iteration stops its owner.
Full suite: 1086 tests / 78 files. Existing parent/world dependency and render tests
were rerun; one intermediate wrong mat4_mul argument order was caught by the parent
transform regression and corrected to (parent, local, destination). Installed Node
coverage checks original failure identity, zero leftover nodes and numeric-reset
replacement preservation. No new physical GPU rendering or allocation-pressure
qualification is claimed; construction adds a temporary cleanup stack and transform
updates add conversion/liveness checks, without a throughput claim.

### Mesh preparation, initialization and topology lifetime (2026-10-31)

Mesh construction now captures core/generation/owner before metadata, uses guarded
snapshotting, and borrows world/projection matrix handles once. Metadata, source
length, each coordinate conversion and index read check liveness before further
preparation. All unpublished matrix/vertex tensors, the producer and topology
signal use scene-construction cleanup. Successful meshes use whole-scene owner
disposal; ordinary topology signals therefore share the same manual/owner lifetime
as tensors and the producer. Native matrix calls preserve (left,right,destination)
order; dependent storage is rechecked before native writes.

Four failing cases reproduced three leaked mesh tensors after producer failure
(nodes 9 instead of dependency baseline 6), metadata reset adopting a new scene,
continued coordinate reads after reset, and updateIndices continuing after mesh
disposal from a getter. Two more cases verify captured matrix descriptor identity
and reset during topology length reading. All 1092 tests in 78 files pass. Installed
native Node cases verify original initialization error identity, baseline graph
restoration, skipped later coordinate reads and independent replacement storage.

These are native WASM storage/lifecycle and fake-renderer regression checks. They
do not add physical WebGPU/device-pressure or overall rendering-speed qualification.

Two additional scene-disposal checks confirm producer onCleanup failures are
suppressed by the existing owner traversal contract, callbacks execute once, all
mesh/transform resources are released, and repeated disposal is inert. Initial
test expectations of error propagation were corrected to match that contract;
no disposal error-policy change was required. Current full suite: 1094 / 78 files.

### Topology commit guards against getter reentry (2026-10-31)

Each mesh tracks accepted topology commits. updateIndices captures that version
and checks it alongside lifetime after each source read and before publication.
A nested accepted update invalidates the older candidate before it can overwrite
indices or the default format; invalid candidates do not advance the version.
The version advances before topology signal publication, and no old writes occur
after observers run, so observers may commit a replacement without rollback.

Two failing cases reproduced accepted nested updates from length/index getters
being overwritten by outer preparation. Four new cases now verify skipped later
reads, Uint32 topology and default-format retention, failed nested candidate
non-supersession, and observer-time replacement. Full suite: 1098 / 78 files.
Installed native consumer coverage repeats the getter-time newer-commit case and
checks final topology, format and zero remaining graph nodes. No global latest-call
policy or physical GPU/frame-throughput qualification is inferred from this guard.

### Native-reported arena bound before mesh snapshots (2026-10-31)

Rust's MAX_GPU_ARENA_FLOATS is now shared by allocator enforcement and the pure
svet_gpu_get_arena_limit_f32 export. The TypeScript bridge exposes that query;
createGpuTensor preflight no longer duplicates the native capacity literal. Mesh
preparation rejects when its own two vec4 buffers plus mat4 (8*vertices+16 floats)
exceed that capacity, before indices copy, coordinate getters or temporary arrays.
This is a necessary intrinsic bound, not a reservation/availability query: live
dependencies, other allocations and fragmentation remain the allocator's authority.

Three regression cases reproduced large temporary snapshot attempts for aggregate
overflow, a single-buffer overflow and a near-safe-integer coordinate length. A
guarded Float32Array constructor intercepted those attempts before real large
allocation; the new path makes zero such attempts and skips index/coordinate reads,
preserving graph nodes and arena length. Current native limit is 67,108,864 floats
(256 MiB); this is distinct from device-wide async snapshot admission budgets.

45 native Rust tests passed, including exporter/allocator limit agreement and
above-limit failure preserving existing arena data. Release SIMD WASM rebuilt to
53,601 bytes. Full JS/WASM suite: 1101 tests / 78 files. Installed Node coverage
checks the new export, oversized tensor preflight and aggregate mesh rejection
with a safety guard preventing large snapshot allocation if a regression occurs.
No real near-capacity allocation, allocator reservation or physical GPU pressure
qualification is inferred from these bounded checks.
