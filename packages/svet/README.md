# Svet Engine (`svet-engine`)

**Svet** is a hybrid **TypeScript + Rust WebAssembly + WebGPU (UMA/DUMA)** UI framework under active development. Qualification scope and remaining work are tracked in [DEVELOPMENT.md](DEVELOPMENT.md).

---

## Core Architecture & Advantages

1. **Zero Virtual DOM & $O(1)$ Surgical DOM Updates ([`src/engine/dom.ts`](src/engine/dom.ts))**
   - Component functions execute **once** upon mount.
   - Signals bind directly to individual `Text` nodes, attributes, styles, and two-way form inputs (`model={signal}`).
   - Global Event Delegation attaches a single listener on `document` for high-frequency events (`click`, `input`, `keydown`, etc.).

2. **Rust WASM Push-Pull Reactive DAG with Slot Recycling ([`crates/svet-wasm/src/lib.rs`](crates/svet-wasm/src/lib.rs))**
   - Zero JS Garbage Collector pressure: graph nodes, edges, and dirty queues live in flat WebAssembly linear memory (`Vec<u32>`).
   - Glitch-free hybrid Push-Pull evaluation guarantees diamond dependencies (`A -> (B, C) -> D`) execute downstream effects strictly once per batch.
   - `free_list` slot recycling + hierarchical `OwnerScope` (`createRoot`, `onCleanup`) ensure zero memory growth across long-running SPA sessions.

3. **WASM $O(n \log n)$ Longest Increasing Subsequence Reconciler (`<For>`)**
   - Computes the mathematically minimal number of DOM `insertBefore` operations in native WASM linear memory (`svet_compute_lis`).

4. **Destructurable Stores (`createStore` & `createDeepStore`) ([`src/engine/reactivity.ts`](src/engine/reactivity.ts))**
   - Every property at any nesting depth is an independent WASM-backed Signal. Destructuring (`const { theme } = store.user.profile`) retains 100% reactivity without `.value` boilerplate.

5. **WASM Tensors and WebGPU Uploads ([`src/engine/gpu.ts`](src/engine/gpu.ts))**
   - `GpuTensorSignal` exposes direct JavaScript views of WASM linear memory with allocation recycling and lifetime checks.
   - Both `UMA` and `DUMA` policies use `queue.writeBuffer` to copy data into allocated device buffers. Browser physical-memory sharing and zero-copy GPU access are not established by these labels.
   - The uploader preserves full arena contents when a buffer grows or a device changes. DUMA transfers aligned dirty spans; telemetry counts submitted bytes, including alignment.
   - `executeGpuVectorFma` executes WASM SIMD multiply/add with separate float32 rounding. `dispatchGpuComputeKernelAsync` executes WGSL on WebGPU with mapped readback, cancellation and validation. The synchronous `dispatchGpuComputeKernel` remains a host-fallback compatibility API.

6. **SSR (`renderToString`), DOM Binding Resumption (`resumeContainer`), & Optimistic RPC (`createServerAction`) ([`src/engine/resumable.ts`](src/engine/resumable.ts))**
   - Synchronous isolation via `runWithIsolatedContext()` uses a separate `WebAssembly.Instance`. For `await`, the Node-only `svet-engine/server` entry provides `runWithIsolatedContextAsync()` and `renderToStringAsync()`.
   - Restores signals and DOM bindings without re-running components. Work scales with signal and binding counts; annotation fallback scans the container.

7. **Complete Application Primitives ([`src/engine/advanced.ts`](src/engine/advanced.ts))**
   - Hierarchical Dependency Injection (`createContext`, `useContext`)
   - Lifecycle-aware Portals (`<Portal mount={...}>`)
   - Async Resources (`createResource`), `<Suspense>`, and recoverable `<ErrorBoundary>`
   - Signal-backed Router (`createRouter`, `<RouterView>`, `<Link>`) with dynamic route parameters (`/users/:id`)
   - Vite Auto-Reactive JSX Compiler (`svetVitePlugin` in [`src/engine/compiler.ts`](src/engine/compiler.ts))

---

## Commands

```bash
# Build Rust WASM core and embed into TypeScript
npm run build:wasm

# Run full Vitest suite
npm test

# Verify native fetch cancellation/concurrency against a loopback HTTP server
npm run test:actions

# Serve the manual browser HTTP fixture on loopback port 5189
npm run dev:actions

# Serve Node SSR -> browser resumption checks on loopback port 5189
npm run dev:resume

# Start interactive Vite dev server
npm run dev

# Build production bundle
npm run build
```

## Concurrent server actions

`createServerAction` keeps `pending()` true until every overlapping invocation has
settled. `error()` reflects the most recently started invocation: a late older
failure still rejects its own promise and runs its rollback, but does not replace
the newer invocation's error state. Starting an invocation clears the visible error.
Optimistic preparation is covered by error/finalization handling, including throws
before RPC begins. When RPC and rollback both fail, callers receive AggregateError
with both errors in order and the RPC error as cause. Observer failures during
failure publication do not mask that primary rejection.

Each call runs its own optimistic rollback; the helper does not serialize RPCs,
cancel older calls merely because a newer one starts, or coordinate overlapping
writes to shared application state. Design rollback callbacks to restore only the
state owned by that call.

Actions expose idempotent `dispose()` and register it with their construction
owner. RPC receives a second `AbortSignal` argument; one-argument callbacks remain
compatible. Disposal aborts active signals, rejects callers with exported
`ServerActionDisposedError`, finishes pending and releases both action signals.
Callers settle promptly even if RPC ignores abort; transport/server side effects
still require cooperative cancellation. Calls after disposal reject before reading
callback options. Optimistic rollback is suppressed after lifetime invalidation,
so any required optimistic-state cleanup belongs to the enclosing application.

Callback option getters, optimistic updates, RPC synchronous prefixes, rollback,
error normalization and abort listeners run in the action core with dependency
tracking paused in the caller and action core. Their signal reads do not subscribe
those surrounding observers. Invalidation during error string conversion prevents
subsequent rollback and rejects with `ServerActionDisposedError`. User callbacks
after `await` still require explicit continuation context capture. Native Promise
adoption accesses an RPC result's `then` getter and invokes its method in the
action core without collecting caller dependencies, including nested thenables.
Methods keep their original receiver and asynchronous invocation; native resolution
controls first settlement, and terminal result identity is preserved. Returned RPC
Promises remain observed even when the RPC has just disposed the action.

Ancestor teardown and effect-rerun cleanup deactivate descendant actions before
their individual cleanup is visited: retained calls reject before reading callback
options, applying optimistic updates or starting an RPC.

Owner disposal and graph reset cancel automatically, including calls whose RPC
never settles. Active actions subscribe to reset completion only while calls are
pending. Cancellation runs after old graph storage is cleared, so replacement
state/actions created by abort listeners survive that reset. Requests in other
WASM cores remain independent. Stale completions cannot mutate fresh reused graph
slots. Retained signal values remain frozen at their pre-reset snapshot after a
raw graph reset.

`npm run test:actions` builds the ESM package and runs native Node fetch against
an ephemeral loopback HTTP server. It checks disposal and graph reset before
headers, owner teardown during JSON streaming, transport closure and overlapping
response bodies.
The CI Svet job runs this command separately from Vitest. For browser checks,
start `npm run dev:actions` from the checkout, open
`http://127.0.0.1:5189/tests/browser/server-action-http.html` and run its checks.
The dedicated Vite harness supplies bounded test-only HTTP endpoints; ordinary
app dev servers do not. The browser fixture also verifies server-observed closure,
owner cleanup, overlapping fetches and calls after disposal. These loopback checks
do not establish remote server side-effect cancellation or all-browser parity.

SSR render helpers validate caller/request lifetimes during view evaluation,
HTML serialization and signal snapshot serialization. A reset during those phases
rejects stale output. Sync SSR resets its own isolated graph on completion, retiring
bare request signals as snapshots while preserving the caller's graph. Request
cleanup may reset its own graph after HTML/snapshot strings are complete.

The Node async renderer snapshots view metadata once and rechecks the caller after
request cleanup. Caller-owner teardown cancels its pending request through a private
abort signal; an explicit SsrRequestOptions signal remains effective too. This
closes the request graph even if user work is still awaiting. User code/transport
side effects still need cooperative cancellation; they are not forcibly stopped.
Preflight failures return rejected promises. Unawaited async work remains outside
the synchronous renderToString contract; use the Node async entry for await work.

SSR DOM serialization uses an explicit traversal stack rather than recursive
calls. Deep DOM output preserves close-tag/sibling order, internal-anchor omission,
comment escaping and live input/textarea/option state. Traversal snapshots each
node's children as it is visited and collects output chunks before joining them;
it is not a constant-memory stream or a globally immutable DOM snapshot.

## Resumed binding semantics

`resumeContainer` uses the common model binding implementation for annotated and
explicit-table model bindings: checkbox booleans, radio values, numeric conversion,
multiple-select arrays, nullable FileList models and text composition follow normal render rules. Resumed
attribute bindings share presence booleans, ARIA/data/enumerated tokens and null
removal. Type/value attributes notify existing model bindings when required.
Attribute names such as model/ref/children remain literal attributes rather than
special props; event-name and URL filtering remains in place. Disposing a resumed
container releases shared listeners, select observers, bindings and restored signals.

