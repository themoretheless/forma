# Svet Engine (`svet-engine`)

**Svet** is a production-ready hybrid **TypeScript + Rust WebAssembly + WebGPU (UMA/DUMA)** UI framework engineered to eliminate the architectural bottlenecks of React, Vue, Svelte, and Blazor.

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

5. **Deep GPU + UMA + DUMA Integration ([`src/engine/gpu.ts`](src/engine/gpu.ts))**
   - **UMA Mode (Unified Memory Architecture — Apple Silicon / Integrated SoCs)**: 16-byte-aligned `GpuTensorSignal` slices live directly in the shared WASM `gpu_f32_arena` with zero staging copies and `gpu_free_blocks` recycling.
   - **DUMA Mode (Discrete VRAM + Host RAM over PCIe)**: Tracks 128-byte (32-float) dirty cache-line blocks in WASM (`svet_gpu_mark_dirty`) and transfers **only** the 16-byte-aligned dirty window via partial DMA (`queue.writeBuffer`).
   - **Reactive Compute Kernels (`dispatchGpuComputeKernel`, `executeGpuVectorFma`)**: Declaratively binds WGSL compute shaders and WASM kernels into `effect()`.

6. **Full-Stack SSR (`renderToString`), $O(1)$ Zero-Hydration Resumability (`resumeContainer`), & Optimistic RPC (`createServerAction`) ([`src/engine/resumable.ts`](src/engine/resumable.ts))**
   - Concurrent request isolation via `runWithIsolatedContext()` (separate `WebAssembly.Instance` per SSR request).
   - Resumes server-rendered HTML in $O(1)$ without re-running component trees on the client.

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

# Start interactive Vite dev server
npm run dev

# Build production bundle
npm run build
```

## Async resource lifecycle

`createResource(source, fetcher)` cancels the previous request when its source
changes. A source value of `false`, `null`, or `undefined` cancels pending work
and pauses automatic fetching, retaining the last successful value.

`resource.abort()` immediately clears `loading` and releases the resource's
`Suspense` pending state, even if the fetcher ignores its `AbortSignal`.
Superseded requests cannot overwrite current data or errors, or release
`Suspense` while their replacement is still pending. Disposing the owner aborts
pending work; subsequent `refetch()` calls resolve to `undefined` without fetching.

## Effect handle lifetime

Each `effect()` call returns its own handle. Calling `dispose()` repeatedly is
safe, including after its internal WASM slot and JavaScript entry have been
reused by another effect. The handle's `id` remains the original numeric slot ID;
slot IDs may be recycled and are not unique lifetime identifiers. Owner cleanup
uses the same lifetime-bound handle, so manually disposing an owned effect cannot
cause later owner teardown to stop an unrelated replacement.
