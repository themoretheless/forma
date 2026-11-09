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

## Early local verification (2026-10-31)

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

### Bounded arena reservation qualification (2026-11-01)

Arena growth now reserves geometrically with try_reserve_exact, capped at the
native element limit, before resizing. Small-limit regressions cover reserved
capacity, amortized growth, zeroed tails, preservation of existing values and
capacity-overflow rejection without changing the previous allocation.

Current verification passed: 48 native Rust tests, release SIMD WASM rebuilt to
54,035 bytes, full production build, 1101 JS/WASM tests across 78 files, nine
installed-package checks and four real HTTP action tests. These bounded tests do
not qualify actual near-limit allocation, process RSS, transient allocator peaks,
WASM linear-memory reclamation or physical GPU memory pressure. Performance
measurement and broader work remain tracked in PLAN.md.

### Fragmented arena churn (2026-11-01)

A native regression runs 2000 cycles of 128 differently sized allocations with
deterministically shuffled frees, retaining a live 16-float prefix throughout.
It checks zeroed reused slices, preserved prefix values, full free-block
coalescing after every cycle, unchanged arena address/capacity and dirty-bitmap
capacity after the first cycle, and a bounded free-list capacity. No reset masks
retained storage. All 49 Rust tests passed (0.83 seconds for the suite locally).
The duration is a test observation, not a controlled performance comparison.
This qualifies 256,000 native allocation/free pairs, not JS heap, WASM runtime
memory pressure, GPU allocations or whole-application longevity. Production code
is unchanged by this test-only addition.

### Real WASM arena measurement (2026-11-01)

Reproduce after building the package:

```sh
node --expose-gc packages/svet/scripts/measure-arena-churn.mjs /tmp/svet-arena-churn.json
```

The script imports the compiled internal WASM bridge, creates an isolated core,
checks data and reuses storage without reset between batches. It records module
hash, Node/OS/CPU, raw batch timings, process memory and WASM linear-memory size.
GC and memory sampling occur outside timed batches. The recorded run is in
`benchmarks/arena-churn-2026-11-01.json` (Node 22.23.4, Apple M4 Max).

Growing to 1,048,576 floats via 4096 allocations took 2.119 ms locally. After 100
warmup cycles, seven batches of 500 cycles made 448,000 allocation/free pairs;
median batch duration was 36.073 ms including JS assertions and data writes.
These are observations without an A/B baseline, not allocator-only throughput
or a speedup claim. Timing variation requires repeated controlled comparisons.

After warmup, WASM memory remained 9,764,864 bytes and arena address stayed fixed.
Post-GC heapUsed changed from 4,211,552 to 4,233,960 bytes; RSS from 30,411,248 to
30,460,400 bytes across measured batches. These finite samples do not establish
long-term process-memory bounds. Reset cleared logical arena length to zero but
retained the same WASM linear-memory size. No physical GPU or near-limit pressure
was exercised. Full application lifecycle and multi-core memory checks remain open.

### Mixed lifecycle churn on two WASM cores (2026-11-01)

```sh
node --expose-gc packages/svet/scripts/measure-lifecycle-churn.mjs /tmp/svet-lifecycle-churn.json 20000
```

The developer probe alternates two isolated cores without reset, constructing a
root with a signal/effect, four captured contexts, settled and uncooperative async
resources, settled and uncooperative server actions, transform, camera and mesh.
Bare input signals are explicitly registered for cleanup because their lifetime
is independent of roots. It checks completed results, cancellation settlement,
disposed captures and zero graph nodes/edges/pending jobs after every teardown.
There are no physical GPU allocations or DOM nodes in this scenario.

Both 2000- and 20,000-cycle runs passed; raw results are saved in
`benchmarks/lifecycle-churn-2026-11-01.json` and
`benchmarks/lifecycle-churn-20k-2026-11-01.json`. The longer run observed 20,000
root cleanups, 40,000 effect executions, 40,000 resource aborts and 20,000 action
aborts. Each core retained capacity 29, 88 arena floats and 1,179,648 WASM bytes
from cycle 100 onward. Native pending queue entries were zero at all samples.

Process-memory readiness remains open. In the longer run, post-GC heapUsed/RSS
were 6,922,664/33,935,248 bytes at cycle 100, 7,650,920/62,710,712 at 10,000 and
7,775,520/64,170,016 at 20,000. External memory stayed at 5,456,334 bytes. These
samples include a growing in-process measurement log and runtime warmup; they
neither isolate retained application references nor prove a process-memory
plateau. Follow-up requires bounded measurement storage and retention analysis,
plus concurrent pending roots, shared devices and reset scenarios.

### Bounded lifecycle recorder and collection checks (2026-11-01)

The lifecycle probe now retains at most 32 rolling samples plus two baseline
samples. It checks WeakRefs to nine objects per cycle: root owner, input signal,
effect handle, settled resource, pending resource, action, transform, camera and
mesh. WeakRefs are drained every 100 cycles, with up to ten explicit collections
separated by event-loop turns. Dereferencing in the same job before GC would
itself keep targets alive; the probe avoids that ordering. Collected objects are
asserted independently of zero native graph counts.

The new 20,000-cycle run passed all 180,000 collection checks, with up to nine GC
rounds needed in a sample. Raw results, recorder bounds and probe/module hashes
are in `benchmarks/lifecycle-retention-20k-2026-11-01.json`. Native graph and WASM
storage remained at the earlier stable values. From cycles 16,900 to 20,000,
post-GC heapUsed changed from 7,544,392 to 7,574,928 bytes and RSS from 60,206,024
to 60,714,648 bytes. This supports collection of the sampled lifecycle objects;
it does not prove all engine references or a flat process RSS. No cause is
assigned to the residual RSS change without heap/allocator profiling. The older
unbounded-recorder artifacts remain historical measurements, not results of the
new probe. Overall memory readiness remains open.

### Bounded compute pipeline reuse (2026-11-01)

Compute dispatch now caches validated pipelines by device identity and exact
WGSL/entry-point pair. The device-local LRU retains at most 32 entries and 1 MiB
of UTF-16 key bytes; oversized sources execute without caching. It does not
cache bindings, tensor views, snapshots or result buffers. Complete transactions
remain serialized per device. New entries are retained only after successful
readback, resource cleanup and error-scope resolution; GPU transaction failures
evict their key, so invalid pipelines and shader errors are retried.

Device loss clears the cache, as does destruction of an engine-owned device.
Detaching an external device preserves its reusable pipelines for other cores
or later attachment. Core reset preserves device pipelines independently of
cleared tensor storage. Keys include the entry-point length to avoid ambiguous
string concatenation; workgroups and data changes do not invalidate pipelines.
The key-byte limit does not bound driver pipeline memory.

Nine new deterministic device-double regressions cover repeated dispatch with
new input/workgroups, WGSL and entry-point discrimination, device replacement
and reattachment, cold/cached validation failure, LRU count eviction, byte-budget
eviction, oversized-source bypass, isolated-core sharing with distinct bindings,
and shader-compilation failure/retry. The first regression failed before the
implementation because two dispatches created two shader modules.

Full production build, 1110 JS/WASM tests across 78 files, nine installed-package
checks and git diff --check passed. Release WASM remains 54,035 bytes. These
tests qualify cache control flow and output/buffer lifecycle using device
doubles; native browser and physical GPU cache qualification, driver memory and
throughput measurement remain open. No end-to-end speedup is claimed.

### Compute cache on native browser WebGPU (2026-11-01)

The expanded `tests/browser/gpu-compute.html` fixture passed 10/10 in the local
in-app browser. Adapter information reported vendor apple, architecture metal-3
and isFallbackAdapter false. The fixture wraps real GPUDevice creation methods
to count shader modules and compute pipelines while all WGSL, buffers, bindings,
submission and readback use the actual device.

Three dispatches of the original source (including changed input, explicit main
entry point and changed workgroups) created one shader module and one pipeline,
with checked numerical results. A two-entry-point source created two distinct
pipelines and reused the alternate entry on its second invocation. Injected
out-of-memory/internal scope errors evicted the cached original pipeline; the
fixture checked the exact recompilation count during failure and recovery.
Invalid WGSL rejection, publication count, transfer accounting, admission budget,
queue drainage, device loss and replacement also passed.

Text and screenshot evidence are saved in
`benchmarks/compute-pipeline-metal-2026-11-01.txt` and the adjacent PNG. The outer
real validation scope completed without error. This adds hardware correctness
evidence to the deterministic cache tests; it does not measure speedup, driver
memory, hardware LRU eviction, other adapters/backends or resident GPU graphs.

### Hardware compute-cache A/B (2026-11-01)

Reproduce from the repository root, specifying the pre-cache revision explicitly:

```sh
node packages/svet/scripts/prepare-compute-cache-bench.mjs 48a8c5954d4b81daee605e87578e5fd028e4a94a
npm --prefix packages/svet exec -- vite packages/svet --host 127.0.0.1 --port 5189 --strictPort
```

Open `/tests/browser/compute-cache-bench.html` and run the benchmark. Preparation
creates immutable source snapshots below node_modules/.cache; only gpu.ts is
replaced with the baseline revision, while other engine sources and embedded
WASM are identical. Metadata includes source/JavaScript hashes, baseline commit,
compiler version and fixture hash. The candidate hash was verified against the
current worktree; both artifacts differ only in gpu.ts.

Two runs on the same non-fallback Apple Metal 3 adapter used identical WGSL and
data on a common GPUDevice, with separate engine instances. Each workload warms
both variants with ten calls, then alternates baseline/candidate order across
seven rounds of 40 calls per variant. Numerical outputs and queue credits are
checked; the second run additionally checks zero graph nodes after disposal.

| Run | Floats | Baseline median / 40 calls | Cached median / 40 calls | Change |
| --- | ---: | ---: | ---: | ---: |
| 1 | 7 | 28.400 ms | 17.700 ms | -37.68% |
| 2 | 7 | 29.800 ms | 19.300 ms | -35.23% |
| 1 | 65,536 | 53.000 ms | 41.500 ms | -21.70% |
| 2 | 65,536 | 56.600 ms | 44.100 ms | -22.08% |

Every measured baseline batch created 40 shader modules/pipelines; every warm
candidate batch created zero. Raw rounds and hashes are in
`benchmarks/compute-cache-ab-1-2026-11-01.json` and
`benchmarks/compute-cache-ab-2-2026-11-01.json`, with screenshot
`benchmarks/compute-cache-ab-metal-2026-11-01.png`. All result/count/queue checks
passed; no uncaptured GPU errors were observed. The temporary server was stopped.

These observations measure sequential dispatch/readback wall time including JS
snapshots, buffer allocation, validation, publication and per-call result checks.
They do not isolate GPU kernel execution or prove application frame-time gains,
resident-chain throughput, memory savings or superiority over another framework.
The first run predates the extra post-workload graph check and fixture hashing;
those checks lie outside measured batches. Driver caches and concurrent machine
load are not controlled across machines; retain the raw data for reproduction.

### Resident intermediates within a compute dispatch (2026-11-01)

Async compute options now accept `chain: readonly GpuComputeChainStage[]`, with
up to 31 additional stages after the original kernel. Each stage has its own
WGSL, optional entry point and workgroups. Additional stages read the previous
same-length float32 output at binding 0 and write binding 1. The output is seeded
from the previous result through a device-to-device copy, preserving untouched
elements for partially writing kernels.

The executor compiles/checks all stage pipelines before creating storage, reuses
identical pending pipeline keys locally, and publishes cache entries only after
the complete transaction validates. Any GPU transaction failure evicts all its
attempted keys. Two output buffers alternate for all stages; input snapshots are
uploaded once. A single command encoder/submission, final staging map and host
publication cover the chain. Logical upload/readback reports exclude GPU copies.
The device queue and snapshot credit count this as one operation; metadata is
bounded to 32 kernels but this is not a total GPU memory budget.

Owner teardown, reset, explicit abort, output revisions and device lifetime use
the existing transaction checks. Stage metadata is copied with checks after
user-controlled accesses; workgroups for every stage are validated before output
revision reservation. Hardware absence invokes the explicit fallback once for
the entire chain. There is no automatic WGSL-to-host translation.

Ten new regressions cover numerical chaining, one submit/map/publication, local
pipeline reuse, constant storage at 32 stages, partial writes, late pipeline
failure/retry, owner cancellation during map, descriptor mutation and disposal,
oversized metadata, invalid-stage revision isolation, and whole-chain fallback.
The device double now captures each dispatch's bindings when recording commands,
so multi-pass tests cannot silently use the final group's bindings for all passes.

Production build, final strict TypeScript check, all 1120 JS/WASM tests in 78
files and nine installed-package checks passed. Installed coverage includes the
chain option, exported stage type and a native Node whole-chain fallback call.
The native browser fixture passed 13/13 on non-fallback Apple Metal 3, including
three stages, partial writes and 32 stages. Wrappers around actual GPU methods
confirmed one submit, 84 uploaded bytes, one map of 28 bytes and one publication
for the three-stage/two-input case. Maximum stage count allocated five buffers
total (two inputs, two outputs, one staging); all were destroyed after completion.
The outer device validation scope returned no error. Evidence is in
`benchmarks/resident-chain-metal-2026-11-01.txt` and the adjacent PNG.

Persistent resident result handles across dispatches, branching graphs, different
intermediate lengths, direct renderer bindings and chain performance measurement
remain open. This does not close the general GPU-resident API milestone. Earlier
cache A/B artifacts describe the candidate before this executor generalization;
they are historical evidence, not a performance measurement of the new chain path.

### Native chain failure and cancellation qualification (2026-11-01)

The real WebGPU fixture now passes 15/15 on the non-fallback Apple Metal 3
adapter. A late-stage shader requiring three bindings is supplied only the
two bindings defined by the chain ABI. The shader compiles, all five temporary
buffers are created, and real device validation rejects the incompatible bind
group/command transaction. No fallback or intermediate publication occurs;
the original public output remains intact.

The second scenario starts a real native mapAsync and uses a test-only gate to
delay observation of its result. A separate root owns the dispatch and is
disposed after the mapping call begins, while public tensors remain owned by
the original root. Cancellation rejects without changing output or notifying
its observer. The gate does not establish that the GPU is still executing at
the moment of cancellation; it deterministically holds readback completion
pending at the engine boundary. The native mapping operation is always observed.

After both failures, the fixture checks that every allocated buffer received
destroy, the device scope stack returned to its single outer validation scope,
and pending job/snapshot credits returned to zero. A new valid chain executes
and publishes the expected result after each failure. Final outer validation
returns no error. Evidence: `benchmarks/chain-transaction-metal-2026-11-01.txt`
and the adjacent PNG. Destroy calls do not prove immediate driver-memory
reclamation or cancellation of already submitted GPU commands.

The fixture also disables its run button during execution, clears old status,
and observes a dispatch failure while waiting for the map gate so an early
failure cannot leave that test waiting forever. Production engine code is
unchanged by this qualification step; broader memory and resident-handle work
remain open.

### Resident-chain versus serial readback measurement (2026-11-01)

Prepare a frozen current candidate with
`node packages/svet/scripts/prepare-compute-cache-bench.mjs 48a8c5954d4b81daee605e87578e5fd028e4a94a`,
serve the package root through Vite, and open
`/tests/browser/compute-chain-bench.html`. Both modes import the same candidate
snapshot on one device; the baseline snapshot produced by preparation is not
imported. Metadata includes all engine source/JavaScript hashes and the chain
fixture hash. Both raw artifacts were checked against the current source files.

The first kernel combines two inputs, followed by two unary transforms. Serial
execution uses private host intermediate tensors with three separate dispatches;
chain execution keeps the intermediates on GPU. Both modes have the same single
observer on the final output and publish it once per request. This avoids adding
extra intermediate observer callbacks to only one side of the timing comparison.
Every request checks the changed first value; each batch checks the full tail,
actual queue/map/upload calls, destruction counts and queue credits. All pipelines
are warm before timing, with no pipeline creations in measured batches.

Two runs on a non-fallback Apple Metal 3 adapter used ten warmup requests per
mode and seven alternating rounds of 30 three-kernel requests per mode:

| Run | Floats | Serial median / 30 requests | Chain median / 30 requests | Change |
| --- | ---: | ---: | ---: | ---: |
| 1 | 7 | 61.200 ms | 24.600 ms | -59.80% |
| 2 | 7 | 62.200 ms | 25.000 ms | -59.81% |
| 1 | 65,536 | 128.700 ms | 50.600 ms | -60.68% |
| 2 | 65,536 | 122.900 ms | 54.100 ms | -55.98% |

For each request, measured uploads decrease from 28 to 12 bytes per element,
readback from 12 to 4 bytes per element, submissions/maps from three to one,
and temporary buffer creation/destruction from ten to five. Both modes publish
the final tensor once, including with private serial intermediates. All numerical,
transfer, publication, graph-node and queue checks passed, with no uncaptured GPU
errors. Raw rounds and source hashes are saved in
`benchmarks/compute-chain-ab-1-2026-11-01.json` and
`benchmarks/compute-chain-ab-2-2026-11-01.json`; screenshot:
`benchmarks/compute-chain-ab-metal-2026-11-01.png`.

The observations cover wall-clock request latency, including snapshots, buffers,
validation, publication and first-value checks. Two device-to-device copies per
chain are excluded from the upload/readback counters. This is not isolated GPU
kernel timing, persistent-resident-handle throughput, whole-application frame
performance, total driver memory or a cross-framework comparison. Performance
across other devices and workloads remains unqualified.

### Mesh index snapshot byte budgets (2026-11-01)

Mesh construction and updateIndices previously allocated the complete private
typed index array after checking only triple alignment and safe-integer length.
Two guarded regressions reproduced attempted oversized allocations for uint16
and uint32; the guards threw before any real large allocation occurred.

createMesh3D now captures maxIndexBytes, a nonnegative safe integer whose default
equals the native arena byte limit (currently 256 MiB). This is an independent
host index-snapshot policy, not a Rust arena allocation. The mesh exposes its
fixed budget through a read-only getter. Both constructor and topology updates
compare index count against floor(budget / bytesPerIndex) before constructing
the private array or reading its elements. Division avoids overflow when the
advertised count is near MAX_SAFE_INTEGER. Explicit budgets may be smaller or
larger; native typed-array and actual allocation limits still apply.

Over-budget candidates throw RangeError without replacing topology, changing the
selected index format, advancing topology commit version or notifying observers.
A rejected nested update leaves an otherwise valid outer candidate admissible.
Zero bytes permits empty or nonindexed topology; format widths are accounted
independently (two bytes for uint16, four for uint32). Invalid budgets reject
before positions/index source reads and without coercing object-valued inputs.

Eleven new tests cover both widths at default-limit overflow and near-safe-integer
lengths, constructor/update preallocation guards, invalid budgets, exact boundary
acceptance, format changes, source-option mutation, runtime read-only metadata,
empty/nonindexed topology and nested rejection. Large-array constructor guards
make the regressions safe if the preflight check is removed accidentally.

Full production build, 1131 JS/WASM tests across 78 files, nine installed-package
checks and git diff --check passed. Installed Node probes verify custom-budget
width rejection, unchanged topology/format and guarded default-budget rejection;
strict consumer types check the option and read-only getter. No near-limit
allocation or physical GPU pressure was attempted. The budget covers one index
snapshot, not process RSS, GPU storage or combined old/new snapshots during an
update. Whole-application memory qualification remains open.

### Variable-length GPU intermediates (2026-11-01)

Async dispatch and GpuComputeChainStage accept outputLength, defaulting to the
public output tensor length at each stage. The final stage must match that
tensor. Preflight checks every length for a safe integer float32 byte count,
every hardware binding against device buffer/storage limits, and each stage's
workgroups against its own output length before reserving the output revision.
GPU-only intermediate sizes are independent of the native WASM arena bound.

Two output buffers are sized for the largest stage, while bind-group entries
expose only that stage's logical input/output byte ranges. Between stages, the
overlapping prefix is copied; any newly exposed tail is cleared on GPU before
the next pass. This prevents old values in the alternating buffers reappearing
after shrink/growth. The first stage uploads only the overlapping prefix of the
host output snapshot, leaving its remaining private storage zero-initialized.
Transfer reports now account for that shorter upload when appropriate; CPU
snapshot admission still includes the complete host output snapshot.

Thirteen new cases qualify resized bindings, stale-tail clearing, per-stage
default workgroups, shortened first-stage upload accounting, invalid lengths,
final-shape revision isolation, intermediate device limits, descriptor snapshots,
noncoercion and separation from the host arena limit. The latter uses a controlled
GPU allocation guard: a permitted above-arena descriptor reaches the device
without allocating a large actual buffer. Existing-buffer cleanup and credits
are checked on that deliberate failure.

The final production build and nine installed-package checks passed. The default
parallel suite initially passed all functional cases but its existing comparative
benchmark exceeded its 5000-ms timeout. No timeout or benchmark assertions were
changed. A bounded run with
`npm --prefix packages/svet test -- --maxWorkers=4` passed all 1144 tests in 78
files (41.69 seconds locally). Default parallel benchmark timing remains sensitive
to execution conditions; this result does not claim a measured performance gain.
Strict consumer declarations cover both initial and following outputLength fields.

Native browser checks passed 16/16 on non-fallback Apple Metal 3 after the final
build. The new scenario expands two seven-element inputs to 129 values, reduces
all 129 to one sum, then grows a partially writing result to seven elements.
It verifies [12241,0,0,0,0,0,0], one final publication, 84 uploaded/28 readback
bytes, destroyed temporary buffers and a clean outer validation scope. This also
exercises enough initial workgroups to process beyond 64 invocations. Evidence:
`benchmarks/shaped-chain-metal-2026-11-01.txt` and the adjacent JPG.

Persistent resident handles, branching graphs, direct renderer bindings, aggregate
GPU memory budgets and variable-shape performance qualification remain open.
Earlier serial/chain benchmark artifacts describe the pre-shape executor snapshot;
their hashes preserve that measurement boundary.

### Untracking all synchronously suspended cores (2026-11-01)

An untracked callback could collect a dependency into a third suspended core:
owned callback entry paused its caller and callee, while another synchronous
observer between them remained active. Public untrack also left foreign paused
observers collecting incidental reads. Three initial regressions reproduced
extra effect runs and computed reevaluation before the fix.

Effect and computed execution now register their synchronous context frames in
a module-local stack, unwound on success, throw and reset. untrack temporarily
pauses the active observer of each represented core, deduplicating repeated
contexts by their paused state. It restores observers only within the same reset
generation. Explicitly created child observers still track their own reads, and
writes to already-read dependencies retain their rerun behavior. Owned callback
entry still resolves the ambient core before selecting its captured core, so
adapter-triggered disposal/reset is observed by the existing lifetime rechecks.

Six public regressions cover three-core untrack/action RPC, thrown callbacks,
suspended computed evaluation, foreign reset and already-read writes. Production
build, all 1150 JS/WASM tests in 79 files (maxWorkers=4), nine installed-package
checks and git diff --check passed. Native Node consumer coverage includes the
third-core action scenario and verifies teardown in each core.

The lifecycle probe was rerun for 20,000 cycles/180,000 WeakRefs. Two early default
Node runs failed the unchanged ten-GC criterion for one old owner each
(owner:2582 and owner:2385). A local heap snapshot showed a path from VM strong
roots labelled IdentityMapBase through a scene-constructor context to its owner;
the new observer stack was not in that observed path. The raw heap remains local.
Failure logs are preserved as
`benchmarks/multicore-retention-default-failed-1-2026-11-01.txt` and `-2-...txt`.

Subsequent default and synchronous-compilation runs used identical probe hashes:
both collected all 180,000 objects, with maximum ten rounds in ordinary Node and
one round with --no-concurrent-recompilation. Artifacts:
`benchmarks/multicore-retention-default-20k-2026-11-01.json`,
`benchmarks/multicore-retention-sync-20k-2026-11-01.json` and
`benchmarks/multicore-gc-diagnostic-2026-11-01.json`. Background compiler retention
is an inference supported by the roots and controlled flag comparison, not a
general absence-of-leaks proof. Default GC timing and process-memory plateau
remain unqualified; the failed observations are not erased by later successes.

Probe metadata now records execArgv. Optional SVET_LIFECYCLE_HEAP_SNAPSHOT saves
a local diagnostic heap on failure; SVET_LIFECYCLE_GC_DIAGNOSTICS can record later
idle observations. Neither option changes the original collection assertion.
Tracking across separate package copies, user-await context propagation, and
performance measurement of the added observer-frame bookkeeping remain open.

### Lazy-cache correctness and managed state-view lifetime (2026-11-01)

Correctness assertions in the nested-untrack benchmark found stale effect-free
computed values after repeated writes. The native no-terminal-effect cache
returned without invalidating reachable computed nodes. Cache population also
included unrelated dirty computeds and could classify a source using foreign
pending effects. Cache topology is now collected from that source's reachable
subscribers only; the fast path requires an empty pending queue and invalidates
its reachable lazy computeds even when no terminal effect exists. Four native
and six JS regressions reproduced the failures before these changes.

A separate reproduction found that Rust's states Vec can relocate without a
WASM memory ArrayBuffer change. Managed node allocation now invalidates the
cached JS states view for signals, computeds, fresh/recycled effects and tensors.
The view is reused between allocations. Regressions pre-grow linear memory and
require an actual internal pointer relocation before checking all memo values.

Observer-context registration now keeps one stack entry per synchronously
observed core with a nesting counter. Reset deliberately leaves that counter
intact: retired JavaScript frames release their entries in their finally blocks.
The benchmark preparation script retains both correctness fixes in both variants
and reverses only context registration for the baseline. The runner checks that
only reactivity.js differs, validates every measured result and teardown, and
records raw samples and module hashes. Measurements made before the correctness
fixes are unsuitable for performance claims. Synthetic untrack results do not
qualify application frame time, cross-package tracking or await propagation.

Final qualification of this snapshot: all 1160 functional tests passed across
80 files; the complete 81-file/1161-test run failed only the existing synthetic
comparison's unchanged 5000-ms timeout (6046 ms locally). Total run 188.23 s.
Other projects had active test workers during the observation; their contribution
to that timeout is not proven. The full suite is therefore not recorded as green.
All nine installed-package checks passed, including lazy-cache and stable-memory
pointer-relocation consumer probes. The reset-core GC probe collected all 64
isolated exports objects in one round per batch under Node 22.23.4 with
--expose-gc --no-concurrent-recompilation. Result:
`benchmarks/reset-core-gc-2026-11-01.json`. This does not prove process RSS plateau.

Frozen A/B raw results are in `benchmarks/untrack-core-depth-2026-11-01.json`
and `benchmarks/untrack-core-control-2026-11-01.json`. Seven alternating rounds,
32 evaluations and 4000 untrack calls per evaluation gave baseline→candidate
medians of 3.196→2.620, 24.054→5.310, 68.626→3.357 and 128.817→6.635 ms at
nesting depths 1, 32, 128 and 256 respectively. The no-untrack control used 1000
evaluations: 0.185→0.526, 4.197→4.898, 84.008→68.092 and 154.463→126.898 ms.
The shallow control regressed in this observation, and background machine load
was uncontrolled. These results support the bounded scaling of repeated untrack
with unique cores, not a universal speedup or neutral bookkeeping overhead.
Application performance and repeatability remain open. Both variants include
identical embedded WASM and all pointer/cache correctness fixes; the runner
asserted that only reactivity.js differed.

The same comparison test subsequently passed in isolation with its unchanged
5000-ms timeout (`npm --prefix packages/svet test -- tests/benchmark.test.ts
--maxWorkers=1`, one test passed, total process 5.84 s). That does not replace
the failed full-suite gate. No timeout, assertion or workload was weakened.

The final native rerun passed 53 tests (zero failures); production rebuild passed
with a 54,320-byte embedded SIMD WASM. Preserved timeout and isolated-pass summaries:
`benchmarks/core-suite-timeout-2026-11-01.txt` and
`benchmarks/core-benchmark-isolated-2026-11-01.txt`.

A subsequent complete run with maxWorkers=4 also passed 1160 functional tests
and failed only the same unchanged 5000-ms comparison timeout (5994 ms; total
60.32 s). Summary: `benchmarks/core-suite-workers4-timeout-2026-11-01.txt`.
Full-suite timing qualification remains open; the isolated pass is not substituted
for it. Package, native and production-build summaries are retained alongside
these observations as core-package-check, core-native-check and
core-production-build-2026-11-01.txt.

### Benchmark evidence boundaries corrected (2026-11-01)

Inspection of the comparison timeout found that most measured wall time belongs
to local VNode/proxy simulations, not Svet. This fixture imports no React, Vue
or Svelte runtimes. Its former framework/version labels therefore overstated
what was measured. They now explicitly identify local simulations. Historical
output from that fixture must not be used as framework comparison evidence.

The SSR report also previously added 65% of another traversal's time to the Vue
column and invented the Svelte column as 45% of that time. Both formulas are
removed. The two columns now time their own actual DOM-child and sibling-cursor
walks and assert that each visits all 501 expected children. The restored Svet
binding must contain 1025, and the diamond workload must run its terminal effect
exactly 5001 times. Existing loads, assertions and the 5000-ms timeout remain.

Stable native capacity is now labelled as capacity, not "0 B leak"; diamond
updates no longer claim zero JS GC. Mock-device upload and WASM CPU FMA/scene
workloads are labelled accordingly. Unmeasured framework/GPU capabilities no
longer receive speculative descriptions. These changes improve evidence quality;
they do not implement real-framework or hardware performance comparisons.

The focused updated benchmark passed with the unchanged timeout. Actual runtime
comparisons, application benchmarks, memory plateau and full-suite timing
qualification remain separate open work.

Final complete run after the report corrections passed all 1161 tests in 81 files
with maxWorkers=4 (47.70 s). The existing benchmark timeout remains 5000 ms;
no original workload or assertion was removed, and result assertions were added.
Summary: `benchmarks/core-suite-verified-2026-11-01.txt`. Earlier timeout records
remain preserved; this pass does not establish stable timing across machine loads
or attribute a performance improvement to the report changes.
TypeScript noEmit and git diff --check also passed after the final fixture edit.

### Independent async SSR subtree ownership (2026-11-01)

The Node server entry now provides runWithOwnerScopeAsync(contextEntries, fn,
options) inside an isolated request. A separate AsyncLocalStorage owner store
keeps interleaved sibling and nested child scopes independent across await.
Successful children remain attached to their parent through request serialization;
rejection, external abort or parent disposal immediately drains their resources.
Each settled child's implicit late continuations are closed even while its
request remains open. The helper awaits thenables through the existing guarded
request race. An options getter that resets the graph is rejected before user
work starts. Long-lived device subscription escape now leaves both async stores.

The reactive core resolves the async owner only outside synchronous owner frames.
A frame depth counter gives createRoot, child scopes, effect execution and
captured reentry explicit precedence and unwinds across reset generations.
Computed disposal registration uses its resolved construction owner. The browser
entry retains no Node dependency. Synchronous runWithOwnerScope callbacks now
receive a bound disposer; existing zero-argument callbacks remain valid.

Tests cover sibling interleaving, inherited nested context, request-core identity,
resource abort and exact cleanup on child failure, parent/explicit cancellation,
request cancellation, reset, captured root priority, closed late work, preflight,
serialized sibling DOM and updates after successful child settlement. Added
installed-package Node and declaration probes qualify the new server export.
This is an explicit async subtree helper: h(asyncComponent), automatic browser
await propagation, Edge adapters and cross-package coordination remain open.

Final verification: production build (including TypeScript noEmit), all 1172
JS/WASM tests in 82 files (maxWorkers=4, 46.97 s), nine isolated installed-package
checks and four real HTTP action tests passed under Node 22.23.4. git diff --check
also passed. Saved suite summary: `benchmarks/async-owner-suite-2026-11-01.txt`;
package and HTTP summaries are adjacent. No browser/Edge ownership claim follows
from these Node tests. Callback cancellation is effective while that callback is
pending; successful child resources subsequently follow their parent lifetime.

### Owner construction rechecks and async-child churn (2026-11-01)

Three new regressions reproduced an owner-construction hole: context iterators
and tuple value getters can dispose the parent or reset the graph before scope
attachment. runWithOwnerScope checked only before reading that user metadata,
then invoked a stale body and could leave the new generation's current owner
poisoned. It now rechecks generation and parent lifetime after context copying,
before attaching a child or mutating the current owner. Four tests cover both
invalidation paths, a tuple getter, preservation of a fresh post-reset signal,
and idempotent bound disposal with parent unlinking. All 15 construction/async
owner tests passed after the change; three failed before it. An installed-package
consumer probe also covers metadata reset and a usable fresh graph afterward.

The new measure-async-owner-churn.mjs exercises successful, rejected and cancelled
children within one live isolated Node request without reset. Each child owns
computed/effect/resource/captured-context objects and cleanup. Successful children
are stopped via their synchronous parent after settlement. Errors/cancellations
must drain immediately. Each cycle checks an empty graph, no retained parent
children or cleanup hooks, and external AbortSignal listener removal. WeakRefs
cover five objects per child. Samples are bounded to 32, with 750 outstanding
WeakRefs per GC batch; no unbounded diagnostic log is retained.

Two 1000-cycle runs each checked 3000 children, 3000 cleanups and collection of
15000 objects. The identical probe SHA256 is
631fe135a8f85f4ee2f7b11bc10eaa8d2926caadb33fd2000b6bdd31d9df8dae.
Default Node 22.23.4 needed up to four GC rounds per batch; with
--no-concurrent-recompilation it needed one. After cycle 50, native capacity stayed
6 and WASM memory stayed 1179648 bytes. Raw results are
`benchmarks/async-owner-churn-default-1000-2026-11-01.json` and
`benchmarks/async-owner-churn-1000-2026-11-01.json`.

Observed default JS heap grew 6906728→7188200 bytes and RSS 30347008→35542128;
the synchronous-compilation run observed heap 6905752→7167312 and RSS
30134064→37393592. These observations do not prove JS/RSS plateau, full allocator
reclamation or concurrent DOM/GPU memory bounds. The GC criterion remained ten
rounds. No failed observations were discarded or threshold relaxed.

Final qualification of the construction guard: production build passed; the
complete suite passed all 1176 tests in 83 files (maxWorkers=4, 22.78 s); all nine
isolated installed-package checks passed, including metadata-reset construction
and preservation of a fresh graph. TypeScript includes the four new regressions.
Suite summary: `benchmarks/owner-construction-suite-2026-11-01.txt`; package report:
`benchmarks/owner-construction-package-2026-11-01.json`. git diff --check passed.
The final four native HTTP action tests also passed (zero failures).

### Persistent immutable GPU compute results (2026-11-01)

Added hardware-only dispatchGpuComputeResident and explicit readGpuResidentTensor.
Resident results retain private device buffers across dispatch calls, accept mixed
host-snapshot and resident inputs, and support branching graphs. No intermediate
WASM output nodes, map or host publication are required. Each result has immutable
length/byteLength/uploadedBytes metadata, active state and idempotent disposal.
The output begins zeroed and becomes public only after queue completion and all
validation scopes finish. Explicit readback revision-checks its host target and
publishes once. Raw GPU buffers are not exposed.

Owner teardown, core reset, device detach/replacement and loss retire buffers.
Disposal of an input cancels pending consumers, while already-completed independent
results remain valid. Caller cancellation applies to pending calls, not to a
successfully published result. Shared lifetime hubs avoid one permanent AbortSignal
listener per handle and drain all registered disposers on reset/loss. The hard
1024-result boundary is tested with actual mock allocations and a bulk reset.

A device-shared persistent-storage budget defaults to 256 MiB, independent of
pending CPU snapshot admission. Both live results and queued reservations count;
early-cancelled queued calls retain credits until their closures drain. Budget
validation precedes publication of either configuration budget, so an invalid
resident budget cannot partially change the snapshot budget. Telemetry reports
residentComputeBytes/residentComputeCount/maxResidentComputeBytes. Upload/staging,
renderer, driver and process memory remain outside that persistent-output policy.

Review caught an immutability hole in auto pipeline layouts: a shader could declare
an input read_write. Resident pipelines now use explicit read-only-storage inputs
and one writable private output. Cache keys include a resident namespace and input
count, sharing the bounded device cache without reusing incompatible mapped auto
layouts. Native WGSL attempting to overwrite an input is rejected, and explicit
readback confirms the input's original values. Shader source is not regex-filtered.

Device-double coverage checks branching, host snapshots, explicit readback,
revision races, source disposal, queued cancellation credits, shared budgets,
configuration atomicity, validation failure/recovery, owner/reset/detach/loss,
shared-device core isolation, capability-getter reconfiguration, immutable binding
layouts and result-count limits. One initial new test incorrectly used the synchronous
withWasmCore wrapper across await; it now explicitly reenters before reading its
foreign-core tensor. This was a test-context error, not automatic await propagation.

Native browser fixture passed 7/7 on non-fallback Apple Metal 3: four-dispatch
branching graph uploaded 28 bytes once, made no intermediate map or WASM output
node, then one explicit 28-byte readback published once. Additional cases reject
input-writing WGSL without mutation; run GPU-only dispatch with a zero host
snapshot budget; reject persistent admission before allocation; recover from
invalid shader; cancel during native mapping on owner teardown; and retire results
on reset and actual device loss. Evidence:
benchmarks/resident-compute-metal-2026-11-01.txt and adjacent JPG. The first fixture
attempt reused a consumed GPUAdapter for the loss case; it was corrected to request
a fresh adapter. Earlier existing hardware compute checks passed 16/16, recorded in
benchmarks/resident-regression-metal-2026-11-01.txt.

The complete suite passed 1207 tests in 84 files (maxWorkers=4, 12.06 s). Production
build includes TypeScript noEmit. All nine isolated installed-package checks and four HTTP tests also passed.
This is correctness/transfer evidence, not application speed or memory-plateau
qualification. Direct renderer bindings, aggregate GPU/driver budgets, other
backends and scheduling/throughput measurements remain open.

Final evidence summaries: benchmarks/resident-suite-2026-11-01.txt,
resident-package-2026-11-01.json and resident-http-2026-11-01.txt. Source hashes and
qualification scope are recorded in resident-qualification-2026-11-01.json.
The count-boundary test resets all 1024 results together, verifies every buffer
is destroyed, and verifies persistent reservations and queue credits return to
zero. git diff --check passed. No commit, push or publication was performed.

### Resident vertex renderer interoperation and on-demand picking (2026-11-01)

Canvas3D/Viewport3D now accept gpuVertices: an immutable resident handle, a
reactive accessor, or null for the existing host mesh path. A selected source
must match the renderer core/device and provide one clip-space float4 per mesh
vertex. Topology still comes from the mesh. Resident storage has VERTEX usage and
is bound directly: no duplicate vertex buffer, host transformedVertices read,
vertex upload or per-frame map. Renderer teardown releases its index/depth/context
allocations and subscriptions without disposing caller-owned resident storage.
Source disposal invalidates its frame; selecting a new compatible source recovers.
Failed resident rendering cannot substitute unrelated host geometry in Canvas2D.

The internal typed bridge stores executor-owned records in one WeakMap with no
runtime import cycle. Each record supplies its snapshot reader directly, avoiding
module-global callback registration or reliance on preserved import side effects.
readGpuResidentSnapshot returns an explicit private CPU float32 copy without a
WASM output tensor/reactive publication. Canvas picking invokes it only on click,
then transports that immutable frame into the existing Rust scratch picker with
indexed occlusion. New clicks, frame replacement, valid resize, source disposal,
reset and renderer teardown retire pending picks and staging. Callback delivery
requires the same current frame and restores the captured core and owner.
VertexPickFrame3D.gpuVertices identifies a resident source; mesh host positions
are not its rendered coordinates. Raw snapshots still consume shared compute
admission and may be expensive for large meshes.

A scheduled retirement regression failed before the fix: replacing an in-flight
source and disposing it left the latest dirty frame unrendered after completion.
Retirement now follows existing failFrame/drain logic, keeping backpressure until
the old submitted queue completion is observed, then scheduling the latest source.
A second regression reproduced loss of the actual pipeline diagnostic behind a
generic unavailable-renderer message. Resident recovery now preserves the primary
error while refusing host substitution. Initial-submit owner teardown also removes
the renderer registration and all renderer allocations. Before-fix summaries are
saved alongside the final suite evidence.

The final suite passed 1221 tests in 85 files (maxWorkers=4, 11.25 s), production
build including TypeScript noEmit passed, all nine installed-package checks passed,
and four real HTTP tests passed. The package check also builds a resident-render
HTML entry importing installed svet-engine, alongside the existing JSX/Forma
consumer. Its production assets contain no Node SSR import. This artifact was
served through Vite preview and tested on the actual GPU, qualifying the exports,
module boundary and tree shaking as well as development-source execution.

Both dev and installed-package production fixtures passed 7/7 on a non-fallback
Apple Metal 3 adapter. They verify native GPU triangle pixels from data different
from the host mesh, only an 8-byte padded topology upload and zero vertex upload/
map during initial drawing/resize, one explicit picking map with correct frame
identity/dimensions, native map cancellation on replacement, original driver
validation diagnostics without Canvas2D substitution, source retirement and
independent recovery, controlled queue-completion-observation gating for scheduled
retirement, and complete logical buffer/texture retirement plus zero reserved
credits. The scheduling gate delays observation, not actual GPU execution.
The previous nonresident renderer fixture also passed 17/17 on this source.

Early fixture pixel capture after awaiting validation returned transparent data;
pixel probes now capture the submitted canvas texture synchronously, as in the
existing render fixture. These verification-only canvas copies are separate from
application GPUBuffer map/upload counters. A second initial fixture expected an
ideal zero pointer distance; native MouseEvent integer coordinate conversion gave
0.3712871287 pixels. The assertion now compares the exact projected emitted-event
position against the known GPU vertex, rather than changing the picking radius.
Two new device-double assertions also needed float32 expected values and strict
Array.at nonnull annotations; neither changed the rendering workloads.

Native evidence: benchmarks/resident-render-metal-2026-11-01.txt/JPG,
resident-render-package-metal-2026-11-01.txt/JPG and
resident-render-legacy-metal-2026-11-01.txt. The visible image is labelled as a
fixture-only captured GPU frame. Qualification covers bounded pixels and behavior,
not general raster equivalence, app frame time, click latency, aggregate GPU/
driver/process memory or JS/RSS plateau. Those measurements remain open.


## Реальное deformation-приложение: resident/mapped, 2026-11-01

Production snapshot `snapshot-1793535370465`, Node 22.23.4, Vite 8.3.5,
Chrome 156, физический Apple Metal 3 (`isFallbackAdapter=false`), DPR=2.
Одинаковые WGSL, topology и параметры; compute → renderer, RAF-paced updates.
Протокол `interleaved-controls-v2`: 30 warmup frames на сценарий, две отдельные
калибровки по 60 кадров, пять пар на каждом размере по 120 кадров на путь,
три resident/resident A/A-пары между большими A/B-раундами. Всего 3240 измеренных
кадров / 28 сценариев. Калибровки исключены из сравнительных итогов.

| Вершины | mapped median p50 / p95, мс | resident median p50 / p95, мс | upload/frame mapped → resident | readback/frame |
| --- | --- | --- | --- | --- |
| 2400 | 1.400 / 1.600 | 1.400 / 1.505 | 115216 → 16 байт | 38400 → 0 байт |
| 38400 | 1.800 / 2.305 | 1.100 / 1.400 | 1843216 → 16 байт | 614400 → 0 байт |

На большой сетке парные p50 изменения −38.9…−45.0%, p95 −33.3…−39.3%.
A/A p50 −9.1…+4.0%, p95 −20.0…0.0%: хвост остаётся шумным.
Малый сценарий не подтверждает устойчивое ускорение. Ранний пилот показал
временной дрейф и сохранён отдельно; причина дрейфа не установлена.
Это локальный результат конкретного приложения, а не универсальный speedup.
Latency включает queue completion и validation; чистое время GPU kernel не измерено.
RAF median около 8.3 мс в обоих путях; прирост FPS этим не доказан.

Геометрия GPU и пиксели совпали точно во всех парах, CPU reference error <2e-5.
Uncaptured GPU errors и long tasks не наблюдались. Runtime assertions проверили
стабильность графа/WASM, отсутствие pending credits и освобождение logical GPU
allocations после каждого сценария. На большой сетке live buffer bytes выше
у resident: 1382400 против 768000; пик logical buffer bytes ниже: 1996816 против
2611216. Это учёт буферов приложения, не driver memory/RSS и не доказательство
длительного плато. Heap snapshots остаются наблюдениями без вывода о plateau.

Артефакты:
- `benchmarks/resident-app-pilot-metal-2026-11-01.json` — пилот с дрейфом;
- `benchmarks/resident-app-interleaved-metal-2026-11-01.json` — raw frames,
  A/A, hashes исходников и bundle, окружение, quality и lifecycle observations;
- `benchmarks/resident-app-interleaved-analysis-2026-11-01.json` — независимый
  пересчёт quantiles, counters, совпадения quality, порядка и числа кадров;
- `benchmarks/resident-app-interleaved-metal-2026-11-01.jpg` — результат UI;
- `benchmarks/resident-app-package-2026-11-01.txt` — девять package checks.

Воспроизведение: `npm --prefix packages/svet run bench:resident-app:prepare -- --serve`,
открыть выведенный localhost URL, запустить default 120 frames / 5 rounds / A/A.
Не запускать CPU builds/tests одновременно с измерением. Raw JSON доступен в
DOM `#raw-results`. Независимый анализ: `python3 packages/svet/scripts/analyze-resident-app-bench.py <raw.json>`.
Preparer прошёл `node --check`, frozen production fixture собран Vite, installed
package checks — 9/9. Runtime в этом этапе не менялся; полный suite не повторялся.
Открыты повтор на другом запуске/оборудовании, другие приложения и полная
квалификация startup, GPU timestamps, DOM/GPU/RSS длительной нагрузки.


## Одновременно живые roots: controlled Node churn, 2026-11-01

Добавлен `scripts/measure-live-roots.mjs`: отдельный Node-процесс, два настоящих
WASM cores, 64 одновременно живых roots. Каждый root содержит signal/effect,
не завершающийся resource с abort listener и captured context; по 16 roots
заменяются за batch, новый создаётся перед teardown старого. Reset не вызывается.
Каждые 25 batches: принудительный GC, WeakRef-проверка retired owner/signal/
resource/context, снимок process.memoryUsage и графа. Скрипт ограничен 10000
batches / 402 samples; содержит assertions и сохраняет failure JSON.

Первый вариант оставлял начальную ротацию в верхнем async module frame.
В двух прогонах после десяти GC оставались четыре объекта root id=79 — последний
root начальной ротации. Диагностический прогон сохранил эти identity и heap
snapshot `/tmp/svet-live-roots.heapsnapshot`. Перенос всей ротации в отдельную
синхронную функцию устранил наблюдаемое удержание без изменения runtime.
Это свидетельство в пользу ссылки измерителя/VM frame; полный retainer path
из snapshot ещё не разобран, дефект runtime на этом основании не заявляется.
Отказы сохранены в `live-roots-default-2026-11-01.json`,
`live-roots-no-concurrent-2026-11-01.json`, `live-roots-diagnostic-2026-11-01.json`.

Итоговый обычный Node 22.23.4 (`--expose-gc`, без отключения compiler):
10000 batches, 160080 roots, 640320 WeakRef-объектов освобождены, максимум
9 GC turns. Прогон `--no-concurrent-recompilation`: 1000 batches, 16080 roots,
64320 объектов, один GC turn. Оба завершились с nodes/edges/pending=0,
cleanup и abort ровно по числу roots. Во время нагрузки на каждом core:
192 nodes, capacity=198, edges=32, WASM memory=1179648 байт — стабильны.

В обычном прогоне после batch=100 heapUsed 7747376…8190008 байт,
RSS 43259328…52076776 байт; external=4429948 и arrayBuffers=33315 стабильны.
Heap и RSS выросли: длительное плато не доказано. Записи самого измерителя
также растут до ограниченного количества samples, что следует учитывать.
Это ускоренный churn, а не многочасовой soak. DOM, GPU, SSR actions, сцены,
реальная параллельность и общий GPU device этим сценарием не квалифицированы.

Raw: `benchmarks/live-roots-default-isolated-frame-2026-11-01.json` и
`benchmarks/live-roots-isolated-frame-2026-11-01.json`; содержат времена,
execArgv, исходный hash измерителя и hash reactivity runtime, все memory samples.
Запуск: `node --expose-gc packages/svet/scripts/measure-live-roots.mjs <output.json> 10000`.
Проверены синтаксис и два полных выполнения. Runtime не менялся.


### Продолжение: фиксированный журнал и 1.6 млн roots

`measure-live-roots.mjs` теперь хранит кольцо из 32 samples и одну baseline,
плюс четыре фиксированных online-агрегата по четвертям запуска. Не более 1600
pending WeakRef-проб между GC; верхняя граница batches увеличена до 100000.
Полные samples не накапливаются, порядок ring восстанавливается при записи JSON.

Обычный Node на 10k batches остановился на batch=2450: четыре объекта root
id=39201 остались после десяти GC. Лимит не ослаблен, raw отказ сохранён:
`benchmarks/live-roots-ring-10k-2026-11-01.json`. Причина не установлена;
ранний успешный обычный Node-прогон не доказывает стабильность освобождения.

Изолированный запуск с `--expose-gc --no-concurrent-recompilation` прошёл
100000 batches за 73.3 с: 1600080 roots, 6400320 WeakRef-объектов освобождены,
всегда один GC turn, 4002 наблюдения, только 32 samples сохранены в кольце.
Все cleanups/aborts выполнены, capacity=198 и WASM=1179648 на каждом core
стабильны, финальные nodes/edges/pending=0. Средний RSS по четвертям:
45.93 / 47.19 / 45.53 / 44.10 МБ; последний 43.95 МБ. Средний heapUsed:
8.044 / 8.061 / 8.075 / 8.126 МБ, после teardown 7.498 МБ.
External и arrayBuffers постоянны. RSS в этом запуске не рос монотонно;
небольшой рост heap и короткая длительность не позволяют заявлять длительное
плато или устранять вопрос с обычным compiler mode.

Raw: `benchmarks/live-roots-ring-no-concurrent-100k-2026-11-01.json`.
Независимо проверены counts, 4000 window samples, 32 retained samples,
cleanup/abort, graph/WASM invariants и финальный teardown; результат:
`benchmarks/live-roots-ring-analysis-2026-11-01.json`. Синтаксис измерителя и
`git diff --check` прошли; runtime не менялся. Следующий диагностический шаг —
retainer path обычного Node при повторном отказе, затем временной soak с
фиксированным журналом, DOM/GPU и реальными async-интеграциями.


### Retainer evidence обычного Node

Добавлен offline `scripts/inspect-live-root-retainers.py`: находит owner-пробы
из WeakRef-журнала и печатает входящие сильные edges и кратчайшие пути до
synthetic roots, исключая weak edges. Проверен на трёх реальных Node snapshots.

Ранний snapshot действительно содержит путь SourceTextModule → Generator →
parameters_and_registers → объект измерителя → owner. Это подтверждает удержание
того root кадром module generator в первом варианте измерителя. Сохранено:
`benchmarks/live-roots-initial-retainer-paths-2026-11-01.txt`.

Новый обычный Node 10k batches прошёл (до десяти GC), но следующий 100k запуск
упал на batch=50850 после 813680 созданных roots, удерживался root id=813610.
Snapshot показывает только сильный root `weak_refs_keep_during_job` и взаимные
ссылки закрытого owner subtree; отдельный runtime/global retainer path этим
анализом не найден. Сам snapshot меняет VM/GC state, поэтому отсутствие пути
не доказывает отсутствие исходного удержания. Raw успешного и упавшего запусков:
`live-roots-ring-retainer-2026-11-01.json`,
`live-roots-ring-retainer-100k-2026-11-01.json`; пути:
`live-roots-ring-retainer-paths-2026-11-01.txt`.

Дополнительный диагностический запуск упал на batch=10000: root id=160006,
четыре объекта после исходных десяти GC. После snapshot, нового event-loop job
и одного GC все исчезли (`afterSnapshotRemaining=0`). Исходная assertion всё
равно падает; лимит не изменён и диагностическое освобождение не превращает
результат в pass. Snapshot также показывает только weak_refs_keep_during_job.
Raw: `live-roots-postsnapshot-diagnostic-2026-11-01.json`, пути:
`live-roots-postsnapshot-retainer-paths-2026-11-01.txt`.

Heap snapshots оставлены локально в `/tmp`, SHA256 сохранены в
`benchmarks/live-roots-snapshot-hashes-2026-11-01.json`. Стабильность обычного
compiler mode и точная причина GC delay остаются открытыми. Данные совместимы
с временным VM удержанием, но не доказывают его механизм. Runtime не менялся;
полезный следующий шаг — отдельный soak без требования освобождения в фиксированное
число GC, с прежним строгим gate как самостоятельной проверкой и независимым
контролем роста retained objects/heap/RSS во времени.


### Двухминутный timed soak в обычном Node

Добавлен отдельный `scripts/soak-live-roots.mjs`, протокол
`timed-retention-soak-v1`. Строгий `measure-live-roots.mjs` не изменялся.
64 одновременно живых roots / два WASM cores / rolling replacement, одна
forced-GC попытка на sample. Неосвобождённые WeakRef-пробы переносятся в следующий
sample: измеряются backlog и возраст, вместо требования немедленного освобождения.
Post-GC backlog ограничен 10000, final drain — десять секунд; превышение budget
или неосвобождённый final backlog дают failure. Запись: 64 ring samples,
одна baseline и четыре online memory windows. Текущие цели WeakRef не сохраняются
сильными ссылками. Массив probes может дополнительно содержать до 1600 новых
проб очередной группы ротаций перед следующим GC; budget относится к post-GC.

Smoke 1 с прошёл. Основной обычный Node 22.23.4 `--expose-gc` прошёл 120 с:
5912928 roots, 23651712 WeakRef-объектов освобождены, cleanups/aborts совпали
с числом roots. 14784 наблюдения, сохранены 64. Максимальный post-GC backlog=41,
максимальный наблюдавшийся возраст 44.21 мс; final backlog=0. Возраст измеряется
только для объектов, живых в момент sample, а не как точное время их сборки.
Graph capacity/WASM стабильны, в конце nodes/edges/pending=0.

Средний heapUsed по четвертям 8.121 / 8.168 / 8.172 / 8.172 МБ;
RSS 53.71 / 53.53 / 54.15 / 54.58 МБ. После начальной четверти heap почти
не изменялся в этом ограниченном наблюдении; RSS слегка вырос. Это evidence
против накопления retired objects в данном двухминутном сценарии, а не
многочасовой bound, отсутствие любых утечек или отмена строгих GC failures.
DOM/GPU, actions, сцены и реальная серверная конкурентная нагрузка открыты.

Raw: `benchmarks/live-roots-soak-120s-2026-11-01.json` содержит runtime JS hashes,
source hash, samples и windows; smoke — `live-roots-soak-smoke-2026-11-01.json`.
Независимый `scripts/analyze-live-roots-soak.py` проверил итоговое число roots,
полный accounting объектов/cleanup/abort, время нагрузки, invariants сохранённых
samples и teardown. Вывод: `benchmarks/live-roots-soak-analysis-2026-11-01.json`.

Запуск: `node --expose-gc packages/svet/scripts/soak-live-roots.mjs <raw.json> 120000`.
Анализ: `python3 packages/svet/scripts/analyze-live-roots-soak.py <raw.json>`.
Синтаксис и `git diff --check` прошли; runtime не менялся. Далее — расширить
нагрузку на actions/сцены, затем браузерный DOM/GPU soak с отдельным учётом
устройства; многочасовую проверку планировать отдельно от ускоренного churn.


### Timed soak: actions и CPU-сцены

`soak-live-roots.mjs` расширен явным сценарием `actions-scenes` (четвёртый
позиционный аргумент). Каждый из 64 живых roots содержит прежнюю нагрузку,
server action с успешным RPC и отдельным не завершающимся запросом, transform,
perspective camera и triangle mesh. Результат успешного action проверяется до
следующей ротации; teardown должен отменить pending запрос, вызвать abort hook
и вернуть `ServerActionDisposedError`. Вычисляются transformedVertices.
Транспорт RPC в этом сценарии локальный Promise, а не HTTP. Pending cancellation
promises ожидаются группами; журнал и backlog остаются ограниченными.
WeakRef-проб восемь на root, до 3200 новых между samples сверх прежнего backlog.
Guard 10000 относится к post-GC backlog. Graph check теперь включает arenaFloats.

Обычный Node 22.23.4 `--expose-gc`, 60 с: 743680 roots, 743680 успешных
результатов, 743680 отмен и abort hooks; 5949440 WeakRef-объектов освобождены.
1861 sample, 64 retained. Максимум post-GC backlog=56, наблюдавшийся возраст
69.06 мс; финальный backlog=0. На каждом core во время нагрузки nodes=800,
capacity=825, edges=352, WASM=1310720 байт, arenaFloats=2904 стабильны.
После teardown nodes/edges/pending=0; arena остаётся выделенной для reuse.

Средние heapUsed по четвертям 10.928 / 10.956 / 10.967 / 11.003 МБ;
RSS 87.80 / 91.41 / 92.37 / 89.97 МБ. Накопления retired объектов до конца
запуска не обнаружено; многочасовой plateau, DOM/GPU и concurrent HTTP server
не квалифицированы. Строгий ten-GC gate остаётся отдельным и не изменён.

Raw: `benchmarks/live-roots-actions-scenes-60s-2026-11-01.json`;
независимый accounting: `benchmarks/live-roots-actions-scenes-analysis-2026-11-01.json`.
Сохранены также smoke расширенного сценария и повтор default roots после
изменений измерителя. `analyze-live-roots-soak.py` проверяет objectsPerRoot и
полный action accounting. Runtime не менялся, полный suite не повторялся.

Воспроизведение:
`node --expose-gc packages/svet/scripts/soak-live-roots.mjs <raw.json> 60000 actions-scenes`.
Следующий этап — браузерный DOM/GPU lifecycle с общим device и независимым
учётом allocations; HTTP transport и async SSR нагрузку проверять отдельно.


## Браузерный DOM/GPU lifecycle soak, Apple Metal 3

Добавлены `tests/browser/dom-gpu-soak.html` и
`scripts/prepare-dom-gpu-soak.mjs`: frozen production Vite bundle, хеши
исходников и assets, localhost preview на 4200. Четыре одновременно живые
сцены на двух WASM cores и одном физическом устройстве. Каждый кадр — четыре
resident dispatch, замена selected GPU handle с dispose старого, настоящие
DOM click dispatch и проверка reactive текста. Каждые восемь кадров — resize
и замена одного owner; старый retained button после teardown не вызывает handler.
Каждые 16 кадров — четыре явных readGpuResidentSnapshot с проверкой данных.
Это программные native DOM events, не проверка физического пользовательского input.

Первый запуск без полного allocator warmup остановился на tick=40: arenaFloats
одного core 228→240; GPU allocations не росли и teardown прошёл. Raw сохранён:
`benchmarks/dom-gpu-soak-before-warmup-2026-11-01.json`. Добавлен фиксированный
прогрев 128 owner rotations с восемью graph observations. Арена выросла до
252 floats к rotation=32, затем стабильна до rotation=128 и весь measured run.
Факт стабилизации не доказывает точный механизм начального роста аллокатора.

Production snapshot `snapshot-1793536454305`, Chrome 156, Apple Metal 3,
physical adapter, DPR2, visible. 60 с, 7194 RAF-paced ticks, 1031 scene
created/retired (включая initial/warmup), 28776 resident updates и DOM callbacks,
1796 snapshots. 33665 буферов созданы и уничтожены, bytes 1582928 в обе стороны;
map count точно совпал с числом explicit snapshots. После settled tick:
8 live buffers / 256 bytes, 4 depth textures, 192 resident bytes; pending credits=0.
После teardown buffers/textures/resident reservations/nodes/edges/pending=0.
На каждом core capacity=45, WASM memory=1179648, arenaFloats=252 стабильны.
Uncaptured GPU errors не наблюдались. Raw samples ограничены кольцом 64.

Сохранённые heap observations 8.08…17.90 МБ; forced browser GC не выполнялся,
JS retention/plateau и driver/process memory не доказаны. Проверка application
allocation metadata не учитывает внутренний кеш драйвера. Picking, stale map
cancellation, device loss/reset и другие adapters не входят в этот soak;
их отдельные предыдущие fixtures не заменяют совместную длительную проверку.
Частоту RAF этого запуска не трактовать как измерение FPS.

Артефакты: `benchmarks/dom-gpu-soak-metal-2026-11-01.json`, `.jpg`,
`benchmarks/dom-gpu-soak-analysis-2026-11-01.json`. Независимый пересчёт проверил
scene/update/snapshot counts, сбалансированные buffer/byte allocations, maps,
сохранённые settled samples и final graph teardown. Собран production fixture,
preparer прошёл node --check, git diff --check. Runtime не менялся.
Metadata.scope измеренного snapshot унаследовал описание app A/B preparer;
это неточность описания, фактический протокол задают fixture и raw.protocol.
Текст scope исправлен для следующих builds; raw измерения сохранён неизменным.

Запуск: `node packages/svet/scripts/prepare-dom-gpu-soak.mjs --serve`, открыть
выведенный URL и Run 60-second soak. Следующие задачи: совместные reset/device
loss/stale-pick cancellation и браузерная матрица сценариев, затем долгий soak.


## Общий GPU device: reset, pending picking и восстановление

Добавлены `tests/browser/shared-gpu-recovery.html` и frozen production preparer
`scripts/prepare-shared-gpu-recovery.mjs` (localhost 4201). Два WASM cores,
один физический Apple Metal 3 device. Fixture задерживает наблюдение одного
настоящего mapAsync через Promise gate: native GPU операция выполняется,
но приложение ещё ждёт результат. Это управление JS completion observation,
а не принудительная остановка GPU execution.

Production snapshot `snapshot-1793536740359` прошёл четыре совместные проверки:
1. Reset core A при pending picking отменяет callback и GPU handle/context A;
   core B сохраняет handle/context, точные данные snapshot и renderer.
2. Новый root core A снова рисует на прежнем общем device.
3. Explicit device.destroy() при pending picking B инвалидирует GPU handles
   обоих cores; после async drain buffers/textures возвращаются к нулю,
   stale callback B не доставляется.
4. После нового requestAdapter/requestDevice оба cores рисуют, picking нового
   кадра доставляет ровно один callback. Fixture проверяет реальные GPU pixels.

Первый запуск проверял allocation count сразу после device loss и teardown;
один staging buffer 48 байт ещё ждал async closure. Добавлено ограниченное
ожидание фактического нулевого allocation count (до 5 с), а не ослабление
итогового нуля. Audit telemetry после detach имеет device=null и может показывать
нулевые device credits до завершения старой closure, поэтому не заменяет tracking.
Отказ: `benchmarks/shared-gpu-recovery-before-fix-2026-11-01.json`.
Следующий запуск очистил старое устройство, но requestDevice на прежнем adapter
получил Dawn OperationError "adapter is consumed". Recovery запрашивает новый
adapter; отказ сохранён в `shared-gpu-recovery-adapter-diagnostic-2026-11-01.json`.

На старом device созданы/уничтожены 12 buffers / 480 bytes, на новом 7 / 272;
map count 3 и 1. Конечные allocations, graph nodes/edges/pending и GPU credits
нулевые, uncaptured errors отсутствуют. Независимо проверен accounting двух
устройств и итоговый teardown. Нет проверки спонтанного hardware/driver crash,
других adapters, многочасового soak или полного JS/driver memory plateau.
Runtime в этом этапе не менялся.

Артефакты: `benchmarks/shared-gpu-recovery-metal-2026-11-01.json`, `.jpg`,
`shared-gpu-recovery-analysis-2026-11-01.json`,
`shared-gpu-recovery-package-2026-11-01.txt`. Девять installed-package gates
прошли; recovery fixture добавлен отдельным entry в production build consumer
установленного пакета. Аппаратный запуск этого этапа использовал workspace
production bundle; installed-package native recovery отдельно ещё не запускался.
Scope metadata измеренного snapshot унаследовал описание DOM/GPU soak; raw.protocol
и fixture задают фактический тест. Текст preparer исправлен для следующих builds.

Проверены node --check preparer/check-package и git diff --check. Следующий этап —
собрать браузерную матрицу с конкретными сценариями и свежими evidence, а также
добавить повторяемые recovery cycles/installed-package hardware qualification.


## Production browser matrix

Добавлены BROWSER_MATRIX.md, `tests/browser/browser-matrix-cases.json`, runner
`browser-matrix.html` и frozen `scripts/prepare-browser-matrix.mjs`. На Chrome156
production snapshot snapshot-1793536910588 прошёл 11 fixtures / 90 assertions.
Каждый fixture выполнялся в свежем same-origin iframe, runner сверял X/X,
число PASS lines и отсутствие FAIL. Независимо сверены manifest/counts и все
source hashes с текущим worktree. Артефакты: `browser-matrix-chromium-2026-11-01.json`,
`.jpg`, `browser-matrix-analysis-2026-11-01.json` в benchmarks.
Runtime не менялся; Vite multi-entry build, node --check и git diff --check прошли.
Router history, Node SSR/resumption, HTTP, две независимо собранные копии,
installed-package matrix и другие browsers явно остаются открытыми в матрице.


## Свежие browser router / Node SSR / HTTP проверки

Router history: frozen production snapshot snapshot-1793537045359, Chrome156,
14/14 в отдельной disposable top-level tab. Реальные Back/Forward/go, guards,
compensation без роста history, полные query/hash, redirects, reentrant navigation,
шесть traversal bursts и graph teardown. Output и screenshot сохранены:
`benchmarks/router-history-production-2026-11-01.txt` / `.jpg`.

Node SSR/browser resume: 8/8 с Node renderer из dist/package и Vite dev client.
Добавлен raw report в resumption-roundtrip.html: точные HTML/stateJson ответа,
Node graph cleanup counts, browser UA и результат взаимодействий. Проверены
native DOM identity, escaped values, checkbox/number/radio/multi-select/type,
re-resumption и teardown. Raw/screenshot: `node-ssr-browser-roundtrip-2026-11-01.json`
/ `.jpg` в benchmarks. Это не frozen production browser SSR qualification.

HTTP browser actions: 7/7, настоящий fetch к loopback Vite fixture. Abort до
headers и во время JSON streaming подтверждён server stats closed=true,
complete=false; overlapping bodies сохраняют pending до завершения обоих.
Disposed action не начинает новый fetch, graph nodes=0. Raw report теперь
сохраняет bounded журнал diagnostic HTTP replies (макс256); получено12,
dropped=0. Raw/screenshot: `browser-http-actions-2026-11-01.json` / `.jpg`.
Node native HTTP tests повторены: 4/4, log `browser-matrix-http-node-2026-11-01.txt`.
Это dev browser, не frozen production HTTP qualification и не load/soak.

`router-ssr-http-qualification-2026-11-01.json` хранит hashes TS runtime,
dist/package JS, fixtures/server scripts, точных SSR HTML/stateJson и router
build metadata. Независимо проверены PASS counts, raw statuses и серверный
SSR graph cleanup. Browser matrix и PLAN обновлены. Runtime не менялся;
preparer синтаксис, production router build и git diff --check прошли.
Следующие пробелы: independently built instances, installed-package browser
matrix и production browser SSR/HTTP, затем остальные lifecycle fixtures.


## Две независимые копии текущего движка в одном document

`event-instances.html` отвязан от исторических adoption baseline/candidate.
Новый `scripts/prepare-event-instances.mjs` собирает текущий engine index.ts
двумя независимыми Vite library builds, каждая со своей полной копией module
state и embedded WASM. URL разные, bytes и SHA256 одинаковые. Build сохраняет
полный source/asset manifest и отвергает source drift во время подготовки.

Frozen production snapshot snapshot-1793537272214 прошёл 9/9 в Chrome156:
разные signal function identities / WASM cores; handlers по одному разу;
child-before-parent bubbling в смешанном дереве; additive bindings на одном
узле с независимым teardown; stopImmediatePropagation через границу копий;
сохранение callback второй копии после teardown первой; closed Shadow DOM;
reset одного module не удаляет второй reactive graph/effect; оба graph nodes=0
после teardown. Проверка reset добавляет реальные signal/effect, чтобы итоговый
нулевой граф не был лишь тривиальным результатом event-only roots.

Raw/screenshot: `benchmarks/independent-engines-production-2026-11-01.json` / `.jpg`.
Независимо проверены9 PASS lines, все source/asset hashes и равенство двух
bundle hashes; `independent-engines-analysis-2026-11-01.json`. Bundle SHA256:
ad38e154d0c8470c55b14c6a5524e88b41d853097792fa0bfc36c90a732b9ff2.

Граница: эта проверка подтверждает shared native dispatcher и независимые
module-owned graphs, не перенос tracking/untrack/async contexts между копиями.
Разные версии пакета, installed-package duplicates и async/SSR adapters открыты.
Runtime не менялся; прошли production page + обе library builds, node --check
preparer и git diff --check. Далее — installed-package browser matrix и
production browser SSR/HTTP; остальные lifecycle fixtures ещё не все включены.


## Installed-package production browser matrix и GPU recovery

`check-package.mjs` теперь собирает matrix runner и11 scenarios из manifest
как дополнительные entries девятого package gate. Импорты переписываются на
публичный svet-engine, оставшийся /src/engine/ запрещён. Первый прогон обнаружил
прямые wasm-bridge imports в двух fixtures; guard остановил сборку. Отказ
сохранён в `benchmarks/installed-matrix-package-before-import-fix-2026-11-01.txt`.
После перевода этих импортов на package export все9 gates прошли.

--keep оставляет isolated consumer. Добавлена browser metadata: SHA256 packed
archive,15 runtime JS, fixture sources и36 production assets; installed JS
побайтно сверяется с workspace dist/package. Metadata отдельно предупреждает,
что production build ещё не является browser pass. Новый
`scripts/serve-installed-browser.mjs` запускает только preview готового output
на localhost4206 без aliases/dev compilation.

Из consumer установленного архива в Chrome156 прошли11/11 fixtures /90 assertions.
На том же consumer physical Apple Metal3 recovery прошёл4/4: pending picking,
core reset без потери соседа, explicit device destruction, fresh adapter/device,
rendering и один успешный pick. Счётчики allocations обоих устройств
сбалансированы, final nodes/edges/pending/credits=0. Raw metadata одинаков в
матрице и recovery. Независимо повторно проверены counts, metadata, все assets
и installed runtime hashes после browser execution.

Archive SHA256: 6d429df42a28a2334ddb1265ab1493dab3abd1ae87f7c87b5468817b7cefd0c1.
Consumer и archive сохранены в /tmp, точный путь в qualification JSON.
Артефакты: `installed-matrix-package-2026-11-01.txt`,
`installed-browser-matrix-2026-11-01.json` / `.jpg`,
`installed-gpu-recovery-metal-2026-11-01.json` / `.jpg`,
`installed-browser-qualification-2026-11-01.json` в benchmarks.

Runtime не менялся; прошли syntax checks и git diff --check. Production browser
SSR/HTTP, installed-package independent duplicates и остальные fixtures ещё
открыты; эти результаты не доказывают cross-browser или driver memory plateau.


## Installed production Node SSR / browser HTTP

В `check-package.mjs` добавлены production entries resumption-roundtrip и
server-action-http, public package imports и metadata hashes. Девять gates прошли.
Общий `browser-fixture-endpoints.mjs` заменяет дублирующий код dev servers;
`serve-action-browser.mjs` и `serve-resumption-browser.mjs` сохраняют прежние URL
и вызывают эти handlers. Installed preview подключает handlers через
configurePreviewServer: отдаёт готовый production output без dev transforms/HMR.
Node SSR engine импортируется из изолированного consumer/node_modules/svet-engine,
а не workspace dist/package. Точный SSR ответ теперь содержит provenance этого
архива/runtime JS/endpoint source; старый dev wrapper передаёт provenance=null.

Chrome156: installed production SSR→resume8/8, HTTP actions7/7. Проверены те же
native form identity, values/attributes, dynamic options, re-resumption/cleanup;
fetch abort до headers и во время streaming подтверждён server closed=true,
complete=false. Overlapping requests корректно удерживают pending до второго
completion. Node native HTTP tests повторены4/4. Это реальные loopback fetch,
не Promise mock и не simulated SSR timings.

Archive SHA256 1bd11a4f374c7783b21d7f744e392d36832f6b19ee44a860a553e603dce5b938.
Независимо сверены15 installed runtime hashes,41 production assets, точные
SSR HTML/stateJson, Node/server cleanup0,8/7 PASS lines и два server-observed
aborted replies (до headers и streaming). Raw/proof:
- `benchmarks/installed-production-ssr-2026-11-01.json` / `.jpg`;
- `benchmarks/installed-production-http-2026-11-01.json` / `.jpg`;
- `benchmarks/installed-production-ssr-http-qualification-2026-11-01.json`;
- `benchmarks/production-ssr-http-package-2026-11-01.txt`;
- `benchmarks/production-ssr-http-node-2026-11-01.txt`.

Consumer/archive сохранены по пути qualification JSON. Runtime не менялся;
production build, package gates, syntax checks и git diff --check прошли.
Отдельный raw результата DOM/GPU матрицы на новом архиве не заявляется:
предыдущая installed DOM/GPU qualification относится к сохранённому старому
архиву; runtime JS идентичны, но архив и assets имеют свои manifests.
Открыты нагрузка/длительная concurrency, arbitrary async SSR components/Edge,
остальные lifecycle fixtures и cross-browser qualification.


## Расширенная installed production lifecycle matrix:20/176

В browser-matrix-cases.json добавлены9 существующих fixtures с явными expected
counts: attribute-lifetime8, captured-context11, for-current-item7,
fragment-construction9, h-construction11, model-adoption8, model-reset-roots16,
model-reset8, style-lifetime8. Новый installed consumer production build включает
все20 fixtures; прежние tests/assertions не ослаблялись. Отдельная квалификация
новых fixtures выполнена настоящим Chromium, не только по наличию файлов.

Chrome156:20/20 fixtures /176 assertions прошли. Дополнительно86 assertions
закрывают binding teardown, explicit continuation reentry после await,
same-key values/indexes и nested Forma loops, fragment/h construction rollback,
model adoption/reset, changing roots и closed ShadowRoot. Это explicit capture,
не автоматическое async h/SSR ownership и не cross-module untrack propagation.

Archive SHA25613d64c3f8815b112f40187db206ad617cd50a364ec5f14d227df6f93ba4f5e82.
Сохранены raw output каждого fixture, UA/DPR и build metadata. Независимо
сверены manifest names/counts,176 PASS lines,60 production assets и15 installed
runtime JS; runtime bytes совпадают с workspace dist/package. Package gates9/9,
git diff --check прошли. Runtime не менялся, полный engine suite не повторялся.

Артефакты: `benchmarks/expanded-installed-lifecycle-2026-11-01.json` / `.jpg`,
`expanded-installed-lifecycle-analysis-2026-11-01.json`,
`expanded-lifecycle-package-2026-11-01.txt`. Consumer/archive сохранены по пути
analysis JSON. BROWSER_MATRIX.md отражает20 scenarios; старые proofs сохраняются
со своими package hashes. Следующий этап — определить и реализовать недостающую
async SSR component integration, сохраняя явные границы browser/Edge и Node.


## Автоматические async h/JSX components в Node SSR

Реализована интеграция Node renderToStringAsync с function components h/JSX.
Node server entry устанавливает внутренний DOM adapter, активный только в ALS
render session. Каждый function component запускается в runWithOwnerScopeAsync;
синхронный Node возвращается немедленно, async Node публикуется в marker range.
Nested tasks и tasks, созданные node/signals getters результата, ожидаются до
сериализации HTML/state. Session закрывается перед serialization. Ошибка
fail-fast отклоняет request без ожидания uncooperative sibling; request teardown
закрывает все child owners. Markers удаляются при commit, stale ranges отвергаются.
Async owner callback теперь начинает синхронную часть сразу в установленном
контексте; cancellation promise наблюдается даже при синхронном abort+throw.

Добавлен AsyncComponent type и JSX.ElementType; h/JSX по-прежнему возвращают
Node. Вне Node renderToStringAsync Promise component вызывает явную диагностику,
rejection наблюдается. Node async_hooks не попадает в browser output.
Synchronous component DOM access сохранён. Provider children, которым нужен
его context, создаются lazy callback; eagerly evaluated children уже исполнились
до Provider. Host должен держать DOM implementation живой весь async request.

Новые8 regression tests: interleaved providers и nested components после await,
ошибка/cleanup, abort/late allocation refusal, immediate sync DOM, result getter,
fail-fast с зависшим sibling, concurrent request isolation, unsupported async
component outside renderer без unhandled rejection. До реализации два из трёх
первых тестов падали и были3 unhandled errors; дополнительные integration tests
поймали раннюю несовместимость sync DOM/getter. Отказы сохранены в
`async-components-before-2026-11-01.txt` и `async-components-integration-before-2026-11-01.txt`.

Итоговые gates: Rust53/53; production build; JS1229/1229 /86 files /maxWorkers4,
7.91с; package9/9 с installed Node async component execution и strict JSX type
fixture; native HTTP4/4. Existing benchmark timeout5000ms не изменён.
Installed production browser DOM matrix20/20 /176 assertions повторена на новом
runtime. Новый async-components-roundtrip прошёл5/5: Node provider values,
request cleanup0, отсутствие pending anchors, DOM identity/state after await,
native numeric model input и browser teardown0.

Первый browser async roundtrip получил document is not defined: старый sync host
устанавливал DOM globals только на время синхронного render. Host fixture теперь
устанавливает DOM implementation на lifetime сервера и восстанавливает globals
при close; движок сам их не устанавливает. Failure raw сохранён в
`benchmarks/async-components-browser-host-before-2026-11-01.json`.

Артефакты в benchmarks: async-components-native /production-build /suite-final
/package /http-2026-11-01.txt; async-components-browser-2026-11-01.json/.jpg;
async-components-browser-matrix-2026-11-01.json/.jpg;
async-components-qualification-2026-11-01.json. Qualification связывает source,
archive,15 runtime JS,62 assets, точные HTML/stateJson и endpoint provenance.
Archive079ef952516abfa419cb92b85969460963b6232970b87b4008afca39806fbf27.

Открыты browser async component renderer, Edge adapter, streaming SSR,
cross-copy async propagation, arbitrary native DOM fault combinations,
retention/latency/throughput новых session/owner scopes и многочасовая нагрузка.
Это завершённая Node integration в указанном объёме, не весь async/SSR roadmap.
README, browser matrix и PLAN обновлены. Ничего не committed/pushed/published.


## Async component request churn и retired-core cache

Добавлен `scripts/measure-async-component-churn.mjs`: success/failure/cancel
renderToStringAsync requests с настоящим WASM/JSDOM, component owner, source,
effect, resource, captured token и DOM node. Error/cancel возобновляют старую
continuation после teardown: попытка h() должна отказать. Проверяются cleanup,
external abort hook removal, native graph/pending0 и WeakRef collection.
Проба ограничена30 requests между GC,32 ring samples + baseline и четырьмя
online memory windows. Десять GC attempts не ослаблены; failures сохраняют raw.

Первый запуск обнаружил ошибку самого измерителя: неверное имя native pending
API. Затем стабильное удержание одного core после10 GC воспроизвелось и без
concurrent recompilation. У runtime есть single-entry hot context cache; переход
на ambient graph перед GC освобождал core. Этот вариант на5000 cycles прошёл
15000 requests /105000 probes, но требовал ручного cache advance.

Исправление runtime: runWithIsolatedContextAsync в finally вызывает internal
retireReactiveContextCache. WeakSet отмечает retired cores, очищается совпадающий
hot cache; late cleanup читает своё состояние из WeakMap, но больше не записывает
retired core в hot cache. Простое одноразовое очищение оказалось недостаточным:
асинхронная очистка могла заполнить cache снова. WeakSet не держит cores сильно.
Живые/default cores сохраняют прежний быстрый cache-hit путь; влияние cold miss
WeakSet check и throughput отдельно не измерено, speedup не заявляется.

Native-mode GC regression без ambient mutation падала до исправления на одном
core, после retirement guard прошла. Финальная проба дополнительно отслеживает
WebAssembly.Memory:15000 requests,120000 objects (exports/memory/owner/source/
effect/resource/capture/DOM),15000 cleanups,10000 rejected late continuations.
Все объекты освобождены, максимум6 GC turns,501 samples /32 retained. В JSON
`retireCache=false` означает, что sampler НЕ переключает runtime cache вручную;
сам runtime retirement включён. Четвертные heap means40.104/40.261/40.302/40.316МБ,
RSS134.88/137.26/138.50/139.68МБ. RSS рос: длительное plateau и native physical
reclamation этим не доказаны. Тайминги — наблюдения маленького JSDOM сценария
с forced GC, не A/B, не app latency guarantee и не доказательство ускорения.

Installed package gate теперь запускает ту же пробу отдельным Node --expose-gc
процессом:300 requests /2400 objects, всё освобождено за один GC turn без ручного
cache advance. Это автоматическая регрессия удержания, а не проверка внутреннего
флага. Production build и полный JS suite1229/86 прошли (7.88с,maxWorkers4);
package9/9 и native HTTP4/4 прошли. Rust не менялся, новых native tests не добавлено.

Артефакты в benchmarks:
- async-component-churn-default /default-verified /no-concurrent-2026-11-01.json — ранние отказы;
- async-component-core-cache-before /after /5000-2026-11-01.json — cache experiments;
- async-component-retention-memory-2026-11-01.json — финальные15000 requests;
- async-component-retention-installed-2026-11-01.json — installed regression;
- async-component-retention-analysis-2026-11-01.json — независимый accounting;
- async-component-retention-build /suite /package /http-2026-11-01.txt — gates.
Независимо сверены counts, windows, native mode, runtime/source hashes и installed
probe. Предыдущие passing probes сохранены отдельно и не заменяют финальную пробу.

Воспроизведение после build:package:
`node --expose-gc packages/svet/scripts/measure-async-component-churn.mjs <raw.json> 5000 native`.
Не запускать builds/tests одновременно с timing observations. Open scope:
длительный timed soak/concurrent requests, browser async rendering/Edge/streaming,
профилирование cache retirement cost и process/driver memory.


## Конкурентный async component soak:24 одновременно живых requests

Добавлен `scripts/soak-async-components.mjs`: timed нагрузка Node/JSDOM с24
одновременно живыми renderToStringAsync requests. В каждом batch8 success,
8 component failure,8 request cancellation. Shared resource-ready barrier
проверяет activeRequests=24,24 разных WASM cores и ненулевые живые graph nodes
до освобождения запросов. Child после barrier проверяет свой core/owner/Provider
context и reactive effect update. Failure/cancel возобновляют позднюю continuation
после teardown и требуют отказа h(). Это реальный interleaving с ALS, не24
последовательных операций в Promise.all без проверки overlap.

На каждый request отслеживаются8 объектов, включая WASM exports и Memory.
Нет ручного ambient-cache advance. После batch graph/pending0, abort hooks0,
GC до10 turns;32 ring samples + baseline и4 online windows, без накопления raw
истории на каждый request. Batch deadline10с прерывает некорректное зависание,
не заменяет ожидаемые component errors. Первый smoke отказал в измерителе:
observer продолжал требовать старое значение после намеренного source update;
доступ к result.value скрыл исходную ошибку. Ожидание effect и диагностика
result.error исправлены; raw `async-concurrent-smoke-2026-11-01.json` сохранён.

Финальный60-секундный запуск:2568 barriers/batches,61632 requests,
493056 objects освобождены,61632 cleanup,41088 late refusals, maxGC7.
На mode20544 операций. Active requests в конце0, peak24.
Четвертные heap means37.220 /36.318 /36.345 /36.347МБ;
RSS137.23 /135.39 /124.93 /126.07МБ. После прогрева heap почти не изменялся
в этом ограниченном интервале, RSS к концу ниже первого окна. Это не
многочасовой plateau и не общий bound процесса. Тайминги включают ожидание
barrier; forced-GC loop не является production throughput/latency benchmark.
Нет настоящего HTTP, браузера, Edge или GPU в этом soak.

Installed package gate дополнен коротким отдельным concurrent probe:
61 batch /1464 requests /11712 objects, peak24, maxGC5, всё освобождено.
Все9 package gates прошли. Runtime в этом этапе не менялся, полный engine suite
не повторялся. Независимо проверены mode/counts, overlap barriers, final active0,
GC samples/windows и source/runtime hashes; syntax и git diff --check прошли.

Артефакты в benchmarks: async-concurrent-smoke-fixed-2026-11-01.json,
async-concurrent-soak-60s-2026-11-01.json,
async-concurrent-soak-analysis-2026-11-01.json,
async-concurrent-installed-2026-11-01.json,
async-concurrent-package-2026-11-01.txt. Consumer/archive сохранены по пути
analysis JSON. Запуск после build:package:
`node --expose-gc packages/svet/scripts/soak-async-components.mjs <raw.json> 60000`.
Длительность от1000мс до1ч; runtime может быть выбран installed package path
в пятом argv (как делает package gate). Не запускать CPU builds/tests параллельно.
Далее — adapter architecture browser/Edge и streaming, настоящий concurrent
HTTP async SSR/disconnect, более длительный soak и независимые performance controls.


## Конкурентный настоящий HTTP async SSR / disconnect

Добавлен `scripts/check-async-ssr-http.mjs`: native node:http server/client на
loopback, установленный или workspace runtime, JSDOM host и renderToStringAsync.
12 requests одновременно доходят до component-ready barrier на12 разных cores;
все graphs ещё живые. В каждом batch4 success,4 component failure,4 disconnect.
Клиентские sockets закрываются до headers; server response.close с
writableEnded=false прерывает signal только соответствующего render.
Перед release соседей проверяется activeRequests=8 и их живые graph nodes.

20 batches:240 requests —80 HTML200,80 expected component error500,
80 unfinished-response closures. Все successful HTML содержат только свой
Provider/request id после await. Disconnects отклоняют render с host reason,
не пишут ответ (writes=0), поздний h() отказывает. После каждого request
cleanup1, native nodes/pending0, external abort hooks0; activeRequests в конце0,
peak12. Это реальный transport test, не Promise-only cancellation.

Package gate запускает этот же script с installed runtime на4 batches:
48 requests, peak12, всё прошло; package9/9. Runtime в этом этапе не менялся,
полный engine suite не повторялся. Независимо сверены counts/mode outcomes,
closed-before-end, отсутствие disconnected writes, teardown и source/runtime
hashes, installed runtime совпал с основным прогоном. Syntax и diff check прошли.

Raw/анализ: `benchmarks/async-ssr-http-concurrent-2026-11-01.json`,
`async-ssr-http-installed-2026-11-01.json`, `async-ssr-http-analysis-2026-11-01.json`,
`async-ssr-http-package-2026-11-01.txt`. Consumer/archive сохранены по пути
analysis JSON. Script принимает installed package directory в argv2 (пустая
строка выбирает workspace), output JSON в argv3, число batches в argv4:
`node packages/svet/scripts/check-async-ssr-http.mjs '' <raw.json> 20`.

Границы: HTTP disconnect проверен до headers при полном HTML render, не во
время streaming SSR. Нет reverse proxy, browser client, Edge, load/soak,
memory plateau или production security qualification. Node default agent и
серверные connections закрываются в finally. Дальше — portable adapter
architecture/Edge и streaming SSR с отдельными transport/lifetime критериями.


## Progressive Node SSR data protocol / native browser receiver

Добавлены renderToChunksAsync в Node server entry и stream.ts с публичным
SsrStreamChunk/createSsrStreamReceiver. Это настоящее progressive output:
shell с boundary ranges, patches по readiness, complete с final HTML/stateJson.
Async boundaries публикуются serially с one-slot rendezvous/backpressure;
return() закрывает outstanding next(), signal/caller teardown отменяют request.
Default maxChunkBytes8MiB encoded JSON, maxBoundaries1024 total async boundaries.
HTML/DOM serialization transient allocations и вся DOM tree не входят в эти bounds.

Stream receiver имеет child ownership, captured reentry после await и reset
finalizer; replacement receiver не даёт старому handle изменять новый container.
Contextual Range parsing сохраняет SVG namespace. Snapshot resumption выполняется
на complete. Protocol не генерирует inline patch scripts; HTML остаётся trusted
application markup, это не sanitizer. Финальный полный HTML исправляет reactive
writes после отправленного boundary: matching DOM сохраняется, mismatches
заменяются до resume. Цена — повторная передача полного HTML; оптимизация открыта.

Новые9 unit regressions: early shell, nested patches/final coherence, native input
identity, return+pending next, failure with hanging sibling, stale receiver,
limits/owner disposal, SVG/parent cleanup, paused backpressure + abort hooks,
undefined rejection/reset (несколько assertions сгруппированы в9 tests).
Первый focused run выявил detached createRoot у receiver и неподходящий '{}'
для empty resumption snapshot; исправлены parent scope/reset cleanup и валидный
serializeState({}). До-исправления log: streaming-receiver-before-2026-11-01.txt.

Gates: production build; JS1238/87,maxWorkers4,8.12с; package9/9 с installed
stream execution и strict public chunk types; native action HTTP4/4. Existing
benchmark timeout5000ms не менялся. Rust implementation не менялась.

Native Chrome156 installed production browser roundtrip6/6 прошёл. Контрольный
HTTP endpoint держит async component до отдельного release-запроса: shell211байт
при finished=false/nodes1 доказывает отсутствие full-tree buffering. После release
patch132366байт и complete132457байт; model/input identity сохранены, native input
обновляет resumed state, escaped script text не создаёт script element. Payload
выше Node writer highWaterMark даёт2 настоящих write(false)→drain. После complete
server nodes0/cleanup1, после receiver.dispose client graph0.

Первый browser run ошибочно ожидал graph nodes>0, хотя pending component создал
только owner и ещё не signal. В fixture добавлен owned pending signal до gate;
ошибка исходного assertion сохранена streaming-browser-before-2026-11-01.json.
Node HTTP body disconnect после полученного shell отменяет pending next и чистит
request graph; следующий свежий stream проходит через shell/patch/complete с drain.
Эти2 transport checks включены в installed package gate.

Артефакты в benchmarks: streaming-focused /production-build /suite /package-final
/action-http-2026-11-01.txt; streaming-browser-final-2026-11-01.json/.jpg;
streaming-http-final-2026-11-01.json; streaming-installed-http-final-2026-11-01.json;
streaming-qualification-2026-11-01.json. Последний связывает chunk counts/bytes,
server stats, source/runtime/asset hashes и archive
 d03f29915d5c8f03aa2baf55c34cb40145628975eb4ca55e25ca2a9f43aa43b8.
Все16 runtime JS и production assets независимо сверены; consumer/archive сохранены.

Открыты: protocol version negotiation, stream retention/latency/throughput,
incremental final reconciliation вместо full HTML, больше native DOM/parser
комбинаций, concurrent long streams/disconnect during drain, proxy behavior,
Edge/browser producer adapters и другие browsers. Это bounded Node protocol
с native receiver proof, не завершение всего streaming/adapter roadmap.

HTTP stream checker дополнительно переведён на incremental newline framing,
без предположения о границах TCP/read chunks. Оба checks повторно прошли
на workspace и сохранённой installed сборке; ранний prefix-only результат сохранён.

Финальная lifetime guard проверяет отмену и caller lifetime повторно после
channel.next(), перед возвратом chunk. Suite повторён1238/87 (8.27с), package9/9,
production build и installed browser6/6 повторены на итоговом архиве
afe5cac7fb36f42d0e401ea25ed0c647e6bd02ac4feab58657d228d95acbe005.
Финальные файлы: streaming-suite-final /package-qualified /production-qualified
-2026-11-01.txt, streaming-browser-qualified-2026-11-01.json/.jpg,
streaming-installed-http-qualified-2026-11-01.json. Qualification обновлён;
ранние successful/failed snapshots сохранены, не выдаются за итоговую сборку.


## Stream wire v1: version, sequence и pre-shell ownership

Добавлен shared stream-protocol.ts: SSR_STREAM_VERSION=1 и envelope
{version,sequence,streamId,payload}. Payload содержит прежние shell/patch/complete
поля. Producer назначает contiguous sequence от0 в единой send path. Receiver
сначала проверяет version и sequence, затем stream identity, и только после этого
читает payload/HTML. Missing/unknown versions, skipped/replayed/noninteger records
отвергаются до DOM writes. Envelope намеренно несовместим с первым unversioned
черновиком: старый receiver не видит прежних top-level type/html и отказывает.
Никаких fallback или implicit negotiation не добавлено.

Пять новых регрессий проверяют unknown version без вызова payload getter,
legacy rejection, first-sequence0/replay rejection, negative/fractional/NaN/
Infinity/unsafe/skipped sequences и сохранение ранее resumed bindings при отказе
до shell. Последнее обнаружило отдельную проблему ownership: receiver cleanup
без принятого shell снимал чужой resume state. Теперь ownsResumption устанавливается
только после принятия shell; protocol failure до этого освобождает лишь receiver.
Ранний failing log сохранён: stream-protocol-before-2026-11-01.txt.

Production build сначала нашёл одну не обновлённую legacy запись reset-теста;
исправление тестовой формы и успешный build записаны отдельно в
stream-v1-build /build-qualified-2026-11-01.txt. Итоговый suite1243/88,
maxWorkers4,24.51с прошёл без изменения5000ms benchmark timeout. Package9/9
прошёл с strict literal-version type fixture и installed Node producer/receiver,
native action HTTP4/4 прошёл. Installed HTTP stream disconnect/recovery checks
также прошли на v1 и сверяют version/sequence0,1,2.

Chrome156 installed production browser7/7: unknown version отклоняется до payload
getter/native DOM mutation; slow-component shell всё ещё приходит до release;
реальные HTTP records имеют version1 / sequences0,1,2; resume/native input,
escaped text, writer drain и teardown прошли. Bytes248/132403/132494 для
shell/patch/complete. Независимо проверены17 runtime JS и64 production assets,
record metadata и source hashes. Qualification archive:
2d907fd65a07935cc1f57a60b1205c1dc854ec627a9577587603f8d103ddbdbf.

Дополнительно реально загружен сохранённый старый compiled receiver (не его
имитация): legacy отверг v1 с Invalid SSR stream chunk, новый receiver отверг
legacy с Unsupported SSR stream version; DOM остался прежним. Это Node/JSDOM
проверка двух actual builds, не native browser legacy/new matrix и не гарантия
совместимости произвольных версий. Raw: stream-v1-legacy-compat-2026-11-01.json.

Артефакты benchmarks: stream-protocol-focused-2026-11-01.txt,
stream-v1-suite-qualified /package /http-2026-11-01.txt,
stream-v1-browser-2026-11-01.json/.jpg,
stream-v1-installed-http-2026-11-01.json,
stream-v1-qualification-2026-11-01.json. Archive/consumer сохранены по пути metadata.
README, browser matrix и PLAN обновлены; git diff --check прошёл.
Открыты HTTP version negotiation/migration, optimization final snapshot,
stream retention/throughput/long concurrent transport, proxies и Edge adapters.

## Disposed stream receiver: воспроизведённое удержание DOM (2026-11-01)

Добавлен `scripts/measure-stream-receiver-retention.mjs`: 32 сохранённых receiver
handles после dispose удерживают 32 detached JSDOM containers за 10 GC-раундов.
Контроль без сохранённых handles освобождён полностью. После удаления handles
освобождаются и все 32 ранее удержанных контейнера. Raw:
`benchmarks/stream-receiver-retention-before-2026-11-01.json`.
Это воспроизведение дефекта, не успешная квалификация: требуется разрыв ссылок
receiver при teardown. Iterator, complete без dispose, браузерный GC и RSS
данным измерением не проверяются. Package build прошёл; runtime не менялся.

### Исправление receiver handle retention

Публичный receiver вынесен в отдельное замыкание с очищаемым slot. Любой
teardown (dispose, reset, replacement, protocol error) обнуляет slot до
освобождения resumed bindings. Сохранённый handle после teardown не хранит
внутренние DOM/context/cleanup closures; повторный dispose безопасен, apply
отклоняется до чтения полей chunk.

GC probe: 32 сохранённых handles, четыре способа teardown, 0 retained
containers, контроль 0; после освобождения handles тоже 0. Исходный отказ
32/32 сохранён отдельно. Raw: `benchmarks/stream-receiver-retention-after-2026-11-01.json`.
Полный suite: 1243 теста / 88 файлов, 11.66 с. TypeScript и package build прошли.
Installed gate дополнен этим GC probe; первая попытка выявила неверный путь
к installed entry в harness, исправлено на dist/package/index.js; журнал
первой попытки сохранён. Iterator/active complete/browser GC/RSS не квалифицированы.

Итоговая квалификация: installed-package 9/9 и HTTP actions 4/4 прошли.
Журналы: `benchmarks/stream-retention-package-qualified-2026-11-01.txt`,
`benchmarks/stream-retention-http-2026-11-01.txt`,
`benchmarks/stream-retention-suite-2026-11-01.txt`. `git diff --check` прошёл.

## Terminal stream iterator retention (2026-11-01)

Новый `scripts/measure-stream-iterator-retention.mjs` сохраняет 24 iterator
handles после полного чтения, consumer return и producer error. До исправления
удерживались 24 view, 24 captured objects и 8 core/memory объектов. После
удаления handles все объекты освобождались: исходный отказ сохранён в
`benchmarks/stream-iterator-retention-before-2026-11-01.json`.

Публичный iterator теперь находится в отдельном замыкании и отсоединяет
внутренний producer после terminal next/error/return. WeakSet терминальных
внутренних iterators позволяет отличать завершение от отклонённого конкурентного
next, не удерживая сами iterators. Регрессия подтверждает, что concurrent next
не закрывает ожидающее чтение; return/throw после завершения также проверены.

После исправления 0 удержанных объектов всех пяти типов при сохранённых
handles: `benchmarks/stream-iterator-retention-after-2026-11-01.json`.
Probe включён в installed-package gate. Suite 1244/88 прошёл за 7.78 с;
TypeScript и package build прошли. Это object collection в Node/JSDOM,
не RSS-плато; external abort без последующего next/return и никогда не
запущенный iterator пока не проверены.

Итог: package 9/9 (включая установленный GC probe), HTTP actions 4/4,
`git diff --check` passed. Журналы `benchmarks/stream-iterator-package-2026-11-01.txt`
и `benchmarks/stream-iterator-http-2026-11-01.txt`.

## External abort without terminal read (2026-11-01)

Расширенный iterator probe воспроизвёл удержание 16 view/captured objects
после abort до запуска и после первого shell, без следующего next/return.
Raw: `benchmarks/stream-abort-retention-before-2026-11-01.json`.

Request release теперь уведомляет отдельный public iterator slot, очищая
ссылку на producer независимо от действий consumer. Slot сохраняет только
непрочитанную причину ошибки. Поздний next отклоняется с тем же значением
один раз, далее done; return очищает причину. Удалён промежуточный WeakSet
терминальных iterators: slot напрямую знает о request teardown.
GC probe 40 handles (complete/return/error/abort-idle/abort-started): нет
удержанных view, captures, core, memory или nodes; контроль тоже пуст.
Raw: `benchmarks/stream-abort-retention-after-2026-11-01.json`.
Регрессия включает pre-aborted signal, отсутствие вызова view после abort,
причину по identity и повторное terminal чтение. JS1245/88 прошёл за 11.59с;
TypeScript/build:package прошли.

Незапущенный и неотменённый iterator обязан сохранять view для будущего next.
Пользовательская причина ошибки сама может содержать произвольные ссылки:
они сохраняются до чтения/return согласно контракту ошибки. Длительное
RSS-плато, proxy transport и browser GC этим не проверяются.

Итоговая квалификация: package9/9 (расширенный installed GC probe), HTTP4/4,
`git diff --check` passed. Журналы: `benchmarks/stream-abort-package-2026-11-01.txt`,
`benchmarks/stream-abort-http-2026-11-01.txt`, `benchmarks/stream-abort-suite-2026-11-01.txt`.

## Concurrent native HTTP streaming soak (2026-11-01)

Добавлен `scripts/soak-ssr-stream-http.mjs`: реальный loopback HTTP/fetch,
8 конкурентных clients, один из трёх отключается после shell; остальные
читают с паузой 5 мс между reader.read. Последовательность/version/streamId
проверяются у каждого полного ответа. Каждый запрос требует finished,
nodes=0, cleanup=1; полный ответ требует минимум двух writer drains.
Fixture state забывается после каждого запроса, samples ограничены 32,
агрегаты памяти хранятся отдельно по четвертям.

60.034 с: 11952 requests, 7968 complete, 3984 disconnect, 15936 drains,
23904 reader pauses, 2113691280 прочитанных bytes. Все request invariants
прошли; финальный ambient graph пуст. Heap диапазоны по четвертям, MiB:
37.75–42.08 / 38.55–38.94 / 38.77–39.00 / 38.85–39.17. RSS к последней
четверти ниже раннего пика; универсальное или многочасовое плато не доказано.
Raw: `benchmarks/stream-http-soak-60s-2026-11-01.json`; независимая сверка
сумм, числа samples и source hash: `benchmarks/stream-http-soak-analysis-2026-11-01.json`.

Повтор: `node --expose-gc packages/svet/scripts/soak-ssr-stream-http.mjs OUTPUT 60000`.
Это delayed fetch reads и реальные drain events, но fetch/OS имеют свои
буферы: длительное TCP saturation и proxy buffering этим не подтверждены.
WASM memory capacity/GPU/браузер и аппаратные recovery сценарии не измеряются.
Runtime не менялся; syntax и diff checks прошли.

## Native HTTP proxy streaming/disconnect matrix (2026-11-01)

Добавлен `scripts/check-ssr-stream-proxy.mjs`: настоящий origin и отдельный
HTTP proxy, четыре сценария streaming/buffering × complete/disconnect.
Streaming relay отправляет порции до 4096 bytes с паузой 2 мс; клиент читает
с паузой 5 мс. Buffered mode собирает ответ с жёстким cap1MiB.
Сначала подтверждается незавершённый origin с живым graph до release gate.
Streaming доставляет ранний shell; buffered mode скрывает его до complete.
Оба полных ответа проверены по sequence/type/version/streamId. Disconnect
в обоих режимах закрывает upstream: origin finished/nodes0/cleanup1.

4/4 passed, 2 propagated disconnects, buffered peak265148 bytes, relay67
writes. Origin drains2 на каждый полный ответ. Downstream relayDrains0: это
не доказательство TCP saturation. Raw с source/runtime hashes:
`benchmarks/stream-proxy-qualified-2026-11-01.json`. Ранний прогон сохранён
отдельно. Проверка добавлена в installed-package gate.

Не квалифицированы nginx/CDN, compression, TLS, HTTP2/3, длительное
насыщение socket buffers или proxy, который не пересылает disconnect.
README теперь явно описывает зависимость раннего shell от buffering и
необходимость распространения отмены. Runtime не менялся.

Installed-package9/9 passed с новой proxy проверкой: журнал
`benchmarks/stream-proxy-package-2026-11-01.txt`. Node syntax и
`git diff --check` passed; runtime suite не повторялся, runtime не менялся.

## Opt-in HTTP stream version negotiation (2026-11-01)

Публичные `negotiateSsrStreamVersion` и `assertSsrStreamResponse`, а также
константы Svet request/response headers/content-type добавлены в общий
protocol module и package index. Server helper принимает bounded список
positive integer versions (16 entries/256 chars), выбирает v1; отсутствие,
malformed или несовместимый offer возвращает undefined. Client helper
проверяет ok, явный version1 и NDJSON content type до чтения body.
Это application protocol; hosts должны явно включить policy в route,
задать Vary/status406 и сохранять остальные Vary fields. Старые routes
автоматически не меняются; full HTML fallback является решением host.

Native HTTP + JSDOM integration7 checks: missing/version2/malformed дают406
и renderCalls0; wrong response version/type отклоняются с неизменным DOM;
offers1 и2,1 дают полный render/resume. Raw hashes:
`benchmarks/stream-http-negotiation-2026-11-01.json`. Проверка включена в
installed-package gate. JS1247/89 passed за8.33с, TypeScript/package build
passed. README содержит server/client usage, cancellation на ошибке и CORS
ограничения. Mixed actual runtime versions в браузере, CORS/preflight,
capabilities negotiation и production host routes пока не квалифицированы.

Итоговая квалификация: installed-package9/9 с negotiation integration,
HTTP actions4/4, `git diff --check` passed. Журналы:
`benchmarks/stream-http-negotiation-package-2026-11-01.txt`,
`benchmarks/stream-http-negotiation-actions-2026-11-01.txt`,
`benchmarks/stream-http-negotiation-suite-2026-11-01.txt`.

## Stream boundary topology validation (2026-11-01)

Перед final HTML optimization обнаружен correctness defect: receiver принимал
пересекающиеся sibling ranges (start1,start2,end1,end2), а Number conversion
допускал aliases01/1e0. Регрессия до исправления упала: отчёт
`benchmarks/stream-ranges-regression-before-2026-11-01.txt`. Первый ошибочный
запуск из repo root поднял чужой Vite config; startup failure сохранён отдельно,
не считается воспроизведением дефекта.

Scanner теперь требует canonical positive decimal IDs и LIFO closure для
каждого DOM parent; duplicate/unclosed/foreign-parent markers отклоняются.
Shell fragment полностью валидируется до dispose старого resume state и
replaceChildren. Patch fragment проверяется до insert/remove; новые IDs не
могут дублировать границы вне удаляемого поддерева. Нормальная вложенность
и sibling ranges сохраняются. Invalid shell/patch регрессии проверяют
сохранение identity старого input. Installed async-components gate включает
реальный crossing-shell отказ без изменения DOM.

TypeScript/package build прошли, полный JS1250/89 за11.73с:
`benchmarks/stream-ranges-suite-qualified-2026-11-01.txt`. Ранний suite1249
после shell fix сохранён отдельно. Final HTML duplication пока не изменено;
ускорение или снижение bytes этой правкой не заявляется.

Итог: package9/9 и HTTP4/4 прошли; `git diff --check` passed. Журналы:
`benchmarks/stream-ranges-package-2026-11-01.txt`,
`benchmarks/stream-ranges-http-2026-11-01.txt`.

## Optional lossless HTML-reference transport codec (2026-11-01)

Добавлены public encoder/decoder и frame types, `svet-html-ref-v1`. Codec
использует только предыдущий HTML: если он целиком входит в новый HTML,
передаёт prefix/referenceSequence/suffix при меньшем encoded JSON. Иначе
полный v1 record. Получатель восстанавливает authoritative HTML побайтно
перед обычным receiver: renderer/resume/coherence semantics не меняются.
State хранит только один предыдущий HTML, закрывается на complete/error/dispose.
Decoder проверяет identity/sequence/base и bounds до конкатенации, затем
UTF8 decoded JSON bound. Невалидная session закрывается без восстановления.

Пять tests: Unicode/JSON roundtrip, малые/несвязанные HTML fallback, unknown/
stale/cross-stream references, byte bounds/dispose, independent sessions.
Первый suite имел parse error в новом test (лишняя скобка); исправлен,
исходный журнал сохранён. Итог JS1255/90 за8.06с, TypeScript/build:package passed.

Node async SSR integration: shell/patch/complete, большой escaped Unicode
fragment и model input. Plain520757 bytes → encoded260752 bytes; complete
260300→295 bytes. Декодированные records deepEqual оригиналам; plain и
codec receivers сохраняют identity input и оба возобновляют native model12.
Raw/hashes: `benchmarks/stream-codec-2026-11-01.json`. 100 codec roundtrips
сохраняют диагностические timings и один memory sample, без A/B speedup
или plateau утверждения. Integration включена в installed-package gate.

Codec opt-in, не auto-selection wire negotiation. Host обязан отдельно
согласовать формат; напрямую reference frames в legacy receiver не подавать.
README описывает explicit agreement, framing bound и disposal. Реальный
HTTP/browser codec, paired latency/memory/compression, general diffing и
снижение server final serialization CPU остаются открытыми.

Installed-package9/9 с codec integration и HTTP actions4/4 passed;
`git diff --check` passed. Журналы `benchmarks/stream-codec-package-2026-11-01.txt`,
`benchmarks/stream-codec-http-2026-11-01.txt`,
`benchmarks/stream-codec-suite-qualified-2026-11-01.txt`.

## HTTP codec agreement and installed browser qualification (2026-11-01)

`svet-stream-codec` request/response header, exported constant and
`negotiateSsrStreamCodec` added: missing offer→identity, explicit unsupported/
malformed→undefined, ref preferred when explicitly offered. Response validation
now defaults to identity and rejects unexpected encoded formats before body.
Callers selecting ref must pass `{codec}`. Fixture endpoint enforces codec
selection before creating state/SSR, returns Vary/version/codec headers and
encodes frames with finally disposal. Legacy raw fixture requests remain identity.

Native installed HTTP4 checks passed: encoded body disconnect and fresh recovery,
identity client rejects encoded response, unknown offer406 with no created
request state. Evidence `benchmarks/stream-codec-installed-http-2026-11-01.json`.
Browser fixture accepts query codec1, validates response headers and decodes
frames before receiver. Production installed Chrome1567/7 passed: shell pending,
patch, complete/input identity, model12, escaped text, server graph cleanup and
receiver cleanup. Encoded bytes248/132403/301. Raw/screenshot:
`benchmarks/stream-codec-browser-2026-11-01.json` and corresponding jpg.
18 installed runtime JS files match workspace dist/package byte hashes; all
production assets checked against metadata. Qualification:
`benchmarks/stream-codec-browser-qualification-2026-11-01.json`.
Archive SHA f2aff8ba9a61ac1ec7e8c13581b2852f2e2c25771da570824ec9a8d2aad114a9.

JS1256/90, TypeScript/build:package and package9/9 passed. CORS/preflight,
malformed codec frames through native browser, real mixed old/new deployments,
proxy/compression and paired performance remain open. No general latency/RSS
improvement claimed.

HTTP actions4/4 и `git diff --check` passed; журнал
`benchmarks/stream-codec-negotiation-actions-2026-11-01.txt`. Временные
browser tab/server закрыты, consumer/evidence сохранены.

## Isolated paired transport/codec/gzip cost (2026-11-01)

`scripts/bench-ssr-stream-codec.mjs` генерирует три deterministic frozen
wire-v1 workloads: small, repeated Unicode, varied pseudorandom HTML.
Четыре раунда на workload, чередование plain/codec и gzip/codec-gzip плюс
A/A plain в каждом раунде. Каждый trial в свежем Node process, 5 warmup +
12 measured roundtrips, deepEqual restored records outside timing. Всего72
processes/864 measured roundtrips; measured operation: serialize, codec,
optional level6 gzip/gunzip, parse/decode. Нет renderer/DOM/network time.

Во время первого прогона найден лишний state.size(chunk) в encoder после
canonical validation. Уже вычисленный byte count переиспользуется; остальные
проверки и bounds сохранены. Первый raw сохранён, итоговый повтор отдельный.

| Workload | Plain bytes | Codec bytes | Gzip bytes | Codec+gzip bytes |
| --- | ---: | ---: | ---: | ---: |
| small | 354 | 354 | 170 | 170 |
| repeated | 192344 | 96421 | 556 | 454 |
| varied | 134206 | 67352 | 110179 | 55267 |

Итоговые paired processing costs: small codec/plain+132…177%, без bytes
экономии; repeated codec/plain+20…57%, codec-gzip/gzip+19…25% при102 bytes
экономии поверх gzip. Varied codec/plain+91…98%, но codec-gzip/gzip−26…−31%
при почти двукратном bytes сокращении. Varied A/A−0.9…+0.7%. Codec не
является универсальной CPU оптимизацией и остаётся explicit opt-in.

MemoryBefore/postWarmup/postGc и sampled heap/RSS maxima сохранены по trial.
Это samples, не peak allocation accounting/plateau; Node allocator/process
variation исключает вывод об общем memory budget. Gzip буферизует весь
NDJSON response: это не progressive compression с flush/backpressure и
не измерение HTTP latency. Отдельный before/after run не считается paired
доказательством ускорения от удаления лишней сериализации.

Raw: `benchmarks/stream-codec-paired-qualified-2026-11-01.json`; независимый
анализ с checksums, порядком, counts и A/A: `scripts/analyze-stream-codec-bench.py`,
`benchmarks/stream-codec-paired-qualified-analysis-2026-11-01.json`.
Baseline до удаления лишнего прохода сохранён в stream-codec-paired без qualified.
Suite1256/90 за8.05с passed; syntax/diff checks passed.

Итог: TypeScript, package9/9 и HTTP4/4 passed. Журналы
`benchmarks/stream-codec-cost-package-2026-11-01.txt`,
`benchmarks/stream-codec-cost-http-2026-11-01.txt`,
`benchmarks/stream-codec-cost-suite-2026-11-01.txt`. `git diff --check` passed.

## Progressive gzip transport qualification (2026-11-01)

Fixture host поддерживает explicit query gzip1: Node createGzip + pipeline
с downstream response, write/drain на compressor input и Z_SYNC_FLUSH после
каждого frame. Pipe распространяет output backpressure/close; при ошибке
encoder/compressor уничтожаются в finally, abort закрывает SSR request.
Gzip является transport опцией fixture, не новым default runtime и не
автоматическим Accept-Encoding negotiation. Vary codec сохраняется.

Native HTTP gzip identity4 checks и codec-gzip5 checks прошли: shell
декодируется до component release, все3 frames flush, body disconnect
освобождает graph/cleanup и fresh stream работает. Оба пути включены в
installed-package gate; package9/9 passed. Raw:
`benchmarks/stream-progressive-gzip-2026-11-01.json`,
`benchmarks/stream-progressive-codec-gzip-2026-11-01.json`; journal
`benchmarks/stream-progressive-gzip-package-2026-11-01.txt`.

Installed production Chrome156 gzip+codec7/7 passed: native fetch gzip
decoding, early shell, decoded patches, preserved input identity, model12,
escaped text и teardown. Server:3 flushes,1 compressor drain,715 compressed
body bytes, nodes0/cleanup1. Raw/screenshot:
`benchmarks/stream-progressive-gzip-browser-2026-11-01.json` и jpg.
18 runtime JS совпали с workspace dist/package, production assets hashes
проверены: `benchmarks/stream-progressive-gzip-qualification-2026-11-01.json`.

Один compressor input drain не доказывает TCP saturation. Large varied data,
abort во время активного compressed write/flush, proxy/CDN, compression
negotiation/CORS и многочасовой memory bound остаются открытыми. Runtime
не менялся; новые проверки находятся в scripts/installed consumer/browser.

## Large gzip active-write cancellation (2026-11-01)

Fixture payload override ограничен1…4MiB; varied1 генерирует deterministic
ASCII text. Stats показывают phase, pipelineSettled и compressorClosed.
Host AbortSignal теперь явно уничтожает compressor/HTTP response; listener
удаляется в finally, encoder history очищается. Это закрывает transport wait
при host abort независимо от следующего iterator read.

`scripts/check-ssr-stream-gzip-abort.mjs` проверяет четыре native HTTP cases:
identity/ref codec × downstream disconnect/explicit host abort. После early
shell async component release запускает4MiB patch. Тест ждёт реальную
phase drain/flush, nodes>0, только1 завершённый flush и compressedBytes>4096,
то есть компрессор уже произвёл output beyond shell. В квалифицированном
прогоне все четыре отмены произошли в drain, не в flush. После отмены
обязательны finished/failed, pipelineSettled, compressorClosed, nodes0 и
cleanup1. Новый gzip stream затем завершается с3 flushes и sequences0/1/2.

Raw: `benchmarks/stream-gzip-active-abort-qualified-2026-11-01.json`.
Ранний успешный прогон с более слабым output predicate сохранён отдельно.
Новая проверка включена в installed-package gate. Это Node loopback
active compressor-write qualification; pending-flush cancellation, kernel
socket saturation, browser/proxy и длительное memory plateau остаются
открытыми. Runtime не менялся; browser happy path предыдущего этапа
относится к сохранённому archive/fixture snapshot, не к новой версии fixture.

Итог: installed-package9/9 с strengthened active-output predicate, HTTP
actions4/4, syntax/diff checks passed. Независимо сверены4 cases и hashes
script/fixture. Журналы `benchmarks/stream-gzip-active-abort-package-qualified-2026-11-01.txt`
и `benchmarks/stream-gzip-active-abort-actions-2026-11-01.txt`.
Полный runtime suite не повторялся: изменения только fixture/harness/docs.

## Pending native gzip flush + repeated recovery (2026-11-01)

В fixture добавлена узкая fault injection `abort-flush=patch`: после
настоящего native compressor.flush(Z_SYNC_FLUSH) и до его callback записываются
pending/phase/nodes/flushes, затем host AbortSignal отменяет request/transport.
Это deterministic host-abort timing, не remotely timed client race и не fake
compressor. Обычные requests этот hook не используют.

Новый `scripts/check-ssr-stream-gzip-flush.mjs`: шесть abort/recovery cycles
для каждого identity/ref codec. Все12 injected aborts пришлись на pending
patch flush, nodes>0, только shell flush завершён. После каждой отмены
finished/failed, pipelineSettled, compressorClosed, nodes0/cleanup1. Все12
следующих gzip requests успешно декодированы, sequence0/1/2 и3 flushes.
Новый deadlock не воспроизведён; менять runtime на основании этого probe
не потребовалось.

Raw: `benchmarks/stream-gzip-pending-flush-qualified-2026-11-01.json`.
Независимая проверка12+12 phases/counts/teardown и script/fixture hashes:
`benchmarks/stream-gzip-pending-flush-analysis-2026-11-01.json`. Первый
успешный probe сохранён как pending-flush-before (не воспроизведение дефекта).
Проверка включена в installed-package gate; package9/9 passed.
Kernel saturation, browser/proxy timing, full compression negotiation и
многочасовое plateau остаются открытыми. Runtime suite не повторялся:
изменения ограничены fixture/harness/package gate.

HTTP actions4/4, syntax и `git diff --check` passed. Журналы:
`benchmarks/stream-gzip-pending-flush-package-2026-11-01.txt`,
`benchmarks/stream-gzip-pending-flush-actions-2026-11-01.txt`.

## Node SSR → Web ReadableStream adapter (2026-11-01)

Public server export `renderToReadableStreamAsync(view, options)` возвращает
ReadableStream<Uint8Array> с UTF8 NDJSON и optional explicitly selected codec.
HighWaterMark0 запрещает eager pull; iterator запускается при первом read.
Cancel закрывает pending next и request; external AbortSignal/calling owner
ошибочно завершает stream и освобождает ownership. Terminal adapter очищает
ссылку на iterator, signal listeners, caller cleanup hook и codec history.
View находится за отдельным helper closure. Compression/headers/HTTP policy
остаются у host. Это Node entry, не Edge ALS replacement.

7 regressions: lazy pull/one frame, cancel-before-first, pending read cancel,
external abort identity/closed without read, owner disposal, encoded references,
producer errors/invalid codec. JS1263/91 за9.52с, TypeScript/package build passed.
Raw suite: `benchmarks/readable-ssr-suite-2026-11-01.txt`.

Native HTTP4 scenarios через new Response(body), Readable.fromWeb и pipeline:
identity/reference × complete/disconnect. Ранний shell наблюдается при живом
graph, full decode совпадает с final HTML, cancelled requests graph0/cleanup1.
Raw: `benchmarks/readable-ssr-http-qualified-2026-11-01.json`. Первый probe
ошибочно ожидал nodes>0 у fixture без signals; добавлен pending signal, исходный
assertion failure сохранён отдельно. Проверка включена в installed-package gate.

Readable stream retention/churn, immediate producer failure while no read is
pending, Edge/browser async ownership, actual browser Response host и длительное
streaming memory plateau остаются открытыми.

Итог: installed-package9/9 с Readable/Response HTTP probe, HTTP actions4/4,
`git diff --check` passed. Журналы `benchmarks/readable-ssr-package-2026-11-01.txt`
и `benchmarks/readable-ssr-actions-2026-11-01.txt`.

## Readable terminal notification + retained-handle GC (2026-11-01)

Две регрессии воспроизведены до fix: background component failure после shell
не завершал reader.closed без нового read; complete frame тоже требовал
дополнительного consumer read для закрытия stream. До исправления2 tests
failed/7 passed: `benchmarks/readable-ssr-background-regression-before-2026-11-01.txt`.

Internal chunk stream теперь уведомляет readable adapter об ошибке при
request retirement. Public iterator API не меняется; callback latch не
публикует вторичную consumer-close ошибку вместо первичной причины. Adapter
создаёт iterator через отдельный одноразовый factory closure, удаляет factory
сразу после construction. После enqueue complete внутренний terminal next
ожидает teardown и закрывает stream без дополнительного consumer demand.
Никакой следующей data frame этот read не производит.

Suite1265/91 за8.72с passed, TypeScript/package build passed.
`measure-readable-ssr-retention.mjs`:64 requests, identity/ref, complete/cancel/
abort/background-error; сохранены stream+reader handles. Все384 WeakRef
объекта (capture/view/core/memory/signal/node) освобождены при сохранённых
handles, контроль после release также0. Native cache вручную не продвигался.
Raw/hash evidence: `benchmarks/readable-ssr-retention-qualified-2026-11-01.json`.
Probe включён в installed-package gate с5сек deadline на request.

Это bounded object collection, не throughput/RSS/WASM buffer peak или long
plateau. Error objects создаются вне request; произвольная application error
может содержать явные ссылки, и Web Stream обязан хранить исходную причину
для последующих read. Edge/browser async runtime всё ещё не реализован.

Итог: installed-package9/9 с retained-handle GC и Readable HTTP probes,
HTTP actions4/4, source/runtime hash verification и `git diff --check` passed.
Журналы `benchmarks/readable-ssr-background-package-2026-11-01.txt`,
`benchmarks/readable-ssr-background-actions-2026-11-01.txt`,
`benchmarks/readable-ssr-background-suite-2026-11-01.txt`.

## Concurrent Readable SSR isolation/churn soak (2026-11-01)

`scripts/soak-readable-ssr.mjs` держит16 одновременных isolated cores на
barrier после shell. Identity/ref codecs × complete/cancel/abort/background
error представлены в каждом batch. После отмены половины streams проверяется,
что только они graph0, а оставшиеся cores живы. Success проверяет собственный
signal ID после await и собственный final HTML/sequence; error наблюдается
через closed без нового read. Каждый request требует cleanup1/nodes0.

После batch удерживаются16 terminal stream/reader pairs; WeakRef probes
на capture/view/core/memory/signal/node должны освободиться до следующего
batch. Журнал ограничен32 samples и4 quarter aggregates.

60.022с:64720 requests/4045 barriers, по16180 complete/cancel/abort/error.
Все388320 objects collected при сохранённых terminal handles. Post-GC heap
по четвертям MiB:38.01–38.98 /38.90–39.03 /37.66–39.04 /34.88–37.77;
RSS125.75–131.22 /124.80–132.72 /124.79–126.84 /123.66–126.51.
Это конкретная минутная Node нагрузка; не universal/multi-hour memory bound
или HTTP/browser/Edge throughput. Произвольный user error может явно
содержать сохраняемый graph; controlled errors созданы вне request.

Raw: `benchmarks/readable-concurrent-soak-60s-2026-11-01.json`. Независимо
проверены sums/16-way barriers/6 probes per request, samples и source/runtime
hashes: `benchmarks/readable-concurrent-soak-analysis-2026-11-01.json`.
Короткий1000мс variant включён в installed-package gate; package9/9 passed.
Syntax и `git diff --check` passed. Runtime не менялся; JS suite не повторялся.

## Host-neutral SSR runtime/carrier contract (2026-11-01)

SSR implementation moved to `src/engine/ssr-runtime.ts` without Node imports.
`server.ts` is now a thin Node ALS/UUID host adapter preserving public function
exports and option/view types. New public `svet-engine/server-runtime` export
provides createSsrRuntime(host), carrier interfaces and bounded async assertion.
Host creates independent request/owner/render carriers with getStore/run/exit
and supplies unique IDs; DOM remains host-provided. Singleton installation
prevents a second factory from overwriting engine hooks; invalid carrier shape
or shared carrier instances fail before hook installation.

Carrier assertion checks native await, two concurrent contexts, async exit,
nested rejection restoration and caller restoration. Real Node ALS passes;
synchronous stack fails. Injected host source test renders4 concurrent async
subtrees in separate cores with correct values and cleanup. Full suite1268/92
passed12.40с, TypeScript/package build passed. Source/runtime contract changes
are tested through existing stream/readable/retention/HTTP package regressions.

Installed package probe resolves the actual import-only public export from
a consumer ESM bridge, qualifies native ALS, rejects stack and runs8 concurrent
async renders via injected host. First package attempt used CommonJS resolver
on an import-only export and failed; harness corrected to actual ESM import.
Failure preserved in `benchmarks/ssr-carrier-package-2026-11-01.txt`.
Qualified gate passed9/9: `benchmarks/ssr-carrier-package-qualified-2026-11-01.txt`.
Browser build includes a direct public server-runtime import and rejects
node:async_hooks/node:crypto in emitted JS. This is build qualification, not
a live browser execution of custom async SSR.

No Edge/browser async-context adapter is claimed: the carrier assertion is a
bounded probe, not a universal guarantee. Host must preserve native await and
concurrency; DOM compatibility/global lifetime and real deployment validation
remain necessary. Node memory/HTTP tests do not qualify other hosts.

Retained installed consumer: raw custom-host8 requests подтверждён и сохранён
в `benchmarks/ssr-carrier-installed-host-2026-11-01.json`;19 runtime files
сверены с workspace dist/package. Aggregate:
`benchmarks/ssr-carrier-qualification-2026-11-01.json`. Package archive SHA
e5307f71d43c650a9e78c02ecede465c704702be30cc2fdbe46e47b36238eae9.
Final package9/9, HTTP actions4/4, `git diff --check` passed; logs
`benchmarks/ssr-carrier-package-retained-2026-11-01.txt`,
`benchmarks/ssr-carrier-actions-2026-11-01.txt`.

## Initial real workerd host + precompiled WASM (2026-11-01)

Temporary tooling pinned: Miniflare4.20260730.0/workerd1.20260730.1,
LinkeDOM0.18.15, esbuild0.28.2; project dependencies/lockfiles unchanged.
`tests/edge/worker.mjs` uses workerd native node:async_hooks ALS, shared
server-runtime host contract and real LinkeDOM. Carrier probe passed, then
actual workerd refused embedded-byte compilation: CompileError/Wasm code
generation disallowed by embedder. Raw failure:
`benchmarks/edge-ssr-wasm-before-2026-11-01.json`. Earlier startup path error
was separate Miniflare modulesRoot configuration, not an engine failure; log
`benchmarks/edge-ssr-before-2026-11-01.txt` preserved.

Added public installSvetWasmModule and ./core.wasm asset/types export.
Installation checks native module/import/export shape against a generated
export manifest, before bootstrap; different module identity cannot replace
an already selected module. Identical module object is idempotent. Full ABI
equivalence is not inferred from export names; host must use the same-build
asset. build:package emits the asset from embedded bytes, not a second compile.
Node/browser default byte-compilation path remains available.

Real workerd CompiledWasm binding fixes the failure. Two concurrent fetch
events ×8 renders: own values0…7 and100…107,8 unique cores/event, cleanup8
and graph0 per event. Runner supports installed public package exports;
optional SVET_EDGE_DEPS activates it inside package gate. Reports include
actual tool versions, bundle/worker source/WASM hashes. No Cloudflare account
access or remote deployment performed.

Native Rust53 passed; production build with isolated CARGO_TARGET_DIR passed;
JS1269/93 passed8.18с; TypeScript passed. Packaged/embedded/native freshly built
WASM all54320 bytes, SHA9aba5a7f785fff76022772c6a357dfb3d38cc479960bd2f52db6631b9da07340.
Precompiled-module regression checks invalid shape, isolated instances, signal
updates and late replacement rejection.

Also fixed embedding path: old script hardcoded crate/target and ignored
CARGO_TARGET_DIR. Three isolated script tests cover default/absolute/relative
target with different valid WASM inputs; all3 passed and enter package gate.
Raw `benchmarks/edge-embed-target-2026-11-01.txt`. The production build and
byte hash comparison verify the real /tmp target artifact is embedded.

Local installed workerd success does not qualify Edge streaming/cancel/error
paths, full LinkeDOM fidelity, CORS, other Edge hosts, native browser carrier,
long churn or deployed Worker limits. These remain open.

Final qualification: optional installed Edge package gate9/9 passed with
SVET_EDGE_DEPS; raw copied to `benchmarks/edge-ssr-installed-final-2026-11-01.json`.
Native /tmp target, embedded bytes and installed ./core.wasm are identical;
19 installed JS files match workspace outputs. Aggregate:
`benchmarks/edge-precompiled-qualification-final-2026-11-01.json`. Archive SHA
a36503b69800714bf30b60a6166a7952d2c58943cca662722764ffeffad3dba1.
HTTP actions4/4 and `git diff --check` passed. Logs:
`benchmarks/edge-precompiled-package-final-2026-11-01.txt`,
`benchmarks/edge-precompiled-actions-final-2026-11-01.txt`,
`benchmarks/edge-precompiled-rust-2026-11-01.txt`,
`benchmarks/edge-precompiled-build-2026-11-01.txt`,
`benchmarks/edge-precompiled-suite-qualified-2026-11-01.txt`.

## Workerd streaming ownership + strict transport EOF (2026-11-01)

Worker fixture now runs8 concurrent streams per fetch event: identity/ref
×complete/cancel/abort/error. Two fetch events cover16 streams alongside
16 prior string renders. Each stream checks own core/value, shell pending,
ordered frames, UTF8 escaping and snapshot. Cancellation preserves neighbouring
graphs; error reaches closed without read; complete auto-closes. Retired scopes
refuse8 late signal creations across the two events. Cleanup is synchronized
with explicit onCleanup gates.

Real Response body tests exposed clean EOF without complete after a producer
error in the Miniflare/workerd path. Existing engine read-error-only assumption
was too weak for this transport. Added public receiver.finish(): require a
successfully applied complete at EOF; otherwise stop partial receiver and throw.
Completed bindings stay live, finish is idempotent after completion. Pre-shell
EOF preserves unrelated resumed bindings. Three JS regressions and installed
package checks added; browser fixture/reference example now call finish.

Body cancellation/normal socket destroy did not produce an incoming abort in
initial probes (pending graph remained). Preserved failures:
`benchmarks/edge-stream-transport-2026-11-01.json`,
`benchmarks/edge-stream-transport-eof-2026-11-01.json`,
`benchmarks/edge-stream-request-signal-2026-11-01.json`,
`benchmarks/edge-stream-direct-signal-2026-11-01.json`,
`benchmarks/edge-stream-raw-disconnect-2026-11-01.json`.
These paths remain unqualified; they were not relabelled as passing.

Direct loopback workerd listener +enable_request_signal +forwarded request.signal
+actual TCP resetAndDestroy passed both codecs: signalAborted true, retiredPhase
component-pending, cleaned1/nodes0. Direct response complete/error/reset matrix
is6 cases, plus2 explicit EOF-error observations. All successful bodies decode
and resume through Node/JSDOM receiver, validate finish and leave client graph0.
Read error observations currently report clean-eof-without-complete; both rejected.
The gate does not claim proxy Reader.cancel/FIN cancellation equivalence.

Installed raw: `benchmarks/edge-stream-installed-qualified-2026-11-01.json`;
independent counts/hash verification:
`benchmarks/edge-stream-qualification-2026-11-01.json`. JS1272/93 passed8.40с;
TypeScript and optional Edge package9/9 passed. Runtime implementation changes
are limited to client EOF validation; native Worker tests reuse shared SSR core.
Remote deployment, general proxy cancellation, full browser EOF matrix, Edge
churn/limits and DOM compatibility remain open.

Final HTTP actions4/4 and `git diff --check` passed. Logs:
`benchmarks/edge-stream-eof-suite-2026-11-01.txt`,
`benchmarks/edge-stream-eof-package-2026-11-01.txt`,
`benchmarks/edge-stream-eof-actions-2026-11-01.txt`. Installed runtime19 JS
hashes match workspace. Archive SHA22d0063aec00d94d980415a1fab9ed888384ac52e8935cd74bb9f57583db578f.

## Proxy explicit abort and bounded host deadline (2026-11-01)

Explicit AbortController passed to Miniflare dispatchFetch was tested separately
from reader.cancel. It still did not deliver incoming abort to the Worker:
pending component/graph remained. Preserved negative evidence:
`benchmarks/edge-proxy-explicit-abort-2026-11-01.json`. No proxy propagation
fix is claimed.

Fixture host now combines incoming request.signal with an explicit optional
deadline controller. Test requests choose500ms while the component is pending
for5000ms. Deadline closes SSR before component completion; both identity/ref
show deadlineAborted true, signalAborted false, cleanup1/nodes0 and retiredPhase
component-pending. Timers/listeners are cleared on abort or successful cleanup;
already-aborted incoming signal is forwarded immediately. This policy remains
fixture/host opt-in, not a global Svet timeout.

Raw: `benchmarks/edge-proxy-deadline-qualified-2026-11-01.json`; independent
source hash/state checks: `benchmarks/edge-proxy-deadline-analysis-2026-11-01.json`.
Optional Edge package gate includes both proxy deadline cases. Runtime library
unchanged; this is host policy/qualification, not a new runtime API.

Final optional Edge package9/9 passed, syntax/source-state checks and
`git diff --check` passed. Log `benchmarks/edge-proxy-deadline-package-final-2026-11-01.txt`.
Full JS/Rust suites were not repeated because runtime code did not change.

## Shared SSR timeout API (2026-11-01)

Added optional timeoutMs to SsrRequestOptions and inherited stream/readable
options: integer1…2147483647, undefined by default. Public SsrTimeoutError
records timeoutMs and is exported by Node/server-runtime entries. Common
deadline signal composes caller abort, preserves its reason when first, and
releases timer/listener on abort/settlement. Isolated requests own deadline
cleanup in finally; child scope releases even on setup failure. String renders
forward timeout into isolated request; chunks retain it through backpressure
and Web streams forward it to the chunk lifetime. Stream timer begins during
construction, even before first read. Synchronous CPU work is not preempted.

4 new regressions: uncooperative render cleanup/graph0, pending chunks and
unread Web stream error, caller reason identity and zero success/cancel timers
(fake timers), bounded child/parent survival and invalid values. JS1276/94
passed8.36с, TypeScript/build:package passed. Actual workerd Web stream timeout
added to both concurrent fetch events: SsrTimeoutError10, cleanup1/graph0 without
second read. Raw: `benchmarks/edge-runtime-timeout-2026-11-01.json`.

Optional Edge installed package9/9 passed; native/receiver/churn regressions
remain enabled. Runtime deadline API does not fix proxy signal forwarding.
Timeout-specific retained-error/parent-core GC and long concurrent deadline
churn remain to be qualified.

Final HTTP actions4/4 and `git diff --check` passed. Logs:
`benchmarks/ssr-timeout-suite-2026-11-01.txt`,
`benchmarks/ssr-timeout-package-2026-11-01.txt`,
`benchmarks/ssr-timeout-actions-2026-11-01.txt`.

## Retained timeout diagnostics and final qualification (2026-11-01)

Node GC reproduced retained parent cores/WASM memories through saved child
SsrTimeoutError objects: 8 cores and 8 memories remained until errors were
released. Moving error construction before the timer did not fix it. Both
negative reports remain in ssr-timeout-retention-before/after-2026-11-01.json.
Snapshotting the lazy stack to a primitive string removes the observed retention;
the exact heap retainer path was not separately captured. The final implementation
constructs diagnostics only at expiration and catches formatter failure, falling
back to name/message. Successful deadlines avoid error/stack construction.

Added regression for an actually expired render with throwing prepareStackTrace:
typed timeout preserved, cleanup1 and graph0. The pre-aborted caller regression
also remains. Final suite1278/94 passed9.13s, TypeScript passed, build/package
with optional local workerd passed, HTTP actions4/4 and diff check passed.
Package gate includes the retained-error probe against the installed archive.
Final native probe: 32 concurrent string/chunk/readable/child operations,
96 objects collected while errors/terminal handles remain held; release control
also0. It does not qualify long deadline churn, RSS plateau or Edge/browser GC.

Evidence: benchmarks/ssr-timeout-retention-final-2026-11-01.json,
benchmarks/ssr-timeout-final-suite-2026-11-01.txt,
benchmarks/ssr-timeout-final-package-2026-11-01.txt,
benchmarks/ssr-timeout-final-actions-2026-11-01.txt.

## Concurrent deadline churn (2026-11-01)

Added scripts/soak-ssr-deadlines.mjs: configurable1s…1h duration,16-request
barriers, identity/ref codecs, complete/cancel/caller-abort/timeout modes.
Every batch first proves16 distinct live cores and own shell IDs. Cancelling
half must leave the other cores alive. Success checks exact frame sequence and
own final HTML; timeout checks typed error/configured100ms; caller abort keeps
its reason. Every operation checks cleanup1, graph0 and explicit removal of
caller abort listeners. Retained terminal readers/streams/timeout errors remain
held through GC; six request objects per operation must collect.

60s qualification passed:8336 requests,2084 each mode,50016 collected objects.
Post-GC heap samples: first window35.96…40.22MB, last36.57…36.69MB;
RSS first130.61…133.14MB, last126.47…127.63MB (decimal MB). These are observed
samples with forced GC and one retained batch, not a proof of long plateau,
production throughput or accumulated arbitrary error retention. Child deadline
churn, direct timer accounting, HTTP/Edge/browser and multi-hour bounds remain open.

Raw: benchmarks/ssr-deadline-churn-60s-2026-11-01.json includes source/runtime
SHA256, mode counts,32-sample ring and four memory windows. A1s run against the
installed archive is now in check-package.mjs. Runtime implementation unchanged;
full JS suite was not repeated for this harness-only change.

Updated installed-package gate passed (exit0), including1s deadline churn. Log:
benchmarks/ssr-deadline-churn-package-2026-11-01.txt. Optional Edge was not
repeated for this harness-only change; previous runtime qualification remains
separate. Syntax and git diff --check passed.

## Child deadline isolation and timer accounting (2026-11-01)

Added soak-child-deadlines.mjs with eight concurrent isolated parents, each
running success/caller-abort/timeout/rejection siblings. It verifies reason
identity, typed timeout, live parent signal updates after child termination,
successful sibling ownership, cleanup exactly once after parent teardown, graph0,
and refusal of signal creation in late aborted/timed-out continuations.
Saved timeout errors remain held while parent core/memory/signals are GC-probed.
A temporary wrapper tracks process setTimeout/clearTimeout handles, removes
expired handles before dispatch and restores globals in finally. Every batch
requires zero remaining timers; all assertion failures propagate outside cleanup.

60s passed:21832 parents,87328 children,43664 late refusals,152824 collected
objects; active timers0, peak25. Raw child-deadline-churn-60s-2026-11-01.json
includes runtime/source hashes and a32-sample memory ring. No throughput,
long plateau, HTTP, browser or Edge claim is inferred.

Added the same timer tracking to the Web-stream deadline harness; separate1s
probe passed160 operations/960 objects with timers0. Previous60s Web-stream raw
is retained with its original source hash and has no direct timer accounting.
Both1s harnesses now run against the isolated installed archive in check-package.
Runtime source unchanged, so full JS/Rust suites were not repeated.
Raw: benchmarks/ssr-deadline-timers-2026-11-01.json.

Updated installed-package gate passed (exit0), including both instrumented
deadline probes. Log: benchmarks/child-deadline-package-2026-11-01.txt.
Optional Edge was not repeated for this harness-only change. Syntax and
git diff --check passed.

## Workerd DOM lifecycle matrix (2026-11-01)

Extended the real workerd fixture with16 DOM render cycles per fetch event,
run in two concurrent events. Covers keyed For identity and index updates,
removed-row teardown, Show branch replacement, SVG namespace, escaped text,
and custom event listener removal. Each cycle verifies one root cleanup, four
row cleanups and graph0. Total32 cycles/64 identity-preserving row moves,
32 removed rows,128 retired row scopes and32 event teardown checks.
The Edge driver requires the matrix counters, preventing silent omission.

Initial raw passed before the event case; final raw includes event checks:
benchmarks/edge-dom-lifecycle-qualified-2026-11-01.json (fixture/bundle/WASM
hashes, pinned tool versions and transport cases). Installed optional Edge
package gate passed exit0: benchmarks/edge-dom-lifecycle-package-2026-11-01.txt.
Runtime library unchanged; JS/Rust suites not repeated for fixture-only edits.
Syntax and diff checks passed. This qualifies local LinkeDOM SSR semantics,
not real browser layout/events, GC, arbitrary DOM APIs, a deployed provider
or long-duration churn. Those remain separate roadmap items.

## Portable single-select model synchronization (2026-11-01)

Expanded actual workerd DOM qualification to text/checkbox/textarea/select
models. Both fetch events reproduced a TypeError assigning LinkeDOM's getter-only
select.value. Preserved negative raw edge-forms-initial-2026-11-01.json.
Single-select model now snapshots options, locates the first matching value,
clears prior selections then sets the matching option. No match clears native
selectedIndex. Lifetime guards surround host reads/writes. Multiple-select code
is unchanged; its host semantics remain unqualified.

Added model-select-host.test.ts: a getter-only select on JSDOM must support
first-match duplicate values, unmatched clearing and change-event roundtrip,
then release its graph. Final workerd form probe passes four controls in each
of two concurrent events, reactive property/HTML updates and cleanup1/graph0.
Raw edge-forms-after-2026-11-01.json; Edge driver now requires form counters.

Full suite1279/95 passed9.21s, TypeScript passed, optional Edge installed-package
gate passed exit0. Logs benchmarks/edge-forms-suite-2026-11-01.txt and
benchmarks/edge-forms-package-2026-11-01.txt. No Rust changes. Browser GC,
multiple-select/reset/validation and deployed Edge environments remain open.

Final HTTP actions4/4 and git diff --check passed. Log:
benchmarks/edge-forms-actions-2026-11-01.txt.

## One-way select initialization (2026-11-01)

Two native regressions reproduced lost initial static/reactive select value:
options were appended after value assignment. Negative log:
benchmarks/select-value-before-2026-11-01.txt. Extracted shared
synchronizeSelectValue for model/value bindings, retaining lifetime guards
and avoiding writes when selection is already coherent. Select value bindings
register a post-children synchronizer, removed on owner cleanup with map identity
protection. They remain one-way: DOM change never publishes to source.
Workerd form matrix now covers6 controls/event, initial static b and reactive
a→b. Raw benchmarks/select-value-edge-2026-11-01.json passed.

Suite1281/96 passed8.72s, TypeScript passed, installed optional Edge package
passed exit0; actions4/4 and diff check passed. Logs select-value-final-suite,
select-value-package and select-value-actions-2026-11-01.txt. An extra raw run
collided with actions rebuilding dist/package; ENOENT evidence preserved in
select-value-edge-build-race-2026-11-01.txt. Sequential rerun passed.
No Rust changes. Late option insertion for one-way value, multiple select/reset
host qualification and browser GC remain open.

## Late one-way select options (2026-11-01)

Native tests reproduced stale selection on option mutations; negative log
benchmarks/select-options-before-2026-11-01.txt. Added a lifetime-bound
MutationObserver for one-way select value. It watches child/text/value changes,
uses the last resolved string rather than rerunning the user accessor, restores
captured core/owner and runs untracked. Selection writes are coherent-guarded;
selected attributes are outside the observer filter. Cleanup disconnects and
unlinks synchronizers even if host disconnect throws; callbacks refuse disposed
owners and stale graph generations.

Four regressions cover insert/remove/reorder, implicit text/explicit option
values with one accessor read, unmount and graph-reset queued work. Full
suite1285/96 passed8.82s; TypeScript and optional Edge installed-package passed.
Workerd now verifies7 controls/event including an explicit late option after
await. Raw benchmarks/select-options-edge-2026-11-01.json. Logs:
select-options-suite/select-options-package-2026-11-01.txt. No Rust changes.
This does not prove browser GC, multiple-select/reset host semantics, or DOM
hosts without MutationObserver. All raw failures remain preserved.

Final HTTP actions4/4 and git diff --check passed. Log:
benchmarks/select-options-actions-2026-11-01.txt.

## Browser WeakRef retention qualification (2026-11-01)

Added native-browser retention-gc.html and loopback serve-retention-gc.mjs,
using current production ESM files. UI controls drive allocation, pressure,
inspection and release; local POST saves raw phases and module/fixture hashes.
32 terminal receiver containers (four disposal modes) and16 one-way select
source signals collect while their public receiver handles/detached select DOM
remain held. Four positive control DOM objects survive held phase, then all
collect after release. Graph0; Chrome156/IAB. Two complete runs passed.

CDP collectGarbage was rejected by the browser API. Natural collection under
bounded task-separated allocation pressure was observed instead, with later-task
WeakRef checks and positive controls. Initial result retained separately with
old scope wording; final fixture/report explicitly identify actual mechanism.
No SSR request-core/browser async carrier/Edge GC/RSS plateau claim is made.

Final raw: benchmarks/browser-retention-gc-2026-11-01.json; screenshot same stem.png.
Saved fixture/index/dom/stream hashes match current files. Runtime unchanged;
full unit/Edge suites not repeated for browser harness-only changes. Temporary
tab/server closed. Browser fixture itself was run twice; source syntax and
git diff --check passed.

## Edge multiple-select host reflection (2026-11-01)

Reproduced silent lost selection in both workerd events. LinkeDOM does not
reflect multiple and its selected setter cannot represent several options.
Added isMultipleSelect attribute fallback when no boolean reflection exists;
attribute-backed hosts use selected attributes, standard reflected hosts retain
native option.selected writes. Selected values are read from filtered options
for change/reset paths, so selectedOptions reflection is no longer required.
Lifetime guards remain around host reads/writes. Native form-reset semantics
for the attribute-backed host itself are still unqualified.

New native fake-host regression covers omitted multiple/selectedOptions,
unsupported selected setter, initial a,c→b update, DOM change roundtrip and
empty selection. Existing native multiple/reset tests remain passing. Actual
workerd matrix now8 controls/event, initial a,c→reactive b→change b,c and final
HTML with both selected attributes, cleanup1/graph0. Negative/final raws:
benchmarks/edge-multiple-before/after-2026-11-01.json.

Final suite1286/97 passed9.29s, TypeScript passed, installed package with
optional Edge passed exit0. Logs edge-multiple-final-suite/edge-multiple-package-
2026-11-01.txt. No Rust changes. Native host reset/validation, deployed providers
and Edge/request-core GC remain separate work.

Final HTTP actions4/4 and git diff --check passed. Log:
benchmarks/edge-multiple-actions-2026-11-01.txt.

## Open Edge concurrency failure exposed by expanded churn (2026-11-01)

Added optional SVET_EDGE_CHURN_MS (0…1h) to the real workerd runner, eight
concurrent full fetch probes per barrier. Every event must pass DOM/forms and
worker-internal stream/string/timeout checks. GC capability reporting confirms
WeakRef/FinalizationRegistry exist; no Edge GC measurement has yet been made.

The ordinary two-event baseline passes, but eight concurrent events fail with
workerd/Miniflare HTTP500 `Promise will never complete`. Bounded string-only
per-trace progress localizes the wait to timeout reader.closed after shell.
Non-JSON errors now preserve response status/body and phases instead of hiding
it behind a JSON parse failure. Negative stages/current reports retained.

Minimal native timer→AbortSignal→ReadableStream.error→reader.closed control,
without Svet/DOM/ALS, passes8/8 both with and without a timer-promise watchdog.
In the composite probe an independent native watchdog timer-promise restores
progress, with typed10ms runtime timeout winning (100ms watchdog is not the
accepted error). This is a diagnostic, not a root cause or runtime fix.
It is opt-in query keepalive, exposed only by SVET_EDGE_KEEPALIVE=1 in churn.
Default fixture/runtime has no watchdog workaround.

Current negative: benchmarks/edge-churn-current-failure-2026-11-01.json.
Diagnostic positive: edge-churn-diagnostic-keepalive-2026-11-01.json,
136 events/2176 DOM cycles/1088 streams/1088 string renders/136 timeouts in1s;
not normal-path qualification or throughput evidence. Native control:
workerd-native-timer-closed-2026-11-01.json and its reproducible standalone
script check-workerd-timer-closed.mjs. Earlier failure artifacts preserved.

Installed optional Edge package gate passed exit0 for the existing two-event
baseline: edge-churn-baseline-package-2026-11-01.txt. Runtime source unchanged;
full JS/Rust suites not repeated. Syntax and git diff --check passed. New P0
is to isolate/fix eight-event timeout progress without keepalive, add regression
and only then run long Edge churn/GC. No broad Edge completion claim is made.

## Native stream retained until producer retirement (2026-11-01)

Fixed the eight-event idle reader.closed timeout failure by retaining the native
ReadableStream in the adapter lifetime until iterator.return/producer teardown
settles, clearing it on either outcome. No watchdog timer or eager next read was
added. Existing close/cancel/error paths and one-slot backpressure remain intact.
Controlled revert restores the failure on the same worker hash; restore passes.
Exact native heap/GC retainer path was not captured: lifetime sensitivity is
supported by A/B and collection tests, not a complete native heap trace.

Isolation evidence preserved: timers fire; abort propagates and controller.error
returns; direct socket also failed before fix. Native controls (direct, promise
hop, ALS, lazy pull and memory pressure) pass. Chunk timeout isolation passes.
Timer-idempotence, early notification, pull readiness, stack formatting, source
factory context and reader-lock experiments did not resolve it and were removed.
No speculative runtime modifications or debug globals remain. Minimal native
controls remain reproducible in check-workerd-timer-closed.mjs; raw failures kept.

Pinned workerd60s normal readable churn passed8 concurrent requests without
keepalive:1464 fetch events,23424 DOM cycles,11712 streams,11712 string renders,
1464 timeouts. Direct native HTTP5s passed416 events. Installed archive gate
passed112 events with normal readable transport/no diagnostics. Default driver
now runs1s; optional installed Edge gate requires8-event normal churn and rejects
keepalive/chunk opt-outs. Timeout probe helper returns only reader, preventing a
fixture-side stream reference from masking lifetime behavior.

Extended Node GC with16 discarded unstarted streams:432 total probes collected,
including objects from64 terminal requests while stream/reader pairs stay
held. Both workspace and installed archive probes passed. Request cores/views/
WASM/signals collect; no new unstarted-stream root is introduced by the lease.

Final suite1286/97 passed8.78s, TypeScript/build passed, optional installed Edge
package gate and HTTP actions4/4 passed. Logs edge-stream-lease-suite/package/
actions-2026-11-01.txt. Qualification JSON verifies identical worker hashes for
A/B, installed/workspace runtime identity, churn scope and GC counters:
benchmarks/edge-stream-lease-qualification-2026-11-01.json. Installed raw receipts
copied from the retained consumer; no commit/push/publish or Rust changes.

Latest workerd override2026-11-01 passes timeout churn after this fix, while its
full matrix still fails when controlled producer failure rejects fetch before a
Response. That separate host transport observation remains unqualified. Binary
override path/version are now recorded. No Edge GC, deployed-provider or
multi-hour plateau claim is inferred from this milestone.

## Closed request/render tokens release owned graphs (2026-11-01)

Added tests/edge/retention.mjs:24 concurrent readable requests, six lifecycle
paths (complete/cancel/caller abort/producer error/component timeout/view timeout),
both codecs, two repetitions. Tracks140 request objects and4 positive controls.
Terminal streams/readers/errors remain held through collection. Every allocated
graph is checked nodes0/cleanup1 before GC; positive controls distinguish absent
collection from retention. Initial pressure-only result was inconclusive.

Scoped workerd inspector utility discovers exactly one generated core:user:
target, uses bounded command/connect deadlines and closes on every exit.
Runtime.enable and getHeapUsage are available. Awaiting collectGarbage alone
initially timed out; dispatching the command separately and waking processing
with getHeapUsage yields acknowledgements. All GC command results are recorded.
The upstream [workerd report](https://github.com/cloudflare/workerd/issues/6824)
also discusses delayed GC acknowledgements; our proof relies on local command
traces/WeakRef controls, not assuming that a missing reply proves a leak.

Inspector collection exposed4 complete-mode cores/memories/views/captures even
after terminal handles were released. Strong heap paths were captured:

1. native AsyncContextFrame storage → closed RequestContext.core → WASM exports;
2. after clearing request.core, native storage → closed AsyncRenderSession.stream
   → publish closure → captured lifetime guard → core.

The snapshots change VM state and establish observed roots, not exact prior GC
scheduling. Raw snapshots stay in /tmp (57MiB/14MiB approximately); hashes and
compact analyses are recorded. inspect-live-root-retainers.py now accepts optional
probe-kind (default owner), so complete:core paths are reproducible.

Runtime now keeps only a closed request token after retirement (core cleared;
local core still handles teardown/cache retirement). retireRenderSession clears
pending tasks, publisher and failure references, resolves the notification and
replaces its resolver. It runs in inner finally and root-owner cleanup, covering
a never-settling view whose inner finally cannot execute. Late component errors
cannot refill a closed session. Ordinary late request access still throws before
falling back to a default graph. No user error identity or public API was changed.

New captured-host-token assertions cover fulfillment, rejection, late refusal,
stream completion and non-settling string/Web views. Before root cleanup the
new regression failed (edge-gc-unsettled-token-before-2026-11-01.txt). Installed
public-host probe now retains12 request tokens and11 render sessions while all
12 core WeakRefs collect. The Node host probe runs with --expose-gc in package gate.
Owner-channel tokens are deliberately separate open scope, not covered by that
retained-token test.

Final installed Edge GC passes24 requests/140 collected targets, controls4 before
release/0 after. Optional GC package mode validates counters and acknowledged
inspector commands. Native no-keepalive8-event churn remains mandatory. A60s
functional churn passes9824 events,157184 DOM cycles,78592 streams,78592 string
renders and9824 timeouts. Final JS1286/97 passed16.84s, TypeScript/build/package
with Edge GC and HTTP actions4/4 passed. No Rust changes or commit/push/publish.

Receipt: benchmarks/edge-gc-qualification-2026-11-01.json. It verifies installed/
workspace runtime identity, sources, snapshots/retainer texts, controls, retained
host tokens and churn scope. Logs: edge-gc-final-suite, edge-gc-qualified-package,
edge-gc-final-actions-2026-11-01.txt. Long raw scope text predates the forced-GC
wording clarification; actual command acknowledgements are recorded in that raw.
This qualifies pinned local workerd/LinkeDOM under inspector collection, not
natural GC cadence, RSS/GPU memory, deployed providers or multi-hour plateaus.


## 2026-11-01 — closed owner-channel tokens: retained-host qualification

Closed async owner tokens now clear both core and owner on success, rejection,
abort, timeout and carrier setup failure. Successful child scopes and their effects
remain alive under the parent. Request identity keeps late implicit calls fenced;
a weak core-to-request index preserves the expired explicit-core refusal after
request retirement without retaining the core. Active captured-root re-entry and
foreign-core overrides remain usable. Regressions cover those boundaries.

Controlled runtime revert matches the preceding request/render qualification hash.
With identical worker/retention fixtures and 20 retained owner tokens it leaves
20 cores and 20 WASM Memory objects alive after terminal handles and controls are
released. The final installed runtime collects all 140 request targets while
20 retired owner tokens remain held; controls survive held phase (4) and collect
after release (0). All four inspector GC commands acknowledge. Installed Node
collects 17 cores with 17 request, 13 owner and 11 render tokens held.

Full JS suite: 1286/97; TypeScript passed; installed package/Edge GC passed;
HTTP actions 4/4. [Qualification receipt](benchmarks/owner-token-qualification-2026-11-01.json)
binds source/runtime hashes, controlled revert and raw installed reports.
Forced GC establishes collection under these roots, not natural GC cadence or
multi-hour RSS bounds. Other providers and native browser carriers remain open.

Final runtime 60-second repeat passed: 5728 events,
45824 streams and 45824 string renders,
8 concurrent events, readable timeout transport, no keepalive. The repeated
inspector GC gate passed with all 20 retired owner tokens still held.


## 2026-11-01 — workerd producer failure before Response

Reproduced the transport gate failure with workerd 2026-11-01 binary and pinned
Miniflare 4.20260730.0. Producer rejection may reset the socket before headers,
so Node fetch rejects with UND_ERR_SOCKET; the pinned July binary instead permits
a clean truncated body. The driver now records fetch rejection, body read rejection
or clean EOF separately. All error paths must refuse receiver completion, reach
the intentional producer failure marker, clean once with graph0, and pass a fresh
complete stream through decoder/receiver for both codecs. Errors on complete or
cancel requests still fail the gate. No runtime behavior was changed.

[Reproduced failure](benchmarks/edge-transport-new-before-2026-11-01.json);
[final November binary probe](benchmarks/edge-transport-new-qualified-2026-11-01.json).
The latter passes concurrency, DOM/forms, transport cleanup/recovery and retained
owner-token inspector GC. This is a binary override using the July Miniflare
controller and July compatibility date, not qualification of the November
Miniflare package, deployed providers or new compatibility-date behavior.

Final installed-package gate with pinned workerd and inspector GC also passed.
Both binary probes share driver/worker/runtime hashes;
[comparison receipt](benchmarks/edge-transport-qualification-2026-11-01.json).
Syntax and git diff --check passed. Runtime unchanged; JS/Rust suites were not
repeated for this driver/fixture-only change.


## 2026-11-01 — Miniflare v5 beta and compatibility-date matrix

Registry inspection found stable Miniflare 4.20260730.0 and newer
5.20261023.3-beta (workerd 1.20261023.1); no 4.20261101.0 exists.
Installed beta dependencies in a separate /tmp directory, preserving project
dependencies and stable qualification. Its constructor rejects v4 flat options;
the driver now uses exported convertV4MiniflareOptions when available. Stable
v4 continues using its native options. Compatibility date is explicitly selectable
with SVET_EDGE_COMPATIBILITY_DATE, default remains 2026-07-30.

Passed local DOM/forms/concurrency, readable transport errors/cancellation/recovery
and retained-owner inspector GC on matched October beta/date, beta controller
with November workerd/date, and stable controller with November workerd/date.
[October beta](benchmarks/edge-beta-matched-after-2026-11-01.json),
[beta + November](benchmarks/edge-beta-november-2026-11-01.json),
[stable + November](benchmarks/edge-november-compat-2026-11-01.json).
The initial beta API refusal is retained as edge-beta-matched-2026-11-01.json.
These are short local probes; beta support is experimental and does not establish
deployed-provider behavior, natural GC cadence or multi-hour RSS bounds.


## 2026-11-01 — natural-GC HTTP long soak in progress

soak-ssr-stream-http now supports SVET_SOAK_GC=natural without --expose-gc;
forced remains the default. Max duration is 24 hours. The 32-sample ring and
four aggregate windows remain bounded; windows now include external/ArrayBuffer
ranges. Atomic .progress.json checkpoints every five seconds retain counters,
memory windows and startup source/runtime hashes. Final hashes must match
startup or the report fails, preventing a later build from silently changing
the qualified artifact identity. Short natural/forced pilots passed; final
source natural pilot confirms zero forced GCs and unchanged hashes:
[raw](benchmarks/http-natural-qualified-pilot-2026-11-01.json).

Started a 7,200,000ms natural-GC loopback HTTP run, 8 concurrent clients,
complete/disconnect mixture and delayed reader calls. Output:
benchmarks/http-natural-2h-2026-11-01.json; live checkpoint appends
.progress.json to that path. Execution session 86231. Completion and resource
bounds are not claimed while it is running. This fixture does not cover gzip,
both transport codecs, proxy behavior or sustained kernel-buffer saturation.


### Natural HTTP soak analysis contract (declared before post-warmup windows)

Added scripts/analyze-http-soak.py. It validates request accounting, graph0,
natural GC, final artifact identity and minimum duration. Running checkpoints
return incomplete, never a passed qualification. The first of four quarters is
warmup; both later quarters are compared to the second quarter for trough and
peak growth, including an intermediate peak that later recovers. Explicit
investigation budgets for this host/run: heap16 MiB, RSS64 MiB, external64 MiB,
ArrayBuffer16 MiB. These are investigation triggers, not universal engine limits.
At least two observations in every quarter and 7,200,000ms are required.
Synthetic controls passed for completed stable windows, intermediate growth,
running status, changed artifacts and forced GC. Current live analysis returns
incomplete as intended; no post-warmup memory conclusion is available yet.

Final analysis command (after the actual final report exists):

```sh
python3 packages/svet/scripts/analyze-http-soak.py packages/svet/benchmarks/http-natural-2h-2026-11-01.json --heap-growth-mib 16 --rss-growth-mib 64 --external-growth-mib 64 --arrayBuffers-growth-mib 16
```

Window extrema alone cannot prove a stationary distribution or exclude slow
leaks. A threshold exceedance must be investigated rather than relabelled passed.


## 2026-11-01 — concurrent HTTP transport soak matrix

Added scripts/soak-ssr-transport-matrix.mjs, separate from the running two-hour
natural-GC fixture. Each round covers both identity/reference codecs, gzip on/off,
complete, shell disconnect, pending-patch disconnect and pending-patch host abort.
Four clients mix outcomes; patch cancellation waits for observed drain/flush
with live nodes, and gzip additionally requires one completed shell flush plus
compressed output. Pending patches use 4MiB deterministic varied HTML; complete
requests use 256KiB and delayed reads. Complete frames validate version, stream
identity, sequence, count state and escaped HTML. Every request must finish with
cleanup1/nodes0; gzip must settle its pipeline and close compressor.

The harness keeps bounded counters per 16 cases and a 32-sample memory ring,
writes atomic progress checkpoints and rejects changed probe/fixture/key runtime
hashes. It uses natural GC and no --expose-gc. Pilot passed the full matrix.
This proves observed application/compressor backpressure paths, not sustained
kernel-buffer saturation or proxy/CDN behavior; a minute is not a long memory bound.

Final minute matrix passed: 94 full rounds, 1,504 requests, 94 repetitions per
case, 4 concurrent clients, 60,627.9ms; probe/fixture/key runtime hashes unchanged.
[Raw](benchmarks/transport-matrix-qualified-2026-11-01.json).
This run overlaps the separate natural-GC HTTP soak; throughput and memory
should not be presented as isolated-host performance measurements. Syntax and
git diff --check passed. Runtime unchanged, so full JS/Rust suites not repeated.


## 2026-11-01 — native constraint validation/model browser check

Added tests/browser/model-validation.html: 13 checks passed in the in-app
Chromium current-source Vite page. Native required/email/range validity, rejected
requestSubmit with invalid events, reactive validity changes, input-to-model,
valid submit, retained custom validity, dynamic required/disabled, noValidate
and submitter formNoValidate, queued reset refusal after teardown and graph0.
[Receipt](benchmarks/model-validation-qualification-2026-11-01.json) binds
fixture/runtime-source hashes and visible text/screenshot. Existing reset
coordinator and browser reset fixtures were already implemented; this adds
constraint-validation evidence rather than reimplementing native browser behavior.
No runtime change or rebuild. Installed production and Edge LinkeDOM/native
validation remain separate open qualifications. The long HTTP process is intact.

Focused existing model-reset/model-reset-roots regressions: 30/30 passed in
2 files; raw model-validation-reset-regressions-2026-11-01.txt.
Temporary browser tab and Vite server closed; two-hour soak session86231 remains live.


## 2026-11-01 — installed production validation and complete DOM matrix

Registered model-validation (13 expected checks) in browser-matrix-cases.json;
check-package now includes it through the existing declarative fixture list.
Rebuilt the retained archive consumer with public svet-engine imports, without
rebuilding workspace dist/package used by the running soak. All installed JS
was byte-compared to workspace dist/package; fresh fixture/artifact hashes and
original archive SHA saved in model-validation-installed-metadata-2026-11-01.json.
Production browser execution passed 21/21 fixtures and 189 native assertions,
including all existing reset/root/adoption cases and new constraint validation.
[Raw](benchmarks/model-validation-installed-matrix-2026-11-01.json),
[screenshot](benchmarks/model-validation-installed-matrix-2026-11-01.png).
Raw report embeds the same fresh build metadata. This is a retained archive
consumer rebuild, not a new package gate invocation; additional browsers and
Edge native form validation remain open. Syntax and git diff --check passed.
Temporary browser tab and preview server closed.


## 2026-11-01 — resource PromiseLike contract and hostile thenables

createResource fetcher signatures now accept PromiseLike<T> for source and
no-source forms; runtime still uses native await assimilation. This exposes
existing thenable support in public types rather than adding a parallel
assimilation path. Four regressions cover first-settlement wins despite repeated
resolve/reject, throwing then getter with refetch recovery, independent abort
settlement and stale fulfillment, and owner disposal during getter access.
The previous Promise-only signature fails the explicit PromiseLike generic type
regression; raw resource-thenable-types-before-2026-11-01.txt. Final TypeScript
passed; target suite45/45 and full JS1290/98 passed. Public installed declaration
fixture now includes both PromiseLike fetcher signatures.

No runtime JS behavior changes; recorded three runtime hashes used by the live
HTTP soak remained identical after the pretest package build. The long process
continues without restart. GPU fallback thenables and cross-copy tracking remain
separate open tasks; these resource tests do not qualify those paths.

Installed package gate with pinned Edge passed, including both PromiseLike
declaration forms and production build with model-validation fixture.
[Receipt](benchmarks/resource-thenable-qualification-2026-11-01.json).
Git diff --check passed. No commit/push.


## 2026-11-01 — GPU fallback thenable accessor fix

The sync compatibility fallback read returned.then to detect async output,
then Promise.resolve read the accessor a second time. A regression reproduced
2 reads instead of 1. Cache the callable once and bind its original receiver
inside native Promise assimilation; keep late rejection observed and working
output private. The sync entry still throws the async-callback diagnostic.

Added regressions for one getter read/one call/original this/unchanged output,
async first-settlement wins despite later reject/resolve, and abort of an
uncooperative thenable followed by stale fulfillment without output publication.
Target GPU suite123 passed; full JS1293/98 and TypeScript passed. This is host
fallback qualification, not a new hardware GPU claim or cross-copy tracking proof.
Raw gpu-thenable-before-2026-11-01.txt retains the failure; after/full-suite/types
logs use the gpu-thenable prefix. Installed public probe now includes the getter
count/receiver/unchanged-output regression.

The live HTTP soak uses already-loaded Node streaming code and does not exercise
GPU dispatch. Its recorded server/stream/reactivity hashes remain the relevant
fixed workload identity; GPU source/build changes are outside that workload.

Final installed public archive gate with pinned Edge passed, including the new
getter regression. [Receipt](benchmarks/gpu-thenable-qualification-2026-11-01.json).
Git diff --check passed; long HTTP process remains running.


## 2026-11-01 — cross-copy untrack: source qualification, package pending

New check-cross-copy-tracking.mjs imports two independently compiled engines
from the frozen event-instance build. Before: foreign untrack failed to suppress
existing observers and reads before a throw; foreign signal invalidation also
failed. Compatible copies now share only the synchronous live-context stack under
Symbol.for('svet.reactivity.observation-contexts.v1'). No permanent module callback
or inactive core registry is added; stack unwinding removes all live entries.

After: foreign untrack suppression, new inner observer tracking, throw recovery,
reset-generation refusal and separate-core teardown pass. Registry length is0
after all cases. Automatic foreign-signal subscription remains failing/open;
the probe intentionally retains requirements-open and nonzero exit rather than
weakening that expectation.
[Before](benchmarks/cross-copy-tracking-before-2026-11-01.json),
[after](benchmarks/cross-copy-untrack-qualified-2026-11-01.json).
This is a same-realm compatible-copy protocol, not arbitrary-version coordination
or async context propagation. Independently built source artifacts include hashes.

Workspace package rebuild/installed gate is deferred until the running two-hour
HTTP soak ends: it currently measures the preceding reactivity runtime. Source
qualification must not be presented as installed-package delivery of this change.

Source TypeScript passed. Full source suite:1292 passed /1 benchmark timeout
in98 files (52.23s, worker startup~1.47s). Benchmark workload took5772ms against
unchanged5000ms limit while the natural HTTP load was live. Raw failure retained
in cross-copy-untrack-full-suite-2026-11-01.txt; focused repeat is separate, not
a replacement claim that the full suite passed.


### Cross-copy untrack expanded source/GC evidence

Expanded independent-copy probe:7/8 scenarios pass. Added nested observers in
both copies, incidental-read suppression across both suspended stacks and
invalidation from a write to an already-read dependency. Automatic foreign
signal subscription remains the sole failing expectation; overall probe stays
requirements-open.
[Raw](benchmarks/cross-copy-untrack-expanded-2026-11-01.json).

Forced Node GC after 24 independently allocated core lifetimes, cross-copy
untrack/reset and fresh observer teardown collects24 cores +24 WASM Memory
objects. Controls4 held,0 released; shared live-context array length0.
[GC raw](benchmarks/cross-copy-untrack-gc-2026-11-01.json).
This qualifies source bundles only, no browser collection or user-retained handles.
No workspace package rebuild while the preceding-runtime HTTP soak is live.

Both independent-copy probes now verify every frozen artifact hash before
import, then embed build metadata. Final expanded and GC probes repeated with
those integrity checks; results unchanged.

Full source repeat with2 workers also retained benchmark timeout:1292 passed/
1 failed,75.81s total. Limits unchanged; this milestone remains partial.
Raw cross-copy-untrack-full-repeat-2026-11-01.txt.


## 2026-11-02 — cross-copy coordinator hot-path revision

First ABBA diagnostic (shared/local/local/shared, one worker, timeout5000ms)
failed both shared-stack runs and passed both local controls. Host load averaged
~19–20 on16 logical CPUs, with independent Rust builds and the HTTP soak alive.
Raw cross-copy-untrack-bench-abba-2026-11-02.json and host-context report.
This warranted investigation rather than attributing all failures to contention.

Replaced globally owned frame array with local per-copy arrays plus global weak
array references under observation-copies.v2. Enter/leave keep the original local
push/pop path; only untrack inspects foreign arrays, and single-copy execution
skips foreign enumeration. Dead weak registrations are pruned on new registration.
The registry owns neither module callbacks nor inactive cores. Previous v1 shared
stack design is superseded; mixed protocol versions are not claimed compatible.

Rebuilt independent copies:7/8 expanded cases pass (foreign dependencies still
open), GC24 cores+24 Memory passes with controls4 held/0 released, TypeScript
passes. Frozen artifact hashes are checked before import.
Raw cross-copy-weak-registry-cases-2026-11-02.json and
cross-copy-weak-registry-gc-2026-11-02.json. A one-copy diagnostic observed one
weak registration,0 inactive frames and1 during the observer; diagnostic test
was removed after saving its raw log. The standalone benchmark still timed out
under current load; a fresh weak/local ABBA is required before any performance
claim. Package rebuild remains deferred for the preceding-runtime long soak.

Second ABBA (weak/local/local/weak): failures1/1/1/0. Both original local-stack
controls now timed out too, final weak passed. This confirms the gate is unstable
under these host conditions; it does not establish a performance improvement or
absence of regression. Raw cross-copy-weak-bench-abba-2026-11-02.json.
Retain local hot path/weak discovery design on ownership grounds. Current
source qualification receipt: cross-copy-weak-registry-source-2026-11-02.json;
previous v1 receipt is explicitly superseded. Full benchmark and installed
qualification remain open. Correctness-only run excludes that benchmark openly
and cannot substitute for the full gate.


### Natural two-hour HTTP run terminated, not qualified

Session86231 exited1 authoritatively. Final report:189144 completed-accounted
requests, elapsed1462380.6ms (~24.4min), unchanged source/key runtime hashes,
forcedGC0. Error stack points to HTTP control request in run finally (forget),
so a cleanup failure may have masked the original body failure. No cause is
asserted from host load alone. Full raw http-natural-2h-2026-11-01.json retained.
Analyzer returns needs-investigation; no two-hour completion or memory plateau
claim. Only first quarter has samples. No automatic restart; next step is
primary/cleanup error preservation before another long qualification.

Correctness-only run ended:1267 tests/95 files passed,2 Vitest worker-startup
timeouts prevented completion of97 expected files (338.24s). Benchmark excluded
explicitly. This is an incomplete gate, not a successful full suite. Limits were
not relaxed. Raw cross-copy-weak-registry-correctness-2026-11-02.txt retained.


## 2026-11-02 — primary HTTP errors survive cleanup failures

soak-ssr-stream-http records control operation/id/elapsed time and original cause.
run catches its primary failure before cleanup; forget failures are kept in a
bounded8-entry cleanupFailures ring and cannot replace that primary. Cleanup-only
failures still fail the run. All concurrent requests now settle before global
server teardown, preventing teardown races with unfinished sibling reports.
Failed batches retain bounded requestFailures records; elapsed time is captured
on failure instead of only the last five-second checkpoint.

Deterministic local HTTP injection returns503 on both stats and forget for the
first request: final error remains stats503, cleanup record separately contains
forget503 and its primary identity, and the batch fails. Deadline10000ms remains
unchanged. Raw http-primary-cleanup-injected-2026-11-02.json. This fixes diagnostic
masking, not the unknown cause of the original24-minute failure.

The two worker-startup-missing correctness files (ssr-lifetime/static-arrays)
passed25/25 using one threads worker, with unchanged test limits. Combined with
1267 tests from the earlier source run, all1292 non-benchmark tests have executed
successfully across runs; this is not a single green full-suite qualification.
Raw cross-copy-missing-files-2026-11-02.txt.

Final-source natural10s (192 verified requests), forced1s and injected HTTP
double-failure probes passed their respective expectations; all source hashes
match and artifactsUnchanged is true.
[Diagnostics receipt](benchmarks/http-primary-cleanup-qualification-2026-11-02.json).
The intentionally injected run exits1; it is not a passing workload sample.


## 2026-11-02 — installed copies and deadline neighbour-barrier correction

Rebuilt package with weak observation registryv2. Initial package gate failed
in soak-ssr-deadlines at the cancellation-neighbour liveness assertion, preserving
its consumer at svet-consumer-wMKAAL. The test required100ms timeout neighbours
to remain alive after a setImmediate cancellation barrier; that wall-clock
assumption is invalid if their own deadline expires during the yield. Complete
neighbours still must remain live. A retired timeout neighbour is now accepted
only with its exact SsrTimeoutError/timeoutMs100, cleanup1 and listeners0;
final graph0 and retained-handle GC assertions remain. Default deadline and
barrier deadline are unchanged. Bounded neighbour observations and a count of
early legitimate timeouts are saved.

A150ms diagnostic barrier delay deterministically exercises that branch:96
requests/576 collected objects,24 early timeouts, complete neighbours preserved.
[Raw](benchmarks/deadline-barrier-delayed-2026-11-02.json). Default targeted probe
also passed112 requests/672 objects; this predates the added delay option and
is not the final full package gate.

prepare-installed-copy-probe creates two physical byte-equal runtime trees
inside the installed consumer so declared Babel dependencies resolve normally,
without symlinking the runtime or sharing its ESM module identity. All frozen
files are hashed before import. Installed copy checks7/8 (foreign signal
dependency still open); forced GC24 cores+24 Memory, controls4 held/0 released.
[Cases](benchmarks/cross-copy-installed-cases-2026-11-02.json),
[GC](benchmarks/cross-copy-installed-gc-2026-11-02.json).
Initial attempt in an unrelated temp root could not resolve @babel/parser;
that harness error is retained separately and resolved by keeping copies beneath
the consumer's dependency root. This does not change package dependencies.

Final repeated package gate with inspector-backed Edge GC passed.
[Installed receipt](benchmarks/cross-copy-weak-installed-qualification-2026-11-02.json)
binds current source/archive/reactivity hashes, independent installed copies,
retained-host/Edge collection and corrected deadline barrier. The selected
untrack milestone is installed-qualified; foreign dependency and full performance
gates remain open. No commit/push or overall goal completion.


## 2026-11-02 — frozen installed HTTP soak restart

HTTP soak accepts an explicit installed package path and hashes all20 JS/WASM
runtime files, plus both harness and fixture at start/end. Final identity mismatch
fails the run but does not overwrite an existing primary workload error.
prepare-http-soak-snapshot preserves harness/fixture bytes under a fresh cache
directory; package imports use the retained installed consumer, while jsdom30.1.3
is an explicitly named harness dependency. Workspace source/build edits cannot
change those selected bytes. Installed artifact directory and snapshot must be
kept for the run.

Frozen final pilot184 requests passed with all identities unchanged. A controlled
post-checkpoint gpu.js edit in a separate runtime copy fails identity validation;
untampered installed package preserved. Frozen HTTP503/503 injection retains
stats primary and forget cleanup errors separately.
[Harness receipt](benchmarks/http-frozen-harness-qualification-2026-11-02.json).

Started a new7200000ms natural-GC run on this snapshot with8 clients and unchanged
10000ms HTTP deadlines. Session handle must be checked live; do not restart from
a stale checkpoint. Output http-natural-frozen-2h-2026-11-02.json, checkpoint adds
.progress.json. Original failed24-minute run is preserved. No two-hour memory
claim until final duration, accounting, identities and windows are audited.


## 2026-11-02 — native reactive domains shared by explicit core identity

Independent copies using the same explicit WASM core previously kept separate JS
node registries, generations and owner state. Reproduced missing computed/batch
updates, reset leaving foreign handles current, and owner teardown missing foreign
computeds. A domain registry now shares JS context, reset cleanup/finalizer maps
and retired-cache membership by core identity using WeakMap/WeakSet storage.
Separate native cores stay isolated; no synthetic cross-core edges are created.

Live execution frames identify when a computed getter from another module copy
belongs to the currently executing native observer. Such shared-domain reads
restore the getter's core without suppressing dependency collection; genuinely
foreign suspended domains keep the existing untracked boundary. Frames unwind
in finally, including reset/throw paths. This extends weak copy discovery rather
than sharing native IDs across different memories.

Expanded independent-build probe11/12: all prior untrack cases plus shared-core
computed/batch, generation invalidation, owner cleanup and foreign resource reset
wait pass. Different-core automatic dependency remains open and intentionally
fails. Full source suite1293/98 passed (threads2); TypeScript passed.
Raw cross-copy-domain-expanded-2026-11-02.json and
cross-copy-domain-full-suite-2026-11-02.txt.

Retirement also clears compatible copies' hot caches through weakly registered
callbacks, and a shared retired WeakSet prevents late cleanup from recaching.
On the preceding installed runtime, a controlled probe retained core+Memory after
A retirement and B late cleanup; selecting B's ambient core collected both, with
controls4 held/0 released. This isolates the foreign-cache root:
[before](benchmarks/cross-copy-cache-before-2026-11-02.json).
New installed qualification is pending. Async owner-carrier propagation, default
separate-core dependencies and arbitrary-version interoperability are not proven
by these synchronous shared-core tests.

Final installed qualification passed:11/12 expanded cases (only separate-core
dependency open), same-probe cache retirement before2 retained targets/after0,
24 cores+24 Memory collected after shared-core reset/computed/foreign-effect
teardown, controls4 held/0 released. Package gate with inspector Edge GC passed.
[Receipt](benchmarks/cross-copy-domain-qualification-2026-11-02.json).
The frozen natural long run continues on the preceding installed weak-registry
version; no natural-GC/RSS bound for these newer native-domain changes follows
from that baseline run. Git diff --check passed.


## 2026-11-02 — cross-copy Node async owner/context delegation

Before: copy B selecting A's shared core after await saw the request root instead
of the active child. Foreign computed/onCleanup allocations survived child
rejection until parent cleanup; late foreign reentry silently used the live
parent. Four installed-copy ownership scenarios failed. Compatible domains now
weakly register per-copy resolver delegates. At ownerDepth0 the local resolver
has priority, then other copies may resolve that explicit core's owner or refuse
its expired scope. Each host callback checks core/request identity. Synchronous
owner overrides bypass async lookup as before. Weak callbacks retain neither
foreign modules nor request cores via the global registry.

Final same-probe installed comparison: before4 failures/2 controls passed, after
6/6 passed, including provider context after await, child rejection cleanup, late
refusal with parent usability, captured root/unrelated-core precedence and two
concurrent requests. With8 completed promises and8 child errors held,8 cores and
8 Memory collect; controls4 held/0 released. Full JS1293/98, TypeScript and package
with inspector Edge GC passed. The six-case async owner/GC probe is now required
in check-package, rather than being a manual-only qualification.
[Receipt](benchmarks/cross-copy-async-owner-qualification-2026-11-02.json).

Scope remains byte-matched Node ALS copies selecting the same core explicitly.
Implicit cross-copy core selection, other hosts and mixed build/version carriers
remain open. Frozen natural-GC HTTP run continues on its older installed baseline,
without inheriting these newer ownership guarantees or memory qualification.


## 2026-11-02 — cross-copy async ownership on local Edge hosts

The Worker now imports a second engine through an independent esbuild namespace,
including16 duplicate engine modules and a separate default core. Both copies
install the same compiled WASM Module asset; their JS state/WASM instances remain
independent. Two concurrent host fetch events exercise8 isolated request cores:
child owner/context after await, failed-child immediate cleanup, closed-child
refusal while parent stays usable, captured root precedence inside the active
child and restoration afterward, plus concurrent child isolation. All graphs0.

With identical driver/worker/copy fixture, the preceding installed runtime fails
Foreign child owner lost. Current runtime passes both pinned July stable and
October Miniflare5 beta with November workerd binary/date. Exact versions/flags
are recorded in the receipt.8 cores+8 Memory are tracked weakly and collect under
acknowledged inspector GC; existing140 request targets also collect, controls4
held/0 released. Extra copy probe requests use separate endpoints, preserving
the existing8-event readable churn workload and its counters.

The Edge-enabled package gate now requires these copy scenarios and all16 copy
targets collected when GC is enabled. Final installed gate passed; HTTP actions
4/4 passed. Runtime source did not change this milestone: every runtime file is
byte-equal to the prior1293-test/full-source-qualified package. Syntax/diff checks
passed. [Receipt](benchmarks/edge-cross-copy-owner-qualification-2026-11-02.json).

Qualification is local, compatible-copy, explicit shared-core ownership/context
and computed lifecycle. It does not prove deployed providers, browser carriers,
implicit core propagation, mixed-version copies, cross-core dependencies or
natural-GC/RSS bounds. The beta profile remains experimental.


## 2026-11-02 — foreign Edge DOM bindings after await

Extended the two-copy Edge fixture to use a parent signal created by engine A
with engine B's button, reactive attribute/text and delegated click binding after
await inside A's async child. Both host events assert updates, and the event
callback restores a descendant child owner, selected native core and provider
context. Child rejection must dispose bindings before the parent ends. The
parent signal then demonstrably changes to late while DOM stays at its prior
value and a new click cannot invoke the removed handler. Parent signal disposal
finally leaves graph0; retained button is removed from the shared document.

Weak target profile now includes8 request cores,8 Memory,2 parent signals and
2 buttons. All20 collect with existing controls4 held/0 released; legacy140
targets collect too. Both pinned stable and beta-controller/November workerd/date
profiles passed. Installed Edge gate requires DOM counters and20-target/kind
accounting. Previous runtime fails the unchanged functional prerequisite
Foreign child owner lost with the same driver/fixture.
[Receipt](benchmarks/edge-copy-dom-qualification-2026-11-02.json).

Runtime code is unchanged and byte-equal to the prior full1293-test-qualified
package; syntax/diff checks and final installed package gate passed. This is a
failed-child teardown proof. Successful child handlers invoked after carrier
closure, native browser events and deployed/mixed-version hosts remain separate
open checks. The frozen natural run remains on its named baseline artifact.


## 2026-11-02 — captured foreign core scopes after carrier closure

A child successfully completed its carrier while its owner stayed parent-owned.
A context captured by copy B was still live, but its callback's copy-A signal
setter consulted A's closed ALS carrier and threw. A second regression showed
that nested explicit scopes from two copies did not agree on the currently
selected core. Before2 failures/6 controls passed with the same final probe.

Explicit synchronous core overrides now use a realm-local versioned stack shared
by compatible copies. Default cores and host async resolvers stay module-local.
A captured scope in B selects its native core for A's bound writes and new
computed allocation; after return the uncaptured closed-child guard still throws.
Nested cross-copy scopes restore in finally, including throw. Nothing holds the
selection over a user await or Promise return. The global stack is asserted empty
by the installed probe after all cases.

After8/8 installed cases passed, including8 cores+8 Memory collected with8
completed promises and8 errors retained (controls4 held/0 released). Full source
JS1293/98, TypeScript, installed package with Edge inspector GC and native HTTP
actions4/4 passed. Browser production-source independent-engine fixture9/9 passed:
separate default cores, bubbling, additive handlers, stopImmediatePropagation,
closed shadow roots, one-copy reset/retirement and final graph0. Screenshot/raw
source hashes bind that browser run to this bridge revision.
[Receipt](benchmarks/cross-copy-captured-core-qualification-2026-11-02.json).

Temporary browser tab/preview server closed. The frozen natural HTTP process
continues on its prior immutable installed baseline; it does not qualify this
newer core selection implementation. Cross-realm/mixed-version and implicit
async core propagation remain open.


## 2026-11-02 — retained successful-child Edge DOM callback

Extended the independent-copy Edge fixture with a real delegated button event
invoked from a completed child carrier while its successful owner remains live
under the parent. Copy B captures the handler; it selects copy A's native core,
updates A's signal and allocates/disposes A's computed. The bound DOM updates.
An uncaptured allocation in that same late continuation still refuses the closed
child. After parent teardown another event cannot invoke the retained handler.

The identical fixture fails on the previous installed runtime with
`Svet SSR owner scope has already completed` inside the source-copy getWasmCore
called by onClick. Current stable workerd and beta/November profiles pass both
concurrent host events. All24 targets collect:8 cores,8 Memory,4 signals,4 DOM
nodes, alongside the legacy140 targets and controls4 held/0 released. Driver now
records wasmBridgeSha256 as well as reactivity/runtime and fixture hashes.

Raw reports: [before](benchmarks/edge-retained-child-before-2026-11-02.json),
[stable](benchmarks/edge-retained-child-after-2026-11-02.json),
[beta](benchmarks/edge-retained-child-beta-2026-11-02.json).
The package gate requires the new counters and exact24-target/kind accounting.
Runtime source is unchanged in this milestone. This proves local LinkeDOM/workerd
behavior for compatible copies; deployed providers, native browser async carriers,
natural-GC/RSS bounds and mixed-version copies remain open.

Installed-package gate completed with exit0, including both new event counters
and24-target inspector-GC assertions. [Receipt](benchmarks/edge-retained-child-qualification-2026-11-02.json).


## 2026-11-02 — shared built-in boundary protocols

A resource created by an independent compatible copy under the same explicit
native core could not see its host copy's Suspense or ErrorBoundary. Their
module-local createContext calls generated different symbols. The same final
probe fails all4 scenarios on the previous installed package.

Built-in Suspense/error-boundary identities now use versioned realm-wide symbol
keys. User createContext identities remain unique. useContext accepts the
read-only id/defaultValue lookup contract; Provider is not required for lookup.
The probe exercises both directions with foreign pending/completion, refetch,
unmount cancellation, rejection and boundary retry. All4 pass on the rebuilt
runtime and are mandatory in check-package. This is explicit shared-core,
compatible-copy JSDOM evidence; other realms, mixed versions, separate-core
reactive dependencies and deployed/browser async carriers remain open.

Raw: [before](benchmarks/cross-copy-boundaries-before-2026-11-02.json),
[after](benchmarks/cross-copy-boundaries-after-2026-11-02.json).

Full JS1293/98, TypeScript, installed package with Edge GC and HTTP actions4/4
passed. [Receipt](benchmarks/cross-copy-boundaries-qualification-2026-11-02.json).


## 2026-11-02 — Edge built-in boundaries and retry core

Added four independent-copy boundary cases in real local workerd with LinkeDOM:
both directions of pending/completion/refetch/unmount cancellation and async
rejection/retry. Each request uses an isolated native core and its own detached
container. The driver and installed gate require all4 cases and graph0.

This exposed an additional defect: copy B's ErrorBoundary retry, invoked under
copy A's ambient SSR core, opened batch on B's default core. Its two bound signal
writes flushed A separately and mounted content twice (mounts3 rather than2).
The previous installed runtime reproduces that failure with the final fixture.
Retry now selects the boundary's core before batching; a WeakRef avoids adding
strong core retention through externally retained retry functions. A source
regression calls retry under a different ambient core and requires mounts2.

Stable and beta/November workerd profiles pass the4 cases; existing cross-copy
24-target and legacy140-target GC checks also pass. Raw:
[retry before](benchmarks/edge-copy-boundaries-retry-before-2026-11-02.json),
[stable](benchmarks/edge-copy-boundaries-after-2026-11-02.json),
[beta](benchmarks/edge-copy-boundaries-beta-2026-11-02.json).
Native browser boundary checks, deployed providers, mixed versions and natural
GC/RSS remain open. The long frozen HTTP run still measures its earlier artifact.

Final full suite1294/98, focused lifecycle10/10, TypeScript, installed package
with Edge inspector GC and HTTP actions4/4 passed.
[Receipt](benchmarks/edge-copy-boundaries-qualification-2026-11-02.json).


## 2026-11-02 — native browser cross-copy boundary qualification

Chrome156 native DOM passed all4 Suspense/ErrorBoundary scenarios in both
independently built source bundles and installed-package production bundles.
The fixture checks foreign pending/completion, refetch/unmount cancellation,
rejection identity, retry mounts2 and graph0 in both directions. Source and
installed artifact hashes are recorded; the installed advanced.js digest was
checked against the retained consumer. prepare-event-instances now accepts a
bounded fixture choice and optional installed package path for reproducibility.

[Receipt](benchmarks/browser-copy-boundaries-qualification-2026-11-02.json).
This proves explicit shared-core scopes and async resource settlement with
native DOM, not a browser async-context carrier or implicit propagation across
await. Other browsers, realms, versions and natural-GC/RSS remain open. Both
temporary tabs and preview servers were closed. Runtime code is unchanged from
the prior1294/98 full-suite/package-qualified revision.


## 2026-11-02 — cross-copy router protocol and atomic foreign-core navigation

Independent copies used different module-local RouterContext symbols, so foreign
useRouter threw even within a host RouterView and foreign Link could not navigate.
The built-in router protocol now uses a versioned realm-wide Symbol; custom
contexts remain unique. The installed probe checks both directions: router
identity, actual Link click interception, route params/query/hash, old-route
cleanup exactly once, single replacement mount, graph0 and inactive retired Link.

A separate reproduction found non-atomic URL publication when navigate was called
outside the router's native core: batch selected the caller's ambient default
while bound signal writes flushed the router core independently. URL observers
saw path/query/hash intermediate combinations. The commit batch now explicitly
selects the router's own core. Source regression and installed third case require
exactly the initial and final URL observations. The same final installed probe
fails all3 cases on the previous package. Runtime changes remain limited to these
two defects; cross-core automatic dependencies and mixed-version semantics remain
open.

Before: [raw](benchmarks/cross-copy-router-before-final-2026-11-02.json).
The new3-case router probe is mandatory in check-package.

Final installed probe3/3, full JS1295/99, focused regression, TypeScript,
package with Edge GC and HTTP actions4/4 passed.
[Receipt](benchmarks/cross-copy-router-qualification-2026-11-02.json).


## 2026-11-02 — native browser installed cross-copy router

Added a reproducible cross-copy-router browser fixture and bounded builder
option. Chrome156 passed3/3 with two independently bundled installed runtime
copies: RouterView/useRouter/Link identity and native click navigation in both
directions, route cleanup once, query/hash/params, retired Link teardown and graph0;
plus atomic URL publication from a non-ambient core. Installed advanced.js hash
was verified against the retained consumer artifact.
[Receipt](benchmarks/browser-copy-router-qualification-2026-11-02.json).
Runtime source is unchanged from the1295/99/package-qualified revision. Script
syntax and scoped diff checks passed. Temporary tab and preview server closed.
Cross-copy back/forward concurrency, rollback, other browsers and Edge router
remain open. The frozen long HTTP run is separate and still baseline-specific.


## 2026-11-02 — native history with cross-copy observers

Expanded installed native browser router fixture to5/5 cases on Chrome156.
Both directions now use a router from one copy and a shared-core observer from
the other while performing actual History API Back/Forward, denied-Back
compensation and guard redirect. Exact observation sequence rejects intermediate
URL publication or compensation publication. Guard runs once on refused Back,
history length stays fixed, redirected slot survives round trips, initial host
state/full URL survive, and teardown leaves graph0.

An initial fixture expected query.q after redirect to ?next=middle. That
assertion was wrong; corrected observer reads q or next, preserving the failing
raw as fixture-error evidence. No runtime change was needed.
[Receipt](benchmarks/browser-copy-history-qualification-2026-11-02.json).
This uses one active router per scenario; competing routers, rapid traversal
bursts across copies and other browsers remain open. Runtime remains the prior
1295/99/package-qualified artifact. Temporary tab/server closed; scoped diff
check passed. The separate frozen natural HTTP run is still live.


## 2026-11-02 — cross-copy rapid history bursts, partial

Added six denied native history.go burst patterns in both copy directions,
checking accepted full URL/history length, no foreign observer publication during
refused traversal, subsequent admitted navigation and graph0. A combined run
passed16/17; the last B→A case timed out waiting for its initial go(-2) to reach
the middle entry, before installing the burst guard. Cause is not established.

Independent fresh tabs selecting each burst direction passed11/11 each (five
prior cases plus all six bursts). Same runtime,3s transition deadline and80ms
quiet interval. This narrows the observation but does not repair or close the
combined-run failure. The first failing raw is preserved. Next investigation:
record native popstate/go timeline and accepted/physical positions around setup
and delayed callbacks; distinguish native traversal scheduling from runtime
restoration. No claim of browser throttling without evidence.
[Partial receipt](benchmarks/browser-copy-burst-qualification-2026-11-02.json).
Runtime unchanged; scoped diff check passed; tabs/server closed. Broad rapid
history qualification and simultaneous competing routers remain open.


## 2026-11-02 — combined history failure timeline and native control

Instrumented fixture-only History API commands and native popstate before/after
microtask, case boundaries, history state and selected router fields (bounded
1000-entry timeline). The full17-case run again passes16 then times out preparing
the last case. Final go(-2) is logged; no following native popstate arrives in
3s. This narrows the failure stage but does not establish its cause.

Added native-only replay fixture with the captured199 commands and original
relative timing, without any Svet imports. It receives a final popstate and
reaches the expected middle URL. Therefore command count/tempo alone did not
reproduce the failure. This control is scheduled replay rather than identical
synchronous event-callback interleaving; it cannot exonerate either the runtime
or browser. Next: compare native event ordering, compensation issuance and pending
traversal at case boundaries. No runtime fix or overall burst pass is claimed.
[Analysis/raw](benchmarks/browser-copy-burst-timeline-analysis-2026-11-02.json).
Scoped diff/script syntax checks passed; tabs/server closed. Frozen HTTP process
was authoritatively polled and remained live, without a final report.


## 2026-11-02 — native navigation control and paced full burst matrix

Found two native navigations missing from the first199-command replay: retired
Link test clicks deliberately had defaultPrevented=false and triggered normal
anchor navigation after engine teardown. Adding these two to native-only replay
produces201 commands and reproduces the missing final popstate without Svet.
The original replay's success therefore did not exclude host navigation refusal.
Exact host quota/mechanism is not established.

Fixture now records defaultPrevented in an independent listener before cancelling
the native default, retaining the engine teardown assertion while removing test
side effects. Unpaced run still fails the last burst compensation16/17, so this
is not a full stress repair. With explicit optional ?paced=1, a10s inter-case
pause after more than120 recent history commands allows all17 cases to pass.
Individual burst commands,3s transition deadline and80ms quiet checks unchanged.
Pacing is recorded in raw timeline. This qualifies those full-case semantics at
the named pacing, not the original continuous high-rate workload.
[Controls/results](benchmarks/browser-copy-burst-controls-2026-11-02.json).
Runtime unchanged; diff/script syntax checks passed; tabs/server closed. The
continuous stress requirement remains open alongside competing routers.


## 2026-11-02 — empty core-override stack hotpath

A fresh-worker installed-artifact AB/AA benchmark exposed a synthetic untrack
regression after shared-core/owner changes. Candidate median was2.6–4.8x the
older frozen baseline in the short pilot; AA controls were noisy. A narrower
comparison changes only wasm-bridge.js: getWasmCore now checks stack length before
indexing its top, avoiding a negative-property lookup on an empty array.

In the longer alternating AB experiment (512 invalidations ×4000 untrack calls,
five measured rounds per worker, nested depths1/32/128/256), candidate/baseline
median ratios are0.348/0.357/0.427/0.291. AA ratios range0.687–1.138. This supports
retaining the small hotpath change for this workload; it is not application FPS
or general throughput. The host ran the frozen HTTP soak and overlapping source
checks, so uncontended/hardware-independent performance qualification remains
open. Runtime file hashes prove only wasm-bridge.js differs in the narrow AB.

New bench-installed-untrack driver uses sequential fresh workers, alternating
AB order and candidate AA controls, saves nested raw reports and verifies all
JS/WASM hashes unchanged before/after. Existing bench-untrack-cores assertions
still require correct values, untracked dependencies, node/edge counts and graph0.
Raw: [initial](benchmarks/installed-untrack-ab-aa-2026-11-02.json),
[short isolated change](benchmarks/core-stack-fastpath-ab-aa-2026-11-02.json),
[long](benchmarks/core-stack-fastpath-long-ab-aa-2026-11-02.json).

Final JS1295/99, TypeScript, installed package with Edge GC and HTTP actions4/4
passed. Installed bridge hash matches the measured candidate.
[Receipt](benchmarks/core-stack-fastpath-qualification-2026-11-02.json).


## 2026-11-02 — retained ErrorBoundary retry closure retirement

Installed Node ALS/JSDOM GC probe found4/4 completed boundary request cores and
Memory retained by externally held retry functions, despite graph0. WeakRef in
reset was insufficient: the reset closure still retained the component lexical
environment through its disposed signal/accessor bindings. A module-level
retirable callback factory now creates the public retry wrapper outside that
environment. Boundary cleanup clears the wrapper's sole implementation slot;
retained retry becomes a no-op and no longer reaches the component closure.

Same final probe before4/4 retained → after0/4 with callbacks held; controls4
held/0 released. New boundary-only installed GC profile is mandatory in package
gate. The full probe remains failing/open for disposed router objects:4/4 cores
and Memory retained; each RouterView unmount leaves1 node while parent request
is live. This is not hidden by the focused boundary gate. Investigate that
owner/retainer path next, preserving terminal snapshots and late navigate=false.

Full JS1295/99, TypeScript, installed package with Edge GC and HTTP actions4/4
passed. [Partial receipt](benchmarks/retained-ui-callbacks-qualification-2026-11-02.json).
GC scope is forced Node ALS/JSDOM; native browser/Edge and natural-GC bounds
remain separate. No claim that all retained UI handles are fixed.


## 2026-11-02 — disposed Router runtime retirement

The apparent1-node unmount residue was an inactive native slot in effectPool,
not a live owner/subscription: getGraphTelemetry drains pending pooled disposals
and reports graph0 both before and after this change. Corrected the GC probe to
use that public accounting boundary. No ownership fix is claimed for the pool.

Disposed router objects did retain4/4 completed request cores and Memory through
signal/method closures. Added module-level retirable router/read-only-signal
wrappers: live calls forward to the implementation; teardown captures terminal
URL values and clears implementation/signal slots. Readers retain id/peek/dispose
and signal branding; navigate returns false, guards no-op, repeated disposal is
safe. A separate pure route matcher preserves match after teardown without
capturing the runtime environment. Guard-removal callbacks also retire their
implementation slots, including externally retained removers.

The final same-probe comparison holds12 references (four retries, four routers,
four guard removers). Before: boundary0/router4 retained; after:0/0, controls4
held/0 released. All request graphs0 after telemetry. Late retry, router methods,
terminal path/query/hash, matcher and repeated disposal are asserted. Full GC
profile now mandatory in check-package rather than boundary-only profile.
Raw: [before](benchmarks/retained-router-before-final-2026-11-02.json),
[after](benchmarks/retained-router-after-final-2026-11-02.json).

Final JS1296/99, TypeScript, installed package with Edge GC and HTTP actions4/4
passed. Installed full retained-UI GC probe holds12 references and collects all
8 cores/Memory pairs with controls4/0.
[Receipt](benchmarks/retained-router-qualification-2026-11-02.json).


## 2026-11-02 — native browser retirement qualification

Retired-router installed artifact passed the full17-case native Chrome156
history matrix with the prior explicit inter-case pacing. Added retained-ui-gc
fixture for two independent installed engine copies. It creates/tears down eight
boundaries and eight routers, retaining40 references (retry functions, router
objects, readers, navigate methods and guard removers). All16 cores and16 Memory
collect under bounded natural allocation pressure. Four live controls survive
held phase; all controls collect after release while40 UI references stay held.
Every allocation asserts graph0 after public telemetry drains the inactive pool.

[Receipt](benchmarks/retired-ui-browser-qualification-2026-11-02.json).
Artifact hashes bind both runs to the installed advanced.js revision. No runtime
change this milestone; scoped diff and builder syntax checks passed. Temporary
tabs/preview servers closed. Pressure is12 task-separated rounds of16MiB bytes
plus500k-number array, repeated after control release; no forced/CDP GC used.
This does not qualify idle cadence, RSS plateau, other browsers, browser SSR
async carrier, Edge retained-router GC or unpaced continuous history stress.


## 2026-11-02 — completed frozen two-hour natural HTTP baseline

Exec85177 exited0. Final elapsed7200032ms;1,232,608 requests =821,739 complete
+410,869 disconnect. No request/cleanup failures, forcedGC0, graph0 after every
request, all frozen harness/fixture/runtime hashes unchanged. Four quarter
windows have sufficient observations. Analyzer passed heap16MiB/RSS64MiB/
external64MiB/ArrayBuffer16MiB growth budgets; neither trough nor peak envelopes
expanded after warmup relative to the second window. This is observed-envelope
evidence, not a stationary-distribution or universal memory-bound proof.
[Receipt](benchmarks/http-natural-frozen-2h-qualification-2026-11-02.json).

That runtime predates shared domains/owner propagation/core selection and later
UI retirement. It cannot qualify current advanced.js/reactivity.js/wasm-bridge.js.
Prepared a new immutable harness manifest and launched full7200000ms natural-GC
qualification on installed consumer LPJBKt, exec86721. All20 runtime JS/WASM
hashes fixed; same explicit budgets. [Run metadata](benchmarks/http-natural-current-run-2026-11-02.json).
The earlier24-minute failure remains preserved and its primary cause unresolved.
Gzip/both-codec/proxy/kernel saturation and long Edge workload remain open.


## 2026-11-02 — retained UI callback GC in real workerd

New Edge retained-ui fixture exercises both independently compiled compatible
copies across16 isolated requests. It unmounts eight failed boundaries and eight
routers, retaining40 references (retry, router object, reader, navigate and guard
remover). After every carrier closes, terminal callbacks/readers still satisfy
their contracts; graph0 is asserted through public telemetry.

The same fixture/driver on the previous installed runtime retains all32 weak
targets (eight boundary cores/Memory plus eight router cores/Memory). Current
stable and beta/November workerd collect all32 with references held, alongside
legacy140 and cross-copy24 targets. Live controls4 survive held phase and collect
after release; inspector GC commands are acknowledged. The driver includes UI
inspection in its convergence condition and fails retained-ui-retention even if
legacy targets pass. Package gate requires exact request/reference/target-kind
accounting and zero UI targets in both GC phases.

Raw: [before](benchmarks/edge-retained-ui-before-2026-11-02.json),
[stable](benchmarks/edge-retained-ui-after-2026-11-02.json),
[beta](benchmarks/edge-retained-ui-beta-2026-11-02.json).
No runtime change this milestone. This is local workerd+LinkeDOM inspector-GC
qualification, not deployed Edge, natural cadence/RSS bounds or History API
support. The new frozen two-hour current-runtime HTTP run remains independent.

Final installed-package gate passed, including mandatory retained UI GC and
subsequent HTTP/codec probes. Runtime remains byte-equal to prior1296/99 artifact.
[Receipt](benchmarks/edge-retained-ui-qualification-2026-11-02.json).


## 2026-11-02 — preserve live route-table replacement

Review of the retired matcher found a regression introduced by extracting its
pure closure: routerMatcher captured options.routes once, so replacing the array
while a router was live no longer affected match/navigation. Added a source and
installed-package regression; previous artifact passes3 copy cases but fails the
new fourth route-table case.

The isolated matcher now reads liveOptions.routes for each live match, remembers
the last observed table and drops liveOptions on disposal. Retired matching uses
that table without invoking later options getters or retaining the config/guard
environment. Terminal snapshot/late navigate behavior is unchanged. Full retained
UI probe still releases all8 core/Memory pairs with12 handles held. README now
states the live/terminal route-table contract.

First full suite:1296 functional checks passed, synthetic benchmark exceeded its
unchanged5s limit (6.76s) during host load. Preserved raw; rerunning full suite
with one worker. A prematurely started package check was intentionally stopped
to avoid competing with that benchmark and will be rerun after the suite.
The currently running frozen2h HTTP check remains on named LPJBKt baseline;
it does not cover this later advanced.js revision.

Single-worker full repeat passed1297/99 with the unchanged benchmark5s limit.
TypeScript and HTTP actions4/4 passed; final package gate running sequentially.

Final package gate passed:4/4 copy-router cases, Node retained UI0/8 pairs
with12 references and Edge retained UI0/32 targets with40 references.
[Receipt](benchmarks/router-route-table-qualification-2026-11-02.json).


## 2026-11-02 — native browser dynamic route tables

Expanded installed cross-copy browser fixture with two route-table replacement
cases. In each direction, one copy creates the router, the other RouterView
renders the initial route, observes the replaced options.routes table and mounts
the new parameterized component. Previous route cleanup1, new params/DOM,
terminal matcher after an options getter is made throwing, late navigate=false
and graph0 are asserted.

Same final fixture on prior installed LPJBKt:17 pass/2 new cases fail. Current
PSJdXD:19/19 pass in Chrome156 native DOM with prior explicit inter-case pacing.
Fixture and installed advanced.js hashes verified. No runtime change this
milestone; scoped diff check passed; both tabs and preview servers closed.
[Receipt](benchmarks/router-table-browser-qualification-2026-11-02.json).
Unpaced continuous history stress, other browsers, realms/versions and the
latest revision's long soak remain open; active frozen2h run still names LPJBKt.


## 2026-11-02 — Rust source to installed WASM identity gate

Native locked Rust suite passed53/53 on pinned rustc1.97.1. Two independent
release/simd128 builds in separate target directories compiled successfully and
are byte-identical:54320 bytes, SHA256
9aba5a7f785fff76022772c6a357dfb3d38cc479960bd2f52db6631b9da07340.
Those bytes match the checked-in WASM_BASE64, declared exports, installed public
core.wasm and installed wasm-binary.js decode/export metadata.

Added check-wasm-provenance.mjs, recording current Rust/lock/toolchain-file hashes
and exact binary/export comparisons. A valid WASM negative control with an extra
custom section retains identical exports but is rejected for byte mismatch.
CI now rebuilds/checks before npm build can regenerate the embedded source, then
sets SVET_REBUILT_WASM for mandatory installed-tarball verification. Ordinary
package-only runs can omit it and do not claim Rust rebuild provenance.

CI YAML parsed using Ruby's standard YAML parser (Node/Python YAML packages were
unavailable). Script syntax/scoped diff checks passed. Remote Linux CI execution
and cross-host reproducibility are not claimed by these local macOS results.
Runtime JS/WASM is unchanged from the prior1297/99-qualified revision.

Final installed package gate passed with SVET_REBUILT_WASM enabled, alongside
Edge inspector GC and existing HTTP/codec checks.
[Receipt](benchmarks/rust-wasm-provenance-qualification-2026-11-02.json).


## 2026-11-02 — compiled Forma AOT factory lifetime

Integration review found module-level AOT _activeInstances retained every created
instance, including production never-mounted/disposed components. The generated
factory now creates the registry only for HMR, registers only after successful
mount, removes on explicit dispose and counts mounted views so one unmount cannot
unregister another live view. Returned unmount is idempotent and unregisters in
finally even if underlying teardown throws.

Same final probe before:production module retains8 core/Memory targets and both
HMR cases fail. After:all3 cases pass,8 targets collect with module held and
controls4 held/0 released. HMR checks cover never-mounted inactivity, two views,
repeated unmount, a changed AST rendering Reloaded2 while preserving state2,
last-view unregister and explicit dispose unregister. The3-case probe is mandatory
in installed-package gate. JS1297/99, TypeScript and HTTP actions4/4 passed.

Separately, source audit confirms native Studio --svet currently selects metadata
and shared vector paint_native path, without a packages/svet core adapter. Native
Studio integration must remain open until a real graph/renderer/lifetime adapter
is implemented and qualified. [Audit](benchmarks/native-studio-svet-integration-audit-2026-11-02.json).
The browser .ui AOT path is distinct from that native integration requirement.

Final installed-package gate passed with mandatory3-case AOT lifecycle/GC probe,
Rust provenance and Edge inspector GC.
[Receipt](benchmarks/forma-aot-lifecycle-qualification-2026-11-02.json).


## 2026-11-02 — native C-ABI graph isolation

Native reuse audit found process-wide UnsafeCell<ReactiveGraph> with unsafe Sync
and no synchronization. Added a sequential cross-thread test: parent creates one
signal, child must see an empty context. Before it deterministically observes1
rather than0; calls are serialized around join so the negative test does not run
the unsynchronized data race itself.

Native C-ABI storage now uses thread_local RefCell<ReactiveGraph>; the shipped
single-threaded wasm32 path retains its per-instance graph. Independent native
windows on one thread must own ReactiveGraph values directly. Added concurrent
8-thread test with256 write/flush/read cycles per thread, own buffer IDs, graph
isolation and disposal. Native full suite55/55 passed. Rebuilt/re-embedded WASM
remains54320 bytes with the same exports, new digest
2bbbcad5fd0859be0b465555c77aff78d93e8dc1fdc73a86ac501718a79508ea.
Provenance guard passes the rebuilt/embedded/runtime comparison.

This is a native safety/context prerequisite, not the native Studio adapter or
renderer integration. Buffer/ID thread ownership is documented. The active
frozen LPJBKt2h HTTP run still measures its older immutable artifact and cannot
qualify this later core revision.

Final55 Rust tests, JS1297/99, TypeScript, actions4/4 and installed package with
Rust provenance/Edge GC passed. Second independent release build is byte-identical
to the new embedded and installed binary.
[Receipt](benchmarks/native-abi-thread-qualification-2026-11-02.json).


## 2026-11-02 — owned native graph adapter primitives

Added native-only native::Context using an owned ReactiveGraph, with weak context
identity and monotonic64-bit allocation serials. Public Node handles validate
context/kind/lifetime before mutation and implement identity-safe equality/hash.
Old queued and already-drained jobs cannot execute a recycled observer; reset
invalidates old slots without reusing serials. Context drop expires weak handles,
and the owned graph can move between threads independently of thread-local ABI.

Tracking and batch scopes restore on unwind; reset refuses active scopes. Native
hosts can evaluate nested computed values through Tracking::evaluate, register
current dependencies, reject recursive/transitive cycles and take deduplicated
ready effect handles. Host owns values/cache/retry/rendering; partial tracking
reads commit on unwind, with that policy documented. This is adapter infrastructure,
not automatic value evaluation, component ownership or native window integration.

Rust suite66/66 passed, including new context/slot/cycle/diamond/unwind cases.
Native binding example executes Count1→3→10 with one job per batch, dynamic branch
switch, ignored retired dependency and graph0. Added its execution to CI. wasm32
example compilation and release provenance checks passed; all20 runtime JS/WASM
files remain byte-equal to the prior1297/99-qualified artifact. No web runtime
change, so that qualification remains applicable to those exact files.
Native Studio graph/renderer/lifetime bridge remains open.

[Owned native context receipt](benchmarks/native-owned-context-qualification-2026-11-02.json)
binds source/CI hashes to native tests/example and unchanged web artifact evidence.

## Native Forma binding scheduler — 2026-11-02

`crates/forma` now depends on the local Svet Rust crate only on native targets.
Each opt-in Form owns a Context with one signal/effect pair per live Field.
Property callbacks notify graph sources; sync drains and re-tracks effects
before dirtying fields and updating Runtime. Identity-safe job lookup avoids
slot reuse aliasing; field drop disposes graph nodes and subscriptions. Form
drop releases its graph even when external Control handles survive. Arbitrary
wake callbacks run outside the graph RefCell borrow.

117 native library tests pass, including actual Runtime values, two-way input,
repeat-write dedup, context replacement, reentrant wake, pending backend switch,
queued binding replacement and retained-Control teardown. wasm32 library check
passes. The first two-way fixture lacked PointerArea and could not focus its
editor; adding the real interactive template made the input assertion pass.
The full final suite includes that assertion. This is typed Form scheduling;
Studio main/retained Document/window lifecycle integration remains open.
Evidence: `benchmarks/native-form-bindings-qualification-2026-11-02.json`.

## Native retained Document scheduling — 2026-11-02

Studio `--svet`, F7 backend selection and hot reload now select an owned
Svet document graph. Each state property has a signal; a structural signal
handles field removal, and a render effect subscribes to all current fields.
Runtime synchronizes actual editor/toggle values into Document, reconciles
signals, drains the render job, rebuilds the scene and transfers the graph
into the replacement Runtime. Render acknowledgement follows successful scene
construction; pending work survives failed construction and backend changes.
Errors from native binding sync are surfaced through the render error channel.

The native library suite passes 119 tests, including real `.ui` compilation,
two consecutive editor updates, dependent summary text, retained focus, stable
graph slots, separate runtimes and backend switching. The document-graph test
covers pending retry and property retirement. Native binary and wasm32 checks
are recorded separately in the qualification receipt. Shared vector painting
remains; no independent Svet renderer or interactive window qualification is
claimed. Next: actual Studio window input/redraw/hot reload/close, auxiliary
window policy, error recovery and retained-resource evidence.

## Native window presentation qualification — 2026-11-02

A real AppKit window exposed two presentation defects that offscreen GPU tests
did not cover: unset CAMetalLayer contentsScale magnified/cropped the initial
Retina scene, and the retained softbuffer sibling covered later Metal frames
after CPU→GPU switching. Runtime/inspector values changed behind a stale image.
The Metal shim now sets the current window scale, raises Metal above the CPU
sibling while GPU rendering is active, and hides Metal on CPU frames/fallback.

Final locked native build was exercised through native UI input: seed→seed1 on
GPU, seed12 on CPU, seed123 after switching back to GPU; hot reload changed the
title while preserving state, and further input produced seed1234 in both
editor and dependent summary. Screenshots confirm pixels, not just a backend
label. Closing the window terminated the process (pgrep exit1). Three hardware
GPU tests pass on Apple M4 Max/Metal, including a new retained .ui editor bounds
probe at DPI1/1.25/2. Existing analytic-text and animation tests remain separate.
The new probe deliberately avoids comparing independent CPU/GPU glyph AA byte
for byte; an initial strict comparison differed at fractional DPI.

CUA typeText/paste attempts timed out; direct key events succeeded. Clipboard,
IME, native accessibility control exposure, injected fallback/device loss,
multi-monitor DPI, auxiliary windows and long-running resource bounds remain
open. Qualification: `benchmarks/native-window-qualification-2026-11-02.json`.

## Retained-document error recovery — 2026-11-02

A negative regression proved that state-dependent template errors made native
sync unrecoverable: the next sync rendered the invalid model before reading
corrective editor input. Runtime now retains the last committed scene's binding
map and uses it when preflight rendering fails. Pending render work survives;
a successful rebuild replaces the cache and acknowledges the graph. Toggle
reads use the current model rather than the old scene snapshot. Ordinary
Document callers retain the existing render-and-sync entry point.

Native Studio keeps binding errors separately from presentation errors, so a
successful GPU redraw of the old frame does not hide a failed template. A live
window test typed seed1 to trigger Expected scalar text, then seed123 to recover
both editor and dependent summary without restart; the error title cleared.
The full native library suite passes120 tests. Recovery runs in legacy and Svet
modes and across both pending backend switches; repeated toggles, graph bounds
and restored scopes are asserted. Locked native build, wasm32 check and three
physical Metal GPU tests pass. Collection reordering, malformed binding metadata,
partial-write transactions and the wider failure matrix remain open.
Evidence: `benchmarks/native-binding-recovery-qualification-2026-11-02.json`.

## Atomic native binding updates — 2026-11-02

The negative control proved a partial model write: the first editor committed
its value before malformed metadata in the second binding returned an error.
Binding synchronization now stages the root scope and commits only after all
bindings succeed. Successful sequential/alias semantics are preserved; errors
leave all scopes unchanged, including an initially empty states vector.
Runtime restores an uncommitted activation after binding failure, allowing one
activation to retry after metadata repair without replay after successful commit.

Rendering and recovery share bounded source annotation. Depth64/node16384
limits are checked before recursively cloning children, avoiding clone-before-
validation for the structural scene tree. This does not qualify nested Value
payload depth. Regression cases cover public/cached binding maps, empty states,
repair/retry, both structural limits and legacy/Svet activation retry through
two failures. Full library suite123/123, native binary check and wasm32 check
pass. Source hashes and logs are in
`benchmarks/native-binding-transaction-qualification-2026-11-02.json`.

Large-model staging cost, multiple newly arriving activations during failure,
collection identity/reordering, and the full malformed-input matrix remain open.
No additional window/GPU qualification is inferred from these headless checks.

## Ordered retained-document activations — 2026-11-02

The previous single last-event slot lost all but the last activation while
binding metadata was invalid. The negative control queued two toggles and
observed true instead of false after repair. Retained Runtime now records
keyboard, pointer and direct activations in one ordered queue. A binding batch
samples current widget values once, then replays activations against staged
state in capture order. Errors retain the queue; acceptance consumes it once.
A later render error does not replay already committed activations.

The queue admits4096 activations per runtime. Overflow poisons synchronization
with an explicit error while leaving the model unchanged; it never commits an
unknown partial prefix. Hosts can inspect `pending_ui_binding_activations()`
and explicitly discard buffered events using `cancel_ui_binding_activations()`;
document replacement also resets interaction state. These host APIs compile
through wasm-bindgen; no new browser runtime qualification is claimed.

Full library126/126, native binary check and wasm32 check pass. Regressions
cover1–3 toggles, selected/toggle ordering on shared state, repeated refusals,
all three activation sources, overflow/cancel/recovery and Svet's no-render
case when even toggles leave final state unchanged. Snapshot-value semantics,
collection reorder/reload migration, admission-limit latency and IME/clipboard
remain open. Evidence:
`benchmarks/native-activation-queue-qualification-2026-11-02.json`.

## Stable collection binding identity — 2026-11-02

A negative regression proved wrong-recipient mutation: a captured activation
on item a at runtime index0 updated b after model reorder. Binding routing now
associates expanded controls with actual widgets by stable control key. New
controls have no old widget to read; removed controls cannot inherit events.

For cached-scene recovery, For expansion retains the observed item key, its
expression and evaluation scope. Writes resolve the current index in a root-
backed collection by that key. Duplicate keys fail before atomic commit. Toggle
reads use the current item object directly: raw Value::Expr does not parse the
constructed bracket syntax previously used by the unqualified collection path.

Library130/130, native binary check and wasm32 check pass. Tests cover fresh
and cached maps, reorder/remove/insert, editor values, repeated activations,
key ambiguity and native .ui compilation into retained Runtime. The integration
case reorders items during a failing template, accepts the original activation
once, then recovers in legacy/Svet modes using a props-dependent key expression.
Fixture corrections during development added the required Rectangle template
root and used the supported `for item in ... key ...` source syntax.

Root-backed paths are covered. Nested/derived collections, cross-document event
migration, component replacement with reused keys, model/widget conflict policy
and large-model key-search/snapshot costs remain open. This is headless native
evidence, not an additional window/GPU qualification. Receipt:
`benchmarks/native-binding-key-qualification-2026-11-02.json`.

## Model/widget value arbitration — 2026-11-02

The negative control showed two writes for one actual widget change: an
untouched editor overwrote a newer model value. Runtime now retains a last-
accepted text/range value snapshot. Existing model values are preserved when
the widget is unchanged; pending widget changes win their binding. Missing
model paths can still initialize from the widget. Baselines acknowledge only
a successful atomic binding commit, so metadata refusal preserves input for
retry and a later rendering failure does not replay an accepted value.
Acknowledgement reuses stored strings/map entries when possible.

Library132/132 plus five existing runtime allocation tests pass, as do native
binary and wasm32 checks. Regressions cover text/range, same-field concurrent
model change, later model updates after acknowledgement, untouched collection
items across reorder/insertion, and render-error recovery corrected from the
model in legacy/Svet and pending-switch cases. Those allocation tests do not
prove zero allocations for document sync. Receipt:
`benchmarks/native-widget-conflict-qualification-2026-11-02.json`.

This is value-snapshot arbitration, not a revision journal: net edits returning
to baseline count as unchanged. Multi-owner Documents sharing one Runtime,
large-model time/peak memory and the full conflict matrix remain open.

## Binding allocation experiment — 2026-11-02

Added `crates/forma/examples/binding_sync_bench.rs`: public Document sync on
8/32/96 keyed text editors with seven alternating noop/one-edit samples. An
initial transaction-local key-index cache failed to demonstrate a stable speed
win and regressed small cases; it was removed. Those A/B/A/B raw results remain
in `native-binding-sync-analysis-2026-11-02.json`.

The retained change borrows collection data for inspection rather than cloning
whole lists; a synthetic-member fallback preserves unchanged-field behavior.
Resolver collections above16384 items are refused before building key context.
Two isolated source trees built the clone reference and retained variant with
the same thread-local counting allocator/harness. Repeated counters matched:
96-editor noop allocations920721→874257; requested bytes116833294→110253175.
One-edit counts920241→874259 and bytes116767846→110253187. This is about5% fewer
allocation calls and5.6% fewer requested bytes, not a live/peak/RSS reduction.

Time did not reliably improve. Latest96-editor one-edit median ratios were
1.004 and1.076; possible latency regression remains open. Instrumentation adds
overhead, and the existing HTTP soak/host activity continued. No app frame-time
or GPU/graph throughput result follows. The remaining≈110 MB requested per sync
is excessive; profile preflight/render vs binding work and redesign shared
state/scope representation before claiming large-model readiness.

135 library tests and five existing allocation-budget tests pass, along with
native binary and wasm32 checks. New regressions cover key-changing writes,
distinct outer local scopes, oversized collection refusal and unchanged virtual
member preservation. Source archive, hashes, raw data and build logs:
`benchmarks/native-binding-copy-qualification-2026-11-02.json`.

Reproduce the retained workload from the repository root:

```sh
CARGO_TARGET_DIR=/tmp/forma-binding-bench cargo run --locked --release --manifest-path crates/forma/Cargo.toml --example binding_sync_bench
```

Use a quiet host for timing qualification; allocation counters are calling-
thread cumulative requests and include realloc's requested new size.

## Render-phase allocation attribution — 2026-11-02

The public benchmark now measures render_scene separately before each noop/edit
pair, using the same thread-local allocator counter. Rendering accounted for
88–93% of requested bytes on8/32/96-editor Column scenes. Inspection found
discarded copies: scene expansion cloned props/children before replacing them,
layout visualization cloned the old children before constructing new ones,
and root measurement always cloned even without a kind conversion.

Expanded/visual Nodes now construct their final fields directly. Measuring
borrows the root unless legacy Frame→Column adaptation needs an owned copy.
State/scope values, public ownership and scene semantics remain the same.
Native library135/135, allocation5/5 and three physical Metal GPU tests pass,
plus native binary and wasm32 checks.

Repeated A1/B1/A2/B2 counters agree. On96 editors, render requested bytes
97090226→83777999 (-13.7%); full noop sync110253175→96940948 (-12.1%) and
allocation calls874257→774668 (-11.4%). One-edit counters show the same
reduction. These are cumulative calling-thread requests, not retained/peak
memory. Time remains inconclusive: one-edit ratios0.741/1.077 and render
0.607/1.046 under existing host activity; no FPS or stable speed claim follows.

The remaining≈97 MB per sync is excessive. Shared state/scope representation,
preflight scheduling and larger/Frame-root application workloads remain open.
Raw data, source archive, hashes and build/test logs:
`benchmarks/native-render-phases-qualification-2026-11-02.json`.

## Instance override copying and final HTTP audit — 2026-11-02

Instantiation previously cloned all supplied props twice: before group selection
and again to restore explicit precedence. Only selected keys can have been
overwritten. The second step now restores that intersection, keeping metadata
visible without copying unrelated __state snapshots again. Existing snapshot
selection coverage now also asserts explicit flag/text precedence and an
expression reading props.__state.caption. Library135/135, allocation5/5, three
physical Metal GPU tests, native and wasm32 checks pass.

A1/B1/A2/B2 release counts repeat exactly. At96 editors, sync requested bytes
96940948→90393124 (-6.8%) and calls774668→725708 (-6.3%); render bytes
83777999→77230175 (-7.8%). One-edit timing ratios0.980/0.899 are local observed
results under the instrumented workload, not application FPS/p95 qualification.
The remaining≈90 MB per sync still needs architectural state/scope work.
Source archive and evidence: native-instance-overrides-qualification-2026-11-02.json.

The LPJBKt frozen HTTP report was already final despite a live-looking legacy
exec handle. Independent manifest/file hashing and final-report analysis passed:
918440 requests,7200008ms, zero request/cleanup failures, forcedGC0 and graph0.
All four caller memory budgets passed; external peak-envelope growth63344093
bytes is close to the67108864 budget, so no universal plateau claim follows.
Receipt: http-natural-current-2h-qualification-2026-11-02.json.

Correction to earlier benchmark context: final HTTP measurements ended at
2026-11-02T00:31:24.645Z. Later copy/render benchmark artifacts were written
after that completion; the old session handle did not establish active HTTP
request churn. Do not attribute those timing variations to that soak. Raw
samples and no-general-speedup conclusions stand. Details are in
native-benchmark-http-overlap-correction-2026-11-02.json. Historical receipts
remain intact; this correction supersedes their concurrent-HTTP assumption.

Latest dist differs from LPJBKt in advanced.js, forma.js, svet-core.wasm and
wasm-binary.js. A fresh2h natural soak now uses frozen harness snapshot-JCOGcm
and installed09qhPH; all20 artifacts matched latest dist at launch. Its process
was observed live (pid99442), checkpoint only. Run metadata:
http-natural-latest-run-2026-11-02.json. Do not qualify it until a final report.

## Borrowed inherited scene scopes — 2026-11-02

Scene expansion now uses Cow for inherited state/props and container child state.
Read-only traversal borrows; nonempty node-local overlays, scope resolution and
Let writes detach. Writable For/Let locals and returned __state/key-scope
snapshots still materialize owned maps. Public Node/Properties/SceneControl
representation is unchanged.

A new regression renders parent/node/later/inner/later across five controls,
asserting that node-local and nested Let writes stay local and old returned
snapshots remain unchanged after model mutation. Native library136/136, five
existing allocation budgets, three physical Metal GPU tests, native binary
check and wasm32 check pass.

A1/B1/A2/B2 counters agree exactly. On96 editors, full noop sync requested
bytes90393124→83842125 (-7.2%) and allocation calls725708→677030 (-6.7%).
Render bytes77230175→70679176 (-8.5%). These are cumulative calling-thread
requests, not retained/peak memory or RSS. Timing is volatile/inconclusive;
latest HTTP checkpoints confirm live request churn during this experiment
(70472→71648 requests). No app FPS/p95 or quiet-host qualification is claimed.

Owned output snapshots and writable For locals still copy root model data;
≈84 MB per96-editor sync remains excessive. Further sharing, preflight
scheduling and large-model memory/latency qualification remain open.
Evidence: `benchmarks/native-scope-cow-qualification-2026-11-02.json`.

### Native output snapshot transfer — 2026-11-02

`render_scene` consumes positioned instances and moves their owned `__state`
into `SceneControl` after all template evaluation, including Slider endpoints.
The metadata remains available to `props.__state` until its last use; returned
snapshots retain ownership and isolation. No public types changed.

Lib136/136, allocation5/5, native-bin/wasm32 checks passed. Release harness
A1/B1/A2/B2 on8/32/96 editors produced repeat-identical allocation arrays.
For96 editors, final-sample public sync requested bytes83,842,125→77,423,306
(−7.66%), calls677,030→629,318 (−7.05%); render requested bytes
70,679,176→64,260,357 (−9.08%). These are cumulative allocator requests,
not peak/live memory. HTTP soak PID99442 was observed live; noisy timings
do not qualify latency or FPS. Large-model state/For sharing remains open.
No physical GPU rerun for this ownership-only change.
[Receipt, raw samples and source archive](benchmarks/native-snapshot-move-qualification-2026-11-02.json).

### Native precomputed visual ownership — 2026-11-02

Consumed precomputed visual nodes now transfer their props through `mem::take`
instead of cloning both props and the complete node. Serialization rectangles
are constructed with empty children directly instead of cloning/discarding them.
Layout, metadata and public snapshot types are unchanged.

Lib136/136, allocation5/5, native-bin/wasm32 checks and physical Metal GPU3/3
passed. Release A1/B1/A2/B2 allocation arrays match across repeats. On96 editors,
public noop sync requested bytes77,423,306→76,702,708 (−0.93%), allocation
calls629,318→621,158; render bytes64,260,357→63,539,759 (−1.12%).
Requested bytes are cumulative, not RSS/peak. Active HTTP soak and other host
compilers were observed; no latency/FPS qualification. Shared scopes/For locals
and redundant preflight remain open.
[Receipt and raw evidence](benchmarks/native-visual-move-qualification-2026-11-02.json).

### Native dimension measurement without tree copy — 2026-11-02

Dimension measurement temporarily swaps resolved properties into the existing
visual node, then restores rectangle properties before flattening/serialization.
Natural dimensions still inspect the complete children; errors return without
publishing a scene. No dimension-node or resolved-properties clone is needed.
Lib136/136, allocation5/5, native/wasm32 checks and physical GPU3/3 passed.
Release A1/B1/A2/B2 allocation arrays repeat identically: on96 editors,
noop sync requested bytes76,702,708→76,342,937; calls621,158→616,934.
Render bytes63,539,759→63,179,988. These are cumulative requests, not RSS/peak.
HTTP soak remains active; no latency/FPS claim. Next candidate is the full
root-state clone per For iteration, followed by shared scope/preflight work.
[Receipt and raw evidence](benchmarks/native-dimension-move-qualification-2026-11-02.json).

### Native For local-state reuse — 2026-11-02

For now clones inherited state lazily once, then reuses its local map across
iterations. Item/index are overwritten; inherited binding key/expression/scope
metadata are restored before each key evaluation, preventing prior-iteration
metadata from leaking and preserving nested loops shadowing the same item name.
Child expansion receives a borrowed map and returned snapshots remain owned.
New regression checks nested shadowing, outer item restoration, keys, no root
mutation and old snapshots surviving a subsequent model change/render.

Lib137/137, allocation5/5 and native/wasm32 checks passed. Release A1/B1/A2/B2
allocation arrays repeat identically. On96 editors, noop sync requested bytes
76,342,937→70,064,493 (−8.22%), calls616,934→571,052; render requested
bytes63,179,988→56,901,544 (−9.94%). These are cumulative requests, not
RSS/peak. Active HTTP soak observed; no latency/FPS claim or new hardware GPU
qualification this turn. Public snapshot cloning/shared scopes/preflight remain open.
[Receipt and source archive](benchmarks/native-for-scope-qualification-2026-11-02.json).

### Retained Runtime skips unchanged render preflight — 2026-11-02

Runtime keeps one owned snapshot of displayed document states. When those states
are unchanged, binding sync borrows the committed scene mapping directly instead
of rendering templates or cloning the fallback map. Changed states still evaluate
a fresh map for identity reconciliation; failures use the displayed map and retain
pending retry. Snapshot advances only after successful Runtime creation. Source
replacement installs a new Runtime/snapshot. Public Document sync is unchanged.

Model-only changes now request a rebuild in both legacy and Svet, even without
widget writes. New regression checks actual editor value and subsequent idle.
Lib138/138, allocation6/6, hardware Metal GPU3/3 and native/wasm32 checks passed.
96-editor retained idle sync observed141761 allocations (budget<200000); an edit
rebuilds successfully. This is still substantial; state staging/key resolution
remain open. One extra model snapshot is retained; peak memory and latency are
not qualified, and no cross-API speed comparison is claimed.
[Receipt and sources](benchmarks/native-runtime-preflight-qualification-2026-11-02.json).

### Lazy binding transaction staging — 2026-11-02

Binding transactions borrow root state until the first attempted write, then
clone once into the private staging map. Only successful complete transactions
commit the owned map; a read-only sync preserves the original root. Empty-state
initialization behavior is preserved. Direct unchanged-field presence checks
borrow actual fields first and retain fallback lookup for synthetic members.

Lib138/138, allocation7/7, native-bin/wasm32 checks passed. New allocation
regression with one scalar editor and1024 unrelated records observed23 allocations
(budget<256), then verifies an edit and preserved payload. Retained idle96 editors
observed141278 allocations: collection-key resolution is still the main open
allocation path. Existing transaction refusal/error/reorder/conflict tests pass.
No new GPU or release timing qualification; no peak-memory/latency claim.
[Receipt and sources](benchmarks/native-lazy-staging-qualification-2026-11-02.json).

### Borrowed scalar collection keys — 2026-11-02

Plain item-member scalar keys now read actual item fields by reference. Full
evaluator context is created lazily for computed/missing/synthetic keys. Exact
dotted root/local fields, enum precedence and reserved namespaces disable direct
lookup; duplicate-key validation and collection limit remain. No index cache.
New regression mixes direct/computed ids, reverses collection order, and checks
exact dotted-field precedence still refuses duplicate keys. Existing scoped key
expressions and key-changing write regressions pass.

Lib139/139, allocation7/7 and native/wasm32 checks passed. The same retained
idle96-editor workload decreased141278→30494 allocation calls (−78.4%); budget
tightened to<40000 and passed. Scalar+1024 unrelated records remains23 calls.
These are debug allocation counters, not release latency/peak memory; scanning
remains quadratic across controls and complex expressions still clone contexts.
No new physical GPU qualification this turn.
[Receipt and sources](benchmarks/native-key-borrow-qualification-2026-11-02.json).

### Collection key string ownership — 2026-11-02

Key encoding borrows text instead of cloning it; duplicate validation consumes
the encoded key after target comparison. Complete scanning/refusal behavior
remains. New key-format regression covers UTF8 byte lengths, empty strings,
typed string/number distinction, negatives and invalid/nonfinite values.
Lib140/140, allocation7/7 and native/wasm32 checks passed. Same96-editor idle
allocation workload30494→12062 calls (−60.4%); budget<16000 passes.
Scalar+1024 unrelated records remains23 calls. Quadratic duplicate scans remain
open. No release latency/FPS/peak-memory or new GPU qualification claim.
[Receipt and sources](benchmarks/native-key-strings-qualification-2026-11-02.json).

### Transaction-local direct-key indexes — 2026-11-02

A transaction now builds an index once for each root collection/member path
whose keys are all direct scalar reads. Full duplicate validation completes
before publishing the index. Computed keys and scope-dependent expressions do
not populate it. Every successful model write clears all indexes, preserving
sequential bindings and immediate key-change validation; no cross-sync cache.
This differs from the previously rejected general scope cache: no cloned
contexts or retained invalidation state.

Lib140/140, allocation7/7, native/wasm32 checks passed. Existing key-changing
write regression confirms new duplicates refuse and roll back; outer local
scopes, mixed computed keys, reorder and recovery tests pass. Same96-editor
idle workload12062→1331 allocation calls (−89.0%); tightened budget<2000
passes. This supports the allocation change, not release latency/FPS or peak
memory. Complex keys/frequent writes can still rescan quadratically. No new
physical GPU run this turn.
[Receipt and sources](benchmarks/native-transaction-index-qualification-2026-11-02.json).

### Release retained Runtime workload — 2026-11-02

`binding_sync_bench --retained` now measures real retained Runtime idle sync
and an edit including scene rendering and Runtime replacement, separately for
legacy/Svet and8/32/96/256 editors. Seven samples per case, two sequential
processes. Each sample validates model/widget values and item count; Svet
tracking/batch depths return to zero. All allocation arrays repeat identically.

Svet96 steady idle:1816 calls/108377 requested bytes; edit434262/57463805.
Svet256: idle4771/278501; edit2796021/377214957. Per-run median edit times
were93.45/95.47ms for256, idle0.403/0.397ms. Legacy256 edit medians
109.84/94.58ms; these do not establish relative backend performance. HTTP
PID99442 was observed active. Timing includes allocator instrumentation and
excludes paint/presentation; no p95/FPS/peak-memory or isolated-host claim.

The results move priority from idle key scanning to owned per-control model
snapshots and full edit-time scene reconstruction. Interactive readiness at
256 editors remains open.
[Receipt, raw results and source archive](benchmarks/native-retained-runtime-qualification-2026-11-02.json).

### Owned layout normalization — 2026-11-02

`normalize_layout_owned` consumes intermediate scene/layout trees instead of
cloning them again. The borrowed normalization wrapper preserves existing callers;
scene_visual and position_scene now pass ownership where already available.
Lib140/140, allocation7/7, native/wasm32 checks and hardware GPU3/3 passed.
Release retained A1/B1/A2/B2 allocation counters match across repeats. Svet256
edit requested bytes377,214,957→330,374,486 (−12.4%), calls2,796,021→2,447,329
(−12.5%); idle unchanged. Timing is highly variable: before117.85/147.87ms,
after80.62/187.45ms. No speed/FPS/peak-memory claim. Full model snapshots and
remaining tree copies still prevent large-model readiness.
[Receipt and raw evidence](benchmarks/native-owned-layout-qualification-2026-11-02.json).

### Owned scene positioning — 2026-11-02

Positioning now consumes expanded Node children recursively. A root/Scroll shell
retains serialization properties while children move into the positioning pass;
leaf nodes enter the instance list directly. This removes repeated subtree and
per-control state snapshot clones without changing public snapshot types.
Lib140/140, allocation7/7, native/wasm32 and physical GPU3/3 passed. Release
A1/B1/A2/B2 counters repeat identically for both schedulers at8/32/96/256.
Svet256 edit requested bytes330,374,486→239,390,366 (−27.5%), allocation
calls2,447,329→1,775,584 (−27.4%); idle unchanged. Edit median before
88.47/76.56ms, after57.74/57.39ms in this run. Host non-isolated with active
HTTP soak: no FPS/p95/peak-memory qualification. Even57ms excludes painting;
large-model interactive readiness remains open. Precomputed visual and owned
public snapshot copies remain targets.
[Receipt and raw evidence](benchmarks/native-owned-position-qualification-2026-11-02.json).

### Precomputed visual ownership through positioning — 2026-11-02

Positioning consumes precomputed visual trees and transfers each leaf to the
instance list; simple layouts consume visual children too. Measurement still
reads the same complete visual before moving its children.
Lib140/140, allocation7/7, native/wasm32 and physical GPU3/3 passed. Retained
release A1/B1/A2/B2 allocation arrays repeat identically. Svet256 edit bytes
239,390,366→238,427,731 (−0.40%), calls1,775,584→1,764,576 (−0.62%); idle
unchanged. Timing before195.28/120.17ms, after153.83/129.59ms is inconsistent
on the non-isolated host; no speed/FPS/peak-memory claim. Small visual-copy
benefit confirms per-control state snapshot copies remain the larger target.
[Receipt and sources](benchmarks/native-owned-visual-qualification-2026-11-02.json).

### Unchanged native scheduler state — 2026-11-02

DocumentGraph reconciliation returns its existing pending flag when model
states compare equal, avoiding graph re-tracking and another owned snapshot
on every idle sync. Failed renders still retry until acknowledged. Also fixed
topology notifications for adding/removing empty scopes: scope-count changes
now notify even when there are no property signals to add or retire.
New regression covers empty scope creation/removal, retry and final graph edges.
Lib141/141, allocation7/7 and native-bin check passed.96-editor allocation
regression now enables Svet and asserts its idle allocation count equals legacy:
1331 calls each (previous Svet1816). No new release timing/peak memory or GPU
claim; native-only module. State equality traversal and edit-time snapshots
remain open.
[Receipt and sources](benchmarks/native-idle-graph-qualification-2026-11-02.json).

### Recursive model paths through lists — 2026-11-02

Model reads, borrowed presence checks and mutable collection lookup now traverse
list indices recursively, preserving exact dotted-object field priority on reads.
State writes descend into existing lists instead of replacing them with objects.
Nested synthetic length remains readable but is not a mutable collection target.
Out-of-range/non-numeric indices do not grow or replace lists.

Lib143/143, allocation7/7 and native/wasm32 checks passed. New regressions cover
nested reads/writes and unchanged state after invalid list writes. A real Runtime
built from an IR Document binds to `state.groups.0.items`; after reorder,
corrective input follows the original item key and preserves the other item.
This does not qualify compiler syntax/browser/AOT nor nested For collections
expressed via parent locals (`outer.children`); parent identity resolution stays
open. No GPU or performance claim.
[Receipt and source](benchmarks/native-nested-paths-qualification-2026-11-02.json).

### Stable parent identity for nested For bindings — 2026-11-02

Collection paths via parent locals (e.g. `outer.items`) resolve recursively
through retained parent stable keys into the current root-backed model path.
Fallback key evaluation receives current ancestor values; direct-key indexes
remain scoped to the resulting collection path and clear after writes.
Parent recursion is capped at64 for cyclic/malformed metadata.

Lib145/145, allocation7/7 and native/wasm32 checks passed. Real `.ui` regression
uses groups with repeated child ids across groups: fresh/cached maps route edits
correctly after parent+child reorder, ignore removed parents, and refuse duplicate
parent keys atomically. Cyclic parent metadata regression refuses boundedly.
This does not qualify shadowed parent names (outer metadata lost, depth refusal),
derived collections, deep computed-key matrices, browser/AOT or performance.
[Receipt and source](benchmarks/native-nested-for-qualification-2026-11-02.json).

### Direct parent item-name shadowing — 2026-11-02

When a nested For uses `for item in item.items`, expansion captures the inherited
lexical parent binding context before replacing item metadata. The resolver
traverses that context by reference to find the current parent identity instead
of recursing into the inner item again. Distinct-name loops keep their old path.
Lib146/146, allocation7/7 and native/wasm32 checks passed. Real `.ui` parent/child
reorder/removal/duplicate cases now run fresh/cached maps with distinct and equal
item names (12 cases); a separate three-consecutive-item-level regression edits
the deepest leaf. Malformed parent cycles still refuse boundedly.
Owned parent snapshots add copies in shadowing cases: performance not qualified.
Nonadjacent shadowing/arbitrary lexical chains and computed/derived collection
matrix remain open. No new GPU/browser/AOT qualification.
[Receipt and source](benchmarks/native-shadow-parent-qualification-2026-11-02.json).

### Nonadjacent item-name shadowing — 2026-11-02

A new For variable that shadows any inherited item now captures the pre-shadow
context, even if its immediate collection parent has another name. Parent path
resolution uses that lexical context for the collection rather than requiring
the immediate parent name to equal the new item name.
Lib146/146, allocation7/7, native/wasm32 checks passed. Three-level real `.ui`
regression covers item/item/item and item/branch/item with reordered root groups
and repeated mid/leaf ids, both fresh/cached binding maps. Earlier removal,
duplicate and malformed-cycle tests remain green.
This is not a complete lexical environment model: later references to earlier
locals through several shadowings and computed keys require a full scope-chain
representation/matrix. Parent context copying, derived collections and browser/
AOT qualification remain open. No performance/GPU claim.
[Receipt and source](benchmarks/native-nonadjacent-shadow-qualification-2026-11-02.json).

### Flat lexical binding frames — 2026-11-02

Expansion now emits a flat list of lexical binding frames plus active-name
indexes. Each frame records visible parent indexes at declaration time, its
collection/key/scope and inherited non-root locals. This replaces recursively
copied whole-state parent snapshots for new scenes. Parent resolution uses
the declaration context even when a later variable shadows an earlier name.
Per-resolution memoization avoids resolving the same ancestor repeatedly;
parents must point to earlier frames and depth stays bounded. Root-level
bindings retain their direct-key fast path.

Lib148/148, allocation7/7 and native/wasm32 checks passed. Real `.ui` regression
accesses earlier `branch.items` after an unrelated `item` shadow and uses
`item.id + leaf.id` as key; fresh/cached maps preserve identity after group
reorder. Adjacent/nonadjacent shadow, removal/duplicate and idle budgets pass.
New malformed-parent-frame regression refuses forward/self references.

This is not final scope qualification: Let/root-name shadowing needs explicit
lexical tracking, frame locals/public snapshots still copy data, and derived
collections/deep failure/browser/AOT/performance matrices remain open.
[Receipt and source](benchmarks/native-lexical-frames-qualification-2026-11-02.json).

### Explicit evaluated Let binding scopes — 2026-11-02

Let writes now retain evaluated lexical values separately from model fields.
Frames apply them after live ancestor resolution, before inserting their own
item. Let hides stale same-name For binding metadata; a new For hides same-name
Let metadata for descendants. Root-level frames with lexical values use the
lexical resolver too, so root model keys cannot overwrite Let locals.
Lib149/149, allocation7/7 and native/wasm32 checks passed. IR regression uses
Let prefix over a root prefix and a compound key; fresh/cached maps keep the
original target after root prefix changes and item reorder without overwriting
the new model prefix. Dynamic Let expression reevaluation, Let-derived aliases,
compiler/browser/AOT and performance matrices remain open. Values here are
evaluated lexical snapshots, not a completed reactive Let contract.
[Receipt and source](benchmarks/native-let-scopes-qualification-2026-11-02.json).

### Ordered dynamic Let replay for binding keys — 2026-11-02

Let metadata now records ordered expressions, captured prop scopes and visible
For parent frame indexes. Binding frame evaluation replays these steps using
current model state, prior Let results and each step's declaration-time parents.
Only still-visible lexical values overlay the key evaluation context. Replay
limits16384 steps and validates parent frames before recursive resolution.

Lib149/149, allocation7/7, native/wasm32 checks passed. Extended IR regression
covers constant/dynamic prefix followed by dependent suffix, changed model
prefix and reordered items, fresh/cached maps. Constant Let keeps identity;
changed dynamic Let changes key, so old input cannot migrate to a new identity.
Replay currently clones model state per step: no performance/peak-memory claim.
Let-derived aliases, full error/deep/compiler/browser/AOT matrix remain open.
[Receipt and source](benchmarks/native-dynamic-let-qualification-2026-11-02.json).

### Let replay transactional retry proof — 2026-11-02

Added a regression with a direct editor preceding a collection editor whose key
depends on Let replay. Removing the Let model dependency causes repeated refusal;
the earlier staged editor write does not commit. Restoring the dependency accepts
both retained edits once, subsequent sync writes zero, and a newer model query
is not overwritten by an unchanged widget. Fresh/cached mapping paths pass.
Lib150/150 and allocation7/7 passed; diff check clean. Test-only change, no new
production behavior or GPU/wasm/performance qualification. IR/native Runtime
proof only; full compiler/live-window/browser and alias/deep matrices stay open.
[Receipt](benchmarks/native-let-retry-qualification-2026-11-02.json).

### Reuse Let replay working environment — 2026-11-02

A binding frame now lazily clones its model environment once and reuses it
across ordered Let replay. Before each step, previous overlay fields restore
the current model value (or disappear when absent), then prior lexical results
and that step's live parents overlay it. This preserves step isolation without
a full model clone for every declaration.
Lib150/150, allocation7/7, native/wasm32 passed, including sequential dynamic/
constant keys, nested scopes and failure/retry rollback regressions. No measured
latency/bytes improvement claimed; one model copy per replayed frame and cloned
lexical values remain. Derived aliases/deep/compiler/browser matrix stays open.
[Receipt](benchmarks/native-let-replay-reuse-qualification-2026-11-02.json).

### Validate lexical frame routing headers — 2026-11-02

Binding resolution validates frame count≤64, active index type/finite integer/
nonnegative/range, matching item name and parent-map type before choosing a
frame. Partial/malformed lexical metadata now refuses rather than silently
using legacy flat routing. Maps with neither lexical field remain supported.
Lib150/150, allocation7/7 and native/wasm32 checks passed. Extended transaction
regression corrupts cached scene item index to−1 after an earlier editor write
would stage: whole model remains unchanged. Restoring the original map accepts
both retained edits once. This proves that failure path, not every malformed
metadata or external/backend matrix. No performance/GPU claim.
[Receipt](benchmarks/native-frame-validation-qualification-2026-11-02.json).

### Symbolic origin for direct Let collection aliases — 2026-11-02

Ordered Let replay tracks symbolic model paths for direct member references and
alias chains. Collection paths resolve via current lexical/parent context, so
a Let alias that shadows a root field routes to its real model origin. Computed
values do not gain a writable origin from matching list contents.
Lib151/151, allocation7/7 and native/wasm32 passed. IR regression covers
rows=state.items→chosen=rows; For chosen/state.chosen; conflicting root chosen
field; reordered items; fresh/cached maps. Original item receives the edit and
root chosen stays unchanged. A literal copied list with identical values has
no writable origin and leaves model data unchanged. Arbitrary computed/filter/
map collections, parent-member alias-specific matrix, compiler/browser/AOT/
window and performance qualification remain open.
[Receipt](benchmarks/native-let-alias-qualification-2026-11-02.json).

### Parent-member Let alias regression matrix — 2026-11-02

Extended the nested For matrix with Let rows=parent.items, including identical
parent/child item names.24 cases now cover direct/alias collection, distinct/
shadowed names, fresh/cached maps, reorder/removal/duplicate parent scenarios.
The alias resolves the live parent path after reorder, does not target a neighbor
after removal, and duplicate keys refuse without committing model writes.
Lib151/151 and allocation7/7 passed; diff check clean. Test-only change.
The project compiles from `.ui`, then the Let wrapper is inserted into IR: this
is not source-syntax/AOT/browser or performance qualification.
[Receipt](benchmarks/native-parent-alias-qualification-2026-11-02.json).

### Source Let siblings inside For — 2026-11-02

Parser syntax is `let rows = outer.items;`. Source fixtures exposed that For
expansion did not propagate Let to subsequent siblings. Expansion now applies
Let sequentially, retains composition depth/node charges, and starts a fresh
inherited scope per iteration when Let siblings mutate it. Non-Let For loops
retain scope reuse.
All24 parent binding direct/alias cases now compile directly from `.ui`; no
IR wrapper injection. A new source regression checks sibling visibility,
iteration-local values, outside/root isolation and stable keys after reorder.
Lib152/152, allocation7/7 and native/wasm32 checks passed. Let-bearing For
performance remains unqualified because inherited state is copied per iteration;
browser/AOT/live-window and general computed/filter/map origins remain open.
[Receipt](benchmarks/native-source-let-qualification-2026-11-02.json).

### Shared sequential lexical scene expansion — 2026-11-02

Containers, For, If, selected Match and nested Let now use one sequential child
expansion helper. Cow-local state propagates Let to later siblings and drops
at sequence exit. For no longer mutates its inherited iteration map via sibling
Let; If borrows selected children instead of clone-then-traverse. Let nodes
retain composition depth/node checks.
Lib153/153, allocation7/7, native/wasm32 and hardware GPU3/3 passed. New `.ui`
If/else test uses two dependent Lets and true→false→true transitions, validates
Runtime construction, branch visibility, outside isolation and unchanged root
model. Source alias24-case/For iteration/key/error/retry tests remain green.
No release timing/peak-memory claim; Match-specific complex/deep scenarios,
derived collections and browser/AOT/window matrices remain open.
[Receipt](benchmarks/native-sequence-scopes-qualification-2026-11-02.json).

### Source Match scope and binding identity regression — 2026-11-02

Added source `.ui` Match with guarded branch Let prefix/collection alias and
fallback dependent Lets plus a separate bound model field. Changing the guard
while an old widget has input makes fresh mapping sync write zero; both model
fields survive. A new fallback Runtime accepts its edit, then restoring guard
ignores obsolete fallback widgets and restores original branch keys. Branch
locals remain absent outside and root prefix is unchanged.
Lib154/154, allocation7/7 and diff check passed. Test-only change; no new
GPU/wasm/performance qualification. Cached branch-switch plus render-error and
full deep/async/browser/AOT matrices stay open.
[Receipt](benchmarks/native-match-scopes-qualification-2026-11-02.json).

### Current branch identity during failed visual render — 2026-11-02

Reproduced a retained Runtime bug: switching Match while the new branch visual
fails let the old cached map commit obsolete editor input to the old branch.
`binding_scene_controls` now expands current branches/keys/state without visual
instantiation, layout or serialization. Render-error recovery uses this map,
so live widget keys only match still-selected controls. Structural expansion
errors refuse and retain document/input instead of authorizing stale-map writes.
Lib155/155, allocation7/7, native/wasm32 and hardware GPU3/3 passed. New `.ui`
retained Runtime regression fails before the change, passes in legacy/Svet,
and checks both models plus the displayed value after repair. Existing corrective
input/queue/reorder/error tests pass. No release timing/peak-memory/live-window
qualification; full selection/error/backend matrix remains open.
[Receipt and failing baseline log](benchmarks/native-branch-recovery-qualification-2026-11-02.json).

### Structural-only changed-model binding preflight — 2026-11-02

Retained Runtime now calls current structural binding expansion directly when
model state differs from the displayed snapshot. It no longer builds visuals
as preflight and then rebuilds them after bindings. Visual rendering occurs
after accepted input; unchanged-model cached mapping is preserved.
Lib155/155, allocation8/8, native/wasm32 and hardware GPU3/3 passed. New
allocation regression has96 collection editors plus query editor and Text
(98 controls): query bad causes a visual error; query ok repairs and rebuilds
with353598 allocations (budget<450000); next idle avoids rebuild. Existing
Match/error/key/queue recovery remains green. No measured before/after release
speed/bytes or peak-memory claim; structural snapshots remain expensive.
[Receipt and source archive](benchmarks/native-structural-preflight-qualification-2026-11-02.json).

### Binding-map parity and compatible Scroll visuals — 2026-11-02

New18-case parity regression compares structural and rendered map counts/order,
node ids, keys, full state and event args across Frame/Column/Row/Grid/Stack and
Column→Scroll→Column, true/false/true branch transitions and reordered items.
Every generated scene constructs an actual Runtime. Event args are annotated
in IR after source compilation.
The test exposed negative y in tall Scroll content: positioning normalized a
legacy Scroll to Frame but reused a precomputed Scroll visual based only on child
count. It now also requires matching normalized kind, otherwise recomputes.
Lib156/156, allocation8/8, native/wasm32 and physical GPU3/3 passed.
A separate discovered issue remains: explicit width/height on Scroll serialize
as properties inside Scroll, while the pinned runtime grammar accepts only
Button children there. Original failure log retained; supported parity fixture
uses a property-free Scroll. This does not qualify Scroll viewport properties,
live-window behavior or performance.
[Receipt and failure log](benchmarks/native-binding-map-parity-qualification-2026-11-02.json).

### Root Scroll viewport lowering — 2026-11-02

Fixed explicit width/height serialization for a sole root Scroll without changing
the pinned parser. Runtime already expresses one viewport through Frame padding:
compiler preserves outer Frame dimensions, lowers the centered viewport into
padding, positions Scroll content in that rect and serializes only controls
inside Scroll. Root padding participates in available viewport dimensions.
Viewport extents must fit the content area; unsupported Scroll styling/actions
refuse rather than silently disappearing. Root Scroll always uses positioning.

Lib157/157, allocation8/8, native/wasm32 and hardware GPU3/3 passed. Source tests
cover numeric/percentage extents, root padding, unchanged outer size, exact
scroll clamp for16 rows, hit testing just above/inside viewport and oversized
refusal. The18-case binding-map parity fixture now uses explicit Scroll extents
and constructs Runtime successfully. General nested/multiple viewports, empty
content extent, resize/complex layouts and viewport-specific pixel/live-window
qualification remain open; no performance claim.
[Receipt](benchmarks/native-scroll-viewport-qualification-2026-11-02.json).

### Scroll-only layout revision and empty collection — 2026-11-02

Reproduced a layout revision bug: centered viewport width changed while all
control bounds and outer dimensions stayed equal, leaving layout_revision
unchanged. Interaction preservation now includes viewport rect and scroll mode
in layout-change detection.
Lib158/158, allocation8/8, native/wasm32 and hardware GPU3/3 passed. Retained
`.ui` regression in legacy/Svet checks width-only resize with stable control
bounds and advanced revision, offset preservation, clamp212→112 on taller
viewport, then zero controls/offset after clearing items and idle no-rebuild.
This qualifies model-driven viewport updates, not OS-window/multi-monitor resize
or viewport-specific physical pixels. Blank logical content extents and
multiple/nested viewports remain open.
[Receipt and failing baseline](benchmarks/native-scroll-resize-qualification-2026-11-02.json).

### Physical GPU Scroll crop reference — 2026-11-02

Added retained `.ui` fixture and independent pixel reference for centered root
Scroll viewport. On Apple M4 Max/Metal, DPI1/1.25/2 × offsets0/32/64 produced
9 frames with exact RGBA agreement over96.875% of each frame. Only the scrollbar
strip inside the viewport is excluded; every outside pixel is checked. Reference
uses background and two colored rectangle rows with independent crop arithmetic.
All4 hardware GPU tests passed. Saved9 lossless PNGs; DPI2/offset32 inspected
visually, confirming red/green crop and no outside leakage. Test-only change;
other native suites were not rerun. No cross-backend, live-window/resize,
scrollbar or rounded/fractional-edge mask qualification, and no timing claim.
[Receipt and PNG hashes](benchmarks/native-scroll-pixels-qualification-2026-11-02.json).

### Fractional-position Scroll center-clip proof — 2026-11-02

New source fixture63×61 yields viewport[32.5,33.5,63,61]. Physical Apple M4 Max/
Metal reference tests DPI1/1.25/2 and offsets0/32/67. All9 frames match exact
RGBA on checked pixels (observed≥96.2% per frame); all outside viewport pixels
are checked. The reference uses physical pixel-center inclusion, matching the
explicit hard viewport clip contract. Scrollbar and primitive AA edge/seam bands
are excluded only inside the viewport; no fractional area-coverage/AA blend
claim. All5 GPU tests pass. Test-only change; other backends/window resize,
rounded masks and performance remain open.
[Receipt and per-frame counts](benchmarks/native-fractional-scroll-qualification-2026-11-02.json).

### CPU/GPU fractional AA parity defect reproduced — 2026-11-02

New strict parity test compares complete CPU/GPU frames, including previously
excluded scrollbar and AA edges/seams. Apple M4 Max/Metal:15/18 cases pass,
3 fractional cases fail max≤4/255 (max59/53/58); all9 integer cases are exact.
At logical pixel(32,33), CPU[77,25,38,255] vs GPU[136,17,26,255].
CPU coarse multisampling and GPU SDF AA produce different corner/seam coverage.
Current full hardware suite:5 passed,1 failed. Test remains failing to expose
the defect; no production renderer change or completion claim. Next priority
is a shared precise rectangle coverage/compositing contract with independent
area reference, preserving GPU quality rather than quantizing it to CPU samples.
[Failing receipt and18-case data](benchmarks/native-scroll-renderer-parity-qualification-2026-11-02.json).

### Exact borderless rectangle coverage on CPU/GPU — 2026-11-02

Fixed the reproduced fractional AA discrepancy for zero-radius borderless
rectangles. Both renderers now integrate exact pixel/rectangle intersection
area. CPU cache retains fractional boundary values sparsely by original canvas
index; solid interiors keep palette paint. Rounded/bordered paths are unchanged.
Test-only uncached CPU reference follows the precise rectangle contract too.
Independent closed-form GPU/CPU assertions cover a quarter-covered corner and
7/8 red→1/8 green source-over seam (≤1/255 error).
Lib158/158, allocation8/8, native/wasm32 and hardware GPU6/6 passed. All18
complete CPU/GPU frame comparisons pass, max1/255 instead of59/255. The failing
receipt remains as before evidence. Rounded/bordered AA, fractional frame masks,
other backends/window matrices and performance/edge-storage cost remain open.
[Receipt,18-case metrics and before/after sources](benchmarks/native-exact-rect-qualification-2026-11-02.json).

### Translucent rectangle source-over parity — 2026-11-02

Extended complete-frame CPU/GPU matrix with alpha128/255 red/green fills.
All18 translucent frames match exactly; combined36/36 frames pass, max1/255.
Independent closed-form references check quarter-covered alpha corner and
7/8→1/8 alpha seam. All6 hardware tests passed on Apple M4 Max/Metal.
Test-only change; rounded/bordered shapes, frame-mask translucency and other
backends/window/performance matrices remain open.
Latest natural HTTP soak PID99442 separately confirmed live; progress checkpoint
updated at≈107.7min/1156288 requests. No final report or memory qualification
is claimed from this checkpoint.
[Receipt and36-case metrics](benchmarks/native-alpha-parity-qualification-2026-11-02.json).

### Exact square rectangle border coverage — 2026-11-02

Extended precise area coverage to square borders: stroke band is outer rectangle
area minus inset area. CPU/GPU use the same source-over contract for translucent
stroke over fill; collapsed inset yields a fully stroked shape. Sparse partial
edge data keeps solid fill/stroke on palette paths. Rounded shapes unchanged.
New1.25 border fixtures reproduced CPU/GPU errors up to32/255. After the fix,
all72 complete-frame comparisons pass max1/255, including alpha128 blue stroke
and border40 fully covering the shape. Independent quarter-corner reference
[45,26,70,255] passes for thin/wide borders. Lib158/158, allocation8/8,
native/wasm32 and hardware GPU6/6 passed. Rounded border AA, sparse-edge cost,
other hardware/backends/window matrices remain open.
[Receipt, before log and source archive](benchmarks/native-exact-border-qualification-2026-11-02.json).

### Fresh cross-core tracking and nested observer attribution — 2026-11-02

Built two independent current Vite engines and verified every snapshot source
hash against the worktree. Current tracking probe11/13 passes. Original foreign
signal invalidation still fails; new nested regression shows a distinct defect:
source-core outer observer gets the read while foreign inner observer does not
(actual runs[2,1], expected[1,2]). Frozen artifact hashes and teardown frame
registries pass. Engine unchanged this turn; no bridge/readiness claim.

Next implementation must select the active domain frame, bridge via native
proxy source/observer nodes in the two Rust graphs, and bind lifecycle to
core/generation/node lifetime. Dynamic deps, reset/dispose/owner retirement,
computed equality/laziness, batching and cycles need qualification together.
[Fresh report and source verification](benchmarks/cross-core-current-qualification-2026-11-02.json).

### Active-domain read attribution; cross-core bridge still open — 2026-11-02

Signal/getter and generic tracking now require the active domain frame to match
the source context. Computed foreign evaluation suspends a paused source-core
observer even when ambient selection alone is insufficient. This removes the
wrong outer dependency; target-core invalidation is not implemented yet.
17 focused tests,1301 JS tests/99 files, TypeScript, package build and installed
consumer/GC gate passed. Four new regressions cover signal/computed reads and
throw restoration with graph-zero teardown. Fresh independent-copy probe stays
11/13: nested actual runs improve[2,1]→[1,1], required full result[1,2] still
needs native proxy nodes/lifecycle. No latency or complete cross-core claim.
[Receipt](benchmarks/cross-core-domain-routing-qualification-2026-11-02.json).

### Frozen2h natural HTTP final: external budget failure — 2026-11-02

JCOGcm/09qhPH completed7200021ms with1289120 requests (859413 complete,
429707 disconnected), request/cleanup failures0, graph after finish0, forcedGC0.
Harness/fixture and20 installed runtime file hashes independently match launch,
final and actual frozen bytes. Heap/RSS/ArrayBuffers budgets pass. External
post-warmup peak growth72409329 bytes exceeds67108864 (64MiB); trough growth
is3383 bytes. Status needs-investigation, not leak diagnosis or plateau proof.
This frozen runtime predates the domain-routing fix: current dist differs in
reactivity.js. No restart or qualification of the new dist was inferred. Need
external/ArrayBuffers/native allocation correlation and queue/socket lifecycle
investigation under the unchanged budget.
[Final qualification](benchmarks/http-natural-latest-2h-qualification-2026-11-02.json).


## 2026-11-02 — Native cross-core proxy bridge; batching still partial

Compatible copies now forward source changes through a source-core effect and
an observer-core signal proxy. Both dependency edges remain in native graphs;
JavaScript transports notifications. Bridge maps are shared weakly across module
copies; dependencies retire on branch changes, source/observer disposal and
both-core resets. Source forwarding ownership is detached from suspended owners.
Computed value versions preserve custom equality, including same-reference
`equals: () => false` updates. Construction-cycle failure releases bridge nodes.

Independent-copy probe: 13/13 (previously 11/13). Full JS: 1312 passed and one
explicit expected failure, 100 files. Types, package build and installed consumer/
GC gate pass. The known expected failure is multi-source cross-core batch
coalescing: target receives redundant notifications. Dedicated bridge weak-GC,
browser, mixed-core stabilization and performance qualification remain open.
Existing installed GC gate is not evidence for every new bridge retention path.
No new HTTP soak ran; the external-memory budget failure remains unresolved.

Evidence: [qualification](benchmarks/cross-core-bridge-qualification-2026-11-02.json)
and [independent-copy probe](benchmarks/cross-core-bridge-probe-2026-11-02.json).


## 2026-11-02 — Cross-core native batch delivery and in-batch computed freshness

The shared synchronous batch boundary now holds native batches in written foreign
cores. Proxy writes update native dirty state immediately while target effects
wait for the outer boundary. Nested compatible-copy batches share this transport.
Held generations are checked before release and replacement writes; errors drain
other targets while preserving the primary body/delivery error. Active effect
flushes retain their own stabilization handling instead of reopening a native batch.
Cross-core transport feedback has a bounded delivery limit and recovery coverage.

An explicit target-computed read inside a source batch previously returned stale
cached data. Before trusting its native clock, computed reads now refresh foreign
boundaries, including boundaries below local computed chains. Signal revisions
and computed value versions suppress duplicate forwarding after this refresh while
preserving custom equality. Outside batch, this refresh returns immediately.

Full JS: 1321 passed / 100 files, no expected failures; types, package build
and installed consumer/GC gate pass. Independent-copy probe expanded to 15/15 with mixed-core nested writes and
in-batch computed-chain reads. Dedicated bridge weak-GC, arbitrary cross-core
scheduling/diamonds, real browser qualification and performance remain open.
No new natural-GC HTTP soak; existing external-memory failure remains open.

Evidence: [qualification](benchmarks/cross-core-batch-qualification-2026-11-02.json),
[probe](benchmarks/cross-core-batch-probe-2026-11-02.json).


## 2026-11-02 — Dedicated cross-core explicit-teardown retention gate

Added `scripts/check-cross-core-retention.mjs` and integrated it into packed
installed-copy package verification. Two independent compatible copies exercise
8 iterations of dispose, dynamic branch removal, source reset, target reset,
throwing batch, and target teardown while a persistent source core stays live.
All 272 weakly observed teardown targets (cores, WASM memories, payloads and
buffers) collect. Four generic controls and three active-subscription controls
remain alive before explicit release, then all collect. Graph counts and a live
persistent source verify functional teardown rather than GC-only disappearance.

Both standalone frozen-copy and integrated installed-package gates pass. Script
syntax and scoped diff check pass. No engine source changed in this milestone.
This is explicit teardown plus forced-GC reachability evidence. It does not prove
natural-GC memory plateaus, browser collection, abandoned live subscriptions or
retained public-handle behavior. Both copies' hot context caches are deliberately
replaced with the live control core before collection. Prior SSR external-memory
budget failure and browser qualification remain open.

Evidence: [GC report](benchmarks/cross-core-retention-2026-11-02.json),
[qualification](benchmarks/cross-core-retention-qualification-2026-11-02.json).


## 2026-11-02 — Three-core diamond delivery in both dependency orders

A three-core fan-out/fan-in regression reproduced duplicate terminal delivery:
[5,10,10] rather than [5,10]. An implicit shared batch for source-core writes
with active foreign forwarders fixed one traversal order. Reversing dependency
read order still reproduced duplication, including explicit batches. Before
releasing a held native batch, the transport now refreshes its live observers'
foreign dependency boundaries while native delivery remains suppressed. This
prevents a terminal observer from invalidating itself during upstream refresh.
Forwarder counts retire with their bridges; purely local cores retain their
existing setter path. Writes already inside active effect flushes retain native
stabilization handling.

Full JS: 1326 passed / 100 files. Focused batch/stabilization matrix: 51 passed.
Independent-copy probe: 19/19, including explicit/implicit batch × both diamond
read orders. Types, package build and installed package/dedicated bridge GC gate pass. Dedicated updated-runtime retention
probe collects all 272 teardown targets with positive controls. Both explicit
and implicit cross-core feedback limits and recovery have regressions.

Arbitrary scheduling topologies, active-effect fan-out, browser qualification and
transport overhead remain open. Observer-boundary refresh scans live effect
registry entries; no throughput or latency improvement is claimed. No natural-GC
HTTP rerun; existing external-memory budget failure remains open.

Evidence: [qualification](benchmarks/cross-core-diamond-qualification-2026-11-02.json),
[copy probe](benchmarks/cross-core-diamond-probe-2026-11-02.json),
[retention](benchmarks/cross-core-diamond-retention-2026-11-02.json).


## 2026-11-02 — Initial effect flush preserves cross-core diamond coherence

A source write from an effect rerun passed the three-core diamond regression,
but effect initialization reproduced a visible intermediate snapshot [5,7,10]
instead of [5,10]. The initial callback queues native writes without passing an
external setter delivery boundary. `flushEffects` now opens a shared transport
boundary when a core has foreign forwarders and no transport is active. All
source native queue rounds finish before held foreign batches deliver. Source
flush failures remain primary while held targets are drained in finally.
Local-only cores retain the existing flush path.

Full JS: 1328 passed / 100 files. Focused cross-core, batch-error and stabilization
matrix: 53 passed. Independently compiled copies: 20/20, including initial-effect
three-core fan-out/fan-in. Types, package build, installed package/bridge GC
gate and updated-runtime explicit-GC probe pass (272 teardown targets collected with positive controls).
Arbitrary active-effect scheduling, topology combinations, browser qualification
and transport throughput/latency remain open. No new HTTP natural-GC soak;
external-memory budget failure remains unresolved.

Evidence: [qualification](benchmarks/cross-core-effect-flush-qualification-2026-11-02.json),
[probe](benchmarks/cross-core-effect-flush-probe-2026-11-02.json),
[retention](benchmarks/cross-core-effect-flush-retention-2026-11-02.json).


## 2026-11-02 — HTTP external-memory correlation diagnostics

The HTTP soak now records bounded simultaneous diagnostics on memory samples and
external-memory extrema: active/opened/closed server sockets, readable/writable
queue bytes, lifecycle traffic totals, active/peak responses, active resource
kinds, V8 external/malloced/physical allocation and main-core WASM buffer bytes.
Global and four window peak observations persist even when the 32-row ring wraps.
Shutdown waits for tracked socket close events before the final diagnostic snapshot.
Request scheduling, concurrency, GC mode and memory investigation budgets remain
unchanged; instrumentation overhead is not measured.

Natural 10s smoke: 1512 requests, no failures, zero forced GC. Forced 1s smoke:
96 requests / 12 GCs. Controlled stats-and-forget failure remains failed, with
request and cleanup evidence. All three runs verify source/runtime identity and
finish with zero tracked server sockets/responses and opened==closed. The natural
smoke's peak external sample had empty server queues; this does not identify the
allocator or rule out client/native retention. Final package/installed GC gate
passed and its consumer is preserved. Frozen snapshot-a31cGx runs a 30-minute
natural-GC diagnostic across the former ~24-minute failure region. No final result
or two-hour budget qualification is claimed while it is running.

Evidence: [instrumentation qualification](benchmarks/http-diagnostics-qualification-2026-11-02.json),
[30m manifest](benchmarks/http-diagnostics-30m-manifest-2026-11-02.json),
[run metadata](benchmarks/http-diagnostics-30m-run-2026-11-02.json).


## 2026-11-02 — HTTP diagnostic reports fail closed on accounting/shutdown errors

The soak analyzer now validates v1 socket, response, resource, V8 and main-WASM
counters, per-window external peak attribution and completed shutdown snapshots.
Nonzero final server sockets/responses, mismatched open/close counts, malformed
or negative counters, unsupported diagnostic versions and recorded request/cleanup
failures refuse qualification. Historical reports without v1 fields retain their
original duration/budget checks. Running reports never qualify.

Six unittest methods cover successful qualification, five corrupt diagnostic
variants, both failure arrays, running status, external budget failure and legacy
duration refusal. Tests pass and their command is added to existing Svet CI;
remote CI has not run. Current 10-second smoke is refused solely for qualification
minimum duration. Historical 2h report still fails the unchanged external growth
budget. Live frozen snapshot-a31cGx remains incomplete, with its process observed
running; no restart or completed memory claim.

Evidence: [analyzer qualification](benchmarks/http-diagnostics-analyzer-qualification-2026-11-02.json).


## 2026-11-02 — Malformed HTTP report schema and truncated JSON refusal

Analyzer schema validation now rejects non-object reports, invalid request/count/
time/array/window types and nonfinite durations before arithmetic. Nested null,
list or string diagnostic maps return structured investigation issues rather than
raising AttributeError. Boolean numeric counters and diagnostics versions are not
accepted as integers. Invalid UTF-8/truncated JSON yields machine-readable
needs-investigation with exit 2, source/report hashes and no traceback.

Nine unittest methods pass, including malformed nested/top-level values and a real
CLI truncated-file subprocess. Live frozen progress still analyzes as incomplete;
the historical 2h report still fails the unchanged external growth budget. Existing
CI regression command includes these tests. No engine source or frozen live-soak
artifact changed, and no memory-cause/completion claim is made.

Evidence: [qualification](benchmarks/http-analyzer-schema-qualification-2026-11-02.json).


## 2026-11-02 — Retained root Scroll preserves empty logical layout extent

Runtime content extents previously came only from control leaves, dropping an
explicit-height empty layout. Positioning now collects logical layout end bounds
and passes optional root-Scroll metadata through the internal retained Document
render path. Runtime load/rebuild applies it before preserving interaction and
clamping offsets. Logical content-size changes also increment layout revision,
including when controls and viewport stay identical. No fake controls are added;
legacy scene grammar remains unchanged.

159 lib tests and 10 allocation-budget tests pass; native-bin and wasm32 library
checks pass. All6 explicit hardware GPU tests pass, including existing full-frame
rectangle/border parity. New regression covers empty512/900/200-high layouts;
retained legacy/Svet regression now preserves fixed-height content after controls
are removed, then clamps offset to0 when the model shrinks that height.

Scope is retained `.ui` loading/rebuild with sole-root Scroll. Public legacy
render_scene tuple/raw scene transport does not carry the optional metadata;
nested/multiple viewports, horizontal/general empty-layout matrix and OS-window
qualification remain open. No throughput claim. Frozen HTTP diagnostic continues
on its independently captured files, not these native sources.

Evidence: [qualification](benchmarks/native-logical-scroll-qualification-2026-11-02.json).


## 2026-11-02 — Public Rust scene artifact and Form logical bounds transport

Added owned `SceneArtifact` (source, templates, control mappings and optional
logical content end), `Document::render_scene_artifact` and
`Runtime::from_scene_artifact`. The constructor validates finite nonnegative
extents and refuses metadata on a non-Scroll scene. Form::sync now uses this
artifact, closing its separate loss of empty-layout bounds on rebuild.

159 lib, 10 allocation and 2 external/public API integration tests pass. Native
binary and wasm32 library checks pass. Public API tests cover empty Scroll extent
roundtrip, zero phantom controls, ordinary scenes, invalid metadata and both
legacy/Svet Form sync. Existing typed retained Runtime path remains supported.
No renderer code changed or hardware GPU/performance qualification claimed here.
The artifact is Rust-native; raw text tuple/scene strings and JSON/JavaScript wire
transport still lack this metadata. Nested/multiple viewports and general layout
matrix remain open. Frozen HTTP diagnostic is not modified by these source edits.

Evidence: [qualification](benchmarks/native-scene-artifact-qualification-2026-11-02.json).


## 2026-11-02 — Versioned scene JSON and executed WASM compile/load boundary

SceneArtifact now supports protocol `forma-scene-artifact/1` JSON with source,
templates, typed control mappings and required optional logical-content bounds.
Serde tags preserve Text versus Expr, AST/interpolation/node variants, internal
binding metadata and event arguments rather than using the older lossy state-JSON
helper. Nonfinite Value::Number cannot serialize silently as null. Unknown top-level
fields, unsupported protocols, missing/malformed/negative bounds refuse decoding.

Rust `load_scene_artifact_native` and WASM `load_scene_artifact` construct the next
runtime before swapping, preserving current state/offset/revisions on refusal.
Successful load advances cache revisions. WASM static `compile_ui_scene_artifact`
compiles UI source and generates the complete transport in Rust. Added serde derive
dependency and refreshed both example lockfiles using offline metadata.

159 lib, 10 allocation and 5 public integration tests pass; native-bin and wasm32
checks pass. Built actual wasm32 module, generated NodeJS ABI with wasm-bindgen
0.2.125 and executed the committed `tests/check-scene-artifact-wasm.cjs` gate.
JavaScript→Rust compilation→JSON→WASM load preserves empty512-height content and
212 scroll offset; malformed/version/negative extent refusal preserves the old
runtime, and valid reload increments layout revision. This is real NodeJS WASM
execution, not browser/DOM/window qualification. Legacy raw markup still needs the
artifact channel for metadata; nested/multiple viewports remain open. No new GPU
or performance claim. Frozen HTTP diagnostic is independent and remains running.

Evidence: [qualification](benchmarks/native-scene-json-qualification-2026-11-02.json),
[actual WASM result](benchmarks/native-scene-json-wasm-runtime-2026-11-02.json).


## 2026-11-02 — Actual browser scene-artifact WASM qualification

Generated browser ESM/WASM bindings and ran the committed
`tests/scene-artifact-browser.html` in the in-app Chromium browser (UA Chrome156).
All5 cases pass: Rust compile→JSON→browser load, empty-layout212 offset/viewport,
atomic bad version/JSON/extent refusal, CPU frame sizes at1/1.25/2 scales, and
valid shrink/reload cache revision. Scrollbar movement changes3720 byte channels,
with zero changes outside the viewport. The Canvas2D frame and page's passed
report were visually inspected; the browser posted its result to loopback host.
`tests/serve-scene-artifact-browser.mjs` binds fixture/module/WASM hashes at launch
and refuses changed served artifacts. Saved result hashes independently match all
three served files and the committed fixture. No Rust/runtime code changed here.

Scope: actual Chromium browser ESM/WASM plus CPU raster/Canvas2D. Other browser
engines, browser GPU, HMR, IME and OS-native windows remain open. QA tab and both
owned temporary servers closed. Frozen30m natural-GC HTTP diagnostic is left
running on its independent artifacts; no completed memory result is claimed.

Evidence: [browser report](benchmarks/native-scene-browser-2026-11-02.json),
[qualification](benchmarks/native-scene-browser-qualification-2026-11-02.json).
Reproduce after wasm-bindgen --target web: copy tests/scene-artifact-browser.html
as index.html in generated directory, run tests/serve-scene-artifact-browser.mjs
with that directory and output JSON path, then open its printed localhost URL.


## 2026-11-02 — Scene mapping/rendered-control consistency gate

Runtime::from_scene_artifact previously accepted control mappings that disagreed
with the actual scene leaves, including a synthetic mapping on an empty scene.
The constructor now checks mapping count and every key in order against the
parsed/linked runtime controls before any swap. Missing, extra, reordered or
renamed mappings refuse; load_scene_artifact_native preserves the old runtime.
The typed-value serialization test is explicitly separated from authorization to
load a nonexistent control, rather than treating that mismatched fixture as valid.

159 lib, 6 public integration and 10 allocation tests pass. Native check and wasm32
build pass. Regenerated Node WASM ABI and reran the committed JS gate: ghost
mapping refusal preserves live viewport/offset/cache revision. No render code
changed; the previous Chromium report predates this validation-only change.
Node/state metadata is not authenticated source provenance, and duplicate equal
keys remain governed by separate binding ambiguity checks, not this constructor.
Frozen HTTP diagnostic remains independent and running.

Evidence: [qualification](benchmarks/native-scene-mapping-qualification-2026-11-02.json),
[actual WASM](benchmarks/native-scene-mapping-wasm-2026-11-02.json).


## 2026-11-02 — Remove discarded square coverage and restore no-reveal interior bypass

RasterCache previously computed four rounded-distance samples for radius0 and
then overwrote their coverage with exact rectangle area. The supersampling is
now only in the rounded branch. Reveal processing still follows either branch.
Also, the no-reveal case was treated as non-default reveal bounds, setting
safe_inset to f32::MAX and disabling the uniform interior fast path. Absence of
reveal now allows the safe interior bypass; custom reveal bounds keep their
previous conservative path.

159 lib and 10 allocation tests pass, as do all6 explicitly run hardware GPU
tests, including the existing72-case full-frame rectangle/border comparison and
independent crop/fractional viewport/glyph references. Native binary and wasm32
library checks pass. This establishes tested raster correctness after removing
work; it does not establish FPS, release latency or peak-memory improvement.
Live HTTP churn is still running, so no quiet-host benchmark is claimed.
Rounded-border AA and other backend qualification remain open.

Evidence: [qualification](benchmarks/native-square-cache-qualification-2026-11-02.json).


## 2026-11-02 — Opt-in request core and WASM-memory finalization accounting

Added an optional fixture onCore observer and `SVET_SOAK_CORE_LIFETIMES=1` harness
mode. A FinalizationRegistry registers each request core and its WASM memory,
retaining only kind/initial-byte-count tokens. Diagnostics expose created/finalized,
pending memory counts and registered-byte totals/high-water counts without WeakRef
polling or forced GC. Default0 leaves lifetime tracing disabled. Registered bytes
exclude subsequent memory growth, and pending includes callback scheduling lag.

Natural10s smoke:1936 requests,0 forced GC. Peak external125093210 bytes co-occurred
with96 unfinalized memory callbacks /106954752 registered bytes; final pending
count8 after1928 finalizations. Forced1s smoke:168 requests,21 GCs, all168 cores/
memories finalized by shutdown. Disabled natural smoke:200 requests, no lifetime
fields. Identity and gzip HTTP compatibility gates pass, as do source/runtime
identity and zero tracked server sockets/responses at shutdown. No retained-target
or stationary-memory claim follows from this short diagnostic. Tracer overhead
is unmeasured. Main frozen snapshot-a31cGx was not modified/restarted and has
passed the old24-minute region without an observed request failure; final pending.

Evidence: [qualification](benchmarks/http-core-lifetimes-qualification-2026-11-02.json).


## 2026-11-02 — Frozen30m HTTP diagnostic completed; two-hour admission still open

snapshot-a31cGx completed359048 requests in1800017.990417ms with0 forced GC,
request/cleanup failures and post-request graph nodes. All119699 server connections
closed, final tracked sockets/responses0. Harness/fixture/final identities and all20
installed runtime file hashes independently match manifest and actual bytes.
Post-warmup peak growth:heap1289736,RSS132416,external22798185,ArrayBuffers1524028
bytes, within unchanged16/64/64/16MiB budgets. Analyzer correctly returns
needs-investigation solely for the unchanged7200000ms qualification minimum.
Former~24-minute failure did not reproduce. This is30-minute diagnostic evidence,
not two-hour qualification, stationary memory or a fix for historical external
budget failure. The process is terminal; do not restart from a stale tool handle.
Next allocation investigation needs frozen lifecycle tracing; current short-run
finalizer correlation is only a hypothesis.

Evidence: [final qualification](benchmarks/http-diagnostics-30m-qualification-2026-11-02.json).


## 2026-11-02 — Frozen traced2h natural-GC run launched with accounting guards

Started snapshot-Nv0ST2 against preserved installed package, natural GC,
SVET_SOAK_CORE_LIFETIMES=1 and7200000ms requested duration. Manifest sources and
all20 runtime files independently match frozen, installed and current dist bytes.
Process58568/session76033 observed live with running progress and0 forced GC.
Final report is pending; do not treat progress as qualification or restart from a
transient tool observation failure. Tracing overhead has not been measured.

Analyzer now checks optional lifetime-counter types and created/finalized/pending,
registered/finalized/pending bytes, peak pending and successful request allocation
accounting. Ten unittest methods pass; eight corrupt counter/missing/count
variants refuse qualification. Real short traced smoke refuses only for duration;
live traced report remains incomplete. Existing CI command includes these tests.
Budgets16/64/64/16MiB and two-hour minimum remain unchanged. Historical external
budget failure is still unresolved.

Evidence: [run](benchmarks/http-core-traced-2h-run-2026-11-02.json),
[accounting qualification](benchmarks/http-core-traced-accounting-qualification-2026-11-02.json).


## 2026-11-02 — Oversized Scroll content remains reachable from its leading edge

A512-wide empty Row inside a320-wide Scroll was centered at a negative origin;
only96 horizontal units were scrollable instead of192, making its leading half
unreachable. Scroll positioning now clamps negative centering offsets to0 for
both nested layout containers and leaf controls. Smaller content remains centered;
non-Scroll positioning keeps its prior alignment. Logical bounds then capture the
complete positive overflow rather than only the visible-side half.

159 lib, 8 public integration and10 allocation tests pass. Empty Row/Column/Stack/
Frame/Grid512x600 fixtures reach192x300 offsets and return0. A wide TextInput
starts at20,50 viewport origin and retains full reachable extents. All6 explicit
hardware GPU tests pass, including existing full-frame/crop/fractional references.
Native and wasm32 checks pass. Rebuilt actual WASM/Node ABI and executed oversized
compile→JSON→load, both-axis max offsets and return-to-origin checks.

Nested/multiple Scroll viewports, arbitrary overflow/direction matrices and browser
interaction remain open. No timing claim; browser artifact report predates this
positioning change. Frozen2h traced HTTP process remains independent and running.

Evidence: [qualification](benchmarks/native-scroll-horizontal-qualification-2026-11-02.json),
[actual WASM](benchmarks/native-scroll-horizontal-wasm-runtime-2026-11-02.json).


## 2026-11-02 — Cross-core computed failures retain subscriptions and recover

Forwarding effects previously threw before a target could handle a foreign
computed failure. Initial failure also closed the bridge, permanently losing
notifications for a target that caught the error. Forwarding evaluation now
retains native dependency tracking through failures, records failure epochs and
invalidates target proxies for error/recovery transitions. Target getters still
throw the original computation error for user handling. Same-epoch failed refresh
avoids duplicate notifications; recovery invalidates even if the prior successful
computed value is unchanged. No exception object is retained as transport state.

Full JS1332/100 files pass, independently compiled copies22/22 pass, types/package
build and installed consumer/bridge GC gates pass. Tests cover initial/later errors,
same-value recovery, target computed chains, uncaught error propagation/recovery,
caller tracking and empty graph teardown. An old failure-path isolation expectation
was updated to the cross-core dependency contract; its local tracking restoration
assertion remains.

Frozen2h snapshot-Nv0ST2 remains live on previous runtime. Its installed bytes were
not modified; current dist differs by reactivity.js only. That allocation diagnostic
cannot qualify this new runtime revision. Browser/mixed failure topology and
performance qualification remain open; no external-memory fix is claimed.

Evidence: [qualification](benchmarks/cross-core-error-qualification-2026-11-02.json).


## 2026-11-02 — Bridge failure-path explicit-teardown GC coverage

Expanded independent-copy retention probe from6 to9 scenarios: foreign computed
failure after success, initial failure handled by target, and uncaught initial
target-effect construction failure that returns no handle. Exceptions capture the
tracked payload and are themselves weakly observed. Recovery is exercised before
teardown where a handle exists; the constructor failure must leave no observer
running when the source recovers.

Standalone frozen-copy forced-GC proof collects440 teardown targets (previous272),
including24 exception objects from the new scenarios. Four generic controls and
three active-subscription controls survive while held, then collect on release.
Both graphs are empty after each tested teardown. Installed package gate now
requires9 cases and440 collected targets; the complete installed package gate passed. This remains explicit teardown plus
forced-GC reachability, not natural-GC plateau, retained-user-handle or browser-GC
qualification. No runtime source changed; the two-hour natural-GC diagnostic keeps
running on its prior independent snapshot.

Evidence: [retention report](benchmarks/cross-core-error-retention-2026-11-02.json),
[qualification](benchmarks/cross-core-error-retention-qualification-2026-11-02.json).


## 2026-11-02 — Cross-core error/recovery in actual Chromium DOM

Added cross-core-errors browser fixture and snapshot-builder option. It exercises
two independently compiled modules/native cores and observable DOM values after
later/initial computed failure and same-value recovery. A three-core error diamond
produces0/error/10 once; root teardown retires its bridge and a retained DOM button
callback. Both main graphs and the third core are empty after tested teardown.

Actual in-app Chromium156 passes5/5 from current sources and5/5 from two copies
of a freshly packed/installed tarball. Full installed package/GC gate passed.
Browser raw-results JSON was read via DOM after observing passed status and
recovered value7 panels. All build artifact hashes independently verified; installed
runtime files match current dist byte-for-byte. Preserved consumer HBE6XC is kept.
No runtime code changed in this milestone. QA tab and both owned preview servers
closed; frozen2h HTTP allocation diagnostic remains live on its older runtime.
Other browser engines, HMR, browser-GC, arbitrary topology and deployed providers
remain open. This is browser DOM behavior evidence, not a throughput qualification.

Evidence: [qualification](benchmarks/cross-core-error-browser-qualification-2026-11-02.json),
[source report](benchmarks/cross-core-error-browser-source-2026-11-02.json),
[installed report](benchmarks/cross-core-error-browser-installed-2026-11-02.json).


## 2026-11-02 — Real installed-package Vite HMR exposed stale Forma listeners

Added isolated real Vite server/file-write runner using the installed compiler
plugin and two mounts of one.ui instance. Initial run failed after the first true
HMR update: a retained old button still incremented state7→8. Forma registered
click and two-way input listeners directly without removing them on view teardown.
Named handlers now unregister through owner onCleanup. Direct listeners preserve
existing detached-container interaction semantics.

New jsdom regression covers stale click/input after hotReload, idempotent partial
unmount and live sibling interactions. Full JS1333/101 files, types, build and
fresh installed consumer/GC gate pass. Actual Chromium Vite HMR now passes5/5:
precompiled UI AST,4 real file changes/afterUpdate events, state7 preservation,
constant11 graph nodes through two-view updates, stale callbacks retired, partial
unmount preserving the other view, final graph0, post-dispose update cannot revive
it. The failed-before browser report remains preserved. QA tab and both isolated
Vite servers closed; fresh consumer NMaqTq retained.

This is real dev HMR, not a mocked hot context. ProductionAOT async/error recovery,
other browser engines and direct instance.dispose with still-mounted views remain
open. No HMR latency claim. Frozen2h HTTP snapshot remains independent on older
reactivity.js/forma.js and cannot qualify current dist.

Evidence: [qualification](benchmarks/forma-real-hmr-qualification-2026-11-02.json),
[browser result](benchmarks/forma-real-hmr-2026-11-02.json),
[failed baseline](benchmarks/forma-real-hmr-before-2026-11-02.json).


## 2026-11-02 — Direct Forma instance disposal retires active views

Regression reproduced10 native nodes after instance.dispose where only1 external
ViewModel signal should remain. Instance now tracks rendered-view effects/roots
and mounted-view handles, retiring them before owned state. Render cleanup removes
its nodes and anchor; mount unmount is idempotent and no longer clears arbitrary
replacement content. Instance disposal is idempotent. New mount/render/HMR,
snapshot restoration and event registration refuse a disposed instance; external
caller-owned model signals remain live. Direct render outside a mount is tracked.

1334 JS tests/101 files, types/build and fresh installed consumer/GC gate pass.
Unit regression exercises2 mounts plus1 direct render, stale events, external
signal ownership and late handle behavior. Installed AOT gate now checks graph0
before handles unmount. Actual Chromium Vite HMR passes5/5 with4 file updates:
state7/graph11 remain stable across replacements; direct dispose clears mounted
DOM/graph, late unmount preserves replacement text, and post-dispose HMR cannot
revive the instance. Fresh consumer ZRGNEB matches current runtime bytes. QA tab
and owned Vite server closed; two-hour traced HTTP process remains independent.

Reentrant disposal during render, failing initialization, pending async/error HMR
and other browser engines remain open. No public-handle GC or latency claim.
Frozen HTTP run predates current forma.js/reactivity.js and cannot qualify them.

Evidence: [qualification](benchmarks/forma-direct-dispose-qualification-2026-11-02.json),
[actual HMR](benchmarks/forma-direct-dispose-hmr-2026-11-02.json).


## 2026-11-02 — Failed child view creation retires state and restores live key maps

Failed child mount/render reproduced8 nodes against parent baseline5: child cleanup
was registered only after rendering. Registration now occurs immediately after the
factory returns, before synchronization/event wiring/rendering. Per-view key maps
publish on successful creation; failed extra views restore the prior live map, and
unmount restores a remaining sibling map instead of exposing retired DOM entries.

Regression covers failed mount/render, empty partial-key lookup, successful retry,
valid sibling DOM/key preservation and final graph0. Full JS1337/101 files passes
with maxWorkers2. Initial default-concurrency run had a benchmark5000ms timeout;
the deadline was not changed. Types/build and installed package/GC gate pass.
Actual Chromium installed-package Vite gate passes7/7 with4 real file updates,
including both child-failure retry paths and prior HMR/direct-dispose cases.
Whitespace-only final cleanup rebuild emits runtime bytes equal to the tested
installed consumer xzJ18u. QA tab and isolated server closed.

This does not provide rollback of an already-mounted view on HMR render errors.
Factories throwing before returning an instance, reentrant disposal, async errors,
other browser engines and performance remain open. Frozen2h HTTP run continues
independently on older runtime bytes; no current-runtime soak claim.

Evidence: [qualification](benchmarks/forma-child-failure-qualification-2026-11-02.json),
[browser](benchmarks/forma-child-failure-browser-2026-11-02.json).


## 2026-11-02 — Reentrant parent disposal refuses unfinished initial mounts

Regressions showed mount returning successfully after parent.dispose inside a
child factory or child.render. Live-instance guards now stop AST/child construction
after disposal. Mount retirement is registered before DOM insertion, and insertion
plus final liveness check retire an unfinished mount on failure. This also handles
a custom element connectedCallback synchronously disposing the parent.

1340 JS tests/101 files pass with maxWorkers2; types/build and fresh installed
package/GC gate pass. Actual installed-package Chromium Vite fixture passes10/10
with4 updates. The3 new initial-creation disposal paths refuse mount and leave
empty DOM/graph0; prior failure/retry, sibling view, state-preserving HMR and
post-dispose cases remain green. Tested installed consumer1g5Ox0 matches current
runtime bytes. QA tab and isolated Vite server closed; frozen2h HTTP process stays
live on independent previous runtime.

Reentrant disposal while replacing an already-mounted HMR view needs separate
coverage. Factories throwing before returning a handle, atomic bad-HMR rollback,
async/pending errors and other browser engines remain open. No throughput or
current-runtime soak qualification follows from this milestone.

Evidence: [qualification](benchmarks/forma-reentrant-qualification-2026-11-02.json),
[browser](benchmarks/forma-reentrant-browser-2026-11-02.json).


## 2026-11-02 — Retired mounted-view owner releases late-returned child

A mounted view updating through hotReload could dispose its parent inside the
child factory, closing the current view scope before the factory returned. Late
onCleanup registration was ignored and3 child signals remained. The renderer now
captures the creation owner and explicitly disposes the returned child when that
owner or parent is retired before cleanup registration. The same rule handles
unmount without terminalizing the parent instance; a later mount succeeds.

1342 JS tests/101 files pass with maxWorkers2 after build finished. An earlier run
was invalid: build-package removed dist while node-server-entry imported index.js.
That orchestration race is preserved; valid rerun changed no deadline/test contract.
Types/build and fresh installed package/GC gate pass. Actual Chromium fixture
passes12/12:2 new public hotReload API disposal/unmount cases plus prior creation,
retry/dispose and4 real Vite happy-path file updates. Installed consumer e2Zh3P
matches current runtime bytes. QA tab and isolated Vite server closed.

Factories throwing before return, arbitrary getter/reset/reentrant callbacks,
atomic failed-HMR rollback and cross-instance accept-error isolation remain open.
No other-browser/performance or current-runtime natural-GC qualification. Frozen2h
HTTP diagnostic stays live on its independent prior bytes.

Evidence: [qualification](benchmarks/forma-retired-owner-qualification-2026-11-02.json),
[browser](benchmarks/forma-retired-owner-browser-2026-11-02.json).


## 2026-11-02 — Cached HMR factory AST and cross-instance error isolation

Generated module tests exposed two failures: an importer retaining createView
created the original AST after a no-owner update, and one instance hotReload
exception aborted the accept loop before healthy instances updated. Generated
HMR modules now share currentAst through hot.data even with no mounted instances.
Production keeps the explicit compileFormaAst(uiAst,options) branch. Accept takes
a registration snapshot, updates shared AST first, skips retired entries and
rethows the first error after draining remaining instances. Newcomers created
inside a callback initialize from latest AST and are deferred from that traversal.

1342 JS tests/101 files pass (maxWorkers2), as do types/build and fresh installed
package/GC gate. Generated-module lifecycle gate expanded3→5 cases, preserving
production core/Memory GC proof. Actual Chromium installed-package fixture passes
14/14: cached old importer createsv5:1 after no-owner update; a controlled failing
instance leaves its ownv5 while healthy reachesv6; restoring its callback and real
file update7 brings both tov7. Six file changes produce5 successful afterUpdate
events and1 intentionally forwarded/logged Vite rejection. Final graphs0. Tested
consumer Rq18ka matches current runtime bytes; QA tab/server closed.

No atomic rollback for arbitrary partial render failure is established. Invalid
semantic modules, async work, other browsers and performance remain open. Frozen
2h HTTP diagnostic is independent and predates current runtime changes.

Evidence: [qualification](benchmarks/forma-hmr-factory-qualification-2026-11-02.json),
[browser](benchmarks/forma-hmr-factory-browser-2026-11-02.json),
[before](benchmarks/forma-hmr-factory-before-2026-11-02.json).


## 2026-11-02 — Semantic-invalid HMR admission and inline contracts

Inline annotations were ignored by line-based parsing/validation; baseline failed
its validation assertion before HMR acceptance. Shared quote-aware token reading
handles inline contracts and quoted closing brackets, ignores text/comments in
validation, and refuses unclosed tokens. Generated HMR rejects invalid validation
before shared AST or instance mutation.

Contract4/4, full JS1346/102 files(maxWorkers2), types/build, installed package/GC,
generated lifecycle6/6 and actual installed Chromium Vite15/15 pass. Update8
preserves exact button/state/graph/cached factory; valid update9 reaches v9:10.
Eight file changes yield6 successful afterUpdate events and2 deliberate rejections.
Installed consumer yDwVPb package runtime bytes equal dist/package. QA tab/server
closed. Arbitrary partial-render rollback, malformed grammar, async lifecycle,
other browsers and performance remain open. Frozen2h HTTP diagnostic is separate
and predates these runtime changes.

Evidence: [qualification](benchmarks/forma-hmr-validation-qualification-2026-11-02.json),
[browser](benchmarks/forma-hmr-validation-browser-2026-11-02.json).


## 2026-11-02 — Refuse malformed known contract arguments

Five regressions reproduced silent acceptance of range missing/extra/nonnumeric
arguments and pattern non-string arguments (including unquoted brackets).
Parser and validator now share a complete known-annotation grammar check and
refuse those forms. Unknown annotation policy and general grammar remain open.

Contract9/9 and full JS1351/102 files(maxWorkers2), tsc and serialized package
build pass. This source change has no fresh installed-package/browser qualification;
previous browser15/15 report qualifies its previous snapshot. No atomic HMR
partial-render rollback or performance claim.


## 2026-11-02 — Pattern parsing parity on fresh installed package

A multiline quoted pattern passed validation but disappeared from parsed AST,
leaving an invalid prop unconstrained. Regression reproduced1 failed/9 passed.
Parser, validator and syntax check now share one quoted-pattern matcher.
Contract10/10 and full JS1352/102 files(maxWorkers2), types and package build pass.
Installed-package gate now asserts all5 malformed-argument refusals and actual
AOT import/runtime multiline constraint refusal plus valid-value recovery.
Fresh consumer6KPnxG passes installed/GC gates and actual Chromium HMR15/15;
all20 package runtime JS/WASM files equal dist/package. Browser fixture validates
HMR lifecycle; multiline pattern runtime proof is in the Node installed gate.
General grammar, unknown annotation policy, arbitrary partial-render rollback,
other browsers/async and performance remain open.

Evidence: [qualification](benchmarks/forma-pattern-parity-qualification-2026-11-02.json).


## 2026-11-02 — Stage HMR view before retiring previous DOM/resources

Regression reproduced old DOM loss after child-render refusal. Each view now
builds a candidate under separate root/disposer and key map, inserts candidate
nodes before retiring old nodes, and discards candidate resources/nodes on failure.
Pending disposer plus view-liveness guards preserve reentrant dispose/unmount
cleanup. Old callback/key lookup and exact button identity survive child-render
and partial insertion failure; successful retry retires old callbacks.

Full JS1354/102 files(maxWorkers2), types/build and fresh installed package/GC gate
pass. Actual installed Chromium fixture17/17 includes both new failure/retry paths
and prior15 HMR/lifecycle cases. Consumer9T7CrE matches all20 dist/package runtime
files. This is per-view preservation: AST/state/lets update before render, multiple
views can commit independently, cleanup exceptions and arbitrary commit callbacks
remain open. Connected callbacks can observe candidate and old DOM simultaneously.
No full transaction, other-browser, async or performance qualification is claimed.

Evidence: [qualification](benchmarks/forma-hmr-staging-qualification-2026-11-02.json).


## 2026-11-02 — Prepare all mounted views before retirement

Regression reproduced partial HMR: first view committed before second child render
refused. hotReload now prepares a snapshot of live view participants, aborting
candidates in reverse order on preparation failure, and retires old views only
after all preparations succeed. Per-view revision effects are replaced by explicit
participants; disposal unregisters participation and drains pending candidates.

Full JS1355/102 files(maxWorkers2), types/build and installed package/GC pass.
Actual installed Chromium18/18 includes second-view failure preserving both exact
old buttons/text/graph and successful retry, plus prior17 cases. Consumer m5w29X
matches all20 package runtime JS/WASM files. AST/state/lets still apply before
preparation, cleanup/commit exceptions cannot yet roll back, connection callbacks
can observe candidates alongside old nodes, and arbitrary reentrant HMR remains
open. This does not establish whole-instance transaction or other-browser/async/
performance/current-runtime soak qualification.

Evidence: [qualification](benchmarks/forma-hmr-multiview-qualification-2026-11-02.json).


## 2026-11-02 — Restore document data after preparation failure

Regression reproduced replaced original computed handles and candidate props/state
remaining after refused rendering. hotReload now retains previous lets until commit,
snapshots document metadata and signal values, and restores them on preparation
failure while disposing/removing candidate signals and lets. Revision invalidation
makes original computed handles and old DOM reactive again. Reentrant nested
hotReload is refused while an update is active. Terminal disposal releases both
old retained and candidate lets instead of restoring a disposed instance.

Full JS1357/102 files(maxWorkers2), types/build, installed package/GC and actual
installed Chromium19/19 pass. Browser checks original let identity, constraint
restoration, candidate signal removal, old DOM reactive update and successful
retry, plus prior18 cases. Consumer0auVyW matches all20 runtime files.

Rollback ends when commit starts; cleanup/commit exceptions remain open. Signal
snapshots preserve value references and cannot reverse arbitrary in-place mutations
or external factory side effects. Candidate connection callbacks can observe
tentative metadata/nodes. Other-browser/async/performance and current-runtime
natural-GC qualification remain open.

Evidence: [qualification](benchmarks/forma-hmr-document-qualification-2026-11-02.json).


## 2026-11-02 — Cleanup-triggered retirement during commit drains candidate

Two regressions reproduced publishing candidate resources after old child.dispose
synchronously disposed/unmounted the parent. Old view disposer is now detached
before invocation, avoiding reentrant teardown of the same handle. Candidate abort
ownership remains registered through old-view retirement, and commit rechecks
view liveness before publishing the candidate. Terminal dispose leaves graph0;
unmount leaves only instance resources and supports a fresh mount.

Full JS1359/102 files(maxWorkers2), types/build, fresh installed package/GC and
actual installed Chromium21/21 pass, including both new commit-retirement paths.
Consumer my49hy matches all20 package runtime files. General cleanup/commit
exceptions, full post-commit rollback, external/in-place side effects, arbitrary
connection callbacks, other browsers/async and performance remain open.

Evidence: [qualification](benchmarks/forma-hmr-commit-retire-qualification-2026-11-02.json).


## 2026-11-02 — Factory errors release completed constructions before return

Regression reproduced child signals retained when a custom factory compiled a
Forma child then threw before returning its handle. Synchronous factory calls now
track completed same-copy Forma constructions. On failure they drain in reverse
order while retaining the original exception; on success the returned handle uses
the existing renderer transfer and nonreturned helpers belong to view cleanup.
Reentrant cleanup clears the construction set before disposal.

Full JS1360/102 files(maxWorkers2), types/build, installed package/GC and actual
installed Chromium22/22 pass. Browser creates two instances before throwing,
checks graph returns to baseline, retries and checks nonreturned helper released
at final teardown(graph0). Consumer6hRRU0 matches all20 package runtime files.

Exceptions inside constructor before completed instance registration, independent
package-copy construction, arbitrary non-Forma resources, external side effects,
general post-commit rollback and async/other-browser/performance remain open.

Evidence: [qualification](benchmarks/forma-factory-qualification-2026-11-02.json).


## 2026-11-02 — Constructor errors release owned reactive allocations

Three regressions reproduced leaks after props getter, locale getter and AST let
metadata threw before a completed instance existed. Constructor-owned signal and
computed creation is journaled until success; failure drains allocations in reverse
order while preserving the original exception. External ViewModel signals are
excluded and stay usable. Successful construction clears the journal and later
HMR/runtime allocations keep existing lifecycle handling.

Full JS1363/102 files(maxWorkers2), types/build, fresh installed package/GC and
actual installed Chromium25/25 pass. Three browser constructor-error cases return
graph to baseline; JS regressions also verify external signal survives. Consumer
LnvJXs matches all20 package runtime files. Logs are preserved beside qualification.
Independent-copy factory construction tracking, arbitrary external/in-place side
effects, general post-commit rollback, async/other-browser and performance remain
open. No current-runtime natural-GC qualification.

Evidence: [qualification](benchmarks/forma-constructor-qualification-2026-11-02.json).


## 2026-11-02 — Construction ownership across independent copies

A-parent/B-child and B-parent/A-child probes both reproduced retained foreign
nodes after factory error. Compatible copies now share a versioned synchronous
construction domain in one JS realm. Installed gate includes both directions,
refusal after two instances, retry and nonreturned helper teardown(graphs0).
Full JS1363/102 files(maxWorkers2), types/build and expanded installed package/GC
pass. Node2/2 report is preserved. Consumer6fmUiE and physical browser peer match
all20 package runtime files each; distinct WASM cores asserted.

Initial peer browser load lacked Babel dependencies; temporary root now links
consumer dependencies. Unpaced browser run passed both new copy cases but stalled
on update5 with no watcher event; failed report is preserved. Explicitly paced
250ms file writes pass27/27 lifecycle scenarios on unchanged runtime bytes. This
paced pass does not resolve high-rate watcher delivery. Other realms/workers,
incompatible versions, async factories, external effects, post-commit rollback,
other browsers and performance remain open.

Evidence: [qualification](benchmarks/forma-construction-copies-qualification-2026-11-02.json),
[unpaced failure](benchmarks/forma-construction-copies-browser-watch-failure-2026-11-02.json).


## 2026-11-02 — Trace watcher pipeline and qualify a bounded unpaced burst

Added bounded raw/watcher/hot-hook/transform/websocket/write traces to the real
Vite probe, explicit configurable write interval, server Node version and separate
burst fixture. Isolated Vite transform-cache diagnostic delivers final source6/6
with/without write stability; this does not establish a throttle root cause.
Unpaced traced Chromium lifecycle27/27 delivers all8 changes with watcher/hooks.
Three unpaced32-write bursts(96 writes total) deliver final versions41/73/105,
preserve state/graph, retire old callbacks and end graph0. Four watcher events
coalesce intermediate changes. Runtime bytes remain unchanged(consumer6fmUiE,
20 files equal). Scripts syntax/diff and actual browser checks pass.

Historical unpaced update5 failure remains preserved and unexplained. Passing
runs do not close it or guarantee every intermediate write delivery. Other
browsers/filesystems/write patterns and performance remain open. Timings are
host diagnostics only; no runtime source change or full-suite rerun needed.

Evidence: [investigation](benchmarks/forma-watcher-investigation-2026-11-02.json).


## 2026-11-02 — Watcher write-pattern matrix

Added shared temporary-file writer for direct write, atomic rename and split write
with20ms incomplete-file interval. Isolated Vite Node18/18 cases(3 modes ×2 write
stability settings ×3 repeats) deliver final module with valid uiValidation.
Actual Chromium atomic and chunked modes each pass3×32 writes, preserving state,
graph and retiring stale callbacks, ending graph0. With prior direct burst this
is288 browser writes across9 bursts. All20 installed runtime files remain equal;
no production runtime changes or full-suite rerun claimed. Probe syntax/diff and
actual browser checks pass. Timings are diagnostic on a loaded host.

Historical update5 loss remains preserved/unexplained. This matrix does not prove
all filesystem/editor patterns or every intermediate delivery. Trace is bounded512
entries; qualification records whether a trace may be truncated. Other systems,
browsers and pathological write patterns remain open.

Evidence: [qualification](benchmarks/forma-watcher-write-matrix-qualification-2026-11-02.json).


## 2026-11-02 — Reject scope-copy optimization without measured allocation gain

Fresh release retained Runtime baseline covers legacy/Svet8/32/96/256 controls,
7 alternating idle/edit samples. Svet256 edit requests240822218 allocator bytes
(excluding paint/presentation), not live memory. Tested Cow scope borrowing and
then skipping identical metadata overlays; both experiments produce exactly the
same idle/edit allocation counts/requested bytes for every sample. Therefore the
runtime source was restored byte-for-byte, not retained as an unmeasured gain.
Experimental implementation passed159 library tests. Baseline and both experiment
reports are preserved. Timings on this loaded host are not a performance claim.

Next target remains measuring deep __state/model clone callsites in instantiation
and expansion, then reducing per-control snapshots without losing props.__state,
lexical frames or owned public SceneControl snapshots. No large-form readiness.

Evidence: [experiment](benchmarks/native-scope-experiment-qualification-2026-11-02.json).


## 2026-11-02 — Frozen lifecycle-traced HTTP 2h diagnostic completed

Natural-GC frozen snapshot-Nv0ST2 completed1144736 requests/7200025.895ms with0
request/cleanup failures,0 forcedGC and graph cleanup. All381595 server sockets
closed. Analyzer passes original budgets(heap16MiB,RSS64MiB,external64MiB,
ArrayBuffers16MiB); external post-warmup peak growth49686991 bytes. All20 runtime
and2 harness hashes match frozen manifest; artifactsUnchanged=true.

Global external peak200920161 bytes co-occurs with160 pending memory-finalization
callbacks/178257920 registered bytes; after shutdown8 pending remain. These are
callback-lag counters, not exact live allocations. This supports investigation of
GC/native allocation pacing but does not prove the historical72409329-byte growth
failure cause. Frozen runtime predates current reactivity/Forma changes and cannot
qualify current package. New source-bound current-runtime2h soak remains open.

Evidence: [qualification](benchmarks/http-core-traced-2h-qualification-2026-11-02.json).


## 2026-11-02 — Borrow visual expansion state until lexical writes

Opt-in benchmark allocation stack sampling records546 bounded samples(no truncation)
and preserves baseline allocation counters exactly; profiling costs are excluded
from counters and instrumented timings are not benchmark evidence. Clone frames
identify expand_sequence as a concrete repeated deep-state copy site. Visual
sequences now borrow inherited state until Let writes; For retains an owned local
copy per iteration. Regression checks lexical shadowing, caller-state preservation
and independent returned visual values.

Svet256 edit requested bytes240822218→104300015(-56.69%); allocations1790945→781793.
Legacy256 also improves;8/32/96 improve. Two candidate release runs repeat all
allocation counters exactly. This excludes painting/presentation and is cumulative
requested allocator bytes, not live memory or validated frame latency. Remaining
__state/model clones in instantiation/scene expansion remain open.

160 library,10 allocation and8 scene-artifact tests pass. Fresh wasm32 release
build and actual generated Node WASM artifact/scroll/refusal gate pass. No fresh
browser/window/GPU qualification or performance timing claim. Profiling is disabled
by default and enabled only by FORMA_ALLOC_TRACE_OUTPUT in the benchmark example.

Evidence: [qualification](benchmarks/native-state-cow-qualification-2026-11-02.json).


## 2026-11-02 — Avoid discarded visual AST clones and borrow definitions

expand previously cloned the source subtree before replacing both props/children.
It now creates the expanded shell directly. instantiate borrows definition nodes
until supplied ContentPresenter content needs mutation. Regression verifies two
distinct supplied contents and a no-content instance leave shared definition
unchanged and returned nodes independent.

Svet256 edit requested bytes104300015→103139311(-1.11%); allocations781793→770785.
Svet8 improves6.46%;32/96 and legacy improve. Two release runs repeat all allocation
counters exactly.161 library,10 allocation,8 scene-artifact tests, fresh wasm32
release build and actual generated Node WASM artifact gate pass. No controlled
latency/FPS/live-memory or current browser/window/GPU qualification follows.
Per-control snapshots and large-form readiness remain open. Current-runtime HTTP
snapshot-RKk0T4 remains independent and running.

Evidence: [qualification](benchmarks/native-visual-shell-qualification-2026-11-02.json).


## 2026-11-02 — Transfer scene state snapshot through visual instantiation

Refreshed stack samples identify instantiate/scene_expand clone sites. Measuring a
scene leaf now transfers its owned __state into evaluation props and returns the
snapshot on Result success/error; props.__state and caller state retain prior
semantics. Root Frame measurement changes only its kind temporarily instead of
cloning the whole expanded tree. Public SceneControl snapshots remain owned.
Regression compares legacy/transferred results, error restoration and missing
snapshot/default metadata isolation.

Svet256 edit requested bytes103139311→57633702(-44.12%); allocations770785→434657.
8/32/96 and legacy improve. Two final-source release runs repeat all allocation
counters exactly.162 library,10 allocation,8 scene-artifact tests pass. Fresh
wasm32 release and generated Node artifact gate pass; two additional actual WASM
cases verify custom props.__state.caption and For root/item snapshots. Node ABI
gates added to studio CI; remote CI has not been run.

This is cumulative requested bytes excluding paint/presentation, not live memory
or validated latency. Non-precomputed instantiation and other remaining model
clones, fresh browser/window/GPU and full large-form readiness remain open. Current
JS runtime HTTP snapshot-RKk0T4 runs independently and unchanged.

Evidence: [qualification](benchmarks/native-snapshot-transfer-qualification-2026-11-02.json).


## 2026-11-02 — Flat Frame transfers expanded nodes instead of cloning snapshots

Flat-root control collection now moves expanded children just as positioned
layout does. Fallback without precomputed visual uses snapshot-transfer instantiation.
Benchmark --frame covers this path separately. Regression replaces document model
after an artifact is returned and verifies old root/item snapshots remain intact;
both artifacts load with expected control count/text.

Frame Svet256 edit requested bytes103231190→57377128(-44.42%); allocations770997→431029.
All sizes and legacy improve. Two Frame runs repeat allocation counters exactly;
Column counters equal the preceding candidate.163 library,10 allocation and8
scene-artifact tests pass. Fresh wasm32 release plus actual generated Node artifact
and snapshot3/3 gates pass; snapshot gate now covers both Column and Frame. Remote
CI not run. No latency/FPS/live-memory/current browser/window/GPU claim.

Remaining model snapshots, scene expansion and large-form readiness remain open.
Current JS HTTP snapshot-RKk0T4 runs independently on unchanged installed bytes.

Evidence: [qualification](benchmarks/native-flat-transfer-qualification-2026-11-02.json).


## 2026-11-02 — Borrow lexical frame with separate resolved collection path

Dropping key scope was rejected: even item.id can evaluate a Value::Expr reading
captured props. A Cow experiment has no allocation effect on the ordinary fast
path and is not retained. New benchmark --lexical exercises a real Let collection
alias; baseline Svet256 requests599367759 bytes/edit. Recovery now passes resolved
collection path separately instead of deep-cloning/mutating the retained frame.
Full key scope and snapshots remain unchanged.

Lexical Svet256 edit599367759→509108669 requested bytes(-15.06%); allocations
4399128→3736088. All measured sizes/legacy improve. Two final-source runs repeat
counters exactly; ordinary Column counters unchanged.164 library,10 allocation,
8 scene-artifact tests, fresh wasm32 and actual generated Node artifact gate pass.
Node snapshot gate4/4 includes live text input through retained Let alias and
no-op second sync. Rust regression forces computed item.id via props through a
Let alias after reorder and verifies immutable retained frames. CI gate includes
new Node case; remote CI not executed.

509MB/edit remains expensive: large lexical forms are not qualified. Cumulative
requested bytes are not live memory; no latency/FPS claim. Remaining Let replay/
model copies and environment qualification stay open. Current JS HTTP snapshot
runs independently on unchanged installed runtime.

Evidence: [qualification](benchmarks/native-lexical-frame-qualification-2026-11-02.json).


## 2026-11-02 — Borrow initial Let replay state and move evaluated locals

First root Let now reads borrowed state until prior locals or parents require an
overlay. Final evaluated Let values move into locals instead of being cloned again.
Caller root state and captured lexical frames remain unchanged. Computed-key
regression now forces a two-stage Let alias after reorder; previous shadowing and
failed-replay rollback/retry regressions remain green.

Lexical Svet256 edit requested bytes509108669→419404479(-17.62%); allocations
3736088→3079704. All sizes and legacy improve. Two candidate runs repeat allocation
counters exactly; ordinary Column counters unchanged.164 library,10 allocation,
8 scene-artifact tests, fresh wasm32 and generated Node artifact gate pass. Actual
Node snapshot/alias5/5 includes single and chained alias input commit plus no-op
repeat. Existing studio CI gate includes the added case; remote CI not executed.

419MB requested/edit remains high and is not live memory. Latency/FPS, remaining
replay/model copies, large-form readiness and other environments remain open.
Current JS HTTP snapshot-RKk0T4 continues independently on unchanged bytes.

Evidence: [qualification](benchmarks/native-let-replay-qualification-2026-11-02.json).


## 2026-11-02 — Do not clone locals replaced by lexical replay

Recovery now omits captured local/parent values whose names are replaced by
replayed lexicals before key resolution. Validation and parent-path resolution
still execute; final lexical values retain prior precedence. Frames without
lexicals use the original clone path. No captured metadata is mutated.

Lexical Svet256 edit requested bytes419404479→374632383(-10.68%); allocations
3079704→2751512. All sizes and legacy improve. Two final-source release runs repeat
counters exactly; ordinary Column counters unchanged.164 library,10 allocation,
8 scene-artifact tests, fresh wasm32 plus generated Node artifact and snapshot/
single/chained alias5/5 pass. Existing nested shadowing,computed key and rollback
regressions remain green. Remote CI not run.

374MB requested/edit is still high and not live memory. Large-form readiness,
controlled latency/FPS and remaining replay/model copies remain open. Current JS
runtime natural2h soak continues independently; no terminal qualification yet.

Evidence: [qualification](benchmarks/native-local-overlay-qualification-2026-11-02.json).


## 2026-11-02 — Omit duplicate private lexical frame locals

Refreshed lexical stack profile840 samples(no truncation) identifies scene_expand
as remaining copy site. Generator no longer duplicates lexical values in frame
__locals because they are retained in __lexicals and overwrite locals on replay.
Full model/item/alias values remain in owned public control snapshots. Regression
reintroduces legacy duplicate fields and compares commits on independent Runtime
instances(sync acknowledges edits), proving identical resulting model state.

Lexical Svet256 edit requested bytes374632383→284706751(-24.00%); allocations
2751512→2094360. Two release runs repeat counters exactly; ordinary Column counters
unchanged.165 library,10 allocation,8 scene-artifact tests, fresh wasm32 and actual
Node artifact/snapshot/alias5/5 pass. Old/new WASM artifact comparison4/4 at8/32/96/
256 controls proves source/templates equal and only duplicate private locals differ.
Preceding local WASM reader accepts new artifacts.256-control JSON35653311→28622783
bytes. This is not a blanket compatibility proof for historical releases.

284MB cumulative requested/edit still needs reduction; live memory, controlled
latency/FPS, large-form readiness and other environments remain open. Remote CI
not run. Current JS HTTP snapshot-RKk0T4 continues independently on unchanged bytes.

Evidence: [qualification](benchmarks/native-frame-dedup-qualification-2026-11-02.json).


## 2026-11-02 — Compact final replay metadata and measure owned artifact retention

Final control transport omits root __binding_lexicals/__binding_let_steps only when
an exact typed copy exists in retained frames. Rendering evaluates the full scope
before compaction; user model/item/alias fields remain. Missing or later unmatched
Let scope keeps original metadata. Both full and structural control maps use this
boundary. Legacy duplicate-map recovery and later-Let guard regressions pass.

Edit requested-byte counters do not improve: allocation happens before transport.
New opt-in --artifact-retention benchmark measures warmed net GlobalAlloc Layout
bytes while owned artifact is alive and asserts zero delta after Drop. Same-source
baseline bypasses only two compaction calls; candidate restored byte-for-byte.
256-control artifact retained bytes181092378→135619610(-25.11%); all sizes improve.
Two candidate runs repeat counters exactly; all baseline/candidate Drop deltas0.
This excludes Document,allocator overhead,RSS,GPU and is not peak/full Runtime memory.

166 library,10 allocation,8 scene-artifact tests, fresh wasm32 and actual Node
snapshot/alias5/5 pass. Old/new WASM comparison4/4 proves source/templates/all other
fields equal after only proven duplicate normalization; preceding local reader
accepts new artifacts.256-control JSON28622783→21516991 bytes. No edit-speed,
controlled latency/FPS or broad historical-reader compatibility claim. Remaining
model copies/large-form readiness remain open; remote CI not run.

Evidence: [qualification](benchmarks/native-replay-transport-qualification-2026-11-02.json).


## 2026-11-02 — Current native WASM browser compile/input/raster qualification

Fresh Cargo wasm32 build plus wasm-bindgen web output served with immutable JS/
WASM/fixture hashes. Current Rust form_document source matches latest ownership/
transport candidate. Actual Chromium1568/8 passes: compile/load,empty scroll extent,
clipping,atomic malformed/refusal recovery,explicit CPU scales1/1.25/2,shrink/cache
revision,single/chained Let alias Unicode input and custom props.__state visible
update. Both alias cases commit Alpha—правка to correct item,leave Beta unchanged,
second sync no-op,and change1197 RGBA bytes within edited control bounds. Actual
canvas inspected for readable text. Owned before-pixel copies avoid stale WASM
views during rebuild/free. Runtime objects freed in finally.

CPU Canvas2D slice only: not OS DPI/window/IME,other browser engines,hardware GPU,
device-loss,full golden parity,browser GC plateau or performance qualification.
Full source-bound candidate/environment matrix remains open.

Evidence: [qualification](benchmarks/native-current-browser-qualification-2026-11-02.json).


## 2026-11-02 — current-runtime natural HTTP soak qualification

Завершённый natural-GC loopback HTTP прогон проанализирован с исходными
бюджетами heap16/RSS64/external64/ArrayBuffers16 MiB и минимумом2h.
Результат within-observed-budget: 1 655 800 requests, 7 200 013 ms,
request/cleanup failures0, forcedGC0. Post-warmup trough growth: RSS147384,
external4035 bytes; ArrayBuffers peak growth13340 bytes; остальные growth0.
Пересобранный package и текущий harness совпали по22 SHA256 с сохранёнными
идентичностями; initial/final runtime hashes и manifest совпали.
Анализатор:10/10 tests. Evidence: benchmarks/http-current-runtime-natural-2h-qualification-2026-11-02.json.
Это не доказательство отсутствия медленных утечек или стационарности;
исторический external failure, gzip/codec/proxy и kernel saturation открыты.


## 2026-11-02 — multi-view HMR cleanup qualification

Добавлены две регрессии: исключение из child.dispose после освобождения
и вложенный hotReload из cleanup во время commit. Existing owner teardown
перехватывает ошибку, продолжает siblings; оба views обновляются, старые
click callbacks inert, повторный HMR сохраняет graph baseline, teardown graph0.
Focused HMR23/23, TypeScript passed. Runtime не менялся.
Общая атомарность commit и arbitrary user side effects остаются открытыми.
Evidence: benchmarks/forma-hmr-cleanup-qualification-2026-11-02.json.


## 2026-11-02 — explicit contract admission

Shared parser/validator contract check now rejects unknown names, empty annotations,
reversed/nonfinite range bounds, invalid regex and patterns rejected by the
existing safety policy. Six new cases failed before the fix; focused contract +
HMR tests40/40 and TypeScript passed afterwards. README states the accepted
grammar and the compatibility impact of rejecting unknown metadata.
Full suite1371/1372: synthetic benchmark exceeded its existing5000ms timeout;
unchanged isolated repeat1/1 passed. This full run remains recorded as failed.
Installed AOT fixture now separately checks parser refusal and HMR validation
admission, since invalid source can no longer produce a parser AST.
Placement/duplicate/dangling annotations and arbitrary AST validation remain open.
Installed-package gate passed after fixture update; evidence: benchmarks/forma-contract-admission-qualification-2026-11-02.json.


## 2026-11-02 — component contract placement

Seven reproduced parser failures now reject duplicate range/pattern contracts,
dangling annotations and contracts separated from their prop by state, let,
tokens or a visual root. Distinct contracts can be stacked across line comments
on one required prop. The validator invokes the same parser placement rules
after token/semantic admission, so an invalid placement is not admitted by HMR
validation. Focused48/48 and TypeScript passed. This covers component-level
declarations; annotations inside visual subtrees and arbitrary ASTs remain open.
Full suite1380/102 files with one worker and unchanged timeout passed; installed-package gate passed. Evidence: benchmarks/forma-contract-placement-qualification-2026-11-02.json.


## 2026-11-02 — nested contract refusal

Five reproduced cases now refuse contracts in nested visual/control-flow/token
blocks and after a visual root. Component-body scanning respects quoted strings,
escapes, line comments and quoted contract contents. HMR source refusal retains
old DOM identity, graph baseline and callbacks; valid retry retires old callbacks.
Focused55/55 and types passed. Installed AOT lifecycle now exercises admission
from the installed package (seven cases); package checker case count updated.
Arbitrary caller-provided AST validation remains open.
Full1387/102 files, types and full installed-package gate passed. Evidence: benchmarks/forma-contract-nested-qualification-2026-11-02.json.


## 2026-11-02 — AST prop contract admission

Eight reproduced AST bypasses now refuse reversed/infinite/extra/string range
bounds, invalid/unsafe/non-string regex and duplicate props before construction
allocations or HMR document mutation. The contract check also verifies prop
metadata shape. Source parser validates emitted contracts; validator runs parser
admission even when no inline annotations occur, rejecting duplicate source props.
Focused64/64 and TypeScript passed. Installed package gate passed with eight AOT
lifecycle cases, including no-allocation malformed AST refusal and live-state
preservation on HMR refusal. Complete AST structure/cycles, accessor/proxy
semantics and post-admission mutation remain open.
Full1396/103 files passed. Evidence: benchmarks/forma-ast-contract-qualification-2026-11-02.json.


## 2026-11-02 — iterative visual AST admission

Eight reproduced malformed/cyclic visual AST cases now refuse before runtime
allocation or HMR mutation. Iterative DFS validates node shape and all branch
arrays, distinguishes shared acyclic nodes from cycles, and admits a20k-deep
acyclic tree without recursive validation (rendering at that depth is unqualified).
Sparse range bounds exposed an Array.every hole; direct numeric bound checks
now reject holes. Sparse roots/styles are also rejected. Stable focused22/22
(tree + contracts), types passed. An intermediate full run overlapped these
source edits and recorded failures; retained as intermediate evidence, not a
qualification. Final package/full gates run after the sources are stable.
Document metadata, accessors/proxies, post-admission mutation and renderer
stack depth remain open.
Stable full1409/104 files, types and rebuilt installed-package gate passed. Evidence: benchmarks/forma-ast-tree-qualification-2026-11-02.json.


## 2026-11-02 — node-kind and document metadata admission

Nine reproduced kind-field failures now reject missing/empty tag, condition,
item/list, match expression/arms, slot name and empty explicit key expression.
Eight metadata failures now reject malformed component name, token/state map
shape, style/i18n string maps and let/shortcut declaration arrays (including holes).
Refusal occurs before construction allocations and HMR mutation; old DOM identity,
graph baseline and reactive state survive. Focused94/94 and types passed.
Semantic expression/name admission, accessor/proxy effects and post-admission
mutation remain open.
Full1426/105 files and installed-package gate passed. Evidence: benchmarks/forma-ast-document-qualification-2026-11-02.json.


## 2026-11-02 — declaration names and overwritten computed leak

Ten new name tests failed before the fix. In the prior built package, constructing
an AST with duplicate lets then disposing left graph.nodes=1 (raw diagnostic
retained); duplicate lets now refuse before allocating overwritten handles.
Reserved AST state/let names and reserved source declaration names are refused.
State/prop/computed dictionaries use null prototypes, preserving valid own names
like toString/valueOf/hasOwnProperty. Three further reproduced HMR reset failures
were fixed by giving declared-default dictionaries null prototypes too.
Focused107/107 and types passed. Accessors/proxies, AST mutation and complete
expression semantics remain open.
Full1439/106 files and installed-package gate passed. Evidence: benchmarks/forma-ast-names-qualification-2026-11-02.json.


## 2026-11-02 — own caller AST snapshots

Seven reproduced cases let post-admission caller edits change visual properties,
let expressions, range bounds or prop object defaults. Object AST construction/HMR
now snapshots recognized document fields, iteratively copies the visual graph,
and detaches contract/let/shortcut/default data. Source-string parser results are
already private and avoid the redundant copy. Caller-added cycles do not affect
private views. An intermediate snapshot regression dropped explicit undefined
state fields; per-value state cloning preserves their presence and reset value.
Focused119/119 passed, including construction/HMR and shortcut mutation; deep
acyclic20k admission and shared-node rendering are retained in the suite.
Accessors/proxies, function data, unknown extension objects and snapshot overhead
remain open; this is not a general arbitrary-JS deep-immutability guarantee.
Stable full1451/107 files, types and installed-package gate (AOT9 cases) passed. Evidence: benchmarks/forma-ast-snapshot-qualification-2026-11-02.json.


## 2026-11-02 — schema-only snapshots and extension retention

Twelve getter regressions reproduced spread-copy reads of unknown extension
fields on document/node/prop/let/shortcut/match-arm objects. Explicit schema
copy now ignores these fields without executing their getters. Focused131/131
and types passed. A Node --expose-gc baseline against the previous built runtime
retained5 of6 unknown extension payloads while the instance remained mounted;
its report records compiler/probe hashes. The new installed gate includes live
reactivity after collection and two held/released positive controls. Recognized
field accessors/proxies, function data and snapshot overhead remain open.
Final full1463/108 files, types and installed-package gate passed. Same GC probe hash before/after: candidate0/6 retained, controls2 held then0 released; mounted state remains reactive. Evidence: benchmarks/forma-ast-extensions-qualification-2026-11-02.json.


## 2026-11-02 — AOT snapshot cost diagnostic

Added scripts/bench-forma-snapshot.mjs: isolated rotating A1/B/A2 workers,
shared trusted immutable AST workload at8/32/96/256 buttons, construction/dispose,
mount and staged HMR timed separately. Functional DOM/state/stale-callback checks
and graph.nodes=0 asserted each iteration. Candidate runtime20 hashes verified;
baseline differs only in an injected unsafe trusted-input snapshot bypass.
Six rounds,18 workers,2592 timed samples; input/harness/runtime identities checked.
On256 controls construction medians: baseline0.088ms, candidate0.253ms; median
paired delta0.164ms. This is diagnostic only: loadavg34–37 on16 logical CPUs,
A/A scatter up to63%; mount/HMR differences are inconclusive. No production bypass,
latency/FPS/allocation or readiness claim. Quiet-host measurements and profiling
remain open. Evidence: benchmarks/forma-snapshot-cost-qualification-2026-11-02.json.
Reproduce after package build:
`node packages/svet/scripts/bench-forma-snapshot.mjs /tmp/svet-snapshot-cost.json 12 6`.
Engine runtime did not change in this milestone; prior correctness qualification
remains hash-matched. Harness passed node --check, pilot and completed diagnostic.


## 2026-11-02 — profile-guided visual validation allocation reduction

CPU sampling20000 constructions of256-control AOT ASTs identified tree validation,
string-map checks and GC as the main sampled work. Replaced Object.values/Array.from
temporary validation arrays with own-property/element loops, per-node kind/string
arrays with constants/switch, and object DFS frames/combined child arrays with a
private exit marker and direct branch scheduling. Sparse/null/undefined nodes,
all branch cycles, shared nodes and deep admission remain checked. No validation
or snapshot bypass in the candidate.
Focused134/134, full1466/108 files, types and installed-package/GC gate passed.
Preserved full baseline runtime archive and both compressed CPU profiles.
Paired A1/B/A2 comparison:6 rounds,18 workers,2592 samples; only forma.js differs
between baseline and candidate. Construction medians256 controls0.199→0.155ms,
paired median delta−0.0446ms; all6 paired construction deltas improve at32/96/256.
This remains local diagnostic evidence: loadavg30–33 on16 CPUs; mount/HMR scatter
and browser/native latency are not qualified. Quiet-host performance gate stays
open. Evidence: benchmarks/forma-validation-opt-qualification-2026-11-02.json.


## 2026-11-02 — HMR admission lock and final cleanup draining

Three initial failures reproduced nested HMR during AST getter admission, missing
instance liveness boundary after getter disposal, and a permanently set update
lock after final computed cleanup threw. Update lock now wraps admission and all
cleanup in an outer finally, with liveness checks before document mutation.
Two further cleanup failures reproduced a retained retired sibling computed and
masking of a primary commit failure. Final cleanup drains all retired siblings,
preserves a primary update error, or propagates the first cleanup failure after
a successful update. No post-commit rollback guarantee is added.
Focused138/138 and types passed; full and installed-package gates run on stable
sources. Recognized accessor side effects beyond these boundaries remain open.
Stable full1470/108 files and installed-package gate (AOT11 cases) passed. Evidence: benchmarks/forma-hmr-admission-qualification-2026-11-02.json.


## 2026-11-02 — rollback/instance teardown draining and ownership

Reproduced candidate computed cleanup failure that replaced the preparation error,
prevented previous computed restoration and orphaned siblings. Rollback now drains
candidate siblings, restores old identity/reactivity and preserves the preparation
error; if cleanup closes the instance, restoration stops. Three direct-disposal
regressions reproduced interrupted owned cleanup, accidental disposal of an external
signal after options.viewModel replacement, and a leaked owned signal for a skipped
invalid ViewModel entry. Successful binding ownership is recorded at construction.
Instance disposal drains siblings/built-ins, clears registries and returns first
cleanup error afterwards; idempotence remains. Wide focused142, final targeted33,
full1475/109 files, types and installed-package gate (AOT13 cases) passed.
Evidence: benchmarks/forma-rollback-dispose-qualification-2026-11-02.json.
Custom failing resource self-release, arbitrary callback effects and post-commit
atomic rollback are not guaranteed. No new latency qualification.


## 2026-11-02 — allocated state ownership independent of public aliases

Three regressions reproduced repeated disposal through aliases, lost original own
state and accidental foreign disposal after public state-map replacement, plus
rollback freeing a foreign alias instead of the allocated candidate signal.
An internal allocated-state registry now owns the actual created handles; public
map edits do not transfer that ownership. Rollback removes/releases its allocated
candidate handle and ignores foreign replacement ownership. A further callback
regression showed a prop setter invoked after closing state cleanup; liveness
guards now stop remaining rollback steps.
Signal.peek is not used as a disposal oracle: tests observe disposer counts, live
external values and graph totals. Focused147/147 and types passed. Direct map
assignment is not a qualified reactive rebinding API; other public resource-map
replacement semantics remain open.
Stable full1479/109 files and installed-package gate passed. Evidence: benchmarks/forma-state-ownership-qualification-2026-11-02.json.


## 2026-11-02 — prop/computed allocated ownership and HMR generations

Six reproduced cases showed repeated disposal through public aliases, hidden own
prop/computed leaks, foreign computed disposal on HMR retirement and candidate
rollback freeing the wrong handle. Separate allocated-prop registry and current
owned-computed set now govern cleanup. HMR keeps previous owned generation apart
from the published let map, drains originals after commit, or restores ownership
and publication on rollback. Foreign public replacements are never acquired.
Focused153/153 and types passed; stable installed/full gates run below. Direct
map mutation remains unqualified as a reactive rebinding API; arbitrary callback
reentrancy and post-commit rollback remain open.
Stable full1485/110 files and installed-package gate (AOT14 cases) passed. Evidence: benchmarks/forma-resource-ownership-qualification-2026-11-02.json.


## 2026-11-02 — allocation liveness and options callback reentrancy

Three initial regressions reproduced options prop getters closing an instance
while HMR continued allocation/rebuilding or returned success with closed data.
Own factories now check liveness before/after resource creation, register ownership
before public-map publication, and HMR checks after apply/rebuild. Old owned lets
are retired only after real detachment; rollback returns ownership before restoring
publication. Three setter closure cases verify state/prop/let allocation publication
and one-disposal old handles. A further rollback regression exposed a repeated
options getter read; restoration now uses previously resolved prop defaults instead
of caller getters, retaining old live handles and the primary preparation error.
Focused160/160 and types passed. Arbitrary callback effects, mutable object rollback,
reactive rebinding and browser/native qualification remain open.
Stable full1492/111 files and installed-package gate (AOT15 cases) passed; current20 runtime hashes recorded. Evidence: benchmarks/forma-hmr-options-qualification-2026-11-02.json.


## 2026-11-02 — current installed Chromium HMR qualification

Fresh packed/installed consumer passed package gate (--keep); current Chromium156
ran expanded browser lifecycle matrix. Legacy fixture passed26 cases then timed out
because it only watched module admission errors, while invalid range now refuses
in Vite transform. An expanded reconnect run passed28 then timed out on its first
update; watcher/hook/ws send were recorded, browser import was absent. Both failure
reports are retained. Added explicit hot websocket readiness acknowledgement and
strict known transform-error observation while preserving old DOM/state/graph,
cached factory and valid recovery checks. Fresh page then passed35 automated cases.
Trusted automation input clicked add (isTrusted=true,v9:1→v9:2), then cleanup
(isTrusted=true,graph0):37/37 total,7 successful HMR updates and one expected
invalid-contract transform refusal. Added current snapshot/ownership/rollback/
closing getter/extension/nested admission browser scenarios.
Verified60 runtime hashes across workspace, installed and physical peer copies;
source/fixture identities and final screenshot preserved. Browser GC, other engines,
production bundle/native/GPU/performance remain open. Readiness does not establish
the cause of historical watcher update5 loss. QA tabs and server closed.
Evidence: benchmarks/forma-current-browser-qualification-2026-11-02.json.


## 2026-11-02 — Forma natural browser WeakRef cohort qualification

Added installed-runtime GC fixture/server. Three independent Chromium156 page
cohorts each track24 unused extension payloads while4 Forma views remain mounted,
4 disposed isolated core exports +4 Memory, and10 held/released controls. Modes:
never-mounted, mounted, repeated unmount and HMR teardown. Two task-separated
bounded allocation-pressure runs per cohort; no explicit forced/CDP GC requested.
All32 targets collect with controls held; views remain reactive (labels2→4).
After release all10 controls collect and default graph32→0. Aggregate96 target
objects collected,30 controls held then collected, forcedGC0 across3 cohorts.
Verified20 installed runtime hashes against workspace/current qualification.
Raw reports, held/final screenshots and source/server hashes retained; QA tab and
server closed. This does not establish RSS/heap stationarity, slow-leak freedom,
retained-handle/DOM behavior, async mixed-core or other-engine readiness.
Evidence: benchmarks/forma-browser-gc-qualification-2026-11-02.json.


## 2026-11-02 — terminal Forma actions compatibility

Public facade initially bypassed action overrides from mounted Button events;
a regression reproduced the failure. Per-action slots now preserve overrides,
wrapping of the original action without recursion, and restoration of original.
Disposal clears original and override slots. Cached terminal reads and inert
writes are covered. Focused52/52, full1494/112 files, types, package build and
fresh installed-package gate passed. Installed retained-handles GC checks all
four modes (16 instances, 32 core/Memory targets): retained0, controls4→0, graph0.
Runtime and probe hashes match the local tested package. This does not qualify
raw captured SDK handles, older SDK cache protocols or the new bytes in a browser.
Evidence: [qualification](benchmarks/forma-public-actions-qualification-2026-11-02.json).


## 2026-11-02 — terminal cached values without private Signal metadata

A reproduced regression showed a foreign Signal losing its terminal value
(7→undefined) when private cache metadata was absent. Terminal snapshots now
fall back to Signal.peek for state/props/locale. Computed.peek is never used as
a fallback: it could execute user code during teardown. A regression verifies
unused lazy computed remains unevaluated. Foreign computed without safe cache
metadata has undefined terminal cache. Focused17/17, types, build, full1496/112
files and fresh installed-package gate passed. Retained public handles keep
zero core/Memory targets, controls4→0, graph0 in the installed Node probe.
[Qualification](benchmarks/forma-terminal-cache-qualification-2026-11-02.json).
Browser and raw SDK retained-handle qualification remain open.


## 2026-11-02 — disposed raw Signal/Computed context detachment

Separately retained state, prop, computed and signal.peek handles each retained
4 cores/4 Memory after Forma disposal; action/unsubscribe retained0. Signal now
drops core/context/direct subscriber/comparator after disposal and active writes
finish. Computed drops core/context/owner/body/dependencies after its observation
frame unwinds. Immediate Signal detachment initially broke2 existing callback
reset/source-replacement tests; active-write counting preserves unwinding.
Four dedicated regressions cover cached/inert methods, comparator disposal,
owner disposal during evaluation and post-disposal computed caching.
Focused117/117, types, build, full1500/113 files and fresh installed-package gate
passed. Same-hash raw probe:24 strongly held handles,48 core/Memory targets,
retained0 in all6 modes, controls4→0, graph0. Natural browser GC, retained root
disposers, reset-only lifetime and hot-path cost remain open. Cached user values
may intentionally retain arbitrary data, including native contexts.
[Qualification](benchmarks/reactive-handle-detach-qualification-2026-11-02.json).


## 2026-11-02 — retained root disposer lifetime

All4 root-disposer modes retained4 cores/Memory before the fix: explicit close,
construction throw, close during construction, and throwing cleanup. Root
disposers now use a terminal cell registered in a WeakMap; physical scope
teardown clears its core/scope references. This avoids changing observable
owner cleanup registrations. A first implementation using an extra cleanup
registration failed3 existing counter tests; the WeakMap version passes them.
Live roots still strongly own their core until disposal. Reentrant cleanup and
escaped disposers after failed construction have dedicated regressions.
Types, focused173/173, full1502/113 files, build and fresh installed-package gate
passed. Same-hash installed GC probe:16 held disposers,32 collected core/Memory
targets, controls4→0, live-root controls2→0, graph0. Current browser runtime
qualification, reset-only retention and bookkeeping performance remain open.
[Qualification](benchmarks/root-disposer-qualification-2026-11-02.json).


## 2026-11-02 — current terminal handles: Chromium GC and HMR

Fresh installed runtime after public Forma facade, raw Signal/Computed and root
disposer detachment passes3 independent Chromium156 natural-GC page cohorts.
Each cohort:120 targets collected with44 terminal handles strongly retained,
88 late calls,10 positive controls held then collected, forcedGC0 and graph0.
Totals:360 targets,132 retained handles,264 late calls,30 controls.11 retained
modes cover instance/mount/bound-render/button/state/props/computed/peek/action/
unsubscribe/root disposer, plus4 earlier lifecycle modes and unknown extensions.
All20 JS/WASM runtime hashes match workspace dist/package. This bounded pressure
probe does not establish heap/RSS plateau or browser-wide leak freedom.
The same installed runtime passes real Vite HMR37/37 (35 automated +2 trusted
input/cleanup), final graph0;60 workspace/installed/peer artifact hashes match.
Historical watcher update5 loss, other engines, reset-only handles and hot-path
performance remain open.
[GC evidence](benchmarks/forma-terminal-browser-gc-qualification-2026-11-02.json),
[HMR evidence](benchmarks/forma-terminal-browser-hmr-qualification-2026-11-02.json).


## 2026-11-02 — graph reset retires retained raw handles

Reset-only retained Signal/Computed getter/peek handles each retained4 cores and
4 Memory in local and compatible peer reset modes (8 modes total). Computed
probe construction uses an independent root so async request teardown cannot
mask reset-only retention. Reset now captures internal retirement callbacks
from the old registries before reset cleanup can allocate fresh nodes. Public
dispose replacement is ignored; cached reads and fresh-slot safety are covered.
The retirement registry is a shared-domain WeakMap. A module-local intermediate
version passed local probes but failed all peer reset modes. Root disposer cells
also live in the shared domain. Types, build, full1503/113 files and fresh
installed-package gate passed. Same-hash final probe:32 held handles,64 collected
core/Memory targets, controls4→0, graph0. Prior browser reports precede these
runtime bytes. Browser requalification, reset-only effects/roots and performance
remain open. [Qualification](benchmarks/reset-handle-retention-qualification-2026-11-02.json).


## 2026-11-02 — terminal public effect handles

Disposed EffectHandle and saved dispose methods retained4 core/Memory each in
local and peer reset modes. A dedicated public-handle cell now detaches native
core, parent owner and pooled internal handle before invoking cleanup. Cleanup
reentry and throws cannot retain or dispose a replacement pooled effect.
Types, build, focused85/85, full1505/113 files and fresh installed-package gate
passed. Same-hash installed probe collects32 effect core/Memory targets with16
handles strongly held, controls4→0 and graph0.
Reset-only roots deliberately retain deferred owner cleanup; reset does not
replace owner disposal. Dedicated regression verifies cleanup stays pending
through reset and runs once on late disposal. Installed local/peer diagnostic
shows root contexts retained until late disposal, then all targets collected.
Reset-only effect payloads, current-byte browser qualification and performance
remain open. [Qualification](benchmarks/disposed-effect-retention-qualification-2026-11-02.json).


## 2026-11-02 — reset-only effect body and cleanup payload separation

A same-hash local/peer GC probe found obsolete body payloads retained along with
pending cleanup after reset. Shared retirement now drops entry.userFn without
executing previously registered cleanup.16 body targets collect at reset while
16 cleanup payloads and32 core/Memory controls stay owned; late disposal invokes
16 cleanups exactly once and all remaining targets collect, with16 public
handles/methods strongly held. Auxiliary controls4→0 and graph0.
An in-effect reset regression verifies earlier cleanup stays deferred while late
registration/returned cleanup execute immediately; late disposal preserves fresh
effects. Focused92/92, types, build and fresh installed-package gate passed.
Full suite:1505 passed,1 synthetic benchmark timeout at5000ms under host load
approaching99. No code/assertion changes: targeted benchmark retry with30s limit
passed in2.95s. A single all-green full run of these bytes remains unrecorded.
Browser requalification and performance remain open.
[Qualification](benchmarks/reset-effect-payload-qualification-2026-11-02.json).


## 2026-11-02 — measured lifecycle cost and reset allocations

Added scripts/bench-reactive-lifecycle.mjs: balanced serial isolated Node workers,
baseline A/A control and A/B, correctness checks for100k writes/observed writes,
10k effect mount/dispose operations and reset of512 signal/computed pairs.
Three experiments:54 workers,2592 timed samples.20 JS/WASM hashes per runtime
were checked;3 runtime archives preserve baseline, retirement and allocation
candidates. Reproduce with `node scripts/bench-reactive-lifecycle.mjs BASELINE_ROOT CANDIDATE_ROOT OUTPUT_JSON`.
Baseline→retirement reset ratio median5.44, approximately0.08–0.10ms versus
0.35–0.55ms in the first experiment. Write/effect overhead remains inconclusive.
Reset snapshot now avoids large intermediate filter/spread arrays. Disposed
computed retirement no longer allocates a terminal compute closure/replacement
deps array. Candidate reset ratios0.86/0.83 in two experiments do not establish
a speedup: A/A scatter0.57–1.92 and host load reaching128 prevent qualification.
Types, build, focused92 and fresh installed-package/GC gate passed. Full suite
passed1491 tests/112 files but dom-lifecycle worker failed to start (pool timeout);
its isolated retry passed15/15. A single all-green full run remains unrecorded.
Quiet-host A/A–A/B, reset scaling/owner subtree cost, applications and current-byte
browser qualification remain open.
[Evidence](benchmarks/reactive-lifecycle-cost-qualification-2026-11-02.json).


## 2026-11-02 — shared-owner reset scales without per-computed array splice

Reset of8192 computeds in one owner took roughly10–12ms with interleaved/tail
user cleanup registrations. Each computed individually scanned/spliced the
shared callback array. Retirement callbacks now optionally receive a per-reset
owner batch, then reset compacts each callback array once. Array identity and
user callback ordering are preserved; user cleanup remains deferred to owner
disposal. Old-copy reset without the optional batch argument retains individual
disposal fallback. A regression checks256 interleaved computeds,32 preserved
callbacks, array identity, reverse teardown order and fresh-slot safety.
Paired diagnostic:18 isolated Node workers,6 balanced rounds,972 timed samples,
512/2048/8192 computeds and none/interleaved/tail callback patterns, host load
10.9–11.5. At8192 interleaved, median12.09→1.38ms (paired ratio0.114); tail
9.79→1.21ms (ratio0.122); all6 pairs improve with A/A near1.512/no-user-callback
has roughly11% overhead, so this is not an all-workload speedup or application
latency claim. Baseline/candidate20-artifact hashes and runtime archives saved.
Types, build, focused211, full1507/113 files (30s test timeout) and fresh
installed-package/GC gates passed. Current-byte browser qualification and
application/native performance remain open.
[Qualification](benchmarks/root-reset-scaling-qualification-2026-11-02.json).


## 2026-11-02 — current reset/owner runtime: browser GC and real HMR

The installed runtime after shared-owner reset batching passes3 fresh Chromium156
natural-GC cohorts. Each has164 targets,72 strongly retained terminal handles,
46 positive controls and132 late calls. Totals:492 targets collected,216 retained
handles,138 controls held then collected,396 late calls,36 cleanup invocations
exactly once, forcedGC0 and final graph0. Coverage adds reset-only raw getter/peek
handles plus pending effect-handle/method/root-disposer payload cohorts. Obsolete
effect bodies collect at reset; pending cleanup and native context stay live
until late disposal, then collect while public handles remain retained.
20 JS/WASM runtime hashes match workspace. This is bounded pressure qualification,
not heap/RSS plateau or browser-wide leak freedom.
The same bytes pass real Vite HMR37/37 (35 automated +2 trusted input/cleanup),
graph0,60 workspace/installed/peer artifact comparisons. Peer-reset browser GC,
other engines/realms, application/native latency and historical watcher update5
loss remain open.
[GC evidence](benchmarks/reset-browser-gc-qualification-2026-11-02.json),
[HMR evidence](benchmarks/owner-reset-browser-hmr-qualification-2026-11-02.json).


## 2026-11-02 — native steady idle avoids repeated lexical binding replay

Runtime now permits an idle shortcut only after a successful full binding sync,
with unchanged document model/widget values, no pending activation/last event
and no pending render. New/rebuilt runtimes and failed attempts still require
a full pass, preserving missing-target initialization and render-error recovery.
Release diagnostic lexical forms8/32/96/256 in both native scheduler modes show
steady idle allocations570/6006/48666/334501→0. This concerns the warmed first
sample; the following idle after each edit/rebuild still takes the full path.
A regression verifies8 allocation-free idle calls, a subsequent edited model
commit and warmed idle after rebuild. Rust166 lib +19 integration tests pass,
as do release example build and wasm32 compile check. No paired latency/p95,
full-frame/window/IME/DPI/GPU or newly shipped browser WASM claim. CPU model
equality and widget scans remain linear; edit-time rebuild/Let replay is open.
[Qualification](benchmarks/native-lexical-idle-qualification-2026-11-02.json).


## 2026-11-02 — native direct root alias/key resolution borrows model data

Native binding replay now recognizes root direct model aliases and scalar member
keys without cloning/evaluating their collection per control. The same plain-key
guard is shared with existing key indexes. Cached indexes are used only after
pure scalar reads; computed key values, exact dotted fields, enum precedence,
complex expressions or parent frames fall back to general replay. A regression
compares alias-chain resolution with general replay under reorder, duplicate
key shadow and computed item ID fallback.
Release instrumented diagnostics in both scheduler modes: cold idle allocation
counts for8/32/96/256 lexical editors570/6006/48666/334501→234/822/2394/6309.
At256 editors (Svet=true), edit allocations2094359→1766167, requested bytes
284706741→239631029; all7 edit samples in8 workloads improve allocation counts
and requested bytes. Timings are unpaired/instrumented, not latency qualification.
Steady idle stays0 allocations. Cold96 budget<10000 is enforced in both modes.
Rust167 lib +19 integration tests, release build and wasm32 compile check pass.
Complex/nested replay, SceneArtifact snapshots, Runtime replacement, actual
browser WASM/window/IME/DPI/GPU and application p95/RSS remain open.
[Qualification](benchmarks/native-alias-binding-qualification-2026-11-02.json).


## 2026-11-02 — native alias semantic boundaries

Added a7-sequence differential matrix against general replay: direct/chained
aliases, state-field shadowing, later shadow preserving an earlier alias,
repeated declarations, an unrelated literal Let and readonly conditional
collections. Origins are checked before/after reorder; equal collection values
do not acquire writable model origin. Missing state in an unused Let still
errors without modifying model state. All169 lib +19 integration tests pass.
Removing exactly the2 new test blocks reproduces the previously qualified
source hash, confirming production implementation is unchanged in this step.
Nested/property-based expression coverage and rebuilt WASM/browser/native-window
qualification remain open.
[Qualification](benchmarks/native-alias-boundaries-qualification-2026-11-02.json).


## 2026-11-02 — rebuilt Rust crates/forma WASM execution

Current crates/forma release WASM (idle guard and borrowed alias resolver) rebuilt
with wasm-bindgen0.2.125 NodeJS/web bindings. Node passes7 artifact +7 binding
snapshot/reload cases. Chromium156 passes10/10 CPU Canvas2D checks. Single and
chained alias cases perform24 idle sync calls, Unicode input, reload with reversed
item IDs[b,a], then edit the correct new first item while preserving the other.
Both reordered input frames change1242 pixel bytes inside the control bounds.
The public API exposes reload, so these are reload-reorder checks; native Rust
stale-frame reorder remains separate. Browser input is programmatic WASM calls,
not trusted DOM keyboard/native-window/IME routing. No hardware GPU, browser
allocation/p95/RSS plateau or release/publication claim. Generated HTML/JS/WASM
hashes and replay bundle saved; production assets were not replaced.
[Qualification](benchmarks/native-current-wasm-qualification-2026-11-02.json).


## 2026-11-02 — native focus loss retires IME/modifier/touch session

WindowEvent::Focused routing now uses a shared focus_changed handler. A regression
using a real editable fixture failed before the fix: Runtime cleared preedit but
NativeHost retained composing=true without a separate Ime Disabled/Commit event.
Focus loss now clears composing/shift/command/alt/touch_last before app/chooser
dispatch. Existing Runtime focus/pointer cancellation remains. Uncommitted preedit
does not change the model, and focused editing can resume.
Native-feature suite passes177 unit +19 integration tests;2 hardware GPU tests
remain ignored. These tests instantiate the host without an OS window. Actual
IME/keyboard/window switches, DPI and hardware qualification remain open.
[Qualification](benchmarks/native-focus-session-qualification-2026-11-02.json).


## 2026-11-02 — explicit native Metal hardware gates

Ran both ignored hardware tests serially with --features native. Tests now print
structured adapter identity and reject DeviceType::Cpu instead of allowing a
software fallback to qualify hardware. Runtime reports Apple M4 Max / Metal /
IntegratedGpu; source audit confirms MTLCreateSystemDefaultDevice and Metal
render encoder/command-buffer commit paths. Reveal matrix asserts126 comparisons
(3 target shapes ×2 control counts ×3 scales ×7 states): max channel error1/255
versus budget2/255. State updates preserve geometry upload count. Renderer reuse
smoke passes7 owned buffers/10496 buffer bytes, geometry/tile upload1 each and
one bind-group creation; unchanged frame avoids repeated payload upload.
Two tests passed with none ignored. This is offscreen debug/test hardware
execution, not window presentation, device-loss, all-renderer/application
latency or global VRAM/RSS qualification.
[Evidence](benchmarks/native-metal-hardware-qualification-2026-11-02.json).


## 2026-11-02 — Tab/Escape retire the old native composition session

A native-handler regression reproduced Tab moving focus while composing stayed
true, suppressing later character routing. The keyboard navigation branch now
has a dedicated handler. Pressed nonrepeat Tab/Escape clear host composing and
Runtime preedit before the existing navigation/modal dismissal. Key release and
repeat preserve active composition. Regression verifies uncommitted model values,
Escape restoring the nonpreedit CPU frame, Tab selecting the next field, next
edit committing to that field and the other field remaining unchanged.
Native-feature suite passes178 unit +19 integration tests;2 GPU tests are
ignored in this run (separate Metal hardware qualification already recorded).
Actual OS IME/key events, pointer/backend switches, reverse navigation and input
latency remain open.
[Qualification](benchmarks/native-ime-navigation-qualification-2026-11-02.json).


## 2026-11-02 — pointer focus changes retire the previous native IME owner

Pointer press into another editor reproduced composing=true after Runtime had
changed focus. Mouse/touch/scroll input now uses a shared wrapper: while composing,
compare editable focus before/after the callback and clear preedit/host session
if the owner changes or disappears. Without composition, dispatch stays direct;
combined touch updates retain one NativeApp callback. Hover preserves session.
Regression covers editor hover/press/release, next-target commit with unchanged
other field, then blank hover/press with no model change.179 native-feature unit
+19 integration tests pass;2 GPU tests are skipped in this run.
Actual OS IME/pointer, same-editor caret/drag, model replacement after callback,
backend/window switching and input latency remain open.
[Qualification](benchmarks/native-pointer-ime-qualification-2026-11-02.json).


## 2026-11-03 — actual NativeHost window input and editor restoration

Built a reusable NativeHost probe as a temporary macOS app bundle. UI automation
confirmed OS-routed ASCII input, Tab/Shift+Tab, pointer target changes, Escape
and visible model summary; fixture journal captured field values/focus indices.
Minimize/reopen exposed focus2→-1→1: the second editor was lost on activation.
NativeHost now remembers the blurred editable stable key and resolves it again
on focus regain, preserving it across duplicate blur. Missing/ambiguous or
non-admitted targets fall back to ordinary Runtime focus admission. A regression
fails before the fix and verifies normal/reordered target restoration after it.
Actual OS rerun shows2→-1→2; subsequent r edits second to seed-Bkr while first
stays seed-A.180 native-feature unit +19 integration tests pass;2 GPU tests
were omitted here (separate hardware gate exists). Probe/Finder windows closed
and owned process termination verified.
No CJK marked-text/clipboard/a11y/DPI-change or latency claim; the standalone
forma binary has an independent host and remains a separate parity boundary.
[Qualification](benchmarks/native-os-input-qualification-2026-11-03.json).

### Native refocus boundary matrix — 2026-11-03

Добавлена регрессия `window_refocus_rejects_retired_or_ineligible_editor_keys`:
удаление, runtime-disabled и замена TextInput на Button вызывают ordinary focus
fallback; последующий text_insert поступает в первое поле, remembered key очищен.
Duplicate key проверен как compiler refusal (`Повторный key b`), а не допустимая
сцена. Полная проверка: 181 unit + 19 integration passed, 2 GPU tests ignored.
[Лог](benchmarks/native-refocus-boundaries-2026-11-03.txt).

Во время подготовки fixtures выявлен открытый native error path:
`load_ui_files_json` конвертирует ошибку в JsValue::from_str и aborts на non-WASM.
Ошибочный fixture использовал unsupported disabled property TextInput; финальная
матрица отключает control через set_binding_disabled и использует native
from_ui_files с String errors. Исправление native reload error API ещё требуется.

### Atomic native project reload — 2026-11-03

Добавлен load_ui_files_json_native -> Result<(), String>; JS-facing wrapper
сохраняет API и делегирует общей реализации. Документ и Runtime собираются до
замены self; successful reload сохраняет выбор native Svet scheduler и повышает
revision/geometry/layout keys. Native refocus regression перешла на native API.

Регрессия проверяет malformed files/state JSON, missing entry/component,
неизменность model/text/focus/versions при отказе, последующий рабочий ввод и
успешную замену для обоих scheduler modes. 201 Rust tests passed, 2 GPU ignored;
wasm32 cargo check passed. Browser runtime и actual OS reload не проверялись.
[Evidence](benchmarks/native-project-reload-qualification-2026-11-03.json).

### Fresh WASM project reload qualification — 2026-11-03

Release Rust WASM пересобран; wasm-bindgen Node/web bindings созданы заново.
Node snapshot suite8/8 и Chromium browser suite11/11 прошли. Новый browser case
проверяет четыре error paths с неизменными state/text/focus/cache versions и
побайтово одинаковыми CPU pixels, последующий ввод и successful replacement.
Artifact hashes, raw reports и generated bundle сохранены. Это программный
WASM input и Chromium CPU rendering; OS IME/другие engines не квалифицированы.
[Evidence](benchmarks/native-reload-wasm-qualification-2026-11-03.json).

### Native composition ownership across Runtime replacement — 2026-11-03

Index-only ownership пропускал замену focused editor на новый Runtime с тем же
key/index. Native Runtime теперь получает u64 instance id без heap allocation;
focus_sensitive_input сравнивает instance + focused index. Новый regression
падает на прежнем comparator и проходит после fix. Полный suite: 183 unit +19
integrations passed,2 GPU ignored; allocation budgets проходят. Это замена внутри
callback; post-callback NativeApp mutation и actual OS IME ещё открыты.
[Evidence](benchmarks/native-editor-session-qualification-2026-11-03.json).

### Post-callback native IME ownership — 2026-11-03

Host сохраняет owner (native Runtime instance,index) из preedit/input и сверяет
его в reconcile_composition перед OS IME update. Wrapper regression заменяет
Runtime после input callback на scene с тем же editor index/key; старая
composition завершается, новый model не содержит preedit. 184 unit +19
integration passed,2 GPU ignored.

Открыто: actual CJK/late OS commit, повторная замена в reconciliation update,
стоимость дополнительного NativeApp.update во время composition.
[Лог](benchmarks/native-ime-post-callback-2026-11-03.txt).

### Single-turn IME presentation — 2026-11-03

Дополнительный reconciliation update убран: ime_presentation получает editing/
caret и сверяет composition owner в одном NativeApp.update. Regression считает
updates, проверяет unchanged owner, очистку stale owner и отмену composition
даже когда app после callback возвращает sync error. 203 Rust tests passed,
2 GPU ignored; allocation budgets проходят. Возможная замена Runtime после
presentation callback остаётся открытой: snapshot caret тогда может устареть.
[Evidence](benchmarks/native-ime-presentation-qualification-2026-11-03.json).

### Read-only NativeApp inspection — 2026-11-03

NativeApp.inspect — обязательный read-only observer retained Runtime. Runtime,
probe и test wrapper реализуют его; миграция документирована в crates/forma README.
IME presentation больше не вызывает update, получает caret после завершённой
замены Runtime и не очищает preedit нового редактора. Regression проверяет
call count0, exact caret, unchanged pixels, inspect error и owner retirement.
203 Rust tests passed,2 GPU ignored; native examples check passed. Draw update
lifecycle и late OS commits остаются открытыми.
[Evidence](benchmarks/native-inspect-qualification-2026-11-03.json).

### Retained native frame observation — 2026-11-03

paint_app использует NativeApp.inspect вместо update: frame и animation status
читаются из Runtime, оставшегося после tick/app synchronization. Удалён второй
app update на paint. Wrapper regression меняет Runtime после tick callback,
сравнивает exact CPU pixels и проверяет call count, invalid buffer и inspect
refusal. 204 Rust tests passed,2 GPU ignored. Actual window/frame latency после
изменения не квалифицированы.
[Evidence](benchmarks/native-frame-inspection-qualification-2026-11-03.json).

### Actual native window after inspect migration — 2026-11-03

Свежий probe подтвердил ASCII в обоих полях через Tab и видимые frame/summary.
Pointer/navigation повтор partial: после клика во второе поле journal показывает
index2→1 и q поступает в first. Есть промежуточные blur/refocus от UI workflow;
root cause без event trace не установлен. Shift+Tab/z тоже не засчитаны.
Screenshot и полный journal сохранены; закрытый app был повторно запущен AX
наблюдением, собственный перезапущенный process затем terminated.
[Evidence](benchmarks/native-inspect-window-qualification-2026-11-03.json).

### Activation click versus remembered editor — 2026-11-03

Rust regression воспроизвела blur→pointer select second→Focused(true): host
восстанавливал remembered first и терял новый выбор. focus_changed(true) теперь
сохраняет уже выбранный control; stable-key restore нужен только без focus.
Регрессия before падает index1/2, after проходит вместе с duplicate activation
и проверкой binding target. 205 Rust tests passed,2 GPU ignored. Actual OS
повтор исправленного binary ещё требуется; event ordering не объявлен
подтверждённой причиной всех наблюдений UI automation.
[Evidence](benchmarks/native-activation-focus-qualification-2026-11-03.json).

### Actual activation focus fix replay — 2026-11-03

Пересобранный native probe прошёл actual OS repeat: first x→ч, click second
q→й, Shift+Tab z→я в first; повторный click second w/Escape/v→цм в second.
Текущая русская раскладка оставлена без изменений. Visible frame/summary и
journal совпадают: first=seed-Aчя,second=seed-Bйцм. Собственный probe закрыт,
process0. Это pointer/keyboard/refocus qualification CPU окна, не CJK IME
и не latency qualification.
[Evidence](benchmarks/native-activation-window-qualification-2026-11-03.json).

### Builtin disabled TextInput — 2026-11-03

Builtin TextInput не объявлял disabled: .ui compiler отказывал с Unknown
property TextInput.disabled. Добавлен default false. Native regression покрывает
state-bound disabled, отказ focus/text, fallback, сохранение locked value,
a11y отсутствие actions, затем unlock/reload и успешный edit/set_value action.
Baseline fails, candidate206 Rust tests passed,2 GPU ignored. Browser/live-model
transition и OS accessibility не квалифицированы.
[Evidence](benchmarks/native-disabled-input-qualification-2026-11-03.json).

### Live disabled binding across native and WASM — 2026-11-03

Toggle checked binding меняет locked state без reload. Оба native schedulers
проверены: отключение активного editor, сохранение committed text без preedit,
запрет focus/input и успешный edit после unlock. Fresh release WASM: Node9/9,
Chromium12/12; accessibility tree actions пусты для disabled editor.
207 Rust tests passed,2 GPU ignored. Generated bundle/hashes и browser report
сохранены. OS accessibility/trusted DOM/CJK отдельно не проверялись.
[Evidence](benchmarks/live-disabled-qualification-2026-11-03.json).

### Disabled editor session retirement — 2026-11-03

Новая regression подтверждает существующий direct runtime путь: disable во
время pointer press/preedit очищает capture/focus/down/preedit; insert refused,
idempotent disable safe, release после enable не активирует старый press.
Refocus fallback test теперь использует настоящий .ui disabled:true вместо
setter workaround. 208 Rust tests passed,2 GPU ignored. Actual OS IME/touch
матрица этим не подтверждена.
[Evidence](benchmarks/native-disabled-session-qualification-2026-11-03.json).

### IME commit dispatch owner guard — 2026-11-03

commit_ime проверяет сохранённого owner внутри app update непосредственно перед
insert: другое focused поле или Runtime replacement не получают старый текст.
Регрессия покрывает оба случая, valid owner commit и direct/no-preedit commit.
209 Rust tests passed,2 GPU ignored. Ограничение: cancellation и empty-preedit
ещё могут очистить owner до commit; полная late-commit state machine открыта.
Actual CJK OS path не проверен.
[Evidence](benchmarks/native-ime-commit-owner-qualification-2026-11-03.json).

### Empty preedit pending commit ownership — 2026-11-03

preedit_ime разделяет удаление marked pixels и завершение pending commit:
пустой preedit сохраняет owner, presentation тоже не стирает его. Commit
принимается исходным editor и отклоняется после Runtime replacement даже
если между ними было presentation inspection. Exact CPU pixels после empty
preedit совпадают с clean frame. 210 Rust tests passed,2 GPU ignored.
Cancelled-session late commit и actual CJK OS ordering ещё открыты.
[Evidence](benchmarks/native-ime-empty-preedit-qualification-2026-11-03.json).

### Explicit native IME cancellation barrier — 2026-11-03

ime_cancelled хранит отмену отдельно от видимого composing и owner. Escape,Tab,
blur и pointer retirement блокируют late commits; empty preedit не снимает
барьер. Новый nonempty preedit/Enabled открывает новую сессию, Disabled закрывает.
ime_event объединяет production routing и regression event sequences.
211 Rust tests passed,2 GPU ignored. Контракт сверён с установленным winit0.30.13
event.rs: перед Commit идёт empty Preedit, после Disabled события ждут Enabled.

Ограничение: после cancellation no-preedit commit без нового Enabled отклонён;
actual CJK/emoji workflows ещё нужны. OS session id отсутствует, поэтому старый
commit после начала новой сессии неотличим от нового.
[Evidence](benchmarks/native-ime-cancelled-session-qualification-2026-11-03.json).

### IME diagnostics and incomplete emoji attempt — 2026-11-03

Actual macOS ctrl+super+space не открыл наблюдаемый picker; emoji insertion
и cancellation recovery не квалифицированы. Собственный probe закрыт,process0.
Добавлен optional NativeApp.on_ime_event, probe пишет отдельный fixture-only
*.ime.jsonl при заданном FORMA_NATIVE_INPUT_PROBE_LOG. Normal apps не логируют
текст по умолчанию. 211 Rust tests passed,2 GPU ignored; example check passed.
Actual OS trace на instrumented binary ещё требуется.
[Evidence](benchmarks/native-ime-diagnostics-2026-11-03.json).

### Layout-only snapshot elimination — 2026-11-03

scene_expand больше не вкладывает owned __state в layout-only узлы: их props
уже вычислены, children получают scope отдельно, а control/template snapshots
сохранены. Regression24 nested Column/256 unrelated records/1 control:
40950→3024 allocations (-92.6%), public control snapshot сохраняет256 records.
Budget10000. 212 Rust tests passed,2 GPU ignored. Это debug allocation evidence;
общие edit latency/RSS и fresh WASM/browser после изменения ещё открыты.
[Evidence](benchmarks/native-layout-snapshot-qualification-2026-11-03.json).

### Fresh WASM nested-layout qualification — 2026-11-03

После layout snapshot optimisation rebuilt release WASM прошёл Node10/10 и
Chromium13/13. Новый case24 nested Column/256 records сохраняет control state
и unrelated payload, live edit→seed-nested; browser CPU frame просмотрен.
Generated bundle, hashes,raw reports сохранены. Это функциональная проверка,
не browser allocations/latency/RSS benchmark.
[Evidence](benchmarks/layout-snapshot-wasm-qualification-2026-11-03.json).

### Paired release nested-form edit cost — 2026-11-03

Binding benchmark получил --nested-layout (12 уровней). Baseline отменяет
только layout snapshot optimisation; candidate использует текущий fix.
Baseline A/A +4 AB/BA pairs,8/32/96/256 controls,оба schedulers,7 edits/run.
Svet256 edit allocations1799435→1764767 (-1.93%): per-control snapshots всё
ещё dominant. Timing inconclusive: A/A до2.056×, allocator-instrumented timings
не дают application latency guarantee. RSS/live/peak не измерялись.
Raw10runs,build logs,baseline source gzip/hashes и reusable pair runner сохранены.
[Evidence](benchmarks/layout-edit-paired-qualification-2026-11-03.json).

### Borrow resolved props during contract validation — 2026-11-03

Два contract validation пути клонировали resolved Properties даже без required
keys. Cow теперь заимствует map; to_mut вызывается только при фактическом
удалении missing required key. Существующие required/enum/constraints tests
проходят. Custom component с List256:5059→4284 allocations (-15.3%); budget4700
падает на baseline и проходит candidate. 213 Rust tests passed,2 GPU ignored.
Полные per-control snapshots сохранены; их устранение требует изменения
внутреннего представления при сохранении public transport. Fresh WASM/browser
и latency/RSS после этой оптимизации ещё открыты.
[Evidence](benchmarks/native-contract-props-qualification-2026-11-03.json).

### Fresh WASM borrowed-contract validation — 2026-11-03

Release WASM rebuilt после contract-props Cow optimisation: Node11/11,
Chromium14/14. Новый custom List256 case загружается, wrong type сохраняет
отказ expects List без изменения текущего state/layout revision.
Generated bundle/hashes/raw reports сохранены. Browser allocations/latency
не измерены; required/enums остаются покрыты native suite.
[Evidence](benchmarks/contract-props-wasm-qualification-2026-11-03.json).

### Move owned control state into public snapshots — 2026-11-03

Snapshot insertion перенесён после child expansion, state.into_owned()
перемещает Cow-owned local scope вместо дополнительного clone. Borrowed
root state по-прежнему клонируется, public transport не меняется.
Public Document.states regression с local payload256:1878→1105 allocations
(-41.2%), budget1400 fails baseline/passes candidate; inherited query и весь
local payload сохранены. 214 Rust tests passed,2 GPU ignored. Fresh WASM/browser
и shared-root representation optimisation ещё открыты.
[Evidence](benchmarks/native-owned-state-qualification-2026-11-03.json).

### Owned-state transfer portability and repeated render — 2026-11-03

Fresh release WASM Node11 binding +7 artifact checks passed. Rust local-state
regression теперь проверяет unchanged Document.states и byte-identical repeated
SceneArtifact render,1105 allocations. Важно: public .ui loader создаёт только
root state, поэтому WASM API gate не исполняет synthetic node-local allocation
case и не доказывает его performance. Browser повтор ещё открыт.
[Evidence](benchmarks/owned-state-portability-qualification-2026-11-03.json).

### Native source and compiler error APIs — 2026-11-03

Добавлены String-error load_component_native/load_native и
compile_ui_scene_artifact_native. JS wrappers делегируют, сохраняют ABI.
Native regression проверяет invalid source/template/project, exact pixels,
focus/control count/versions после отказа, успешную recovery и portable artifact
с editor values. 229 Rust tests passed (включая14 display-list),2 GPU ignored;
wasm32 cargo check passed. Native API usage документирован; fresh generated
WASM execution после изменения ещё открыт.
[Evidence](benchmarks/native-source-api-qualification-2026-11-03.json).

### Fresh WASM source adapter and browser compatibility — 2026-11-03

Fresh release WASM после source API extraction/owned-state transfer прошёл
Node12 binding +7 artifact checks и Chromium15/15. Invalid source/template/
project throwing adapters сохраняют count/layout/geometry, valid UI reload
после отказа восстанавливает работу. Generated bundle/hashes/raw reports
сохранены. Node-local allocation case остаётся Rust-only evidence, browser
perf/OS routes не квалифицированы.
[Evidence](benchmarks/source-api-wasm-qualification-2026-11-03.json).

### Watcher/HMR disk-cache-websocket diagnostics — 2026-11-03

Server probe сохраняет финальные disk/expected hashes, stat, canonical +URL
module cache code hash/version markers/invalidation timestamps. Browser fixture
пишет bounded websocket before/after/error lifecycle trace. Lookup учитывает
macOS /var→/private/var: первоначальный file-only lookup дал пустой module set,
canonical replay подтвердил cached v9. Два unpaced replay37/37,graph0; disk/cache/
DOM=v9,59 workspace/installed compiled files equal. Historical update5 failure
не воспроизведён и не объяснён. Instrumentation diagnostic, не perf baseline;
installed build переиспользован после byte comparison, fresh package gate не запускался.
[Evidence](benchmarks/watcher-diagnostics-qualification-2026-11-03.json).

### Character Viewer accessibility boundary — 2026-11-03

На актуальном instrumented native binary обычный русский input x/Tab/q
даёт first=seed-Aч,second=seed-Bй; IME event log отсутствует (events0).
CharacterPalette process существует, но CUA getApp по path и bundle id timeout,
окно не доступно inventory. Emoji/CJK не квалифицированы. Собственный probe
закрыт,process0; системные настройки/существующий system process не менялись.
Повтор того же пути без новой UI surface не даёт дальнейшего доказательства.
[Evidence](benchmarks/native-character-viewer-qualification-2026-11-03.json).

### Native primary touch ownership — 2026-11-03

Touch routing раньше не учитывал device/id, secondary finger мог менять
координаты и завершать чужой press. NativeHost теперь хранит primary owner,
пропускает только его события; end/cancel/blur освобождают owner, orphan
move/end игнорируются. Regression проверяет все secondary phases, exact
pointer/focus, single correct click, new touch и late events после blur/cancel.
230 Rust tests passed (14 display-list included),2 GPU ignored. Physical
touchscreen и chooser-specific matrix не квалифицированы; pinch/zoom нет.
[Evidence](benchmarks/native-touch-owner-qualification-2026-11-03.json).

### Accumulate subthreshold native touch movement — 2026-11-03

Проверка pan/release подтвердила existing Runtime.scroll cancellation, но
slow-motion extension выявила потерю80×0.25px: touch_last обновлялся до порога,
поэтому offset оставался0. Теперь consumed coordinate двигается только при
накоплении >0.5px по оси. Regression baseline fails; candidate231 Rust tests
passed,2 GPU ignored. Distance residual≤0.5 проверен отдельно; edge-pan не
активирует control, small jitter остаётся tap, новый tap работает. Physical
touchscreen/inertia/pinch не квалифицированы.
[Evidence](benchmarks/native-touch-slow-qualification-2026-11-03.json).

### Modal root-scroll input isolation — 2026-11-03

Runtime.scroll раньше менял base offset при активном Modal (baseline160→140)
и отменял press внутри диалога. Теперь active modal отказывает root scroll
до mutation/cancel; modal press остаётся рабочим, после close scroll resumes.
231 Rust tests passed,2 GPU ignored; fresh release WASM Node13/13 и
Chromium16/16. Actual OS wheel/touch и modal-local nested scrolling не
квалифицированы. Bundle/hashes/raw reports сохранены.
[Evidence](benchmarks/modal-scroll-qualification-2026-11-03.json).

### Disabled keyboard and modal focus regression — 2026-11-03

Текущий runtime прошёл новую targeted regression без production правки:
disable во время Space press очищает gesture; поздний release не активирует
original/другой control после reenable; fresh press/release работает. Tab в
обе стороны пропускает disabled, все disabled дают no focus; empty Modal не
пропускает focus на base, reenabled modal control traps traversal.
Focused1/1 passed; full suite не повторялся для test-only addition.
[Evidence](benchmarks/native-disabled-navigation-qualification-2026-11-03.json).

### Shared clipboard commands in NativeHost — 2026-11-03

Reusable NativeHost не обрабатывал command C/X/V. Platform clipboard adapters
вынесены из standalone main в native_clipboard и используются обоими hosts.
Общий handler выполняет copy/cut/paste через Runtime: cut только после write
success, ошибки сохраняют текст/selection, empty copy не меняет clipboard.
Fake clipboard regressions: Unicode,CRLF normalization,undo/redo,target model,
disabled/unfocused refusal.235 Rust tests passed,2 GPU ignored; native all-targets
check passed. System clipboard не использовался; OS integration/IME/latency open.
[Evidence](benchmarks/native-clipboard-qualification-2026-11-03.json).

### Non-Latin native command shortcuts and OS paste — 2026-11-03

Actual Russian layout Cmd+A/paste сначала не менял model. Native command_key
сохраняет ASCII logical keys (Latin layout positions не remap), non-ASCII
shortcuts получают physical A/C/X/V/Z/Y fallback; оба hosts подключены.
236 Rust tests passed,2 GPU ignored;all-targets check passed. Actual OS
повтор: Unicode paste first=你好 мир🙂,Undo→seed-A,Redo→Unicode,Tab/paste
second=second-paste. CUA App.paste восстанавливает clipboard по tool contract.
Own probe closed,process0. Выявлен отдельный renderer defect: CJK/emoji есть
в model, но невидимы на frame; glyph fallback ещё открыт. OS copy/cut/CJK IME
не квалифицированы.
[Evidence](benchmarks/native-clipboard-layout-qualification-2026-11-03.json).

### Visible missing-scalar fallback in simple renderer — 2026-11-03

measure/hit/glyph_ids fallback использовал table.glyphs[0] (NUL,zero advance,
no outline). Теперь отсутствующий scalar получает существующий ? glyph в
measure/hit/vector/CPU. Regression baseline fails zero width; candidate237
Rust tests passed,2 GPU ignored: UTF8 caret,width,nonempty CPU/vector coverage
и exact question fallback pixels. Actual window shows ?? мир?, model retains
你好 мир🙂; own probe closed,process0. Full CJK/emoji font coverage и fresh
WASM/browser остаются открытыми; placeholders не считаются glyph support.
[Evidence](benchmarks/unicode-fallback-qualification-2026-11-03.json).

### Portable CJK/emoji font source qualification — 2026-11-03

Добавлены unmodified Google Fonts NotoSansSC17,772,300bytes и NotoEmoji1,982,596
bytes, upstream OFL/METADATA, immutable revision/URL/SHA256 provenance. Новый
font_coverage_probe через ttf-parser weight400 проверил11 scalar samples:
你好中文日本語 и 🙂🦀❤☀, positive advances/nonempty outlines. Production renderer
ещё не использует fonts; multi-face units/baseline/CPU/vector/shaping integration,
full sequences и WASM/package size/startup остаются открытыми. Asset-only
probe не считается CJK rendering qualification.
[Evidence](benchmarks/font-assets-qualification-2026-11-03.json).

### Portable real-font simple renderer — 2026-11-03

Default simple text path выбирает primary Ubuntu,NotoSansSC,NotoEmoji. Cached
primary coverage сохраняет fast path; FontGlyph хранит face/id, advances
нормализованы к primary UPEM, outlines масштабируются per-face относительно
общего baseline. Measure/hit/CPU/vector используют общий resolver.
238 Rust tests passed,2 GPU ignored;wasm32 check passed. Real face/glyph,
nonempty/distinct CPU glyph, mixed-font UTF8 caret и streamed/collected vector
parity проверены. Actual native frame показывает 你好 мир🙂, model exact,own
probe closed/process0. Shaped API пока primary-only; full sequences/colour,
fresh release WASM/browser и size/startup/resolver performance остаются открытыми.
[Evidence](benchmarks/real-fonts-native-qualification-2026-11-03.json).

### Real fallback faces in shaped API and WASM — 2026-11-03

ShapedGlyph получил font_index; font_face предоставляет matching immutable
face weight400. Actual CJK/emoji glyph id и normalized advance берутся из
общего resolver; kerning разрешён только внутри одного face и направления,
synthetic glyph0 сохранён для неподдерживаемых scalar.239 Rust tests passed,
2 GPU ignored. Fresh release crates/forma WASM Node14/14 и Chromium17/17 passed;
CPU CJK/emoji frames просмотрены, pixels/caret/simple-shape samples проверены.
Module21,828,894bytes,gzip13,246,694bytes. Bundle дедуплицирует Node/web WASM
через tar hardlink. Full shaping/ZWJ/colour/broader baselines и controlled
startup/RSS/resolver performance остаются открытыми.
[Evidence](benchmarks/shaped-real-fonts-qualification-2026-11-03.json).


### Static font reproducibility — 2026-11-03

Repeated generation with FontTools4.68.0 reproduced both original candidate
SHA256 values, preserving every cmap/UVS table and glyph order. The release Rust
`font_instance_compare` example now fails on topology changes, nonzero advance
error or coordinate error >0.5 font units; the exhaustive run passed for
31,036 CJK and1,891 emoji glyphs. CJK maximum error0.49981689453125, emoji0.
Reports: `benchmarks/font-instance-{compare,reproducibility}-2026-11-03.json`.
Production renderer still uses variable fonts. Static raster comparison,
startup/RSS and deployment gates remain open.


### Static font CPU raster comparison — 2026-11-03

Release test `static_font_candidates_raster_comparison` passed through production
Contours/rasterize: selected CJK/emoji glyphs at9/14/24/48/96px and four subpixel
offsets. Emoji exact-alpha equality is enforced. Raw measurements are in
`benchmarks/font-instance-raster-2026-11-03.json`; CJK differences are measured,
not yet accepted as a production threshold. No renderer switch or full-glyph
raster/OS qualification is claimed.


### Higher-precision static CJK experiment — 2026-11-03

Generator supports isolated --output-dir and --cjk-upem experiments.16000 UPEM
overflows signed TTF glyph bounds (xMax38603);8000 builds preserving all cmap
tables and glyph order,13,487,632bytes. Production CPU raster release test
passed; metrics and original hashes are retained in
`benchmarks/font-instance-upem8000-2026-11-03.json`. Renderer remains unchanged.
This sample gate does not prove all-glyph geometry/metrics or shaping parity.


### Exhaustive higher-UPEM gate — 2026-11-03

Comparator now normalizes geometry/advances to original UPEM and accepts a
separate candidate directory.8000-UPEM CJK retained all31036 glyphs/topology
with maximum coordinate error0.0625 original units, but horizontal advances
differ by up to0.5 original units. Gate correctly fails (exit101); production
replacement is rejected until original metrics are preserved. Report is saved
before assertions so failed gates remain inspectable. Emoji remains exact.


### Preserved metrics for high-UPEM CJK — 2026-11-03

Generation now obtains weight400 reference advances before UPEM scaling and
transfers exact integer-multiplied hmtx/vmtx advances into high-precision contours.
Exhaustive release comparator passes all31036 CJK and1891 emoji glyphs: both
horizontal/vertical advances exact, topology unchanged, normalized CJK coordinate
error0.0625, emoji0. Selected CPU raster gate also passes; residual CJK pixel
differences remain. Report: font-instance-upem8000-preserved-metrics-2026-11-03.json.
Production renderer unchanged; global metrics/kerning/mixed-font integration
and broader rendering gates remain open.


### Static font global metrics and flattening diagnostics — 2026-11-03

Exhaustive comparator now checks normalized ascender/descender/line gap/x-height/
cap height: all exact for preserved-advance8000 CJK and static emoji. Release
CPU raster gate reports original/candidate edge counts per glyph. Diagnostics
are preserved in font-instance-global-metrics-2026-11-03.json; counts alone
do not prove the cause of remaining pixel errors. Renderer unchanged.


### Flattening tolerance diagnostic — 2026-11-03

A test-only per-Contours tolerance override isolates subdivision effects without
changing production0.15px. At0.01px the same selected8000-UPEM CJK sample matrix
reduces maximum alpha error19→3/255; emoji remains exact. This supports a
subdivision-threshold contribution but does not eliminate font rounding or
prove all-glyph raster equivalence. More edges are emitted (worst sample
好96px:356/359 edges). Release library suite passed at default tolerance;
raw test log is font-diagnostics-lib-tests-2026-11-03.txt. Production policy
needs an accuracy/performance comparison before changing renderer behavior.


### Exploratory flattening cost — 2026-11-03

Manual ignored release probe font_flattening_cost_probe alternates tolerance
order, reuses buffers and excludes first warmup pair. Seven measured pairs
per size:0.01px median time ratio1.54/1.76/1.64 at14/48/96px relative to0.15;
0.05px1.11/1.21/1.15. Edges at0.01 grow2.23–3.21×. No A/A process/host noise
control or app-frame claim. Raw measurements in font-flattening-cost-2026-11-03.json;
intermediate-quality raster comparison in font-instance-medium-flattening-2026-11-03.json.
Evidence does not support globally tightening production tolerance merely to
compensate for static-font rounding. Renderer and production tolerance unchanged.


### Production static emoji fallback — 2026-11-03

Renderer now embeds FormaEmoji-Regular.ttf instead of variable NotoEmoji;
source assets/provenance/licenses preserved. CJK and production curve tolerance
unchanged.240 Rust tests passed,2 GPU tests and1 manual probe ignored; fresh
WASM Node14/14 passed. New compare-font-wasm.cjs checks90 exact CPU frames,
mixed caret and simple/shaped metrics against previous variable-font module.
No kern/GPOS table exists in either emoji file. Payload saved1,095,944bytes.
Actual sizes/hashes and remaining browser/OS scope are in
benchmarks/static-emoji-qualification-2026-11-03.json.


### Static emoji browser and native presentation — 2026-11-03

Fresh web bindings passed17/17 in Chromium156. Browser frame shows real CJK,
static emoji and mixed text. Rebuilt native probe received mixed Unicode paste
in second editor and displayed matching summary; journal/screenshots retained.
CUA action batch timed out after paste; re-observation confirmed the effect.
Probe window/tab/server closed. Scope is paste/font presentation, not CJK IME.
See static-emoji-qualification-2026-11-03.json and corresponding screenshots.


### Borrow scene property defaults — 2026-11-03

scene_properties reads definition-over-fallback defaults through borrowed
lookups; merged map materializes only for missing-state fallback expressions.
Regression covers definition precedence, fallback-only and live-state paths.
241 Rust tests passed,3 ignored. Same-command allocation before/after logs
and exact counts saved in native-defaults-qualification-2026-11-03.json.
Shared-root/control snapshots remain owned and dominant; no latency/RSS claim.
Fresh WASM/Node/browser integration still pending.


### Defaults borrowing WASM gate — 2026-11-03

Fresh release wasm-bindgen node/web:Node14/14 and Chromium17/17 passed.
90 CPU frame/caret comparisons against static-emoji baseline are byte-exact.
Reports/build log/screenshot saved under native-defaults-wasm-* and
native-defaults-browser-2026-11-03.jpg. Test tab/server closed. Performance
evidence remains allocation-only; shared-root snapshot storage is still open.


### Shared snapshot ownership audit — 2026-11-03

Traced expansion→props.__state→visual/template evaluation→SceneControl→retained
Runtime binding transaction. Live root already comes from Document.states;
captured locals/lexical frames/key scope remain required. Public owned transport
prevents silently deleting snapshots. SNAPSHOT_STORAGE.md records the internal
shared-scope migration boundary and compatibility gates. Added reproducible
release snapshot_storage_probe:8/32/96/256 controls,256 unrelated records,
public JSON bytes and retained record counts; root immutability asserted.
Raw baseline saved. No shared-storage implementation or RSS improvement claimed.


### Shared Properties implementation — 2026-11-03

Properties now owns Arc<BTreeMap> with COW mutation; serde emits the same plain
map. Sibling snapshots share unchanged root storage and detach on edits; nested
object/list mutation, mutable iteration, owned iteration and serde roundtrip
regressions pass.243 Rust tests,3 ignored,all-targets check; fresh WASM Node14/14,
30 exact public artifact JSON comparisons and90 exact frames/caret comparisons
against defaults baseline. Native96-editor repair allocations140045→41253;
other exact counts/raw logs in shared-properties-qualification-2026-11-03.json.
Rust alias→newtype migration documented. Browser, timing/RSS and mutable
overlay/list cloning remain open.


### Shared Properties browser + retained allocator qualification — 2026-11-03

Chromium17/17 passed on fresh web WASM; report/screenshot saved. Constructed
separate baseline source copy differing only in Properties alias vs COW map.
Same benchmark source, release binaries hashed; A/A+four AB/BA pairs saved.
256 controls retained allocator bytes135619610→5104326 (-96.2%); allocations
1736152→80582. Every artifact Drop returns measured allocator delta to zero.
This excludes Document,allocator overhead,RSS/GPU; instrumented timing cannot
stand for app latency. Small-control A/A noise reaches2.23×. Sources gzipped,
build logs/raw runs preserved, reusable run-snapshot-retention-pairs.py added.
Full Runtime RSS/edit/frame and overlay/list costs remain open.


### Full retained Runtime memory probe — 2026-11-03

Added --runtime-retention benchmark path: both schedulers,8/32/96/256 controls,
complete warmup, two measured samples, construction+3 binding edit/rebuild/CPU
frames at1.25scale, allocator live/peak and after-Drop delta0. Benchmark
diagnostics are allocated while live/peak counters are suspended. Same release
source compiled with isolated old-alias baseline and current COW Properties.
Process /usr/bin/time -l peak RSS367951872→28672000bytes; local getrusage(2)
man page confirms byte units. This is one process/host probe, not per-case RSS
or OS-window/GPU latency. Detailed live/peak counts and raw/source/build/hash
evidence saved in shared-properties-runtime-qualification-2026-11-03.json.


### Sparse offscreen cache allocation culling — 2026-11-03

Phase-specific trace separated load16.44MB,first sync0.42MB from first CPU
frame278.22MB requested. Sparse leaf Cache::build allocated full canvases before
packing even for wholly offscreen controls. Conservative support test now
returns empty cache first, accounting for base/main text/content text/shape
overflow and physical AA margin; clip markers cannot add support. Nonfinite
support conservatively follows old path. Scroll/DPI/key changes rebuild cache.
244 Rust tests passed,3 ignored; explicit regression verifies zero offscreen
canvas capacity, scroll reentry and exact overflowing-shape pixels at4 scales.
Frame requested bytes278224356→13553796,allocations1453→228; load/sync
counts unchanged. Runtime lifecycle still returns allocator delta0; lifetime
peak unchanged. Fresh WASM/browser/native GPU gates remain open.
See offscreen-cache-qualification-2026-11-03.json and saved traces/sources.


### Offscreen culling fresh WASM/browser gate — 2026-11-03

Node14/14,Chromium17/17 passed. Standard90 frame/caret+30 artifact comparisons
against previous COW module remain exact. Added compare-sparse-cache-wasm.cjs:
60 exact multi-control content frames across five parent positions,empty/text/
shape templates and four DPI scales. Visible overflow explicitly nonempty;
transparent controls empty. Negative source parent coordinates are refused by
parser,so fixture uses supported nonnegative parents with negative child offsets.
Browser screenshot/raw reports saved; probe tab/server closed. Actual OS/GPU
presentation gate remains open.


### Current native CPU window + physical offscreen GPU repeat — 2026-11-03

Rebuilt NativeHost probe after shared Properties/offscreen cache changes. Actual
macOS paste second=COW你好 мир🙂 (space retained after COW),ShiftTab/paste
first=first edited; raw journal confirms independent fields and visible summary.
Window closed. Hardware offscreen renderer buffers test and126 Reveal
comparisons passed on Apple M4 Max/Metal,max error1/255. These are separate
CPU-window and offscreen-GPU scopes,not GPU OS presentation. Source audit found
select_backend only changes profile/title and frame-bar decoration while draw
uses paint_native/softbuffer; actual egui/GPU transition remains open.
See offscreen-cache-native-hardware-qualification-2026-11-03.json.


### Actual reusable NativeHost GPU presentation — 2026-11-03

Extracted GpuWindow/Presentation and Metal layer visibility into shared native_gpu
module; standalone consumes it unchanged. Reusable host now requests desktop
Svet GPU frames via NativeApp.inspect, preserves CPU fallback and schedules
surface Retry with event-loop deadlines. CPU/chooser/mobile paths retained.
Optional on_frame_presented observer fires after successful presentation; probe
records separate journal. Actual macOS sequence Gpu→Cpu→Gpu (7087 callbacks)
and Unicode edits across F7 roundtrip observed, final model preserved. Screenshots
and compressed journal saved; probe closed.244 Rust tests,all-targets and two
hardware offscreen tests passed. Egui label clarified as CPU compatibility;
app scheduler is independent. Partial native-host-gpu-qualification-2026-11-03.json
lists failure/resize/other-platform/large-form gates still open.


### Native surface backoff + actual OS occlusion — 2026-11-03

Draw now checks retry deadline before update/window lookup/acquisition so input
redraws cannot bypass the16ms retry timer. OS occlusion blocks draws and clears
pending retry; unocclusion requests redraw. Regression exercises10 repeated
backoff/hidden draws,expiry and resumed acquisition boundary without a window.
Optional on_occlusion_changed observer and timestamped probe presentation logs
added. Actual macOS minimize emitted false→true→false;579.415ms hidden interval
contained0 presented frames,then GPU frames resumed. Unicode first value
preserved and second edited after restore.245 Rust tests/all-targets passed;
probe closed. native-host-occlusion-qualification-2026-11-03.json records exact
scope; forced GPU timeout/device loss and repeated/cross-platform lifecycle
remain open.


### GPU uncaptured error delivery + pre-acquisition health — 2026-11-03

Source audit found gpu-runtime on_uncaptured_error and device-lost registration
were no-op. Implemented shared uncaptured handler state on Device/CommandEncoder;
matching innermost error scopes consume errors and preserve first slot,unscoped
callbacks run outside both mutexes. Scope/reentry/handler replacement tests pass.
Renderer keeps first cause,exposes check_health,and NativeGpu checks it before
surface configure/acquisition so latched faults cannot remain hidden by Timeout.
Hardware test now uses invalid encoder dispatch to trigger actual runtime
validation callback without submitting invalid commands; later Device descriptor
validation cannot overwrite cause,draw rejects without frames/uploads.
2 gpu-runtime tests,245 native tests,all-targets,two hardware gates passed.
Device-loss callback still placeholder and explicitly documented; no physical
device-loss/OS fallback qualification claimed. Root cause and remaining scope
saved in gpu-error-delivery-qualification-2026-11-03.json. Generated local crate
lock preserved with evidence and removed from workspace after isolated tests.


### Metal device-loss callback delivery — 2026-11-03

Replaced callback placeholder with per-logical-device once-only state,late
registration and reentry-safe notification outside mutex. Metal retains pending
command buffers through completion rather than losing diagnostics when last
command pointer changes. Reaps on submit/poll/wait; synchronous internal waits
inspect completion too. Local SDK codes4=AccessRevoked,11=DeviceRemoved latch
backend loss; other errors do not become device loss. Device.poll delivers
notification,Renderer health polls before acquire and setup refuses already-lost
backend.5 runtime tests,explicit Metal Device backend-state injection,245 native
tests,all-targets,two hardware gates passed. Physical removal/reset, idle macOS
notification subscription and OS fallback under injected loss remain open.
Evidence and SDK header hash:device-loss-delivery-qualification-2026-11-03.json.


### Actual OS GPU-loss CPU fallback and explicit recovery — 2026-11-03

Added opt-in native-test-hooks feature bridging gpu-runtime/test-hooks. F8 in
this build injects backend loss into the active GPU Device; ordinary native
feature tree excludes test hooks. Optional on_gpu_fallback observer records
reason before CPU fallback. Actual macOS probe:Gpu→Cpu after F8,one device-loss
error,first field preserved,second edited on CPU;F7 profile roundtrip creates
new GPU and second edit continues. Raw model/error/presentation journals and
screenshots saved.245 tests pass in native and native-test-hooks configurations,
normal all-targets check passed,probe closed. Synthetic backend state does not
qualify physical reset/removal or automatic recovery. Report:
native-host-loss-fallback-qualification-2026-11-03.json.


### CPU self-animation pacing and release frame evidence — 2026-11-03

Saved debug trace showed CPU13.8FPS,so it was not treated as evidence of release
over-rendering. New --cpu-frame release probe measures tick+binding sync+paint
with fixed60Hz dt:2x unfocused median0.047ms/0alloc,focused2.134ms/8.70MB
requested and one leaf rebuild/frame. NativeHost CPU self-animation now targets
60Hz start-to-start via event-loop deadlines;input events can redraw immediately,
GPU keeps FIFO cadence,retry uses nearest deadline and occlusion cancels wakes.
245 tests/all-targets passed. Actual release window CPU median17.058ms,return
GPU8.3385ms;model/binding input survived. No pre-change release OS baseline or
input-latency claim. Visible-leaf full-canvas scratch remains a measured target.
Evidence:native-host-cpu-pacing-qualification-2026-11-03.json.


### Region-aware sparse leaf raster scratch — 2026-11-03

Added text/shape region raster APIs retaining global coordinates,clip and
horizontal accumulation prefix;only storage writes are rebased. Conservative
physical support sizes sparse leaf coverage/content/nested clip layers;packed
spans,rect edges and reveal coordinates remain global. Dense path unchanged.
Exact composited region/full tests cover mixed fonts/shapes/alpha/fractional
scales and invalid regions.247 Rust tests,all-targets,hardware Reveal passed.
Fresh WASM Node14/14,Chromium17/17,90 standard+60 overflow frames and30 artifact
JSON comparisons exact against pre-change WASM. Allocation budget2MB on
1600x1200:baseline17,694,710fails,candidate1,124,666passes. Four releaseAB/BA
pairs+A/A:2x focused requested8,704,257→1,119,813 (-87.1%),median time ratio
0.868,A/A0.987. Instrumented timings not OS latency/RSS. Raw runs/sources/builds
and limits preserved in region-raster-qualification-2026-11-03.json.


## Geometry-only CPU content scratch — 2026-11-03

Geometry-only controls no longer allocate or scan a temporary RGBA content
layer. Clip markers alone are also empty. Dense/sparse geometry and paint
remain unchanged. Cold 800×600 content_pixels requested bytes:6,720,064→4,800,064
(−1,920,000 bytes); allocation regression failed before and passes after.
Exact dense/sparse pixel equality against the transparent-shape layer path
passes at four scales. Default lib/integration suite passed 238 tests, 1 ignored;
native all-targets and wasm32 library checks passed. No browser/window, timing
or RSS qualification claimed. Text/shape scratch, dense geometry/frame storage
and physical large-form qualification remain open.

[Qualification](benchmarks/empty-content-qualification-2026-11-03.json).


## Content-only region for dense and sparse CPU storage — 2026-11-03

Content support is now computed independently of base geometry. Temporary RGBA
and pooled nested-clip layers use only the conservative text/shape support
union; global contours, clip/AA arithmetic and emitted spans remain unchanged.
Dense geometry/frame/output buffers are still full canvas. Empty and offscreen
content require no content layer. Nonfinite support falls back to full canvas.

The cold800×600 nested-clip allocation regression fails before (10,561,888
requested bytes) and passes after (4,805,356,−54.5%). 240 exact comparisons
against full-canvas content storage cover four templates, three scroll offsets,
five scales, dense/sparse and both background modes. The native suite passes266
Rust tests (9ignored); all-targets check passes. Explicit Apple M4 Max/Metal
Reveal gate passes126 comparisons,max1/255. Fresh release WASM passes14 Node
cases,120 dense Button+90 Runtime font+60 overflow exact frames and30 exact
artifact JSON comparisons. Chromium156 passes18/18, including a new dense
nested-clip/mixed-font case; screenshot and report saved.

No timing, RSS, OS input latency or physical-window qualification claimed.
Disjoint overflow can still require a large union, and repeated scratch reuse
remains open. Raw logs, before/after source and hashes are retained in the
[qualification](benchmarks/dense-content-qualification-2026-11-03.json).


## Packed dense coverage and split paint passes — 2026-11-03

Dense caches now pack coverage into global row spans too. Output RGBA and root
frame storage remain dense; geometry-only offscreen controls still compose the
root background. Painting preserves per-pixel base→rect edge→content→reveal→
frame/background order while removing per-pixel sparse-item checks from the
full-canvas pass. Sparse output compositing remains packed.

Cold800×600 nested-content requested bytes4,805,356→4,355,408 (−9.36%); the
4.5MB regression fails on the isolated baseline and passes on the candidate.
A/A+4AB/BA default-feature release probes cover geometry/mixed/nested small
controls and a full-canvas control at1x/2x. At2x, small geometry repaint+RGBA
copy median2.699→1.187ms (paired ratio0.439),full-canvas3.178→1.782ms (0.561).
At2x cold ratios0.820–0.868; observed A/A0.932–1.005 across those cases.
This is public Button content_pixels, not NativeHost or OS input latency/RSS.
All sample arrays (sorted),binary hashes and runner are retained.

266 native-feature Rust tests pass (9ignored),all-targets check passes; explicit
M4 Max/Metal Reveal126 comparisons,max1/255 passes. Fresh release WASM:
210 stateful dense paint+120 dense content+90 mixed-font Runtime+60 overflow
exact frames,30 exact artifact JSON,Node14/14 and Chromium18/18 pass.
Stateful differential covers animated fill/border/reveal,clipping/background,
Scroll and DPI. Browser screenshot/report saved; owned tab/server closed.

Frame/output/native XRGB storage,buffer reuse,physical-window/RSS,other backends
and the broader PLAN remain open. [Qualification](benchmarks/packed-dense-qualification-2026-11-03.json).


## Actual large-form NativeHost matrix — 2026-11-03

Added native_form_probe and the synthetic svet-native-large fixture: retained
Svet scheduler,8/32/96/256 mixed CJK/Cyrillic/emoji bound editors in Scroll.
The fixture regression verifies final-row binding writes and CPU pixels at all
four sizes. Native all-targets check and release build pass. An opt-in journal
records changed model snapshots, successful CPU/GPU present callbacks, raster
counters and getrusage process peak RSS. Instrumentation adds overhead.

Actual macOS windows at Retina2x were operated through CUA: GPU edit first row,
F7 CPU,edit first row,scroll to last row,CPU edit last row,F7 GPU,append text,
scroll back to top.8rows fit without scrolling.256 additionally performed3
quick CPU/GPU roundtrips (2CPU frames each),so this is limited lifecycle evidence.
All original unrelated rows and identifiers match exactly; first/last values,
full row count,final scroll0 and renderer sequences are verified from raw logs.
Zero GPU fallback errors in all four runs. Screenshots include256 CPU/GPU
last-row editing and the return to the first row. Owned windows/processes closed.

Observed instrumented present interval p50 CPU/GPU (ms):8=16.922/8.388,
32=17.196/9.835,96=17.246/9.588,256=17.123/8.712. At256 p95=23.930/17.031ms;
max=54.459/48.792ms. Selection includes all adjacent presents with identical
renderer/focus/scroll and both animating,without a duration cutoff. Raw logs
retain the other intervals and transition stalls. Runtime load (not OS launch)
3.133/5.156/10.097/18.152ms. Process peak RSS139.7/145.2/152.0/171.9MB.
Changed-model input callback+binding sync at256 observed16.65–35.42ms; this is
not input-to-display latency. GPU present callback is not completion/scanout.

Each size is one release run on one machine,without baseline or A/A. Peak RSS
is not current live/GPU memory or evidence of no leaks; extra256 roundtrips
raised observed process peak167.2→171.9MB. Long plateau/scroll/lifecycle runs,
other schedulers/platforms,true CJK IME and latency gates remain open.

[Qualification and raw data](benchmarks/native-large-qualification-2026-11-03.json).
Recheck the saved journals with:

```sh
python3 crates/forma/scripts/qualify-native-form.py packages/svet/benchmarks/native-large-raw-2026-11-03 /tmp/svet-native-large-recheck.json
```


## Frame run storage and composition — 2026-11-03

Dense caches replace the Frame byte-per-pixel map with equal-value runs
(exclusive u32 end +coverage/viewport byte). Original round_coverage and
scrollbar coordinates are preserved. Paint handles viewport/scrollbar runs,
then background and rounded clipping; homogeneous regions with no work skip
per-pixel traversal. RGBA output/native XRGB remain dense. Frame construction
still evaluates every pixel; it has not become a geometric span solver.

Uniform1600×1200 Frame needs one run,capacity≤64bytes instead1,920,000byte map,
including the case where button geometry is wholly offscreen. Cold800×600
nested-content allocation regression:4,355,408→3,875,440 requested bytes;
4MB budget fails on the isolated baseline and passes on candidate.

A/A+4AB/BA release pairs were run separately for public Button pixels with
background and content_pixels without it. At2x background repaint+RGBA copy:
small geometry2.796→1.293ms,full-canvas7.491→5.648ms. Background2x A/A range
0.970–1.014. Content-only A/A varies0.670–1.256; those timing results remain
exploratory and do not qualify a speedup. Both raw sets,sorted sample arrays,
source snapshots,binary hashes and --background runner support are retained.
These are not NativeHost input-to-display latency measurements.

267 native-feature Rust tests pass (9ignored),all-targets check passes,probe
fixture regression8/32/96/256 passes. Explicit M4 Max/Metal Reveal126
comparisons,max1/255 passes. Fresh release WASM:480 exact frames (including
stateful animated fill/border/reveal,clipping/background,Scroll,DPI),30 exact
artifact JSON,Node14/14,Chromium18/18 pass. Browser screenshot retained.

Fresh updated release256-editor OS window also passes GPU→CPU→GPU,first/last
row edits,scroll to9334 and back0; final full model matches expected and
unrelated rows/IDs remain exact,0 GPU errors. Raw journal and CPU/GPU screenshots
saved; window/process closed. This single smoke run is not paired latency/RSS
qualification. Frame construction,output/XRGB storage,buffer reuse,long-lived
resource/RSS plateau,other platforms and broader PLAN remain open.

[Qualification](benchmarks/frame-runs-qualification-2026-11-03.json).
The saved256 journal can be checked independently:

```sh
python3 crates/forma/scripts/qualify-native-form.py packages/svet/benchmarks/frame-runs-native-raw-2026-11-03 /tmp/svet-frame-runs-recheck.json --counts 256 --extra-roundtrips-256 0
```


## Exact monotone Frame run search — 2026-11-03

Frame construction splits every row at the rounded-root center and all
pixel-center viewport/scrollbar comparisons. Pixel coordinates retain the
original `(x+0.5)/scale` arithmetic; threshold conversion uses exact binary
search rather than floor/ceil. On each interval viewport bits are constant
and rounded Frame coverage is monotone,so equal-value prefixes are searched
without evaluating the full width. Original scalar sampling remains for small
canvases(width<64),degenerate/nonfinite root parameters. This optimization is
specific to rounded root/viewport metadata; arbitrary contours are unchanged.

A scalar oracle checks240 generated configurations,including fractional
geometry,padding,scroll offsets,both scrollbars,DPI from tiny positive to very
large,plus3 negative/nonfinite-radius fallback cases. Uniform1600×1200 requires
24,000 scalar samples instead1,920,000; both normal and wholly offscreen button
geometry pass the100,000-sample budget and exact original run output.

A/A+4AB/BA default-feature release public Button pixels(background=true)
probes pass.2x cold build+RGBA copy:small geometry6.865→1.506ms (paired0.219),
full-canvas12.560→7.187ms (0.572). Repaint ratios0.965–0.997 at2x remain within
ordinary variation; no independent repaint speedup claimed.2x A/A0.992–1.028.
Raw runs,sorted sample arrays,source snapshots and binary hashes retained.
These are not NativeHost input-to-display latency/RSS measurements.

268 native-feature Rust tests pass (9ignored),all-targets check passes; explicit
M4 Max/Metal Reveal126 comparisons,max1/255 passes. Fresh release WASM passes
480 exact frames,30 exact artifact JSON,Node14/14 and Chromium18/18. Actual
updated256-editor NativeHost GPU→CPU→GPU,top/bottom scroll and first/last-row
edits pass; exact full final model/unrelated rows preserved,0 GPU errors.
CPU/GPU/browser screenshots and the OS journal are saved; owned windows/server
closed. Native behavior run is single,not paired latency/RSS qualification.

RGBA/XRGB output,buffer reuse,For/local overlays/list detachment,long native
RSS/lifecycle plateau and other platforms/schedulers remain open.
[Qualification](benchmarks/frame-spans-qualification-2026-11-03.json).

```sh
python3 crates/forma/scripts/qualify-native-form.py packages/svet/benchmarks/frame-spans-native-raw-2026-11-03 /tmp/svet-frame-spans-recheck.json --counts 256 --extra-roundtrips-256 0
```

## Scope overlay qualification — 2026-11-03

Shared For/Let/read-only replay scopes and captured locals passed regression,
allocation and exact interop gates. [Final evidence](benchmarks/property-overlay-qualification-2026-11-03.json)
and final AB/BA pair directories preserve measured results and source hashes.
Fresh actual native256 journal verified with:

```sh
python3 crates/forma/scripts/qualify-native-form.py packages/svet/benchmarks/property-overlay-native-raw-2026-11-03 packages/svet/benchmarks/property-overlay-native-qualification-2026-11-03.json --counts 256 --extra-roundtrips-256 0
```

Exact model/unchanged rows, maximum scroll9334 and final0, CPU/GPU presents
and no GPU errors passed. The probe title `egui` denotes CPU compatibility,
not an independent egui renderer. Chromium19/19 and screenshots saved.
Native probe process exited after window close. No RSS plateau/input latency
or universal speedup claim; small-form edit overhead remains approximately1–5%.

## Caller-owned RGBA output — 2026-11-03

Added Rust `Runtime::paint_rgba` / `Button::paint_rgba` with exact buffer-size
and DPI validation, supporting background and content-only output. They copy
visible cached pixels into retained caller storage. Existing owned APIs remain
unchanged. Warmed allocation matrix:0 allocations versus1 owned output Vec;
cache release/reconstruction and unchanged destination on invalid DPI passed.
The independent full-canvas oracle now also validates this path.

```sh
CARGO_TARGET_DIR=/tmp/svet-native-development cargo test --manifest-path crates/forma/Cargo.toml --lib --test runtime_allocations
CARGO_TARGET_DIR=/tmp/svet-native-development cargo check --manifest-path crates/forma/Cargo.toml --features native --all-targets
CARGO_TARGET_DIR=/tmp/svet-native-development cargo check --manifest-path crates/forma/Cargo.toml --target wasm32-unknown-unknown
```

192 lib tests +17 allocation tests passed;1 ignored.
[Evidence and limitations](benchmarks/rgba-output-qualification-2026-11-03.json).
No OS smoke/browser bundle refresh is claimed for this Rust API addition.
Retained raster/XRGB storage and JS-owned output still require further work.

## CPU allocator lifecycle — 2026-11-03

Added `--cpu-lifecycle` to the binding workload and a separate raw JSON verifier.
Both schedulers,8/32/96/256 controls,32 alternating first/last edits, retained
RGBA/XRGB destinations and CPU cache release every4 edits. Each frame checks
owned RGBA equivalence and XRGB compositing; final model checks every row/ID.
One warm lifecycle precedes3 measured lifecycles per configuration.

```sh
CARGO_TARGET_DIR=/tmp/svet-native-development cargo run --manifest-path crates/forma/Cargo.toml --release --example binding_sync_bench -- --cpu-lifecycle > /tmp/svet-cpu-lifecycle-final.json
python3 crates/forma/scripts/qualify-cpu-lifecycle.py /tmp/svet-cpu-lifecycle-final.json /tmp/svet-cpu-lifecycle-summary.json
```

All24 measured lifecycles (768 edit frames) pass. Late16-edit live bytes are
exactly constant; retained bytes repeat exactly across3 samples and after-Drop
delta is0 throughout. Svet256 retained4,132,937/peak6,469,927 Layout bytes.
Raw, build log and source hashes saved with the
[qualification](benchmarks/cpu-lifecycle-qualification-2026-11-03.json).
This is bounded single-thread allocator accounting, including caller output
buffers, excluding diagnostics. No OS presentation, RSS, GPU memory or universal
leak-freedom claim. Long-duration platform lifecycle remains open.

## Logical GPU Queue identity — 2026-11-03

Two request_device calls on one adapter previously produced unequal Device
handles but equal Queue handles because Queue equality compared shared native
Metal pointers. Each logical queue now owns an Arc identity, retained by clones.
No Metal resource isolation or command routing behavior is implied.

The explicit two-device hardware regression failed before the fix and passed
after it; it also checks clone stability after Device drop and existing logical
error-handler isolation. gpu-runtime5 tests passed/2 hardware tests ignored in
default run; explicit queue hardware test passed; native218 passed/3 ignored;
explicit profiled renderer smoke passed on Apple M4 Max/Metal.

```sh
CARGO_TARGET_DIR=/tmp/svet-native-development cargo test --manifest-path crates/gpu-runtime/Cargo.toml distinct_logical_devices -- --ignored --nocapture
CARGO_TARGET_DIR=/tmp/svet-native-development cargo test --manifest-path crates/gpu-runtime/Cargo.toml --lib
CARGO_TARGET_DIR=/tmp/svet-native-development cargo test --manifest-path crates/forma/Cargo.toml --features native --lib
CARGO_TARGET_DIR=/tmp/svet-native-development cargo test --manifest-path crates/forma/Cargo.toml --features gpu profiled_renderer_reports_owned_buffers_and_reuses_them -- --ignored --nocapture
```

[Evidence](benchmarks/queue-identity-qualification-2026-11-03.json). Non-loss
command errors are still dropped by backend completion inspection. Forwarding
must retain submit ownership across deferred compute batches and invoke callbacks
outside backend locks. Idle removal notification/physical reset remains open.

## Metal command failure forwarding — 2026-11-03

Logical Queue captures its error scope stack/handler as an internal sink.
Pending compute batches preserve deduplicated logical sinks; native command
completion records keep them through general/timed commands and staged writes.
Non-loss NSError failures and missing-NSError failures now reach original sinks
as Internal errors. Existing removal/revocation device-loss behavior remains.
The pending-command mutex is released before invoking callbacks, allowing poll
reentry and error-scope operations. Raw public MetalDevice APIs retain their
previous signatures without a logical sink.

Explicit hardware regression submits real clear and pure-compute commands on
two logical devices sharing one adapter and verifies owner metadata. Injected
completion failures then verify routing even if the other device polls, once-only
delivery, Internal scope capture and handler poll reentry. No physical fault is
claimed. Runtime5 unit tests +3 explicit hardware tests passed; native218 passed;
Metal renderer smoke passed. [Evidence](benchmarks/command-error-qualification-2026-11-03.json).

```sh
CARGO_TARGET_DIR=/tmp/svet-native-development cargo test --manifest-path crates/gpu-runtime/Cargo.toml --lib
CARGO_TARGET_DIR=/tmp/svet-native-development cargo test --manifest-path crates/gpu-runtime/Cargo.toml --lib -- --ignored --nocapture
CARGO_TARGET_DIR=/tmp/svet-native-development cargo test --manifest-path crates/forma/Cargo.toml --features native --lib
CARGO_TARGET_DIR=/tmp/svet-native-development cargo check --manifest-path crates/forma/Cargo.toml --features native --all-targets
```

Remaining: OOM-specific classification, physical fault/error-code coverage, idle
removal wakeups, other backends and performance cost of ownership metadata.
Timed/staged failures use the same completion collector but lack forced-fault
coverage. Async error-scope pop semantics remain outside this implementation.

## Metal OOM classification — 2026-11-03

The system SDK MTLCommandBuffer.h defines OutOfMemory=8. Classification now
compares NSError domain against exported MTLCommandBufferErrorDomain before
returning OutOfMemory; other non-loss errors retain Internal and messages.
Removal/revocation codes also require the Metal domain. No new domain/code
conversion runs for successful commands.

Unit matrix tests OOM, other codes, and foreign domains. Existing real-adapter
routing test now verifies OOM scope capture and pop, foreign-domain code8
bypassing OOM scope to Internal handler, no duplicates and callback reentry.
Scope pop is explicitly polled; dropping the guard alone does not remove it
in the current API.6 runtime unit tests,3 explicit hardware tests and218 native
lib tests passed. [Evidence](benchmarks/command-oom-qualification-2026-11-03.json)
contains SDK excerpt/hash and official Apple references.

Synthetic completion failures do not prove actual GPU OOM/device-removal
recovery. Physical faults, forced NSError extraction, idle removal notification
and cross-platform/performance gates remain open.

## Native idle completion service — 2026-11-03

Added Device::is_queue_idle (Metal completion status), GpuWindow::poll_idle and
a NativeHost completion deadline after GPU presents. about_to_wait polls health
without acquiring a drawable or rendering, rescheduling16ms only while work
is pending. A fault retires the GPU layer and requests CPU redraw if visible.
Occlusion suppresses animation/retry paints but preserves completion servicing.
Once complete, the host returns to unbounded Wait. Fully idle device removal
after this point still requires native notifications and is not implemented.

Native218 tests passed/3 ignored; all-targets check passed. Extended scheduler
regression covers occlusion/health deadlines and stopping completion wakeups.
Explicit Apple M4 Max/Metal renderer test drains queue and repeats health checks
without changing the frame counter.
[Evidence](benchmarks/idle-completion-qualification-2026-11-03.json).

Pending: fresh OS-window idle-fallback/occlusion smoke, physical GPU faults,
native removal notifications, energy/performance and other backend behavior.
The non-Metal idle-query branch is not platform qualification.

## Actual idle-fallback window — 2026-11-03

Added F9 only under native-test-hooks. Press/release bypass generic input
redraw; a per-host flag injects logical loss at the start of about_to_wait and
services health immediately. Idle-path fallback reports an explicit origin prefix.
Ordinary native builds omit the injection field and F9 branch. F8 remains the
existing draw-path fault hook.

First diagnostic injected in the key handler; a scheduled caret draw observed
the fault before idle service. That raw journal is retained as draw-race
diagnostic, excluded from qualification. The revised hook's fresh native window
records exactly one `Idle GPU completion` fault, followed by CPU presents and
no GPU presents. Unicode editing succeeds after fallback; final exact model and
all7 unrelated rows verified. Screenshot and raw journal saved with
[qualification](benchmarks/idle-window-qualification-2026-11-03.json).

Both ordinary native and native-test-hooks lib suites pass218 tests/3 ignored.
Debug instrumented OS run on one macOS machine; synthetic logical loss is not
physical removal/OOM, performance, latency or RSS proof. Forced occlusion while
work is pending, OS idle wakeup cessation and native removal callbacks remain
open. Test window/process exited.

## Actual idle completion cessation and OS occlusion — 2026-11-03

Added optional NativeApp::on_gpu_health_checked observer; default is no-op.
The probe journals health service and supports NO_FOCUS=1 to disable caret
animation without changing ordinary fixture behavior. A stop record bounds the
finished journal. Callback errors exit the host, independently of GPU errors.

Actual ordinary debug native8 window (no test hooks) records an OS-occluded
32.24s interval:1 final idle health check,0 presents, then GPU presentation
resumes when visible. A separately timed13.40s live-process observation has a
byte-identical journal after idle. Exact model remains unchanged. Native218
lib tests +1 probe test passed. A reusable verifier checks idle health cessation,
occlusion/presentation ordering and model; it rejects inserted repeated health
checks without intervening new frames.
[Evidence](benchmarks/idle-wakeup-qualification-2026-11-03.json).

```sh
python3 crates/forma/scripts/qualify-idle-wakeup.py packages/svet/benchmarks/idle-wakeup-raw-2026-11-03.jsonl.gz /tmp/svet-idle-wakeup-summary.json
```

This measures recorded health checks/presents, not all event-loop wakeups or
energy/latency/RSS. Queue execution may have completed before occlusion; the
proof covers servicing an outstanding health deadline while occluded. Actual
physical loss while occluded and fully-idle removal notification remain open.
Test window/process exited; screenshot saved.

## Error-scope guard ownership — 2026-11-03

ErrorScopeGuard::pop previously removed the stack's latest entry regardless of
which guard owned it. Each entry/guard now shares a unique Arc token; pop removes
the matching entry. Error dispatch still picks the innermost matching active
filter and preserves its first error. Tests exercise both pop orders, an unrelated
new scope remaining intact, and8 concurrent pops returning their own errors.
Runtime8 unit tests +3 explicit Metal tests and native218 tests pass.
[Evidence](benchmarks/scope-owner-qualification-2026-11-03.json).

This fixes ownership only. Pending-completion-aware async pop and submit-time
scope snapshots remain open; callers still poll before pop, whose future is
immediately ready. Implicit guard Drop behavior is unchanged.

## Submitted error scopes and asynchronous pop — 2026-11-03

Error-scope entries now share result/pending/waker state. Logical Queue submit
and write capture the active scope set before entering the backend. Captured
leases survive scope pop; error dispatch uses that set, so a newly pushed scope
cannot receive an old submission's error. Batching deduplicates clones of the
same submission while retaining distinct scope captures. Unscoped submissions
record an empty capture without allocating snapshot storage.

pop removes its own active entry immediately and returns a future that waits
for outstanding leases. On first pending poll it starts a completion worker;
the worker services backend completions until that scope's work finishes.
Successful and failed completions both release leases outside backend locks.
Waker replacement/drop and wake occur outside scope locks. The collector owns
a batch outside its mutex and restores unfinished records, preserving concurrent
submissions without an extra per-completion delivery vector in the common path.

Unit10 +3 explicit Metal tests, native218 tests and hardware renderer smoke
passed. Tests cover submission snapshots, unscoped submissions, partial command
completion, Pending/wake, successful real command and injected error, no explicit
caller poll, reentrant waker and clean newer scope.
[Evidence](benchmarks/scope-capture-qualification-2026-11-03.json).

This supersedes the earlier immediately-ready-pop limitation. Pending pops use
a detached completion thread per awaited scope; thread pooling/resource limits,
performance and cancellation of driver waits remain unqualified. Physical GPU
faults, raw backend calls without logical sinks and other backends are not
covered by this proof.

## Shared scope completion service — 2026-11-03

Replaced one worker per pending pop with a service shared by cloned Metal
backends and their logical devices. Registrations hold Weak scope-result
references; a single servicing worker drains completions for the active burst.
It retires when requests finish or expire, releasing registration queue capacity.
Scope upgrades/destructors and backend waits execute outside service locks.
Concurrent registrations are merged back after each scan; existing capacity is
reused while work remains.

A real-adapter resource test holds64 synthetic submit leases and polls64 scope
futures. Each of2 rounds cancels32 futures, releases all leases with individual
errors, verifies32 remaining results, one worker launch per burst, retirement
and zero idle registration capacity. Previous real clear-command async-pop,
reentrant waker, OOM and logical-owner tests remain green. Runtime10 unit tests,
4 explicit Metal tests,native218 and renderer smoke pass.
[Evidence](benchmarks/scope-service-qualification-2026-11-03.json).

Worker counts are internal launch instrumentation, not whole-process OS thread
sampling. Throughput, latency, RSS/energy, driver-wait cancellation and other
backends remain unqualified. Registration memory scales with active scope waits;
separate backend instances retain separate services.

## Real GPU scope timing and conditional backoff — 2026-11-03

Added gpu_scope_bench:1MiB real Metal clear under1/8/32/64 nested scopes,
concurrent pops, async service versus explicit blocking wait. Every warm/measured
operation asserts queue idle and exact all-zero readback. Twelve measured
samples/group after4 warmups; alternating mode order. A runner stores baseline
A/A +4 AB/BA pairs and can regenerate medians/nearest-rank p95 with
--summarize-only. Per-group p95 is the maximum of12 samples, not a qualified tail.

Initial release timing exposed the service's unconditional1ms sleep. The worker
now reaps completions before scanning and backs off only when pending leases
outlive entered backend work. Functional gates pass: runtime10,explicit Metal4,
native218,hardware renderer and all-targets.

First paired series overlapped test compilation and is diagnostic. Second
series had no owned build/test jobs, but baseline explicit-wait A/A ratios still
span0.362–1.656; performance qualification remains inconclusive. Async p50
observations improve~0.52–0.58→0.13ms, but no qualified speedup is claimed.
Both complete raw series, source/binary hashes and build logs are preserved in
[qualification](benchmarks/scope-backoff-qualification-2026-11-03.json).

```sh
cargo run --manifest-path crates/forma/Cargo.toml --release --features gpu --example gpu_scope_bench
python3 crates/forma/scripts/qualify-gpu-scopes.py BASELINE_BINARY CANDIDATE_BINARY OUTPUT_DIR
```

Next: stabilize control timing and increase warmup/samples before accepting
performance claims; OS thread/RSS/energy and physical GPU faults remain open.

## Scope benchmark control isolation and rejected confirmation — 2026-11-03

Found the previous unchanged control was not independent: async and explicit
wait used one Metal backend, so a completion worker could take the control's
last command buffer. Each mode now owns a separately requested backend/device/
queue/buffer. Work remains nested1/8/32/64 scopes,1MiB real clear and exact
readback. Warmup/sample counts are configurable (defaults16/64).

Two standalone harnesses use identical benchmark sources, profiles and lock
files, differing only in saved baseline/candidate runtime types.rs. Reusing one
target directory unexpectedly produced identical executables; that entire
series is rejected and retained. The final candidate was rebuilt in its own
target directory; distinct hashes are recorded. The runner now rejects equal
binaries before execution and validates recorded identity/protocol/isolation.

The first valid isolated series stabilizes explicit controls but retains unstable
async pooled medians. Baseline async samples have two order-conditioned modes
(~0.15ms and~0.97ms); pooled50/50 p50 is unstable. Added both order strata while
retaining every sample and pooled gate. A fresh10-run confirmation fails frozen
control/async gates, so **performance is inconclusive**, with no qualified
speedup claim. Failed cells and all raw results are preserved in
[qualification](benchmarks/scope-order-qualification-2026-11-03.json).

Two valid series contain10 runs each,512 measured samples/run and640 exact
clear checks including warmup/run. Negative tests verify equal-binary rejection
and shared-control disqualification. No further retries to obtain green were
performed. Next work should investigate scheduling/order effects and measurement
conditions while keeping failed evidence and thresholds.

## Scope-pop cancellation lifetime — 2026-11-03

Pending scope-pop Drop clears pop_waiting and takes/releases the executor waker
outside its scope mutex. Service scans require both outstanding work and a live
waiter; captured scope state/error routing remains owned by submission leases.
This separates task cancellation from command lifetime.

The unit regression retains a submitted lease, cancels its future and verifies
that an executor resource immediately drops. Its destructor reenters the same
scope state, proving no lock is held during waker destruction. A subsequent
error remains in the original captured scope; dropping the lease does not wake
the cancelled task. Extended real-adapter service regression cancels64 futures
and observes worker/registration retirement while64 synthetic leases remain
alive. Previous partial-burst, reentrant-waker and real-command tests pass.

Runtime11 unit tests +4 explicit Metal tests, native218 and renderer smoke pass.
[Evidence](benchmarks/scope-cancel-qualification-2026-11-03.json). Already-entered
backend wait_idle is still not cancellable; the retirement proof uses held
leases without a hung driver wait. No performance/RSS/energy claim, and earlier
performance gates remain inconclusive.

## Native completion signals instead of blocking scope waits — 2026-11-03

Added target-specific block2=0.6.2 (already present in native dependency lock).
Scoped Metal commands register addCompletedHandler before commit. The copied
block captures only a Weak Send+Sync completion service, invokes no user callback
and owns no Metal backend. Commands without captured scopes skip block creation.
General, pure compute, timed and staged-write command paths install the signal;
metadata publication also notifies to cover completion racing record publication.

The scope worker no longer calls wait_idle. It reaps statuses nonblockingly,
scans live waiters and parks on a generation-counted Condvar. Completion, new
registrations and cancellation notify it;16ms timeout is a fallback. Existing
public explicit blocking waits retain their behavior. Future cancellation can
now remove a parked scope worker without waiting for GPU work completion.
GPU execution itself is not cancelled.

Explicit Metal tests observe real native callback delivery and0 blocking backend
wait calls across successful real clear commands and synthetic scope failures.
Burst/cancellation with live leases, scope ownership, OOM, reentrant wakers and
task destructors pass. Runtime11 unit +4 explicit Metal tests, native218 and
renderer smoke pass. [Evidence](benchmarks/scope-signal-qualification-2026-11-03.json)
includes source/lock hashes and official API references.

Physical hang/removal/OOM, fully-idle removal callbacks and non-Metal execution
remain unqualified. Prior performance gates remain inconclusive and do not
cover this changed implementation; no timing/energy improvement is claimed.

## Completion signal ordering regression gates — 2026-11-03

Extracted generation-checked parking from the worker into a private helper,
retaining the same production behavior. Deterministic tests cover completion
notification before parking, notification after the same-mutex atomic unlock/
park, explicit missing-signal timeout fallback and epoch wrap. The before-park
case checks the GenerationChanged result, so timeout fallback cannot mask a
lost early signal. A real-adapter cancellation test invokes a late production
signal entry point after service retirement and verifies no new worker or
registration storage appears.

Runtime14 unit tests +4 explicit Metal tests and native218 pass. Real native
callback delivery and0 async-path backend waits remain verified.
[Evidence](benchmarks/scope-signal-races-qualification-2026-11-03.json). These
are targeted regression cases, not exhaustive concurrency verification. Physical
faults and performance/resource qualification remain open; failed benchmark
series are retained and do not qualify the signal-driven implementation.

## Grapheme-aware native/WASM input — 2026-11-03

Added direct unicode-segmentation1.13.3 dependency and explicit TextEdit cluster
APIs, preserving existing scalar operations and byte-offset contracts. Complete-
string GraphemeCursor queries retain context for RI pairs and ZWJ sequences.
UI keyboard navigation, deletion, insertion and single-pass hit testing now use
cluster boundaries. Partial scalar selections expand for grapheme replacements;
insertion joining a suffix and live binding replacement align resulting caret.
No shaping/BiDi/IME ownership changes are implied.

Tests cover combining, skin tone, ZWJ family,flags,keycaps,Hangul,Indic,CRLF,
empty/extreme navigation, scalar compatibility, undo/redo, partial selection,
fade ranges, exhaustive hit coordinates and binding replacement. Native220 unit
+4 integration +17 allocation tests pass; fresh WASM Node21/21 passes including
7 cluster scenarios with sync and unrelated payload. Actual8-editor macOS GPU/
CPU smoke verifies ZWJ deletion, undo/selection, combining,flag and skin-tone
deletions with exact intermediate/final models and7 untouched rows;0 GPU errors.
Window/process exited; screenshots/raw journal saved with
[qualification](benchmarks/grapheme-qualification-2026-11-03.json).

Remaining: browser DOM/input smoke, actual marked-text IME, full shaping/BiDi/
ZWJ/color emoji visuals, platform matrix and performance qualification.

## Unicode17 grapheme boundary conformance — 2026-11-03

Vendored complete unmodified GraphemeBreakTest17.0.0 with Unicode license and
provenance. Its766 vectors supply independent expected byte boundaries, matching
the pinned segmentation library's Unicode version. Tests do not obtain expected
results by invoking the segmentation algorithm under test.

Four conformance gates verify forward/reverse/Shift selection, exact deletion/
undo/redo, arbitrary scalar-offset snapping/movement and scalar selection
replacement expansion. Corpus contains1391 reference clusters,2756 scalar
cursor offsets and1990 scalar replacement spans. Four conformance +4 prior
grapheme integration tests pass. Production editor source hashes match the
previous native/WASM qualification snapshot; no production code changed.
[Evidence](benchmarks/grapheme-conformance-qualification-2026-11-03.json).

```sh
CARGO_TARGET_DIR=/tmp/svet-native-development cargo test --manifest-path crates/forma/Cargo.toml --test grapheme_conformance --test grapheme_edit
```

The fixture is offline and version/count pinned. Passing this finite corpus
does not qualify all Unicode strings, font shaping, BiDi, actual marked-text
IME, browser DOM events or performance.

## Native/normalized word-boundary actions — 2026-11-03

Added TextEdit move_words/backspace_word/delete_word, Editor word-key dispatch
and additive Runtime::text_word_key. Non-whitespace UAX29 word-boundary
segments (including punctuation/emoji) are stops. Left targets starts, Right
ends, with grapheme rounding; selection collapse consumes the first step.
Deletion is one undoable replacement. Full-string scanning retains Unicode
context, and bulk movement consumes the iterator once.

Native Option (macOS) or Ctrl without Alt (Windows/Linux) dispatches normalized
word actions before clipboard/plain keys. AltGr guard and ordinary grapheme
keys remain separate. Tests cover punctuation,emoji,combining marks,apostrophes,
decimals,multiline,scalar interior positions,history and Runtime bindings. A
fifth Unicode17 corpus gate checks word movement/deletion never splits the
reference grapheme boundaries; this is not WordBreakTest conformance.

Native220 unit +word4 +grapheme4 +conformance5 +allocation17 tests pass. Fresh
WASM Node23/23 and native all-targets pass. Actual macOS8-editor GPU/CPU smoke
verifies Option backward/forward/delete/Shift selection and undo, exact bound
values and7 untouched rows,0 GPU errors. Screenshot/raw journal saved; window
closed and process exited.
[Evidence](benchmarks/word-qualification-2026-11-03.json).

Windows/Linux runtime, browser DOM modifier forwarding, locale dictionary
segmentation, platform-native word parity, full shaping/BiDi/IME and performance
remain unqualified. Browser hosts can use the tested normalized WASM method.


## Standalone native word dispatch — 2026-11-03

The standalone main.rs host now tracks Control/Alt, forwards Option on macOS
or Ctrl without Alt on Windows/Linux to Rust word actions before clipboard and
plain keys, preserves text-producing AltGr input, and excludes Ctrl+Option
accessibility chords. Blur clears modifiers and preedit state. This closes the
source-level difference from the shared NativeHost; actual standalone OS-window
and non-macOS input remain unqualified.

Native all-targets cargo check passed (existing window_bench dead-code warning),
word_edit4 and browser adapter32 tests passed. Logs retained in benchmarks as
standalone-word-{check,tests,adapter-tests}-2026-11-03.txt. Browser DOM/WASM
smoke remains the next task; adapter unit tests do not qualify trusted OS input.


## Browser DOM → Rust word input — 2026-11-03

Real createVectorPreview plus fresh Rust WASM passed7 browser cases: word delete,
undo, Shift selection/replacement, beforeinput word deletes, composition and
Ctrl+Alt guards, binding callbacks and untouched second row. This exposed a
production platform bug: UA Client Hints returns macOS, which the original
Mac-only regex missed. Added macOS recognition and regression (adapter33 pass).
The fixture derives expected macOS modifiers independently from navigator.platform.
The initial failing diagnostic is preserved. Final JSON/screenshot/hashes are
in browser-word-* benchmark artifacts. Synthetic DOM input only; trusted OS
keyboard/IME and GPU presentation are not qualified by this probe.

Reproduce with a fresh wasm-bindgen web bundle:
`python3 crates/forma/scripts/serve-word-browser.py --wasm-dir /path/to/web-bundle --result /tmp/word-browser.json`,
then open `/crates/forma/tests/preview-word-browser.html` on localhost:8769.
Owned probe tab and server were closed.


## Browser composition cancellation — 2026-11-03

Empty compositionend previously called text_insert("") and could delete a live
selection. It now clears preedit without committing. Cancellation resets the
adapter composition flag (including window blur), and late compositionupdate/
compositionend are ignored until a new composition starts.34 adapter tests pass;
real DOM + Rust WASM9 cases pass, including exact selected-value preservation
and late commit after blur. Screenshot, JSON, logs and hashes retained as
composition-cancel-* artifacts. The shared browser-word-dom JSON now contains
this expanded9-case rerun; earlier7-case qualification describes the prior run.
Actual marked-text IME, trusted keyboard and broader platform matrix remain open.


## Browser composition owner — 2026-11-03

Composition now captures its Runtime identity and focused index. Updates and
commit require the same owner; focus synchronization cancels a displaced owner,
scene replacement clears admission before freeing the previous Runtime, and
hidden/nonediting inputs or destroy cancel admission. Late commit cannot insert
into a replacement Runtime or another focused control. Adapter35 tests passed,
including focus displacement, scene replacement and fresh-owner commit. This is
adapter evidence, not actual IME or browser end-to-end qualification of this
latest ownership change; those remain required.


## Browser composition ownership qualification — 2026-11-03

Real DOM/Rust WASM12 cases pass: focus switches, Runtime replacement and a fresh
composition owner preserve exact models, including unrelated row. CUA browser
keyboard presses separately produced isTrusted=true events: End, Alt+Backspace
changes alpha beta→alpha space; Meta+z restores it. The second row stays intact.
This is browser-dispatched trusted input, not physical keyboard or OS marked-text
IME qualification. Source hashes, JSON and screenshot: composition-owner-*.
Probe/server closed. Fixture server now requires --result and sends no-store
headers, avoiding accidental overwrite of previous evidence and stale modules.


## Rust preedit reconciliation boundary — 2026-11-03

Added independent Runtime regression for keyed editor reorder with committed
edit, select-all and active preedit. Replacement preserves value/selection/focus
and undo, drops preedit, leaves sibling unchanged and does not mutate the prior
Runtime. Existing implementation already satisfies this boundary; no production
Rust change was needed. Native lib221 passed,3 ignored. Evidence and source hash:
preedit-reconcile-* artifacts. Actual OS marked-text IME remains unqualified.
Standalone main.rs still has direct IME commit/preedit handling unlike shared
NativeHost ownership checks; bring this path to parity next.


## Standalone IME ownership — 2026-11-03

Standalone main.rs now captures native Runtime/editor identity for preedit,
checks it before commit and while updating presentation, retires composition on
blur/Disabled, and supports a fresh preedit or Enabled session. Empty commit
clears marked text without deleting selection. Direct commits remain supported.
Runtime::native_editor_session is now a native-only public method in the ordinary
Rust impl (outside wasm_bindgen); no WASM export/schema change.

Two standalone binary tests use real Runtime: focus displacement, replacement,
Disabled late commit, fresh preedit through empty preedit, direct commit and
empty-commit selection preservation. Both pass; native lib221 pass,3 ignored;
native all-targets check passes with existing window_bench warning. Logs/hashes:
standalone-ime-* artifacts. Actual OS-window marked-text/IME and non-macOS
runtime qualification remain open.


## Shared NativeHost empty IME commit — 2026-11-03

Regression reproduced a destructive empty-commit path: with select-all active,
commit_ime("") replaced the entire committed value with empty text. NativeHost
now clears preedit for an empty commit and leaves committed selection untouched.
The expanded owner-dispatch test runs both focus-switch and Runtime-replacement
branches. Before-failure and final logs retained;221 lib +2 standalone tests
pass,3 ignored. Evidence: shared-empty-commit-* artifacts. Actual OS IME remains
open; this verifies native dispatch/model behavior directly.


## Repeated stale IME commits — 2026-11-03

Expanded native owner-dispatch regression reproduced a second stale commit
entering the newly focused field: the first rejection consumed owner identity,
so the next commit was treated as direct input. Shared NativeHost now records
cancellation on owner mismatch; subsequent commits stay rejected until new
preedit or Enabled. Cancellation is recorded after app.update returns even when
that update returns an error after executing its callback. Both focus-switch
and Runtime-replacement branches pass, along with fresh preedit/direct commit
and empty commit selection preservation. Native lib221 +binary2 pass,3 ignored.
Before/after logs and hash retained in repeated-ime-* artifacts. OS IME remains
open; these are native event-dispatch/model regressions.


## Standalone IME navigation cancellation — 2026-11-03

Pressed nonrepeat Tab/Escape now explicitly cancels composing or pending-owner
IME in standalone host before navigation, matching shared-host admission rules.
Blur uses the same cancellation helper. A real Runtime regression covers empty
preedit (composing=false but owner retained), cancellation, two late commits,
unchanged sibling and fresh preedit recovery. Standalone3 tests pass; native
all-targets check passed. Logs/hash: ime-navigation-* artifacts. This verifies
dispatch helper behavior; actual keyboard/OS IME remains open.


## Pending-owner focus cancellation — 2026-11-03

Shared focus_sensitive_input previously skipped ownership tracking whenever
composing=false, including the empty-preedit interval before commit. Expanded
regression failed for pointer cancellation in that interval. The shortcut now
requires no pending owner as well. Eight combinations (Escape/Tab/blur/pointer
with active/empty preedit) verify cancellation, repeated late commits and fresh
session recovery. Standalone also observes owner immediately after binding sync,
which can replace Runtime. Native221 +standalone3 tests pass,3 ignored. Before/
after logs and source hashes retained in pending-ime-owner-* artifacts. Actual
OS IME and dedicated standalone binding-sync/IME end-to-end qualification remain
open.


## Standalone bindings/IME integration — 2026-11-03

Added real Runtime integration for both schedulers. Idle binding sync preserves
composition ownership/admission; changing a sibling binding reconstructs scene
with the same focused key/index but a new native session. Production presentation
observer cancels composition immediately, two late commits stay refused, and a
fresh preedit/commit updates bound state while preserving the sibling value.
Extracted the small ownership observation helper so event and presentation paths
share exactly the tested logic. All4 standalone tests pass. Evidence/hash:
ime-binding-* artifacts. This closes dispatch/model binding integration, not
actual OS-window marked-text IME or cross-platform input qualification.


## Unicode17 full word conformance — 2026-11-03

Vendored complete unmodified WordBreakTest17.0.0 with existing Unicode license,
provenance and SHA256. All1944 vectors pass exact raw boundary comparison.
A second gate tests Rust TextEdit word navigation/deletion from each scalar
position against reference segment markers plus documented whitespace-skipping
and grapheme-rounding policy. Exact removal/undo/redo/value/cursor pass. Expected
word boundaries never call the segmentation algorithm under test; grapheme
rounding uses the library separately qualified by GraphemeBreakTest. Both tests
pass offline. Evidence: word-conformance-* artifacts. No production source change.
Dictionary segmentation, platform-native parity, shaping/IME and performance
remain open.


## Native input CI gate — 2026-11-03

CI runtime matrix previously checked native binary compilation only; GPU-only
lib tests did not compile native_window.rs. Replaced native check with locked
native lib +standalone binary tests, covering host IME/modifier regressions on
configured Ubuntu/macOS/Windows jobs. Tests create no OS windows. Existing GPU
lib/tests step already includes grapheme/word corpus integration tests.

Exact new command passes locally on macOS:221 lib +4 binary,3 ignored. Workflow
YAML parsed with Ruby Psych and OS matrix/gate verified. actionlint unavailable;
remote CI has not run (changes uncommitted/unpublished). Evidence and workflow
hash: native-input-ci-* artifacts. Configuration is not cross-platform runtime
qualification until remote jobs actually pass.


## Explicit editor commands retire transient preedit — 2026-11-03

Regression reproduced undo retaining stale marked text after committed history
changed. Editor now clears preedit after handled ordinary, command and word
keys; unsupported keys preserve it. Hosts continue gating active IME key input;
this hardens explicit Runtime/editor operations. Regression covers undo/redo,
plain navigation/deletion, word navigation/deletion and absence of marked text
in committed value. Before failure retained. Native222 +standalone4 +Unicode/
word/grapheme integrations15 =241 tests pass,3 ignored. WASM compile check passed;
fresh WASM/browser runtime qualification of this source revision remains open.
Evidence/hash: editor-preedit-* artifacts. True OS IME remains open.


## Fresh WASM marked-pixel qualification — 2026-11-03

Rebuilt release WASM from latest editor source; Node24/24 and browser13/13 pass.
New oracle first verifies preedit changes pixels, then compares exact full RGBA
frames against clean editors after plain navigation, word deletion and undo.
Committed values match and marked pixels disappear for all3 paths. Browser
DOM ownership/bindings matrix also rerun on fresh bundle. Bundle archive, build
log, source/WASM hashes, JSON and screenshot retained as editor-preedit-wasm-*.
Owned tab/server closed. This closes WASM execution qualification of the editor
change; synthetic composition does not qualify actual OS marked-text IME.


## Multiline End preserves CRLF grapheme — 2026-11-03

Regression reproduced End targeting the byte between CR and LF in initial
multiline values. Shift+End could then make grapheme-aware deletion consume the
line break. End now targets before the whole CRLF pair. Regression checks plain/
Shift End, exact selected combining text, deletion preserving CRLF, undo,
initial scalar-interior cursor and second-line Home/End. Native223 +standalone4
+integration15 =242 tests pass,3 ignored. Before-failure/final logs and hash:
crlf-end-* artifacts. Fresh WASM/browser qualification and OS input remain open.


## Multiline CRLF hit/vertical navigation — 2026-11-03

Right-edge hit regression reproduced caret inside CRLF: split-newline left CR
in the line-local grapheme scanner. Hit now excludes trailing CR when a following
LF exists, preserving scalar start offsets for subsequent rows. Tests scan1010
x-coordinates on each of2 rows, check exact far-right hits, vertical navigation
and Shift selection across the complete CRLF. Native224 +standalone4 +integration15
=243 pass,3 ignored. Before failure/final logs/hash: crlf-hit-* artifacts. Fresh
WASM/browser regression for both End and hit fixes remains required.


## Fresh CRLF WASM qualification — 2026-11-03

Fresh release WASM passes Node25/25 and browser14/14. CRLF is loaded through
JSON state binding, preserving original bytes rather than insertion normalization.
Public Runtime API test covers End/Shift+End deletion/undo, pointer right-edge
hit and vertical Shift selection/deletion/undo. Exact values and selected bytes
retain complete CRLF. Browser runs same API assertions on real WASM; it does not
qualify trusted DOM multiline keyboard or actual OS IME. Build/bundle archive,
source/WASM hashes, JSON and screenshot retained as crlf-wasm-* artifacts.
Owned probe/server closed.


## CRLF visual line geometry — 2026-11-03

Independent LF-equivalence oracle reproduced CRLF rendering a trailing CR glyph/
advance (first-row width30.45 vs27.03). Content generation now strips CR only
before an existing LF while retaining raw byte lengths for subsequent row and
selection offsets. Exact Content/caret equality against LF passes both selected
and unselected; model retains original CRLF bytes. Native225 +standalone4 +15
integrations =244 pass,3 ignored. Before/final logs/hash: crlf-render-* artifacts.
Fresh WASM/browser/pixel qualification of this render change remains open.


## CRLF fresh WASM pixel equivalence — 2026-11-03

Fresh release WASM passes Node26/26 and browser15/15. New gate compares exact
full RGBA frames and caret bounds of JSON-bound LF vs CRLF editors, both focused
unselected and select-all, and confirms CRLF bytes remain in model/state. Prior
End/hit/vertical/undo and preedit pixel gates rerun. Build, web/Node bundle,
source/WASM hashes, JSON and screenshot retained as crlf-render-wasm-* artifacts.
Fixture now shows all named results with raw evidence collapsed, improving
reviewability. Owned tab/server closed. CPU pixel equivalence does not qualify
native GPU presentation or true OS IME.


## Multiline preferred vertical column — 2026-11-03

Regression reproduced long→short→long Up/Down losing horizontal position.
Editor now retains desired x across vertical key sequences including Shift,
while pointer hits, horizontal/command actions, insertion and focus transitions
reset it. Existing hit/grapheme mapping still selects valid boundaries. Test
checks down/up restoration, Shift anchor, pointer reset and focus reset. Native226
+standalone4 +integration15 =245 pass,3 ignored. Before/final logs/hash retained
as vertical-column-* artifacts. Fresh WASM/browser and OS input qualification
of this change remain open.


## Fresh WASM preferred vertical column — 2026-11-03

Fresh release WASM passes Node27/27 and browser16/16. Runtime-level LF/CRLF
long-short-long navigation retains exact caret x; reverse Shift movement selects
expected complete byte range, deletion/undo restores exact original text.
Prior CRLF/render/preedit gates rerun. Build log, bundle, source/WASM hashes,
JSON and screenshot retained as vertical-wasm-* artifacts. Probe/server closed.
This qualifies Runtime methods in browser, not trusted DOM multiline keyboard,
actual OS IME or GPU presentation.


## Trusted multiline DOM input and modifier correction — 2026-11-03

Actual browser textarea adapter sequence exposed a gap omitted by direct API
tests: standalone Shift keydown reset desired x before each Shift+ArrowUp.
The second arrow selected from column1 instead of7. Editor now resets desired x
only after handled horizontal/command actions; unknown modifier events preserve
it. Native226 lib tests pass (regression now interleaves Shift); fresh Node27/27.
Trusted browser End-state journal verifies two Down keys retain x, Shift+Up twice
selects exact h/newline/x/newline/abcdefg, Backspace yields abcdefgh and Meta+z
restores exact original text/selection. All9 key events have isTrusted=true.
Before/final JSON, screenshot, bundle and hashes: multiline-trusted-* artifacts.
Owned tab/server closed. Browser automation is not physical keyboard/OS IME proof.


## Logical line-edge deletion — 2026-11-03

Added TextEdit::delete_to_line_edge with selection precedence, grapheme rounding,
CRLF preservation and atomic undo. Editor command Backspace/Delete routes it;
shared/standalone word-modifier dispatch still takes precedence for Ctrl word
keys. Browser soft-line beforeinput maps to existing Rust command key method.
No visual wrapping exists here, so boundaries are logical LF/CRLF lines.

Native227 +standalone4 +line integration3 and adapter36 pass,3 ignored. Corrected
the existing composition-cancel stub to report text_editing=true: otherwise
sync correctly retired its fake nonediting session before end. No production
composition admission weakened. Logs/hashes: line-edit-* artifacts. Fresh WASM,
actual browser/native keyboard and soft-wrap support remain open.


## Fresh WASM and trusted line deletion — 2026-11-03

Fresh release WASM passes Node28/28, including command line deletion/undo/redo
for JSON-bound LF/CRLF. Trusted browser textarea Meta+Backspace/Delete removes
only the expected part of current line; each Meta+z restores exact original
text/caret. All8 key events are isTrusted=true. Other lines remain unchanged.
Build/bundle, raw JSON, screenshot and source/WASM hashes: line-wasm-* and
line-trusted-* artifacts. Owned tab/server closed. Native OS-window, actual IME,
visual soft wrap and trusted beforeinput soft-line path remain open.


## Line deletion joining graphemes — 2026-11-03

Regression reproduced line selection deletion leaving caret inside newly joined
RI flag. delete_to_line_edge now aligns caret against resulting full string,
matching other grapheme-aware replacements. Tests cover RI, ZWJ and removed-CRLF
joins in both directions, with exact undo selection/value and redo caret. Native227
+standalone4 +integration19 =250 tests pass,3 ignored; expanded join matrix rerun.
Before/final logs, cases and hashes: line-join-* artifacts. Fresh WASM/browser
qualification of this latest fix remains open.


## Fresh WASM joined-cluster caret — 2026-11-03

Fresh release WASM passes Node29/29 and browser17/17. New Runtime API gate
selects X between RI or ZWJ neighbors, removes it via both command directions,
checks the next Shift+Left selects the complete joined cluster, and verifies
undo restores original X selection plus redo restores joined result. Build,
bundle, hashes, JSON and screenshot retained as line-join-wasm-* artifacts.
Owned tab/server closed. This verifies editing boundaries, not ZWJ visual
ligature shaping, trusted keyboard for this fixture or actual OS IME.


## Browser multiline accessibility metadata — 2026-11-03

Added additive Runtime::control_multiline (false for missing/noneditor controls)
and DOM aria-multiline synchronization from focused Rust editor. Previously both
single/multiline inputs exposed the same textarea semantics. Runtime integration5
and adapter37 pass, including false→true→false focus transitions; WASM compile
passes. Logs/source hashes: multiline-a11y-* artifacts. Fresh WASM/browser AX
inspection remains required. Readonly editing semantics and screen-reader/native
accessibility qualification are still open; this does not claim full a11y parity.


## Fresh WASM multiline browser AX — 2026-11-03

Fresh WASM Node30/30 and4 real focus transitions pass: single/multi/single/multi
Rust metadata equals DOM aria-multiline. Browser Accessibility.getFullAXTree
confirms focused textbox multiline=true. Project fixture adapts load_component
transport to real Rust load_ui_files_json because legacy Button syntax does not
accept arbitrary multiline prop/template comparison. No semantic/editor stub.
Initial unsupported-format diagnostic retained. JSON/AX/screenshot/bundle/hashes:
multiline-a11y-* artifacts. Owned tab/server closed.

New gap observed: textarea transport stays empty, so browser AX lacks current
Rust field value. Value/selection accessibility synchronization is the next
concrete task. Readonly and screen-reader/native platform qualification remain
open; multiline property alone is not full accessibility parity.


## Browser accessibility value/selection mirroring — 2026-11-03

Textarea now mirrors focused Rust value and selection outside composition.
Removed post-beforeinput/end unconditional blanking that erased accessibility
value. Runtime::text_selection_utf16 exports anchor/caret in DOM UTF16 units,
subtracting CRLF pair normalization; DOM newline normalization preserves model
bytes and avoids repeated assignments. Direction is retained with setSelectionRange.
Active composition DOM is untouched. Adapter38 and runtime integration6 pass,
including emoji/combining/reverse selection and CRLF offsets; WASM compile passes.
Logs/source hashes: a11y-value-* artifacts. Fresh WASM/browser AX and trusted input
regression remain required. Accessibility-driven DOM selection→Rust is not yet
implemented; screen-reader parity remains open.


## Fresh WASM accessibility value/selection — 2026-11-03

Fresh release WASM Node31/31 passes, including DOM UTF16 CRLF/emoji offsets.
Four browser focus transitions mirror Rust value; trusted select-all and Unicode
insertion produce identical Rust/DOM values at every event. Two Shift+Left keys
select combining and emoji clusters with Rust [5,1] and DOM [1,5,backward]. All12
logged input events are isTrusted=true. Browser AX tree now exposes value A😀é.
JSON/AX/screenshot/bundle/source hashes retained as a11y-value-* artifacts. Probe/
server closed. DOM accessibility selection→Rust, screen-reader/platform actions
and true OS marked-text IME remain open.


## DOM selection import to Rust — 2026-11-03

Added Runtime::text_set_selection_utf16 backed by Rust Editor mapping normalized
textarea offsets to bytes. Surrogate interior endpoints round outward for ranges,
collapsed caret rounds forward, CRLF normalization maps to original bytes, and
selection expands to grapheme boundaries preserving direction. Clamp/noop/history
behavior tested; no committed edit record is created. Editor resets preferred x
and blink only on changed selection. DOM select handler adopts only active,
noncomposing, value-consistent selection differing from authoritative mirror.
Mirror range updates are skipped when already equal, preventing feedback loops.
Runtime integration7 and adapter39 pass; WASM compile passes. Evidence/hash:
dom-selection-* artifacts. Fresh browser event-order/accessibility selection
and screen-reader/platform qualification remain open.


## Fresh WASM DOM selection import — 2026-11-03

Fresh WASM Node32/32 passes. Browser fixture button calls setSelectionRange
without losing textarea focus; real browser select imports [1,5,backward] into
Rust [5,1]. Trusted X insertion yields AX in both models, Meta+z restores original
A😀é and reverse selection. Twelve observed events are finite, values agree
throughout, no sustained feedback loop observed. Initial mirror transitions
produce4 queued select notifications; these are noops against current Rust state.
JSON/screenshot/bundle/hashes: dom-selection-* artifacts. Probe/server closed.
This is real browser select-event integration, not a screen-reader action or
universal event-order/platform qualification; OS IME remains open.


## Pending DOM selection before editing — 2026-11-03

Before/after adapter regression reproduced beforeinput editing stale Rust range
when native select notification had not arrived. Adapter now adopts current
value-consistent DOM range before beforeinput, keydown and clipboard actions.
Adapter40 pass. Real browser same-task setSelectionRange plus synthetic
beforeinput yields AX (not append); trusted Meta+z restores A😀é and [5,1]
reverse selection. Existing fresh WASM unchanged. Logs/JSON/screenshot/hashes:
selection-order-* artifacts. Probe/server closed. Synthetic insert is deliberate
for ordering; this is not screen-reader or universal OS IME qualification.


## Composition captures pending DOM selection — 2026-11-03

Before/after regression reproduced composition start locking transport before
adopting pending DOM selection. Start now adopts range before setting composing
and capturing owner; active-composition guards remain. Adapter41 pass. Browser
same-task reverse range plus synthetic composition replaces selected emoji/
combining suffix with 新 (A新); trusted undo restores exact original value and
reverse range. Existing WASM unchanged. Journal microtasks see final same-task
state, so per-stage protection is unit-tested separately. Logs/JSON/screenshot/
hashes: composition-selection-* artifacts. Probe/server closed. True OS IME and
screen-reader/platform qualification remain open.


## Stable editor accessible names — 2026-11-03

TextInput now accepts label, forwarded through builtin primitive emission to
ContentInput/Input spec. Parser duplicate-property tracking includes label.
Runtime::control_input_label resolves explicit label, placeholder, then stable
key; current value is excluded. Native accessibility tree and browser textarea
use this name separately from live value. Existing control_label diagnostic API
is unchanged. Tests verify name survives editing and native tree reports changed
value separately. Native227 +integration8 and adapter42 pass,3 ignored; WASM
compile passes. Logs/hashes: input-label-* artifacts. Fresh WASM/browser AX name
stability and screen-reader/platform qualification remain open.


## Fresh WASM stable AX name — 2026-11-03

Fresh WASM Node33/33 passes, including stable label across bound edit/rebuild and
separate native tree value. Browser4 focus transitions expose Search/Notes labels;
trusted select-all/type Changed keeps label Notes at every event. Browser AX
snapshots confirm name Notes before/after while value changes A😀é→Changed.
JSON/AX/screenshot/bundle/hashes: input-label-* artifacts. Probe/server closed.
Screen-reader spoken output/platform actions and actual OS IME remain open.


## Accessibility snapshot JSON escaping — 2026-11-03

Independent serde_json parse regression reproduced invalid escape in manually
formatted accessibility_tree_json containing NUL/ESC. Replaced Rust Debug string
formatting with serde_json object serialization, preserving existing schema
fields. Roundtrip confirms quote/backslash/NUL/ESC/tab/newline value, stable label
and focused node relationship. Native227 +integration9 pass,3 ignored; WASM
compile passes. Before/final logs/source hashes: a11y-json-* artifacts. Fresh
WASM execution and snapshot performance qualification remain open.


## Fresh WASM accessibility JSON execution — 2026-11-03

Fresh release WASM Node34/34 passes. Expanded JavaScript JSON.parse oracle loads
all32 C0 characters plus quote/backslash/emoji through JSON state, in both editor
value and bound label. Accessibility snapshot roundtrips exact strings and
focusedId relationship. Build/bundle/source/WASM hashes and JSON retained as
a11y-json-wasm-* artifacts. This closes Node WASM execution gate; no fresh
browser/screen-reader or serialization performance claim is made.


## Readonly TextInput semantics — 2026-11-03

Added readOnly through builtin TextInput forwarding, ContentInput parsing and
Rust Editor spec. Readonly retains focus/navigation/selection and copy value;
insertion, deletion (plain/word/line), undo/redo and nonempty preedit are refused.
Programmatic binding replacement remains available. Existing history is retained
while readonly and works after reenabling. Runtime::control_readonly exposes
state; browser sets native readOnly and aria-readonly. Native IME admission is
false for readonly editors while keyboard selection path stays active. Semantic
A11yNode now includes read_only, JSON readOnly, and omits set_value action.

Native228 +standalone4 +integration10 and adapter43 pass,3 ignored. WASM compile
passes. Logs/hashes: readonly-input-* artifacts. Fresh WASM/browser AX/trusted
readonly input and actual OS-window/platform qualification remain open.


## Fresh WASM readonly browser qualification — 2026-11-03

Fresh WASM Node35/35 passes readonly editing/selection/semantic action gates.
Browser trusted select-all works; trusted X insertion (including beforeinput),
Backspace, Meta+Delete and Meta+z leave Rust/DOM value and selection unchanged.
AX tree confirms focused multiline textbox readonly=true, label Notes and value
A😀é. JSON/AX/screenshot/bundle/hashes: readonly-* artifacts. Probe/server closed.
Actual native OS-window/IME and screen-reader spoken output remain open.


## Live readonly binding transition — 2026-11-03

Real Runtime Toggle-bound readOnly integration passes for both schedulers.
Committed seed! edit is synchronized, preedit started, then Toggle reconstructs
readonly editor. Value/history remain; editor focus works but native editor
session is absent and insertion/undo are refused. Reenable reconstructs editable
session; original undo restores seed and bound state, with no preedit/blocked
payload. All11 line/editor integration tests pass. Evidence/hash: readonly-live-*
artifacts. No production source change. Actual OS IME delivery and browser live
readonly-transition qualification remain open.


## Browser live readonly reconciliation — 2026-11-03

Actual preview source reconstruction retains edited seed! across Lock/Unlock.
Trusted X/Backspace/Meta+z while locked do not change Rust/DOM; unlock exposes
editable state and original undo restores seed/caret4. All12 observed events
retain focus0, matching Rust/DOM values and readonly flags. Browser fixture
changes source while native integration separately covers bound Toggle state.
Raw JSON/screenshot/source/WASM hashes: readonly-live-browser-* artifacts.
Owned tab/server closed. Actual OS IME and screen-reader/platform actions open.


## Native semantic multiline and selection — 2026-11-03

A11yNode/native JSON now includes multiline and selectionUtf16 anchor/caret.
Offsets correspond to the node's original value, retaining CRLF; DOM selection
API separately uses textarea-normalized offsets. Regression verifies original
A😀CRLFé reverse selection [7,5] vs DOM [6,4], exact value, multiline flag and
noneditor null selection. Native228 +standalone4 +integration12 pass,3 ignored;
WASM compile passes. Logs/hashes: native-semantic-selection-* artifacts. Actual
AccessKit/OS screen-reader integration and fresh WASM execution of new fields
remain open. JSON additions are additive; downstream native adapters must use
original-value units rather than the DOM-normalized selection API.


## Consolidated input/native and semantic WASM gate — 2026-11-03

Exact locked native --lib --tests run passes315 tests,9 ignored (including6
explicit GPU tests). Covers all binary/integration targets, allocation/retention,
resize/artifact/cache and both Unicode corpora. Fresh release WASM Node36/36
passes, including original-value semantic selection [7,5] versus DOM-normalized
[6,4], multiline/readonly metadata. Logs, bundle and source/WASM hashes:
input-full-* and native-semantic-wasm-* artifacts. This is local macOS/native
plus Node WASM evidence, not ignored hardware/cross-platform/OS IME/screen-reader
or universal performance/RSS qualification.


## Current Metal input/render regression — 2026-11-03

Explicit hardware execution on Apple M4 Max / Metal Direct Native Runtime:
all6 runtime_gpu tests pass (retained clipping/scroll/fractional DPI, animation,
analytic text and retained editor/summary). Two ignored lib GPU tests also pass:
retained resource reuse, health polling after queue drain without extra draw,
synthetic encoder-validation latch and Reveal126 comparisons (max1/255 within
budget2 at1/1.25/2x). Logs/source hashes: input-metal-* artifacts. This closes
execution of8 GPU regression tests on this local Metal adapter; ignored font
flattening probe was not run. Physical device faults, OS presentation/input
latency, RSS/energy and other platforms remain open.


## Actual macOS NativeHost readonly keyboard — 2026-11-03

Added instrumented native_readonly_probe (synthetic fixture, append JSONL logs)
and ran actual OS-window CUA input on current debug build. Readonly Unicode
reference remains exact after select-all/x/backspace/undo. Tab editable insertion
produces Editable!, undo/redo gives Editable→Editable!; ShiftTab readonly selection
and F7 CPU presentation preserve both rows while rejecting further edits. GPU
and CPU presents observed,0 GPU errors. Screenshots/raw/source/binary hashes:
native-readonly-* artifacts. Owned process verified absent after window close.

Initial diagnostic log was overwritten when a post-close AX observation relaunched
the app; final run repeated after append logging fix, saved before closing and
verified separately. No IME events observed: actual marked-text OS IME remains
open. This is shared NativeHost, not standalone-window or native screen-reader
qualification. Debug instrumentation provides no latency/RSS/energy evidence.


## Native dead-key diagnostic and explicit IME gate — 2026-11-03

Attempted actual macOS Option+E in focused editable probe. It produced ordinary
U+045E text (Editableў), not marked composition; IME event count0. Native undo
restored Editable. Screenshot/session raw retained as native-ime-deadkey-
diagnostic-*; probe process verified absent. This attempt does not qualify IME.

Added qualify-native-readonly.py: checks immutable reference, full readonly
selection, editable sibling change, GPU errors, optional CPU+GPU presentation,
and explicit --require-ime Preedit/Commit gate. Prior readonly smoke passes;
diagnostic fails required nonempty Preedit/Commit (and lacks full readonly
selection in that narrower run). Verifier JSON saved. Actual input-method
composition lifecycle still requires suitable OS source; no system source was
changed. Journal validation alone does not prove physical input or screen-reader
behavior.


## Direct semantic snapshot serialization — 2026-11-03

A11yRole/Node/Tree now serialize directly with serde, eliminating intermediate
JSON Value nodes. Explicit names/skipped numeric_value preserve field contract;
bounds widen f32→f64 to preserve legacy JSON numeric strings. Parsed complete
value oracle and fractional numeric-text oracle pass. Allocation counts for
1/32/256 controls:136→14,2124→204,16463→1551; JSON lengths580/9285/73302 unchanged
in those fixtures (key order may differ). Frozen7*controls+32 allocation ceiling
rejects the prior path. Full native317 pass,9 ignored; WASM compile passes.
Before/after source snapshots/logs/hashes: a11y-serialize-* artifacts. Counts are
allocator operations, not timing/RSS/energy; fresh WASM execution remains open.


## Fresh WASM direct semantic serializer — 2026-11-03

Fresh release WASM Node37/37 passes. JavaScript verifies fractional bounds equal
Math.fround originals widened to f64, expected semantic field set, noneditor null
selection and omitted legacy numericValue; existing control-character/label/value,
readonly/selection and editor suites rerun. Build/bundle/source/WASM hashes and
JSON: direct-a11y-wasm-* artifacts. This closes Node WASM execution of direct
serializer; no new browser/screen-reader or timing/RSS/energy claim.


## Semantic editor epoch for DOM mirror — 2026-11-03

Editor now owns a process-wide u32 semantic epoch, refreshed for explicit model/
selection operations and new editor construction. Blink/preedit visual changes
do not update it. Runtime::text_model_revision exposes the focused epoch without
allocating. Browser mirror caches value/selection by Runtime, focus index and
epoch; stale DOM can still be repaired from cache, active composition stays
untouched, and hidden/destroyed inputs release cache. Old runtimes without the
getter retain eager synchronization. External binding replacement receives a
fresh epoch even if cursor/index are unchanged.

Adapter44 passes including20 real resize/sync dispatches with exactly1 value and
selection fetch, then refresh on changed epoch. Full native318 pass,9 ignored;
WASM compile passes. Logs/hashes: input-revision-* artifacts. This is call-count
evidence, not latency/RSS qualification. Fresh WASM/browser cache integration
remains open.


## Fresh WASM semantic mirror cache — 2026-11-03

Fresh WASM Node38/38 and real browser3 cache cases pass. Instrumented subclass
calls actual inherited Rust getters;4096-char+emoji field needs zero additional
value/selection fetches across20 unchanged resize/syncs. Real text edit refreshes
mirror once; visual preedit does not advance epoch or copy unchanged value.
Trusted Shift+Left/Y/Meta+z then preserves exact Rust/DOM tail and selection.
Live journal getters contribute to final totals; frozen case deltas are measured
before that journal. JSON/screenshot/bundle/source/WASM hashes: input-epoch-*
artifacts. Probe/server closed. Call counts are not latency/RSS/energy proof;
OS IME/screen-reader/platform qualification remains open.


## DOM selection adoption mirror reuse — 2026-11-03

Selection adoption now reuses cached value/selection only when Runtime identity,
focused index and semantic epoch all match. Missing revision support or stale
cache retains direct Rust getters. Composition and stale DOM value guards remain.
The regression failed before the fix; all 45 adapter tests pass after it and were
rerun when finalizing evidence. Real browser/WASM fixture passes 4/4 cases:
20 unchanged modifier/select pairs add zero value/selection getter calls. Trusted
Shift+Left, Y and undo preserve exact DOM/Rust value and reverse selection.
Before/after logs, browser JSON/screenshot and source/WASM SHA256 are preserved
in adoption-cache-* artifacts. Browser journal getter overhead is excluded from
frozen case deltas. This qualifies call counts, not latency/RSS/energy. Actual
OS marked-text IME, accessibility platform bridge and remaining PLAN gates stay open.


## Clipboard cut transport failure safety — 2026-11-03

Adapter previously deleted selected Rust text even when clipboardData was absent.
Cut now requires setData transport and a write call returning without exception
before deletion; a thrown write is contained and leaves text intact. Regression
failed before the fix; adapter46 passes after it, including successful write then
one deletion. Evidence: cut-transport-* logs/qualification. This verifies adapter
ordering and failure preservation, not actual OS clipboard persistence.


## Empty/unavailable insertion transport safety — 2026-11-03

Paste previously substituted an empty string for missing clipboard transport;
Rust interprets empty insertion as selection deletion. Adapter now prevents
default paste, requires a nonempty plain-text string and catches clipboard read
errors before adopting selection or mutating text. insertText/replacement with
null or empty data similarly does not delete selection. Explicit delete actions
and cut retain their existing semantics. The new regression failed before the
fix; all47 adapter tests pass, including Unicode paste and nonempty replacement.
Raw logs and source hashes: paste-transport-* artifacts. Actual OS clipboard,
real browser platform coverage and accessibility remain open.


## Real browser/WASM clipboard failure safety — 2026-11-03

preview-clipboard-browser.html passes10/10 against actual Rust WASM and current
adapter in IAB. Missing/throwing cut and paste transports, empty paste and null
insertion retain text plus exact selection; Unicode paste and successful cut
mutate the model, and each undo restores original value/selection. JSON, screenshot
and source/WASM hashes: clipboard-browser-* artifacts. Events are synthetic DOM
events, not actual OS clipboard. Owned browser tab/server closed after capture.


## Native empty paste preservation — 2026-11-03

Shared native clipboard adapter also passed an empty successful read to
text_insert, deleting selection. Empty paste now leaves editor untouched.
Regression failed before fix; after fix full native lib/tests suite passes
319 tests, 9 ignored. Regression checks selection, bound query, untouched sibling
and preexisting undo/redo. Both native hosts call this adapter. Evidence and
source hash: native-empty-paste-*; injected reader is not OS clipboard proof.


## Reject malformed native clipboard encoding — 2026-11-03

Native clipboard readers used lossy UTF8 decoding, allowing corrupt bytes to
replace selected text with replacement characters. All readers now share strict
UTF8 decoding and propagate InvalidData; paste preserves original text and
selection. Regression failed before fix, then all6 native clipboard tests pass.
Invalid lead, truncated and overlong sequences plus valid CJK/emoji are covered.
PowerShell read command explicitly requests UTF8 output; Windows execution remains
unqualified. Logs/hash: clipboard-utf8-* artifacts. No actual OS transport or
full platform qualification claim; latest full native319 run predates this change.


## Clipboard writer subprocess ownership — 2026-11-03

All native clipboard writers now share write_clipboard_child: close stdin and
wait even after a write error or missing stdin. Previously early ?/fallback
paths skipped child reaping. Unix test launches actual child processes for
success, early nonzero exit/broken pipe and missing pipe; waitpid returns ECHILD
after helper, proving reaping. Full native suite after UTF8 and lifecycle changes:
321 passed, 9 ignored. Evidence/source hash: clipboard-child-* artifacts.
Timeouts, synchronous UI latency and actual cross-platform clipboard remain open.


## Clipboard mutation invalidates pending composition — 2026-11-03

Successful paste/cut previously mutated Rust while retaining browser composition
ownership, allowing late compositionend to append another commit. Clipboard
mutations now cancel that ownership before text_insert; copy and unavailable
paste keep the session. New tests use an actively editing fixture, reproduce
late commit before fix, and pass after it. Adapter49 passes; clipboard-composition-*
preserves logs/hashes. Actual browser/OS IME ordering remains unqualified.


## Browser clipboard/composition mirror regression — 2026-11-03

Expanded actual-browser/Rust-WASM fixture exposed a missed DOM regression:
late compositionend after paste correctly refused Rust mutation but cleared
textarea value. Nonowned end now clears ownership only if still composing and
resynchronizes the mirror. Failed browser JSON and failing adapter regression
are preserved; after fix adapter50 and browser14/14 pass. Checks include paste/
cut late commits, retained composition across copy/refusal, exact DOM/Rust value
and undo selection. Evidence: clipboard-composition-browser-* and
late-composition-mirror-*; synthetic DOM events do not qualify actual OS IME.
Owned tab/server closed after screenshot capture.


## Semantic epoch exhaustion without cache-key reuse — 2026-11-03

Global u32 editor epoch previously wrapped and reused old keys. Allocation now
saturates atomically; after the last unique nonzero key it permanently returns
zero. Zero disables cache reuse for both mirror synchronization and selection
adoption, preserving correctness at the cost of copies after exhaustion.
Injected MAX-1 boundary test failed before fix, passes after; adapter52 passes
including changing values under zero and stale DOM selection rejection. WASM
target check passes. Evidence/hashes: epoch-exhaustion-*; no fresh release WASM
or browser execution for this change, latest full native321 predates it.


## Fresh release WASM and native epoch/clipboard integration — 2026-11-03

Rebuilt release WASM after epoch exhaustion fix. Node38, adapter52, full native
322 passed/9 ignored; browser clipboard14 with real epoch plus14 forced-zero
fallback and mirror4 pass. Forced-zero subclass only overrides epoch getter;
actual inherited Rust operations/value/selection remain in use. Saturation itself
is proven by native injected-counter boundary test, not billions of browser edits.
Build logs, report JSON, screenshot, node/web WASM archive and hashes preserved
in epoch-release-* artifacts. Server report path was reused across fixtures;
separate mirror/zero reports saved and clipboard rerun last to retain correct
final clipboard JSON. Owned probes/server closed. Actual OS IME/clipboard,
platform accessibility and latency/RSS/energy gates stay open.


## Semantic accessibility selection action — 2026-11-03

TextInput semantic nodes advertise set_text_selection. New exported
accessibility_set_text_selection accepts snapshot-local node ID and original-value
UTF16 offsets, retains CRLF units, rounds to grapheme boundaries, focuses accepted
editor and clears preedit. Readonly permits selection; disabled/modal-blocked,
noneditor/unknown nodes refuse without focus theft. Accepted no-op returns true.
Three integrations pass: CRLF/reverse/surrogate/readonly roundtrip, refusal, modal
scope plus prior undo history. Full native324/ignored9 passed before adding
third test; final focused3 and WASM target check pass. Evidence: a11y-selection-*.
OS accessibility bridge/screen reader and fresh release-WASM execution remain open.


## Fresh release WASM semantic selection execution — 2026-11-03

New action executed through freshly generated wasm-bindgen Node bindings:
all40 checks pass, including readonly/original CRLF/reverse selection, surrogate
rounding, u32::MAX clamp, invalid/disabled/noneditor refusal without focus theft.
Old WASM negative run fails the two new checks because action/schema was absent;
that record is preserved as an API-presence check, not an old semantic bug claim.
Full final native suite 325 passed/9 ignored includes all3 new integrations.
Node/web bundles, reports, build/native logs and hashes: a11y-action-* artifacts
and a11y-selection-fresh-wasm JSON. OS bridge and screen reader remain open.


## AccessKit native translation foundation — 2026-11-03

Added optional native accesskit=0.25.2 and native_accessibility module. Runtime
semantic trees translate to actual AccessKit TreeUpdate: hierarchy, roles,
multiline input, label/value, bounds, numeric value, readonly/disabled/modal
state and focus. Focus/Click dispatch verifies advertised semantic action before
Runtime admission. Focused real-editor test covers readonly Unicode metadata,
focus, disabled/unknown/unadvertised action refusal; native and WASM target
checks pass. Evidence: accesskit-* artifacts. This is partial bridge work:
window adapter/event loop, transforms, text-position descendants, selection/
value/range OS actions and actual AX/screen reader inspection remain open.
Adapter lifecycle reference: https://docs.rs/accesskit_winit/0.34.2/accesskit_winit/struct.Adapter.html


## Shared native AccessKit window adapter and actual macOS AX — 2026-11-03

Added optional accesskit_winit=0.34.2. Shared host creates initially hidden window,
constructs adapter, then shows window. Adapter receives window events first;
custom event-loop events deliver activation/deactivation and ROOT-tree actions.
Active trees update from retained chooser/application runtime after events and
redraw, with root DPR transform. Focus/Click route through app update and native
IME ownership guards. Inactive accessibility avoids snapshot construction.
All-targets check and host18 pass; full native326/ignored9 passes.
Actual M4 Max/macOS probe via CUA exposes Reference/Query text fields with values
in OS AX; clicking Query and native End/Shift+1 changes value to Editable!, which
updates live in AX. AX text, screenshot, raw app journal and binary/source hashes
saved in accesskit-native-* / accesskit-host-*; process closure verified by pgrep.
This proves basic shared-host macOS AX integration, not spoken screen-reader
output or SetValue/text-selection/range support. Those, other OS backends,
standalone parity, stale snapshot IDs and incremental allocation costs remain open.


## AccessKit text SetValue and actual macOS AX mutation — 2026-11-03

Added Runtime accessibility_set_text_value and AccessKit Value payload routing.
Editable text nodes advertise SetValue; readonly/range nodes do not. Full-value
replacement/empty clear is one undoable edit, normalizes CRLF/single-line breaks,
synchronizes through normal app binding update. Admission rejects invalid,
readonly, disabled/modal and oversized requests before changing focus/selection;
raw-size bound avoids unbounded normalization allocation. Same-value action keeps
selection and history. Missing/wrong AccessKit payload is refused.
Actual macOS CUA AX setter changes Query to AX 你好🙂; AX reflects it; native
Cmd+Z restores Editable. Readonly Reference remains unchanged on AX setter attempt.
CUA may focus target first, so only text refusal is claimed for that OS attempt.
AX snapshots/screenshot/raw journal/binary hashes: accesskit-set-value-* artifacts.
Process closure verified; full native329/ignored9 and WASM target check pass.
Fresh release WASM execution, OS selection/range and screen-reader speech remain open.


## AccessKit text runs and actual macOS AX selection — 2026-11-03

TextInput now exposes per-line TextRun descendants with grapheme UTF8 byte lengths
and mapped selection. CRLF counts as one grapheme, final newline has empty trailing
run; synthetic IDs occupy a separate high-bit namespace. SetTextSelection validates
run ownership and character index before converting back to original-value UTF16
and Runtime selection. Unsupported >255-byte grapheme disables text-run selection
without inventing split grapheme units. Native4 focused tests pass; full native
331/ignored9 passes. Actual macOS AX selects readonly 你好 and editable dit;
native Shift+1 replaces selected dit with ! (E!able); Cmd+Z restores Editable
and selected dit. Raw journal/AX/screenshot/binary hashes: accesskit-selection-*.
Process closed. No speech/magnifier geometry/other-OS or actual multiline OS matrix
claim. Word starts, character positions/widths, same-editor IME interleaving and
stale snapshot ID protection remain open.


## Same-editor AX/IME ownership cancellation — 2026-11-03

Factored shared host accessibility_action dispatch. Regression reproduces late
IME commit replacing an AX-selected range in unchanged editor (seed -> slated).
Accepted SetValue/SetTextSelection now cancels nonempty or empty-pending IME
ownership and latches late-commit refusal; fresh nonempty preedit opens a new
session. Refused actions retain IME. Eight combinations cover both actions,
nonempty/empty pending preedit and accepted/refused payload; repeated stale
commits and fresh recovery checked. Host19 pass; before/after logs and source
hash: accesskit-ime-* artifacts. Injected host events are not actual OS IME
qualification; full native331 run predates this change.


## Published AccessKit IDs and scene/text retirement — 2026-11-03

Shared host now maps all semantic/text-run IDs to monotonically allocated
window-lifetime IDs. Old scene requests refuse before and after new publication;
removed IDs are pruned, changed text receives new run IDs, and selection against
unpublished changed text refuses. Existing unchanged IDs remain stable. Native
a11y identity is separate from IME instance identity: internal binding rebuild
retains it only for unchanged topology and uniquely keyed index-preserved controls;
explicit reload/ambiguous structure retires identity. No counter wrap is allowed.
Initial OS diagnostic showed overbroad scene rotation invalidated AX handles
after successful bound SetValue; that raw/AX diagnostic is retained. After identity
refinement, actual macOS SetValue retains Query handle, selection of 你好 on new
value works, native input yields AX !🙂 and undo restores value plus selection.
Accessibility7, host19, full native335/ignored9 and WASM check pass. Logs/
raw/AX/screenshot/hashes: accesskit-stable-ids-*; probes closed. Queued stale
actions are verified at Rust dispatcher, not OS queue injection. Full-tree
allocation costs, speech, other OS and marked-text IME gates remain open.


## Skip unchanged active AccessKit publications — 2026-11-03

PublishedTree compares semantic snapshot, scene identity and DPR before text-run/
ID translation and OS publication. Blink/preedit-only visual changes no longer
rebuild/send trees. Selection/value/disabled/source/DPR changes still publish;
activation explicitly invalidates cache so reactivation gets full tree. Measured
allocation calls per warmed update for1/32/256 controls: full40/774/6038 versus
unchanged10/196/1540. Snapshot construction still allocates and one previous
snapshot remains retained; no latency/RSS/energy claim. Exact no-publication and
invalidation tests pass; full native337/ignored9 passes. Evidence/source
hashes: accesskit-unchanged-* artifacts. Actual OS publication-suppression rerun
remains open; earlier AX integration evidence predates this optimization.


## Actual macOS AX after unchanged-publication optimization — 2026-11-03

Completed pending OS rerun using probe rebuilt from the same source hashes as
native337/allocation qualification. AX SetValue changes Query to Cache 你好🙂;
AX selection of 你好 is visible without changing value, native Shift+1 replaces
only that range (Cache !🙂), and Cmd+Z restores value plus selected 你好. Readonly
Reference still permits CJK selection and refuses native insertion. Raw journal,
AX snapshots, screenshot, build log and binary/source hashes: accesskit-cache-os-*
artifacts. Owned process closed and absence checked with pgrep. This closes basic
OS regression after cache optimization; physical DPR/deactivation/reactivation,
speech/marked-text IME and latency/RSS/energy remain open.


## Full text shaping, BiDi, ZWJ emoji, dictionary word segmentation, soft wrapping, and full AccessKit bridge — 2026-11-06

- **Native Accessibility (`AccessKit`)**:
  - Extended `Runtime` and `native_accessibility` with semantic actions for `A11yRole::Slider` (`Action::Increment`, `Action::Decrement`, `Action::SetValue` accepting normalized `[0.0, 1.0]` or percentage strings), `A11yRole::ScrollView` (`Action::ScrollUp`/`ScrollDown`/`ScrollLeft`/`ScrollRight` isolated under active modals), and `A11yRole::Dialog` (`Action::Collapse` dismissal and `accesskit::Live::Polite` live-region announcements for screen-reader speech output).
  - Integrated `accesskit_winit::Adapter` and `PublishedTree` into the standalone `forma` binary (`crates/forma/src/main.rs`), cancelling active IME composition when an accepted `SetValue` or `SetTextSelection` action mutates the active editor.
  - Added a `450ms` timeout with child-process reaping in `native_clipboard.rs` so external clipboard helpers (`pbpaste`/`pbcopy`/`wl-paste`/`xclip`/`xsel`/`powershell`) never block the UI thread.
- **Text Completeness (`text.rs`, `control_state.rs`, `text_input.rs`, `runtime.rs`)**:
  - Added zero-advance cluster attachment in `shape_text` for ZWJ emoji sequences (`👨‍👩‍👧‍👦`), Fitzpatrick skin-tone modifiers (`👍🏽`), keycap sequences (`1️⃣`), regional indicator flag pairs (`🇬🇪`), and variation selectors (`VS1..=VS16`, `VS17..=VS256`).
  - Added `UAX #9 N1` BiDi neutral resolution in `shape_text` so neutral/whitespace runs between RTL words remain within a single RTL visual run (`"אב גד"` -> `"דג בא"`).
  - Added dictionary-backed CJK/Thai compound word segmentation (`segment_words_dictionary`, `move_words_dictionary`, `backspace_word_dictionary`, `delete_word_dictionary`) in `control_state.rs` while preserving 100% Unicode 17 `UAX #29` conformance.
  - Added visual soft-wrapping layout and hit-testing (`SoftWrappedLine`, `wrap_shaped_text`, `hit_wrapped_text`, `Editor::soft_wrapped_lines`/`hit_soft_wrapped`, `Runtime::text_soft_line_count`/`text_hit_soft_wrapped`).
- **Release & Gate Verification**:
  - `cargo test --locked --manifest-path crates/forma/Cargo.toml --features native --lib --tests --bin forma`: 248 unit tests and all integration test suites pass.
  - `cargo test --locked --manifest-path crates/forma/Cargo.toml --features gpu --lib --tests -- --ignored`: all 9 hardware Metal GPU tests pass.
  - `npm --prefix packages/svet test` (113 files / 1507 tests), `npm --prefix packages/svet run test:package` (62 files, isolated consumer install & build), and `npm --prefix packages/svet run test:actions` (4 HTTP abort/streaming tests) pass.
  - `npm run gate`: all 15 gate checks pass (`tokenizer`, `version-check`, `studio-js` 585/585, `studio-build`, JS compiler/session/tools benches, `runtime-gpu`, `svet-wasm` 66/66, `binding-native`, `language-example`, `native-host`, `native-x86_64-unknown-linux-gnu`, `native-x86_64-pc-windows-msvc`, `studio-legacy`, and `wasm-provenance`).