Resumption validates both the caller and restored scope lifetimes. Pending setup
is cancellable by disposeResumedContainer and caller teardown before its first DOM
write. Reset or disposal during binding rejects the continuation and releases the
partial scope. A newer same-container attempt supersedes an older continuation;
new independently owned bindings keep their registry entry. Nested resumptions
still belong to their calling owner and cannot escape its disposal. Manual teardown
also unregisters the caller cleanup. Malformed snapshots are rejected before an
existing live resumption is replaced. Valid construction failures do not roll back
arbitrary DOM side effects or revive an already disposed previous resumption.

From a source checkout, `npm run dev:resume` builds the ESM package and starts a
Node/JSDOM SSR fixture. Open
`http://127.0.0.1:5189/tests/browser/resumption-roundtrip.html` and run its checks.
HTML and snapshots come from the Node process; the browser parses and resumes
existing nodes, exercises form state, replaces resumption and disposes it. This is
a synchronous Node SSR/browser roundtrip fixture, not a production SSR deployment.

## Conditional view recovery

`Show` commits its truthy/falsy cache only after the new branch has rendered and
inserted successfully. A failed branch or fallback can retry on a later notified
`when` update of the same truthiness. Partial insertion is removed, branch
resources are disposed, and output invalidated by unmount/reset is rejected.
Successful non-keyed truthy branches retain their DOM identity. A reactive `when`
property getter remains tracked and is read once per observer invocation.

`For` snapshots input values and checks unique keys before rendering rows. New
rows remain provisional until preparation and insertion succeed; existing indexes
and lookup entries are published afterward. Preparation failures release staged
rows. Insertion failures remove provisional nodes and attempt to restore the
previous committed row order while keeping those row scopes alive. Later updates
can retry. Keys must be unique under Map/Set equality; duplicates throw before
changing the committed list. Lazy index signals are row-owned, disposed on removal,
and retained index accessors cannot allocate after row disposal.
Index tracking depends on reading the accessor, including callbacks with default
parameters or rest parameters; callback arity does not affect reactivity.

Rows may return `DocumentFragment`, including empty fragments and nested `Show` /
`For` views. Fragment rows keep boundary comments so moving or removing a key
covers its complete current DOM range, including nodes added after initial render.
Interrupted movement retains a temporary node snapshot for cleanup and rollback.
Single-node rows keep their marker-free path. Native LIS selects logical rows;
moving a grouped row costs one insertion per live node, including its boundaries.

## Current keyed row values

`For.children(initial, index, current)` renders once per key. `initial` remains
its first value; use `current()` inside reactive bindings or event callbacks to
read a replacement object with the same key:

```ts
For({ each: rows, keyFn: row => row.id,
  children: (_initial, index, current) =>
    h('span', null, () => `${index()}: ${current().label}`),
});
```

The lazy current-value signal belongs to the row and is disposed on removal.
Retained accessors return their final value after disposal without allocating.
Reordering the same object references does not rerun current-only bindings;
mutating properties in place does not provide deep reactivity. Failed row
preparation preserves previously committed current values. Forma loop variables
use current-value getters automatically, including nested loops, actions and
child-component prop bindings.

## Input model types

Reactive input `type` bindings apply before `model`. Changing between text,
checkbox and numeric types reapplies the current model value without recreating
the input. Checkbox models use checked/change; number/range models use numeric
value/input, with empty numeric input mapped to zero; text models use value/input
and select uses change. Radio models compare the non-null model's string value
with each option value and update checked without overwriting the option. Checked
radio change events publish the option string; unchecked/input events are ignored.
Reactive radio option values refresh selection. Event conversion follows the live native type. Listeners
and type-refresh registrations are released with their owner. External native
type changes are read on events but do not themselves trigger a reactive refresh.

File inputs use `Signal<FileList | null>`: a native `change` publishes `input.files`,
a non-null model assigns through the native `files` setter, and `null` clears the
selection with an empty `value`. Input/cancel events do not republish selection.
Paths and arrays are not FileList values and are rejected by the native setter.
Use a FileList from another input or DataTransfer when assigning files; see the
[native files API](https://developer.mozilla.org/en-US/docs/Web/API/HTMLInputElement/files).
Type changes and resumed model bindings use the same implementation. File objects
and FileList are client state, not JSON-resumable data: initialize SSR snapshots
with `null` and obtain a fresh selection on the client. SSR omits file input value
attributes and does not read selected path strings. This binding does not upload
files; transport, file validation and system-picker interaction belong to the app.

```tsx
const upload = signal<FileList | null>(null);
const picker = <input type="file" multiple model={upload} />;
// upload()?.item(0) is the first selected File; upload.set(null) clears it.
```

`select` applies its initial model after children are inserted. A multiple select
uses an array model (non-arrays select nothing) and publishes selected option
strings in DOM order on change. Array entries are compared as strings; duplicate
option values may select several options. An owner-scoped MutationObserver
reapplies the model after option insertion/removal, option text/value changes and
multiple-mode changes, at the browser mutation checkpoint. Model updates remain
synchronous. Without MutationObserver, initial construction/model updates work,
but automatic option-mutation refresh is unavailable. Observer callbacks ignore
stale graph generations and disconnect on disposal.

Native `form.reset()` and reset buttons publish reset control values to model
signals at the next microtask, after the default action. A canceled reset publishes
nothing. Values are sampled before any signal writes and committed in one batch
per reactive core. This follows HTML defaults (`defaultValue`, `defaultChecked`,
option default selection), which may differ from the initial signal values. Radio
models should allow `null` when no default option is checked; file resets publish
`null`. Successful reset ends text composition. Queued select-option observers
wait until reset publication, and resume ordinary synchronization after a canceled
reset. Teardown/graph invalidation prevents stale queued writes.

Connected forms include external controls linked by `form="id"`. Each live
`h('form')` owns reset routing even before it has model controls, supporting later
native append/move into that form without an observer checkpoint. Mount owns reset
routing on its container and initial tree root, covering open/closed shadow trees,
late native descendant forms, and an enclosing form outside the mount container.
Moving a mounted element container between shadow trees retains local reset routing.
Existing native forms are also registered when binding their controls. Shared
listeners are reference-counted and released with their owners; no extra WASM nodes
are allocated. Resumed model bindings use the same reset implementation. A completely
unmanaged detached form outside these owned areas still needs its controls bound
while associated with that form; arbitrary unmanaged trees are not intercepted.

When a Svet commit adopts a model control into another same-origin document, reset
routing moves with it. The destination document and tree root are retained before
native insertion can dispatch reset; afterward the control's actual document, form
and tree root own the leases. Portal into a foreign native form, including a closed
shadow root, follows this rule. Pending nested adoptions pin their routes separately.
Cancellation and owner/generation checks still apply to queued reset publication.
The event and model routes can share one adopted node. Raw native moves outside
Svet commits and already-adopted controls with stale routes do not trigger refresh.

Text input and textarea models defer intermediate composition input until
`compositionend`. `InputEvent.isComposing` is also respected. During a started
composition, model writes still update the signal and other observers but leave
the composing field's text untouched; the completed user text then wins and is
published to the model. A following identical input event does not notify twice.
Switching to a non-text type prevents compositionend from publishing stale text.
Composition listeners are removed with the binding. This event policy is covered
by synthetic-event tests; platform IME candidate-window behavior is not qualified.

## Reactive fragment children

An accessor child returning a DocumentFragment keeps a start/end DOM range after
the fragment is consumed. Replacing it with text, a node or another fragment
removes all live nodes in that range, including nested Show/For additions. Empty
fragments retain their position; returning the same consumed fragment preserves
its existing range. Regular text-to-text updates retain their fast path. Broken
range boundaries throw before removal begins. Existing preconstructed nodes/views
retain their original resource ownership; replacement does not transfer ownership.
Dynamic replacements prepare candidate nodes before removing the old range and
publish metadata after successful insertion/removal. Failures restore the prior
DOM and candidate fragment while the owner remains active; rollback is best effort
if native restoration/removal itself fails. Lifetime checks reject unmount/reset
during rendering, commit or rollback and remove provisional late insertions.

Accessor children may also return mixed nested arrays. Arrays flatten in order,
omit null/undefined/booleans, preserve native nodes and create reactive bindings
for nested accessors. Flattening is iterative; cyclic arrays and repeated native
nodes throw before insertion. Arrays are read again on each accessor notification,
even when the array identity is unchanged. Group replacement uses the same range
and rollback path as fragments; this is not keyed diffing or LIS optimization.
Use `For` for keyed lists. Preconstructed child resources keep their original ownership. Resources created
while rendering reactive content live in its committed content root: failed
preparation or insertion releases only the candidate root and keeps previous
bindings live. A successful replacement releases the previous content root.
Returning the same native node or consumed fragment retains its content root.
External scopes already disposed by their own owners are not resurrected.

Static array children share the iterative flattening preflight. Deep nesting does
not consume recursive stack frames; a cycle throws before child bindings or DOM
insertion begin. Shared acyclic subarrays remain valid. Accessors run after the
array tree is prepared, so their mutations to source arrays affect a later render,
not the prepared static child list. Flattening uses temporary storage proportional
to the traversed values; this is not a memory ceiling or transactional guarantee
for arbitrary getter/renderer side effects.

## ARIA and data boolean values

Boolean values in `aria-*` and `data-*` attributes serialize as the strings
`"true"` and `"false"` in HTML and SVG, including SSR output. Reactive null or
undefined values remove the attribute. Native presence booleans such as disabled
keep presence/removal behavior. For example, `aria-pressed={false}` preserves
`aria-pressed="false"`; it does not remove the accessibility state.

Boolean props for contenteditable/contentEditable, draggable and
spellcheck/spellCheck serialize as `"true"`/`"false"`, preserving an explicit off
state instead of removing the attribute and restoring defaults/inheritance.
Translate booleans serialize as `"yes"`/`"no"`. Explicit string tokens pass through;
reactive null/undefined removes the attribute. These rules also apply to SSR.
Native boolean attributes such as disabled and readonly retain presence semantics.

Model construction runs in a private child owner scope with cleanup registered
before synchronization or listener/observer installation. Failed setup disposes
its effect and registrations even when a caller catches the error and keeps its
parent owner alive. Owner disposal or graph reset during setup rejects stale
continuations. Runtime events and type refreshes ignore stale lifetimes. Cleanup
deactivates callbacks before native removal; if native removal itself throws,
physical detachment remains best effort while callbacks cannot publish writes.
Use an enclosing mount/root for automatic teardown of successful bindings.

## Nodes from other documents

Static and reactive children recognize native nodes from iframe realms, including
already adopted nodes whose prototype differs from their owner document's Node
constructor. Ordinary insertion preserves node identity and native direct listeners.
Foreign HTML children receive the existing SVG namespace upgrade where applicable.
Plain node-shaped objects are still rendered as text. When Svet insertion itself
adopts a subtree into another same-origin document, its delegated event registrations
move with it. This covers mount, Portal, static/dynamic children and keyed/owned
branches. The destination listener is staged before native insertion can synchronously
dispatch an event; nested insertions retain each pending destination independently.
After insertion, registrations follow actual node placement. Dynamic rollback also
restores routing to original documents. Document listeners stay shared per event type
and release with their last binding; disposal disables adopted callbacks.

This requires a Svet commit to perform the adoption. Raw native adoption/movement
outside the engine, including already-adopted nodes with stale registration leases,
does not trigger this refresh. Arbitrary cross-origin DOM access and hidden shadow
subtrees requiring document-route migration remain unqualified. Cross-document
commits walk the adopted subtree; ordinary same-document insertions skip that walk.
No throughput or heap improvement is claimed.

## Mount commit lifetime

`mount` rejects views invalidated by graph reset or owner disposal during rendering,
old-app cleanup or DOM insertion. Failed commits release prepared resources and
remove their attached nodes, including consumed fragment contents. Unmount always
releases the root even if native DOM removal throws. Rollback is best effort when
native removal itself fails; an old app already disposed during replacement is
not revived.
A newer same-container mount attempt supersedes a pending older attempt, which
throws instead of publishing its stale handle. This also applies if the newer
attempt fails; older attempts are not resumed. Different containers are independent.

## Portal lifetime

`Portal` renders in a private child owner scope while inheriting context. Within a
mount/root, unmount releases its subscriptions/resources and removes the external
container. Failed content construction or partial target insertion releases the
partial subtree even if the caller catches the error. Disposal or graph reset
during construction aborts insertion; synchronous insertion hooks that attach
after owner cleanup are followed by another container-removal attempt. Child
descriptors are read once. Successful standalone portals still require an owned
mount/root for automatic teardown.

## Async resource lifecycle

`createResource(source, fetcher)` validates its owner and graph generation before
allocation and after initial source/fetcher execution. `initialValue` is read once;
reset or owner teardown during that getter rejects construction before the source
or fetcher runs. Initial source/fetcher invalidation rejects construction and
releases partial resource state while preserving replacement graph nodes.

The resource cancels the previous request when its source changes. A source value of `false`, `null`, or `undefined` cancels pending work
and pauses automatic fetching, retaining the last successful value.

`resource.abort()` immediately clears `loading` and releases the resource's
`Suspense` pending state, even if the fetcher ignores its `AbortSignal`.
Superseded requests cannot overwrite current data or errors, or release
`Suspense` while their replacement is still pending. Disposing the owner aborts
pending work and frees the resource's three state signals and source observer.
`resource.dispose()` provides the same permanent, idempotent teardown for resources
outside a root. It unregisters its owner cleanup when called manually. Retained
accessors return their final snapshots; `mutate()` has no effect afterward.
Subsequent `refetch()` calls after disposal or graph reset resolve to `undefined`
without evaluating the source or fetching. `abort()` only cancels the current
request and leaves the resource reusable. Once ancestor teardown or effect-rerun
cleanup begins, retained descendant resources stop starting requests or accepting
mutation, even before cleanup reaches their individual owner scopes.

`refetch()` always returns a Promise, including when its source reader throws
(the Promise rejects with that exact exception). Cancelled, disposed, reset or
superseded loads resolve to `undefined` even if their fetcher later returns data.
Explicit abort, resource/owner disposal, source replacement or disabling the source
settle pending `refetch()` calls without waiting for the fetcher. Late rejection is
still observed. This ends the caller's wait; stopping external work still requires
the fetcher to honor `ctx.signal`. Graph reset also aborts pending requests and
settles their waits, after clearing the old graph storage. Abort listeners may create
replacement state that survives that reset. Cancellation belongs to the reset WASM
core; requests in other cores stay active. Retained signal accessors after reset
keep their pre-reset snapshots, including `loading`; they do not become live state
in the new graph.
Synchronous abort listeners may start a newer refetch; the older attempt does not
start an extra fetcher or clear that replacement's loading/Suspense state. A
loading observer can cancel before the fetcher is invoked. Recovery started by an
error observer suppresses stale forwarding to ErrorBoundary. Previously published
values are not rolled back when callbacks subsequently cancel a request.

Explicit refetch source reads, the fetcher's synchronous prefix, and abort listeners
run in the resource's WASM core with caller/callee dependency tracking paused.
Async data/error/loading publication is batched in that same core, even when the
Promise settles outside it. This does not propagate user callback context after
`await`; explicit continuation capture is still needed there.

`Suspense` and `ErrorBoundary` keep private state in child owner scopes. Unmounting
the enclosing root releases their counters/signals, active content/fallback roots,
and owned subscriptions/resources. Failed construction releases partial state.
`ErrorBoundary.reset()` batches its state changes so retrying an async failure
constructs the next content branch once. Retained reset callbacks do not remount
an unmounted boundary. Use these components within `mount()` or an owned root.

Suspense captures content/fallback descriptors once and checks its owner and graph
lifetime between user rendering, DOM insertion and visibility updates. Invalidated
construction does not invoke a new fallback and removes partial wrappers, including
synchronous late-insertion hooks. A resource startup error from a visibility update
settles loading and permits a later retry; user fetchers are not invoked after
startup was canceled.

## Router lifetime

`router.dispose()` removes its browser listener, clears guards, frees four state
signals and unregisters owner cleanup. Retained state accessors keep their final
snapshots. `navigate()` returns `false` after disposal, owner teardown or graph
reset, without running guards or publishing history. Guard registration after
invalidation is a no-op. A guard that disposes the router or starts a newer
navigation cancels the older transition. New guards registered during a transition
start with the next navigation; removed guards are skipped. Ancestor teardown and
effect-rerun cleanup deactivate descendant routers immediately: earlier sibling
cleanup cannot navigate, register guards or process `popstate` through a router
whose own cleanup has not yet been visited.

Navigation resolves relative, query-only and hash-only destinations against the
current full router URL before guards, route matching or history writes. Same-origin
absolute HTTP(S) URLs become internal pathname/query/hash. Invalid or external
navigation/redirect destinations return `false` before guards/history; an invalid
initial URL throws before allocating state. Raw query text is retained for future
relative navigation, including duplicate keys and percent encoding. Outside a
browser, URL resolution uses `http://svet.invalid/` as a synthetic origin without
making network requests.

Back/Forward guard rejection compensates with `history.go()` to the last accepted
entry in the same tracked history segment, including multi-entry traversal. The
compensating event skips guards; `navigate()` returns `false` while that traversal
is pending. Redirects during `popstate` replace the visited slot, keeping the URL
and reactive state aligned without adding history. A newer successful navigation
from a guard takes ownership of its new entry.

The router lazily tags its initial entry before the first push and tags new entries
under the reserved `history.state.__svet_router_history__` key. Existing plain-object
state fields survive; non-record state is kept under `__svet_router_state__`. Keep
those keys intact and use one authoritative router for a window. External entries
without this router's metadata start a new segment when accepted or followed by a
push. Host code must not copy router metadata to independently created entries.
If a visited entry has no known
position in the accepted segment, rejection repairs its URL in place while preserving
its state and the forward stack; it cannot reconstruct an unknown relative position.
Native traversal is asynchronous, and already-started traversal cannot be canceled
by disposing the router. Guard errors remain errors even if native compensation
also fails. Six finite bursts of Back/Forward/go calls are covered by both JSDOM
and the real-browser fixture, including recovery from a middle history entry with
forward entries still available. This does not qualify arbitrary concurrent user
traversals, cross-document navigation or restoration of scroll/focus.

`RouterView` captures its router and owns its active route/fallback scope. Unmounting
the view releases that branch's subscriptions and resources. Mutating the original
props descriptor does not replace the captured router.

RouterView and ErrorBoundary validate owner/generation while inserting branch
nodes. If insertion fails partway, the partial candidate nodes are removed before
propagating the error or mounting fallback. A later valid navigation can recover
the view. Output from rendering invalidated by unmount/reset is rejected, and an
invalidated ErrorBoundary does not call a new fallback. Already removed previous
branches and committed router/history state are not rolled back.

Route matching splits path segments before decoding them once. Static Unicode
and percent-encoded literals match their URL equivalents; encoded slashes remain
inside one segment. A leading encoded colon in a pattern is literal, while raw
`:name` still captures a parameter. Malformed encoded segments retain their raw
text. Captured parameter names, including `__proto__`, are enumerable own data
properties on an ordinary object without changing its prototype.

`Link` intercepts ordinary same-origin HTTP(S) clicks for the router, normalizing
absolute and relative URLs to pathname/query/hash. Modified/nonprimary clicks,
external schemes/origins, links outside a router, non-self targets and downloads
retain browser defaults. It respects an already prevented event and document
`<base>` URL/target. Optional `target`, `download` and `rel` map to anchor attributes.
Unsafe JavaScript/VBScript/HTML-data URLs remain blocked.

## Effect handle lifetime

Manual disposal of tensors, computed values and effects removes their cleanup
registration from a live owner. Repeated replacement inside a long-lived scene
therefore does not retain one cleanup closure per removed object. Effect cleanup
drains a detached callback list so disposing another owned resource from a cleanup
cannot cause duplicate cleanup calls. Tensors reject construction if their owner
is disposed during initial source conversion.

`effect(registerCleanup => { ... })` supports multiple cleanup registrations plus
a returned cleanup function. All run once in reverse registration order; the
returned function is registered after the body finishes. Draining is iterative and
continues after errors, preserving the first encountered exception for manual
disposal. Non-function body/cleanup arguments throw TypeError; a non-function body is
rejected before owner/native-node allocation. Registrars retained from a disposed or
older invocation run a newly supplied cleanup immediately in their captured core,
untracked in both the calling and cleanup contexts. They cannot replace cleanup
for a newer invocation.

If cleanup prevents a rerun, a later dependency change can retry the effect without
repeating drained callbacks. Native dirty state and computed ancestor caches are
rearmed conservatively; this may force computed re-evaluation on later reads.
Native ancestry checks protect replacement nodes occupying retired dependency
slots. Native repair failure preserves the original cleanup exception; recovery
under arbitrary native faults is not guaranteed.

Retained `createRoot` disposers and effect handles execute synchronous cleanup in
their creation core, restoring the caller's core afterward. Scope/effect cleanup
reads are untracked, including cleanup during child-scope failure, so they do not
subscribe an effect that happened to call the disposer. Cleanup writes still
invalidate dependencies already read by the caller. Reset during cleanup affects
the cleanup's own graph. Teardown order and error policy are unchanged: scope
cleanup continues past callback failures; returned effect-cleanup errors propagate
on manual effect disposal. Foreign cleanup also pauses calling-core tracking,
including reads of caller-core sources. Captured disposal remains available when
the caller's request resolver rejects further work. Async cleanup continuations
need their own context policy.

Signal/computed constructors snapshot `options.equals` once before native node
allocation. A throwing getter allocates no constructor node; graph reset during
metadata evaluation rejects construction without overwriting fresh slots. Computed
also rechecks its owner after metadata reads and rejects a non-function body before
options callbacks. A supplied non-function comparator throws TypeError before
allocation; nullish comparators retain the Object.is default. Bare signals keep
independent lifetime and are not implicitly owned/disposed by the caller root.
User side effects performed inside metadata getters are not rolled back.

A retained signal's synchronous updater/comparator and the effects/cascades caused
by its write execute in the signal's own core, even when invoked from another core.
Computed evaluation/comparison through ordinary reads or `peek()` follows the same
rule. A foreign entry temporarily pauses the caller's and any paused callee's
tracking, while newly executing effects/computeds track their own dependencies.
Caller context/tracking restores on success or failure. Callback writes still
invalidate already-read caller dependencies. Same-core updater/comparator tracking
keeps its existing behavior. Cross-graph value reads do not establish an automatic
reactive bridge; explicitly transfer/invalidate inputs when coordinating graphs.
Cached unobserved computed reads/peek avoid context entry when no evaluation is
needed. Async callback continuations need their own request/context policy.

Synchronous recursive computed evaluation throws exported `ReactiveCycleError`
before re-entering the same computation. Its frozen `nodeIds` array is the closed
evaluation chain (first ID repeated at the end). These are historical slot IDs in
the current context, not globally unique lifetime identifiers. Detection includes
`peek` and comparator recursion; isolated contexts and disposed/reused slots remain
separate. This guard covers synchronous evaluation, not arbitrary asynchronous
Promise graphs or stack depth of acyclic computations.

Computed failures do not publish a candidate cached value. Live reads retry instead
of returning stale data, and reached failed dependencies remain tracked so later
valid inputs can update existing observers automatically. This also applies to
ordinary compute/comparator exceptions and multi-layer computed chains. A
multi-effect notification finishes its remaining valid dirty queue entries before
propagating the first callback failure; removed/replaced lifetimes are still skipped.
These guarantees do not roll back user writes or other callback side effects.

Self-writes to dependencies already read by an effect request another iteration
after its callback returns, rather than recursively entering that same callback.
This includes writes through computed dependency paths, checked by the native Rust
graph, and tensor writes. Previous invocation resources/cleanup are released before
each iteration. Initial effect construction drains writes to existing subscribers
before returning. Writes unrelated to the effect's reads do not request a rerun.

`untrack()` suppresses new dependency reads, while writes still invalidate live
dependencies already read by a running effect. This also applies to paused outer
effects during nested computed/effect callbacks. Their staged read prefixes are
retained across observer changes; retired source slots and dependencies not read
in the current invocation do not request false reruns. Untracked reads alone never
create subscriptions.

An effect is limited to 100 synchronous self-write iterations per invocation.
Queued propagation tracks runs per effect lifetime within a drain and rejects work
that requires another queued run after 100 executions of that same lifetime.
Long acyclic cascades are not limited to 100 rounds. Nonstabilizing work throws
exported `ReactiveStabilizationError`, with `iterations` and `effectId` (`null` for
a queued propagation limit). This is an execution budget, not a mathematical
proof of an infinite cycle: a convergent sequence requiring more than the limit
is also rejected. A failing initial effect is disposed. Existing effects can be
disposed or corrected; after a queued limit error, native pending work remains
available to `flushEffects()`. Already performed writes are not rolled back.

Writes inside an effect queue downstream subscribers until that callback commits,
including unique scalar subscribers. Nested `batch`/`flushEffects` calls also defer
their downstream effects until callback completion. The outer write or effect
construction drains this work synchronously before returning. Explicit nested
`effect()` construction still invokes its initial callback immediately; this
scheduling guarantee covers propagation, not arbitrary user recursion.

`batch()` defers notifications; it does not roll back writes on failure. If its
body throws, that exact value remains the primary error even if end-of-batch
effects also fail. Surviving callbacks in the flushed queue still run. If the
body succeeds, observer failures propagate normally. Work left by a stabilization
limit remains queued for explicit recovery.

Expired native queue entries are periodically collected on writes and disposal
when storage exceeds `2 * active native nodes + 64`. Collection preserves matching
queued lifetimes, flags and order, including clean scheduled entries that can become
dirty again before flush. This bounds stale-entry growth under constant-size churn;
queue allocation retains peak capacity rather than returning WASM memory to the OS.

`getGraphTelemetry()` reports `pendingEffectCount` (dirty native queued jobs) and
`pendingEffectEntryCount` (stored entries, including clean/expired tokens). These
are fresh numeric snapshots and also appear in devtools recordings. They exclude
callbacks already drained into JS and do not measure total scheduler/process memory.
Reading dirty-job telemetry scans the pending queue.

Native topology-cache epochs invalidate every terminal/downstream cache on uint32
rollover, with zero reserved for invalid stamps. Detached/reused slots cannot be
rediscovered through an ancient cache stamp after rollover. The rollover scan is
rare; ordinary topology changes keep the normal increment path.

The raw Rust/WASM `begin_tracking` / `track_read` / `end_tracking` API binds frames
to observer lifetimes. Pruning/replacing dependencies invalidates topology caches;
reordering existing reads preserves edges without rebuilding them. Source disposal
adjusts active discovery cursors, and explicit source replacement restarts discovery.
Invalid/self reads and invalid/recursive begins are ignored with balanced scopes,
so paused or stale inner frames cannot alter an outer pass or a reused observer.
WASM function signatures remain unchanged. Tracking frames carry lifetime metadata;
stable/reordered reads reuse their existing vectors. This qualifies the raw native
cursor path, alongside the separately tested high-level JS tracker.

Rust's pending queue also stores native slot lifetime tokens. Disposal/reuse cannot
turn an old pending entry into a job for a new effect, signal or computed node;
legitimate replacement jobs retain their own queue position and are emitted once.
Counter wrap purges obsolete tokens for that slot while preserving other jobs.
The exposed flush buffer still contains uint32 IDs. Token metadata adds native
per-node and queued-entry storage; it is not a memory-footprint optimization.

Effects created inside an active batch link to the Rust graph immediately rather
than waiting on the unlinked single-subscriber optimization. Subsequent writes in
that batch notify them at batch end. Adding such a peer also invalidates any older
single-subscriber cache so later unbatched writes reach every subscriber.

Multi-effect flushes capture effect-record identity and lifetime before invoking
callbacks. If a callback deletes a queued peer and its native ID or pooled shell
is reused, the old queue cannot execute that replacement. This applies to scalar,
batch and tensor writes and generation changes. Genuine new writes still notify
the replacement through its own subscriptions. Capturing a multi-effect queue
requires temporary identity/version arrays; single-effect paths stay direct.

An executing effect retains its internal pooled shell until the callback unwinds,
even if it disposes itself. It cannot then commit dependencies or overwrite a
replacement's cleanup. Cleanup returned after self-disposal runs immediately;
disposal from previous-run cleanup prevents a new callback invocation. The callback
argument used to register cleanup belongs to that particular run: registering
after its run ended or was replaced invokes that cleanup immediately. New effects
or computed values cannot attach to an already disposed owner; use a new root for
an intentionally independent lifetime.

Each `effect()` call returns its own handle. Calling `dispose()` repeatedly is
safe, including after its internal WASM slot and JavaScript entry have been
reused by another effect. The handle's `id` remains the original numeric slot ID;
slot IDs may be recycled and are not unique lifetime identifiers. Owner cleanup
uses the same lifetime-bound handle, so manually disposing an owned effect cannot
cause later owner teardown to stop an unrelated replacement.

## Resumed container lifetime

`resumeContainer(container, snapshot)` restores signals and binds targets only
inside that container, including annotations on the container itself. Repeating
it disposes the previous bindings. Cleanup follows the current owner; outside an
owner, call `disposeResumedContainer(container)` to release signals, effects, and
input listeners. Malformed snapshots are rejected before replacing live bindings.

SSR serialization preserves current input, checkbox, textarea, and option state.
`renderToString` runs owner cleanup on both success and rendering failure.

## Async SSR on Node

Import the Node adapter separately; the main browser entry does not import Node
APIs. The package ships compiled ESM and declarations, verified on Node 22.23.4
without a TypeScript loader or type-stripping flags.

```ts
import { h, signal } from 'svet-engine';
import { renderToStringAsync } from 'svet-engine/server';

// The host supplies a DOM implementation for h() and DOM serialization.
const result = await renderToStringAsync(async () => {
  const title = signal(await Promise.resolve('Hello'));
  return {
    node: h('h1', { 'data-svet-text': 'title' }, () => title()),
    signals: { title },
  };
});
```

`runWithIsolatedContextAsync(callback, { signal })` and
`renderToStringAsync(view, { signal })` preserve a request's WASM core across
awaits, including nested requests. They release the request ownership root and
reset graph registries on success, failure, or cancellation. `signal` is optional
and can come from a disconnect handler or host-controlled deadline. Cancellation
cannot stop arbitrary JavaScript or I/O; pass that signal to cancellable I/O too.

Await request work and return plain results or serialized HTML; reactive objects
belong to the request. Async work attempting to resolve the core after completion
throws instead of falling back to the default graph. Owner context inside
individual asynchronous component subtrees is not yet propagated independently
of the request root. Edge runtimes and browser SSR are not qualified by the Node
adapter tests.

## JSX compiler

The compiler parses TSX/JSX into an AST and inserts lazy thunks only in JSX
expression containers with eager calls. It handles adjacent children, nested JSX,
optional calls, template substitutions, comments, regex literals, TypeScript
casts, and signal-dependent style objects. Event handlers, binding attributes,
spreads, and deferred callbacks retain their shapes.

```ts
import { transformSvetJsxWithMap, svetVitePlugin } from 'svet-engine/compiler';
const { code, map } = transformSvetJsxWithMap(source, '/app/Counter.tsx');
```

The Vite plugin returns high-resolution source maps for JSX transformations.
Malformed JSX reports `SvetJsxCompileError` with filename, line, and column. The
existing `transformSvetJsx(source)` string API and main-entry exports remain
available. Unused compiler modules can be removed from browser bundles; the Node
server entry is explicitly marked as having initialization side effects.

The compiler treats eager calls conservatively: it does not infer which functions
are signals. Calls inside lazy JSX expressions must be safe to execute again when
their tracked dependencies change. Parsing is not TypeScript type checking.

## Element construction and ref callbacks

`h` checks caller lifetime before prop metadata, then builds in a private owner
scope. Enumerable own props, including symbols, are snapshotted once with the same
receiver and child-argument override semantics as object spread. Teardown/reset
between getters stops remaining metadata reads. Construction checks lifetime after
native element creation, prop binding, child insertion and component factory output.
A native custom-element constructor cannot invalidate its caller and still publish
that rejected element.

If metadata, a ref callback, component factory or child rendering fails, resources
owned by the failed construction retire while a surviving caller remains live.
Earlier event/model bindings on the rejected element retire too. Independently
created replacement graph lifetimes after reset survive old-scope cleanup. Bare
signals keep their independent disposal contract.

Public `bindProp(element, 'ref', callback)` also checks lifetime and owns provisional
callback resources in a private scope. A ref runs once with the element during
binding; its return value is ignored. Use `onCleanup` inside the ref for teardown.
Metadata reads retain caller reactive tracking; function component bodies remain
untracked. During teardown, a descendant of a stopping ancestor cannot execute
event/model callbacks, effects or new child construction even before that descendant
is physically visited by cleanup. Computed reads keep their cached snapshot. The
existing child visitation and reverse cleanup order are preserved. These scopes add JavaScript bookkeeping without extra reactive WASM
nodes; construction throughput and heap cost have not been measured for this change.
Completed native DOM operations and arbitrary user side effects are not rolled back.
Async component/ref continuations are outside this synchronous lifetime guarantee.

## Explicit continuation context after await

Call `captureReactiveContext()` inside an owned event handler, effect or root before
waiting. It captures the owner, reactive core and graph generation. After waiting,
use `context.run(callback)` for the synchronous part that needs that view's context:

```ts
async function onClick() {
  const context = captureReactiveContext();
  try {
    const result = await loadData();
    context.run(() => applyResult(result));
  } catch (error) {
    if (!(error instanceof ReactiveContextDisposedError)) console.error(error);
  } finally {
    context.dispose();
  }
}
```

Pass this function as a Svet handler on an owned view. Both APIs and the
`CapturedReactiveContext` type are exported from `svet-engine` and
`svet-engine/reactivity`.

`active` checks current validity. `run` preserves the callback return value, enters
the captured provider/core/owner, suppresses caller/callee tracking, and restores
previous state even on errors. Explicit observers created inside it still track and
belong to the view. Nodes created there inherit the view's shadow event area.

Owner disposal, effect rerun, graph reset or explicit `dispose()` invalidates the
capture. A capture from an earlier effect invocation never revives when that effect
reuses its owner. Inactive `run` throws `ReactiveContextDisposedError` with code
`SVET_CONTEXT_DISPOSED` before callback execution. Capturing requires an active owner.
`dispose` is idempotent and releases its captured state/group membership; it does not
dispose the view or resources already created under it. Captures of one owner share
one cleanup hook, detached when its last active capture is released. The first capture
uses a single slot; concurrent captures use a lazily allocated Set. An empty group is
weakly cached with its owner. Manual release avoids a per-capture owner-array search;
other cleanup order and effect-rerun invalidation remain unchanged.

Each `run` covers its synchronous region. If its callback returns a promise, context
is restored immediately; another section after an inner await needs another `run`.
Parallel tasks retain independent captures. Callback exceptions do not roll back
view-owned resources created before failure. This API does not provide automatic
Promise propagation, async JSX component rendering, transport cancellation or
latest-request ordering. Those need their own rendering/request policies.

## Direct Fragment construction

`Fragment({children})` validates caller lifetime before native fragment creation,
then reads `children` metadata once inside a protected owner scope. Native creation,
metadata and insertion have lifetime checks. Failure retires resources owned by the
attempt while preserving a surviving caller and independent replacement lifetimes
after graph reset. The root and JSX-runtime Fragment exports use this implementation.
A reactive accessor child still updates after the fragment is consumed into a host;
its observers retire with the owner. The metadata getter snapshot does not turn a
reactive accessor into a one-time evaluation.

Effect rerun cleanup also makes its previous descendant scopes temporarily inactive
before visiting individual children. Computed reads in those old scopes keep their
cached value; captured-owner reentry/new owned work is rejected there. Physical child
visitation flags and cleanup order remain unchanged, and the temporary marker clears
before the next effect body. Independent roots and bare signals retain their lifetime
contracts. Completed DOM writes and arbitrary user effects are not rolled back.

## Context Provider lifetime

`Provider` validates its calling lifetime before reading props, then snapshots
`value` once inside a private construction scope with the parent context, then
`children` once inside a nested scope carrying the provided value. Effects created
by the value getter retain their parent context on later runs; children see the
provided value. A function child is invoked once per Provider call. Provisional
effects retire if construction fails.
Reset or caller teardown rejects stale output before moving children, preserving
independent replacement graph lifetimes. `useContext` keeps its existing lookup
and default-value semantics.

Child insertion checks lifetime around static coercion and native writes and between
array entries. Scoped renderers forward their publication guard through the shared
inserter, retaining their own lifetime/supersession diagnostics. Completed native
operations or arbitrary user side effects are not rolled back; invalidation stops
remaining work. Context propagation through asynchronous component subtrees and
arbitrary native-fault combinations are not qualified by these guarantees.

## DOM ownership and remounting

Model listeners and event callbacks are removed when their owner is disposed.
Direct and delegated event callbacks check their captured graph generation and
owner before invocation. Stale callbacks skip user code and detach when encountered;
a reset in one handler suppresses old peers later in the same dispatch while allowing
new registrations to remain live. Synchronous callback execution enters its captured
WASM core and bound view owner, retaining native `this` and the event facade.
useContext sees the bound provider; callback-created effects and onCleanup hooks
belong to that view. Callback reads are untracked in the current engine's caller and
callee core contexts, while explicitly created observers collect their own reads.
Nested callbacks and exceptions restore the previous owner/tracking/core state.
Nodes created by callbacks inherit the bound mount's context and shadow event area.

Callback exceptions retain native event error reporting. Resources created before an
exception remain view-owned until owner teardown; event notifications do not perform
per-call rollback. Bindings created without a managed owner retain their unowned
lifetime contract. No per-dispatch owner scope or native listener is allocated.
Direct registration is also transactional: disposed owners,
registration errors and teardown reentry cannot leave an active late listener.
Async user work after `await` needs its own lifetime/cancellation policy; these
checks guard entry and synchronous execution. Complete immediate listener removal
still requires owner disposal or unmount.
Delegated events share one listener per event type and owner document, plus a
listener per type used in each shadow mount area. Listeners release with their last
owned binding. Multiple bindings on one node are
additive; disposing one scope removes only its own callback. Delegated callbacks
receive a facade with logical `currentTarget`, native event methods, and target
retargeting for open/closed shadow mounts. Bindings created within a shadow mount
inherit its local event area through their owner scopes, including later Show/For
branches. Noncomposed events reach the local area; document fallback handles Portal
output in the document and skips duplicate calls when the local route is present.
Slotted light-DOM nodes use the document route. Reusing an Event object in successive
dispatches remains supported. Existing nodes bound while already inside a shadow
root infer that root. Create view-owned bindings inside the mount callback for
routing and automatic teardown. Svet-driven cross-document adoption refreshes
registered descendant document routes as described above. Raw native moves,
arbitrary prebound unowned subtrees, and moving individual bound nodes outside their
area into another closed tree remain unqualified.

Independent compatible Svet copies on one document or shadow event area share a
versioned native dispatcher registry on that target. Bindings remain additive,
retain child-before-parent bubbling order and keep their own owner/core-aware
callbacks. Disposing one copy's owner releases only its bindings; the last binding
retires the native listener and target registry. Interoperation with older bundles
that do not implement this registry protocol is not qualified.

Attribute/property bindings validate their caller/run lifetime around resolver
reads, coercion and native attribute/property operations, including reads whose
value already matches. Type/value model refresh uses the same guard. Reactive
applications and static object coercion own provisional scopes, so failures retire
user-created temporary effects and later valid reactive values can recover. Plain
static primitives do not allocate a private scope. Resumption supplies its own
publication guard to preserve its supersession/lifetime checks. Completed native
writes are not rolled back if their setter then invalidates the owner.

`mount(view, container)` accepts an `HTMLElement` or `ShadowRoot` (including a
closed root retained by the caller). It prepares a replacement view before releasing
the existing mount. If
view creation throws, its bindings are cleaned up and the original view remains.
Repeated `unmount()` is safe, stale handles cannot remove a replacement, and a
mount follows the enclosing owner's lifetime. Reactive object styles remove
previously managed CSS keys that disappear from the next value. Style bindings own
nested property subscriptions in private scopes; each reactive snapshot also gets a
scope so failed updates retire provisional effects immediately and later valid
snapshots can recover. Enumerable keys are snapshotted before reading property
values, with lifetime checks between getters, coercion, and native CSS operations.
Owner disposal or graph reset stops subsequent reads/writes, including matching
native reads that need no mutation. Ordinary replacements release old nested
accessors. CSS already written before a failure is retained; this is not an atomic
CSS rollback. The private scopes add JS ownership bookkeeping, not extra WASM nodes
beyond the existing style/property effects.

`resetReactiveSystem()` invalidates previous reactive node lifetimes. Old signals
and computed values retain their last cached reads but cannot subscribe, update,
or dispose newly reused graph IDs. The same lifetime checks apply when a signal
updater/equals callback or computed evaluation/equals callback disposes its own
node: the interrupted operation cannot publish values, dirty state or dependency
edges into the recycled ID. Disposed computed reads keep their last cached value.
It does not replace DOM owner teardown; use
`unmount()` or dispose the owner for listener cleanup.

For native browser lifecycle checks, run `npm run dev` in this package and open
`/tests/browser/dom-lifecycle.html`, then click **Run checks**. This covers a bounded
set of browser scenarios, alongside the jsdom regressions.

## Package build and consumer verification

Public exports resolve to JavaScript and `.d.ts` declarations in `dist/package`:
`svet-engine`, `svet-engine/reactivity`, `svet-engine/jsx-runtime`,
`svet-engine/jsx-dev-runtime`, `svet-engine/compiler`, and `svet-engine/server`.
The main and reactivity entries share the same graph implementation. This is an
ESM package; CommonJS `require()` compatibility is not qualified. Supported Node
versions follow the declared engine range; the local qualification used 22.23.4.

```sh
npm run build:package  # ESM + declarations from the embedded WASM source
npm run build          # rebuild Rust WASM, type-check, build package and demo
npm run test:package   # pack, install and verify in a separate temporary project
npm pack               # prepack builds ESM and declarations; does not publish
```

The archive contains the compiled library, source maps with embedded sources, and
documentation. Rust build outputs, source demo, tests, and development scripts are
excluded. Package installation needs no Rust toolchain or TypeScript loader.
`prepack` uses the checked-in embedded WASM; run the full build first when changing
Rust. Demo output lives separately in `dist/demo`.

`test:package` verifies native ESM imports, concurrent SSR, generated Forma module
imports, all public entrypoints, strict JSX declarations without installing Vite,
a negative type fixture, and an external Vite build of JSX and Forma. It uses the
package's development tool binaries to check the consumer, not aliases or source
imports. It installs runtime dependencies through the public npm registry and
writes `dist/package-verification.json`. Successful temporary projects are removed;
pass `-- --keep` to retain one for browser inspection. Failed projects are retained
with their path printed for diagnosis.

Configure automatic JSX with `jsxImportSource: "svet-engine"` (TypeScript) or
`importSource: "svet-engine"` in the bundler. Generated Forma modules now import
`svet-engine` by default. Internal source demos using an alias can set
`svetVitePlugin({ runtimeImportSource: "svet" })` explicitly.

## GPU resource lifetime and validation

GPU configuration, upload buffers and telemetry are isolated per WASM context.
Replacing or detaching a device destroys the old arena buffer. Configuring within
an owner registers teardown for that configuration. Explicitly supplied devices
remain caller-owned; devices created by `initWebGpuHardware` are owned by Svet and
destroyed when replaced or their owner ends. Grow/replacement buffers receive a complete arena upload.
A queue failure preserves dirty ranges for retry, and a clean arena performs no
upload. Without a device, transfer counters remain zero.

Tensor lengths and indices must be valid integers. The float-arena capacity is
reported by Rust/WASM (currently 67,108,864 float32 values, 256 MiB), and tensor
preflight uses that same limit. Mesh preparation rejects an intrinsically oversized
request before index copying, coordinate reads or the homogeneous temporary buffer:
its two vec4 vertex buffers and mat4 alone require `8 * vertexCount + 16` floats.
Passing this necessary bound does not reserve capacity; existing allocations and
fragmentation can still cause the native allocator to reject a smaller request.

Ancestor teardown and effect
rerun cleanup deactivate descendant tensor methods before their individual cleanup
is visited. GPU configuration, tensor/camera construction and hardware initialization
also reject retired ambient owners; configuration/tensor/camera option getters are
not evaluated in that state. Disposed or reset-invalidated tensors reject
reads/writes through their methods, and cannot free reused IDs or
storage. Cross-context compute is rejected. Compute dispatch captures owner/generation
before option inspection. Metadata and input iteration are checked between
user-controlled accesses; a reset/teardown stops preparation before kernel startup
or publication into replacement output. Input getters are read once, including
inherited or nonenumerable `inputs`. A `Float32Array` obtained from `view()`
is borrowed: it cannot be revoked and must not be retained past disposal/reset or
arena relocation or WASM memory growth. Reacquire it from the tensor when needed.
Use `write`/`setAt` for tracked changes; direct mutations of a borrowed view do
not automatically mark dirty ranges or notify observers.

`setAt` compares float32 values, preserving signed zero and avoiding repeated NaN
notifications. Scalar numeric conversion completes before lifetime revalidation and
view acquisition, so disposal/reset during conversion cannot overwrite replacement
storage and memory growth does not lose the write. `write` handles overlapping
typed-array views. Scene transforms,
cameras and meshes stop their producer effects before disposing matrix storage.
Transform/camera construction releases partially initialized signals, effects and
matrix tensors on failure. Option inspection checks lifetime between reads;
transform metadata keeps its enumeration order and captures its parent matrix
dependency. Tuple iteration and numeric conversion complete before native transform
writes, followed by lifetime/storage checks, preserving independent replacement
state after reset or owner teardown.
Mesh construction uses the same partial-resource cleanup. It captures borrowed
world/projection matrix handles and checks lifetime between metadata, coordinate
and index reads before allocating/publishing native resources. `updateIndices()`
rechecks lifetime during copying and before replacing topology; disposal/reset
from an input getter stops later index reads and preserves replacement state.
A successful nested topology update also stops the older preparation with a
`Mesh3D topology changed during update` error, preserving the newer indices and
format. Failed nested candidates do not supersede valid outer preparation.
Publication observers may commit a replacement; no older write follows it.

Run `/tests/browser/gpu-uploads.html` via the dev server to check real WebGPU
uploads and readback. The local browser passed full-buffer growth, DUMA partial
uploads, UMA dirty-window uploads, clean-sync skipping and readback without GPU
validation errors. This is correctness evidence for this browser/API path, not a
GPU performance measurement or device/vendor matrix. Render-resource lifetime,
throughput optimization and broader device coverage still need work.

## Synchronous host compute

`dispatchGpuComputeKernel` requires a synchronous `fallbackKernel`; WGSL runs only
through the asynchronous API. The callback receives private snapshots of its
inputs and initial output. Success publishes once through the tracked output
write; failure leaves live output unchanged. Callback edits to input arrays do
not mutate input tensors. This requires temporary copies rather than borrowed
live views. Memory growth during the callback does not detach these arrays.

Fallback callbacks run with dependency tracking paused in the dispatch core. Input
tensor snapshot reads remain tracked, so an enclosing effect still responds to
input changes; incidental signal reads inside the callback do not subscribe it.
Deferred async fallback restores the dispatch core and original owner for its
synchronous prefix. Effects/cleanup created there retire with that owner. Code
after the callback's own `await` still needs explicit continuation capture.

A reentrant output write/disposal, owner disposal or graph reset rejects stale
publication. Failed kernels do not supersede earlier accepted async work. An
async callback or returned thenable is rejected; late rejection is observed and
late array mutations remain private. Use `dispatchGpuComputeKernelAsync` for
asynchronous callbacks. User side effects through other explicit APIs are not
rolled back.

## Asynchronous WGSL compute

`dispatchGpuComputeKernelAsync(options)` returns a `Promise<GpuComputeResult>`.
Inputs are bound in order at group 0, bindings `0..N-1`; output is binding `N`.
All bindings use float32 storage buffers. The entry point defaults to `main`,
and default workgroups are `[ceil(output.length / 64), 1, 1]`; set `workgroups`
explicitly for a different shader workgroup size. Empty hardware bindings and
invalid counts/sizes are rejected before reserving the output.

```ts
const result = await dispatchGpuComputeKernelAsync({
  wgsl: shaderSource,
  inputs: [inputA, inputB],
  output,
  workgroups: [Math.ceil(output.length / 64)],
  signal: controller.signal,
  fallbackKernel: async ([a, b], result) => {
    for (let i = 0; i < result.length; i++) result[i] = a[i] + b[i];
  },
});
// result.backend: 'webgpu' or 'fallback'
// result.uploadedBytes / readbackBytes: logical API transfer sizes, not bus measurements
```

Descriptors and input values are captured at call time. Results are copied back
into the live tensor only after mapping and validation complete. A later dispatch
or a tracked host write supersedes an older pending result; this includes internal
scene writes to local/world transforms, camera projection, MVP matrices and
transformed mesh vertices. The older Promise rejects instead of overwriting newer
state, for hardware compute and asynchronous host fallback alike. Raw borrowed-view writes are not
versioned, so use tensor methods for observable changes.
`createGpuTensor(initialValues)` converts initial data into a private float32
snapshot before native allocation. A throwing source leaves no tensor allocation;
a graph reset during conversion rejects construction. This also protects borrowed
WASM input views from detachment when allocating the destination grows memory.
Initialization uses a temporary JS copy; subsequent tensor views remain zero-copy.

`tensor.write` stages ordinary arrays and other non-typed `ArrayLike` sources
before publication. Throwing getters/conversions cannot leave a partial framework
write. It reacquires the WASM view and checks tensor lifetime after conversion,
so memory growth or disposal during source access cannot silently target stale
storage. Typed-array sources retain the overlapping-copy fast path. Only elements
that fit the destination are converted; user getter side effects are not rolled back.


Owner teardown, `AbortSignal`, graph reset, device replacement and device loss
cancel affected work. Started operations release buffers and error scopes before
cancellation settles. The framework cannot stop arbitrary host fallback code;
fallbacks run on private working copies and cannot partially publish on failure.
Fallback is selected when compute support is unavailable, not to conceal shader
or GPU validation errors. Empty fallback outputs are allowed.

Device loss clears the configured device. Configure another live device or call
`initWebGpuHardware` to request a new one. Late initialization cannot overwrite a
newer configuration or retain a device after owner teardown. Shared-device loss
subscriptions are deduplicated; Node SSR subscriptions run outside request async
context. Per-device compute queues serialize validation/readback and are removed
when drained. Each device admits at most 32 unfinished hardware compute calls,
including the running call and cancelled entries whose queue closures have not
drained. Excess calls reject before tensor snapshots or output revision changes;
retry after earlier work settles. The limit is shared across reactive contexts
using the same device. Input snapshots plus the private output copy also share a 256 MiB per-device
budget. Configure it with
`configureGpuSubsystem({ device, maxPendingComputeBytes: 64 * 1024 * 1024 })`.
The value must be a nonnegative safe integer; zero disables new hardware calls.
An explicit update affects every context sharing that device. Omitting the setting
preserves it. Lowering the budget never evicts accepted work; new calls reject
until there is capacity. Rejected calls do not reserve output revisions.
Capacity rejection uses exported `GpuComputeCapacityError` (an `Error` subclass).
Use `error instanceof GpuComputeCapacityError` to distinguish admission pressure
from descriptor, shader, device and host-kernel failures. Its `reason` is `count`
or `snapshot-bytes`; `queue` is a frozen rejection-time snapshot of the four
queue telemetry fields. `requestedSnapshotBytes` is the attempted input plus
private-output size for a byte rejection, or `null` when the count limit rejects
before tensor inspection. No rejected call reserves output revisions or creates
snapshot copies. The error contains no device/tensor references.

A capacity error does not prove that the unexecuted descriptor or shader is valid.
If a single request exceeds the byte limit, waiting alone cannot make it fit;
reduce the request or deliberately change the device budget. The error snapshot
does not update when work drains; query current telemetry when deciding to retry.

`getGpuTelemetry()` returns fresh, non-reactive snapshots of the configured device:
`pendingComputeCount`, `pendingComputeSnapshotBytes`, `maxPendingComputeCount` and
`maxPendingComputeBytes`. Pending counts include running and queued calls, plus
cancelled entries not yet drained. Shared-device contexts see the same counters;
switching devices selects that device's counters. With no configured device,
pending values are zero and limits show defaults. Existing telemetry objects do
not change. These fields also appear in explicitly recorded devtools snapshots.

These limits do not cover WASM arenas, GPU buffers, mapped readback/result copies,
JavaScript overhead, or host fallbacks; they are not a process memory ceiling. Arena sync telemetry excludes these operation-local transfers.

The implementation follows the [WebGPU specification](https://gpuweb.github.io/gpuweb/)
for storage bindings, command submission and mapped readback. Run
`/tests/browser/gpu-compute.html` to reproduce WGSL output, reactive publication,
shader diagnostics and device replacement checks. This is a correctness-first
readback API: pipeline caching, resident GPU results and throughput qualification
remain open.

### Adaptive 3D viewport

`Viewport3D` accepts the same exported `Canvas3DProps` as `Canvas3D` and returns
an `HTMLDivElement` containing independent WebGPU and Canvas2D surfaces. It starts
in 2D without a device, upgrades on `configureGpuSubsystem({ device })`, restores
2D after device loss/detachment and reuses the GPU canvas after replacement.
Orbit/picking handlers stay on the stable wrapper and are removed on owner cleanup.

```ts
const viewport = Viewport3D({ mesh, edges, orbitTransform: transform, width: 360, height: 220 });
```

Forma exposes `Viewport3D { width: 360; height: 220; }` explicitly. Existing
`Scene3D` and `OrbitCube3D` retain their canvas return type. The demo uses the
adaptive viewport. All surfaces should be created inside a disposable root.
A WebGPU render configuration requires a native `GPUDevice`; structural device
mocks used by unit tests do not satisfy browser canvas configuration.

Run `/tests/browser/gpu-render.html` via the dev server for real-device submission,
validation, loss/replacement and teardown checks, with pixel checks on 2D recovery.
This fixture does not establish GPU pixel correctness or cross-browser parity.

`width` and `height` also accept reactive accessors (for example, `width: widthSignal`).
A resize updates both drawing buffers and CSS sizes, replacing only the depth
texture/view. Geometry buffers, pipeline and canvas configuration are reused.
Dimensions must be positive uint32 integers. Invalid updates throw before changing
the previous surface; a later valid update can recover. Forma tracks dimension
expressions and updates camera aspect. Canvas3D itself does not own a caller's camera.
The native fixture additionally samples the GPU triangle center before/after resize;
this checks a bounded color sample, not whole-frame raster equivalence.

`mesh` can be a `Mesh3D` or a reactive accessor such as `mesh: selectedMeshSignal`.
Changing it reuses fitting render buffers, grows insufficient capacity and tracks only the new mesh;
programmatic picking queries the current mesh, while viewport callbacks use a frame snapshot.
The caller owns all mesh/transform/camera
lifetimes, including meshes no longer selected. `edges` and `faces` are independent
caller-supplied annotations and must match the selected geometry.

Perspective camera construction and parameter `set`/`update` calls validate the
actual float32 projection in Rust. Parameters must remain finite after conversion:
`0 < fovYRad < pi`, positive aspect/near and `far > near`, with finite nondegenerate
matrix coefficients. Invalid construction allocates no camera nodes; invalid
updates throw `RangeError` and preserve parameter values, projection and dependents.
Use `camera.setProjection({ near: 2000, far: 3000, aspect: 2 })` to validate and
publish interdependent parameter changes together. Individual setters validate
against the other current parameter values, even inside an outer batch.

Rust uses a shared pure matrix builder for validation and publication and computes
the depth translation without an overflowing `near * far` intermediate. Direct
raw `svet_gpu_mat4_perspective` calls with invalid parameters leave destination and
dirty state unchanged; `svet_gpu_valid_perspective` exposes the same native check.
This validates finite projection coefficients, not the numerical conditioning or
visibility of every scene transformed by them.

Transforms and perspective cameras created inside a root/effect belong to that
owner as complete resources: teardown releases their parameter signals as well as
matrix tensors and computations. Effect reruns release the previous scene before
reconstruction. Manual `dispose()` is idempotent and unregisters the aggregate
cleanup. Outside an owner, call `dispose()` explicitly. Borrowed parents, transforms
and cameras are not recursively owned by their dependents.

Transform parents and mesh transform/camera dependencies must be live in the
current reactive context. Constructors validate these dependencies before allocating
child signals or tensor storage. Constructor descriptors are captured: editing the
caller's `parent`, `transform` or `camera` fields later does not rewire existing
objects. Update the original reactive signals or construct a new scene object.

`createMesh3D` validates complete xyz/triangle triples, finite float32 positions,
and vertex indices within both the mesh range and selected index format. It throws
`RangeError` before reactive allocations for malformed input. `uint16` remains the
default (`0..65535`); use `indexFormat: 'uint32'` for larger indices. Mesh indices
are copied into `Uint16Array` or `Uint32Array`, and WebGPU binds the matching format.
Direct mutations of a mesh's exposed index array do not signal a redraw; call
`mesh.updateIndices(values, format?)` to validate, copy and publish new topology.

Projection now uses the standard WebGPU viewport mapping in both backends and
Rust vertex picking: `x = width/2 * (1 + clipX/clipW)` and
`y = height/2 * (1 - clipY/clipW)`. This removes the previous 0.65 scale from 2D
and picking. The picker and 2D vertex markers exclude non-finite vertices,
non-positive w and vertices outside `-w <= x,y <= w`, `0 <= z <= w`.
Picking validates finite queries and positive viewport dimensions in Rust.
Rust/WASM clips 2D annotation segments and convex quad faces against all six
homogeneous clip planes before perspective divide. JavaScript transports the
result to Canvas2D; invalid indices, non-finite inputs and unprojectable degenerate
results emit no path. Clipped faces are sorted by their resulting average depth.
The 2D fallback still uses depth sorting rather than a z-buffer; non-convex faces,
annotation/cross-mesh occlusion and full raster equivalence remain separate work.

```ts
const mesh = createMesh3D({ positions, indices, indexFormat: 'uint32', transform });
```

The browser regression renders a mesh with 65,537 vertices using `[0, 1, 65536]`
and verifies an interior GPU pixel, device replacement and resource teardown.
This establishes uint32 index handling for that case, not large-scene throughput.

`updateIndices` retains the last selected index format when its second argument is
omitted. Reading `mesh.indices` in an effect tracks topology changes. `[]` means
an indexed mesh with zero triangles; `undefined` selects the nonindexed line-strip
path. Invalid updates leave the previous topology untouched. Calls after disposal,
owner teardown, context reset or from another reactive context throw. Fitting topology updates reuse
the index allocation, vertex buffer, depth texture and pipeline. Growth replaces
only the index buffer; switching between indexed triangles and nonindexed lines
replaces the pipeline and releases/creates the index allocation. Annotation `edges`/`faces`
and vertex picking remain independent of indexed triangle membership.

```ts
mesh.updateIndices([0, 1, 2, 2, 3, 0], 'uint32');
mesh.updateIndices([]); // Clear triangles, preserving vertices and mesh identity.
```

Each GPU frame uses validation, out-of-memory and internal error scopes when supported. Inspect
`data-render-validation` (`pending`, `passed`, `failed`, `unsupported`, `inactive`,
`disposed`) and `data-render-error` on the canvas/adaptive wrapper for diagnostics.
`passed` means all three scopes reported no error; it does not await frame
presentation. Validation failure releases render resources and draws the independent
2D surface. A subsequent reactive update retries GPU initialization. Results after
resource teardown/reset are ignored; an older failure cannot invalidate a newer
validated frame. A stream of pending frames does not suppress unresolved failures.
Rendering also preflights exposed `maxBufferSize` and `maxTextureDimension2D`
limits before creating GPU resources, including aligned/padded allocation sizes.
An over-limit update releases old render resources and uses 2D; a fitting update
can retry GPU initialization. A frame error from any filter initiates cleanup
without waiting for the other results. Partial scope setup still closes every
successfully opened scope. The filters follow the [WebGPU error scope specification](https://www.w3.org/TR/webgpu/#error-scopes).
Other device limits and physical memory availability are not guaranteed. OOM/internal
cleanup tests inject error results; they do not exhaust memory or crash a driver.

Async WGSL compute also captures validation, out-of-memory and internal errors.
All scopes are popped synchronously in reverse order before awaiting their results;
readback remains private until all succeed. Failure rejects without invoking the
unavailable-device fallback or publishing output, and temporary buffers are freed.
Per-device compute serialization and cancellation cleanup apply to the whole group.
The browser compute fixture injects OOM/internal error results into native scope
responses; it does not force physical memory exhaustion or hardware faults.

`pickNearestVertex3D` defaults to occlusion by the mesh's indexed triangles.
Rust clips triangles first, interpolates depth in screen space and skips candidates
behind them before applying the existing distance/depth score. Both index widths
are transported losslessly in a context-local uint32 scratch buffer. Set
`occlusion: 'none'` to select through surfaces. Viewport `pickOcclusion` overrides
the default: indexed on WebGPU, none on the 2D wireframe surface.

This tests vertex positions against the same mesh's indexed surfaces, not arbitrary
annotation faces or other meshes. It uses a conservative 1e-6 NDC depth bias and
small barycentric tolerances; it is not bit-exact GPU depth/raster readback. Current
implementation clips all triangles and scans candidates without spatial acceleration.

Mesh replacement retains the canvas configuration, depth attachment and compatible
pipeline. Vertex/index buffers grow independently only when their capacity is
insufficient. Smaller meshes retain capacity until cleanup and use their current
draw counts; stale trailing buffer contents are not drawn. Triangle/line topology
changes still require a new pipeline. Device replacement and failures release the
renderer cache. This proves allocation reuse, not frame-time improvement.

### Coalesced frame scheduling

Use `renderMode: 'animation-frame'` on Canvas3D/Viewport3D (or Forma) to coalesce
reactive updates. The first draw stays synchronous. Later updates retain only a
redraw request and draw the latest state on requestAnimationFrame. A viewport
admits at most one GPU submission until both queue.onSubmittedWorkDone and every
error scope settle successfully. Failure releases resources and shows 2D, while GPU retry waits for already-submitted
work to settle. Teardown/reset cancel deferred callbacks and ignore late completions. GPU devices
without queue completion support fall back to 2D in this mode. Without browser
requestAnimationFrame, drawing remains synchronous. Mode is selected at construction.

`sync` is the default; the demo opts into animation-frame mode. Coalescing skips
intermediate canvas frames while model computations remain reactive. The bound is
per viewport, not a global device scheduler. Run `/tests/browser/gpu-scheduling.html`
for submission counts and controlled backpressure checks; its single update-loop
timings are observations, not frame-time or throughput qualification.

Viewport picking uses an immutable copy of the vertices and indices from its last
completed Canvas2D draw or WebGPU submission. Pending model/mesh/topology updates
cannot change that hit test before a new draw. `onVertexPick(index, distance, z,
frame)` adds exported `VertexPickFrame3D` metadata: source mesh identity, draw id,
dimensions and backend. Existing three-parameter handlers can ignore the extra
argument. Source mesh identity may refer to a replaced/disposed caller-owned mesh;
the hit test does not read that mesh again. The synchronous callback invocation restores its WASM core.

Copies are created only for viewports with a picking callback. Picking transports
the saved geometry into isolated Rust scratch without allocating reactive nodes or
changing the live tensor arena. This adds per-draw copy cost and context-local scratch
capacity; it does not establish compositor-confirmed presentation or bit-exact GPU
visibility. `pickNearestVertex3D(mesh, ...)` remains a live programmatic query.


The native browser render fixture additionally checks validated perspective camera
updates against GPU pixels and automatic complete-scene teardown (17 checks).
The compute fixture checks byte-budget rejection before native buffer allocation
and exact-budget WGSL execution with queue telemetry returning to zero (8 checks).
The scheduling fixture contains 6 checks. Run these pages with their visible Run
buttons; unit tests and package checks do not substitute for a browser run.

## Reproducing adoption traversal measurements

Run `npm --prefix packages/svet run bench:adoption:prepare` from the repository root,
then start the package Vite server and open `/tests/browser/adoption-bench.html`.
Preparation writes two independently compiled engine snapshots under the ignored
node_modules cache. They differ only in child traversal: the baseline uses a NodeList,
while the candidate uses firstChild/nextSibling pointers in the same traversal order.
Unique snapshot paths prevent cached old modules from contaminating new measurements.
The fixture records source hashes, browser identity and raw timings in its result.

The four scenarios cover ordinary insertion, plain and deep cross-document trees,
and trees with delegated events plus model bindings. Each has two warm-up rounds and
nine alternating measured rounds; timed sections include native writes, while tree
construction, correctness checks and teardown occur outside timing. This is a local
A/B tool, not a whole-application benchmark or a comparison against other frameworks.
