/**
 * # Svet WASM Bridge (`wasm-bridge.ts`)
 *
 * Connects TypeScript directly to the compiled Rust WebAssembly reactive core (`svet-wasm`).
 * - Synchronous instantiation so zero `async/await` ceremony is required at app boot.
 * - Supports isolated WASM instances (`createWasmCoreInstance` / `runInSsrContext`) so concurrent
 *   SSR requests never share global graph state!
 * - Zero-copy `Uint32Array` views directly into WASM linear memory for batch effect flushing
 *   and O(n log n) Longest Increasing Subsequence (LIS) reconciliation.
 */

import { WASM_EXPORTS } from './wasm-binary.ts';

export const KIND_SIGNAL = 0;
export const KIND_COMPUTED = 1;
export const KIND_EFFECT = 2;

export const STATE_CLEAN = 0;
export const STATE_CHECK = 1;
export const STATE_DIRTY = 2;
export const STATE_DISPOSED = 255;

export interface SvetWasmExports {
  memory: WebAssembly.Memory;
  svet_reset(): void;
  svet_create_node(kind: number): number;
  svet_dispose_node(id: number): void;
  svet_begin_tracking(target: number): void;
  svet_end_tracking(): void;
  svet_pause_tracking(): void;
  svet_resume_tracking(): void;
  svet_track_read(source: number): void;
  svet_trigger_write(source: number): void;
  svet_trigger_and_flush(source: number): number;
  svet_get_state(id: number): number;
  svet_set_state(id: number, state: number): void;
  svet_batch_start(): void;
  svet_batch_end(): number;
  svet_prepare_flush(): number;
  svet_get_flush_ptr(): number;
  svet_get_states_ptr(): number;
  svet_get_node_count(): number;
  svet_get_capacity(): number;
  svet_get_edge_count(): number;
  svet_alloc_u32(len: number): number;
  svet_free_u32(ptr: number, len: number): void;
  svet_compute_lis(ptr: number, len: number): number;
  svet_get_lis_ptr(): number;
  svet_prepare_lis_input(len: number): number;
  svet_compute_lis_buffered(len: number): number;
  svet_sync_sources_buffered(target: number, len: number): void;
  svet_link_1(target: number, s0: number): void;
  svet_link_2(target: number, s0: number, s1: number): void;
  svet_is_tracking(): number;
  // GPU + UMA + DUMA Arena Exports
  svet_gpu_get_arena_limit_f32(): number;
  svet_gpu_alloc_f32(len: number): number;
  svet_gpu_free_f32(offset: number, len: number): void;
  svet_gpu_get_arena_ptr(): number;
  svet_gpu_get_arena_len(): number;
  svet_gpu_mark_dirty(offset: number, len: number): void;
  svet_gpu_get_dirty_min(): number;
  svet_gpu_get_dirty_max(): number;
  svet_gpu_clear_dirty(): void;
  svet_gpu_collect_dirty_spans(): number;
  svet_gpu_vector_fma(
    srcOffset: number,
    dstOffset: number,
    len: number,
    scale: number,
    bias: number,
  ): void;
  // 3D Scene Graph Linear Memory Math Kernels
  svet_gpu_mat4_mul(aOffset: number, bOffset: number, dstOffset: number): void;
  svet_depends_on(target: number, source: number): number;
  svet_pending_effect_count(): number;
  svet_pending_effect_entry_count(): number;
  svet_gpu_valid_perspective(fovYRad: number, aspect: number, near: number, far: number): number;
  svet_gpu_mat4_perspective(
    dstOffset: number,
    fovYRad: number,
    aspect: number,
    near: number,
    far: number,
  ): void;
  svet_gpu_mat4_compose_trs(
    dstOffset: number,
    tx: number,
    ty: number,
    tz: number,
    rx: number,
    ry: number,
    rz: number,
    sx: number,
    sy: number,
    sz: number,
  ): void;
  svet_gpu_transform_positions_vec4(
    matOffset: number,
    vertsOffset: number,
    dstOffset: number,
    vertexCount: number,
  ): void;
  svet_gpu_clip_primitive_3d(
    offset: number,
    count: number,
    a: number,
    b: number,
    c: number,
    d: number,
    size: number,
  ): number;
  svet_gpu_clip_output_ptr(): number;
  svet_prepare_gpu_pick_vertices(length: number): number;
  svet_gpu_pick_snapshot_3d(
    count: number,
    indices: number,
    occlude: number,
    vw: number,
    vh: number,
    x: number,
    y: number,
    radius: number,
  ): number;
  svet_prepare_gpu_pick_indices(length: number): number;
  svet_gpu_pick_visible_vertex_3d(
    offset: number,
    count: number,
    indices: number,
    vw: number,
    vh: number,
    x: number,
    y: number,
    radius: number,
  ): number;
  svet_gpu_pick_nearest_vertex_3d(
    clipVertsOffset: number,
    vertexCount: number,
    viewportW: number,
    viewportH: number,
    clickX: number,
    clickY: number,
    maxRadiusPx: number,
  ): number;
}

let compiledModule: WebAssembly.Module | null = null;
let defaultCore: SvetWasmExports | null = null;
// Explicit synchronous scopes select one native core across compatible copies.
// Only live calls are stored; async continuations use their host carrier again.
const coreOverrideHost = globalThis as unknown as Record<symbol, SvetWasmExports[] | undefined>;
const coreOverrides = (coreOverrideHost[Symbol.for('svet.wasm-core.overrides.v1')] ??= []);
let asyncCoreResolver: (() => SvetWasmExports | undefined) | undefined;
let escapeAsyncContext: (<T>(callback: () => T) => T) | undefined;

/** Keep long-lived device event subscriptions outside a request's async store. */
export function withoutAsyncCoreContext<T>(callback: () => T): T {
  return escapeAsyncContext ? escapeAsyncContext(callback) : callback();
}

/** Internal adapter hook; the browser entry has no Node dependency. */
export function installAsyncCoreResolver(
  resolver: () => SvetWasmExports | undefined,
  escape?: <T>(callback: () => T) => T,
): void {
  asyncCoreResolver = resolver;
  escapeAsyncContext = escape;
}

/** Install this package's precompiled asset before any core is created. */
export function installSvetWasmModule(module: WebAssembly.Module): void {
  if (!(module instanceof WebAssembly.Module))
    throw TypeError('Expected a precompiled Svet WebAssembly.Module');
  if (compiledModule) {
    if (compiledModule === module) return;
    throw Error('Svet WASM module is already initialized');
  }
  const actual = WebAssembly.Module.exports(module).sort((a, b) =>
    a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
  );
  if (
    WebAssembly.Module.imports(module).length ||
    JSON.stringify(actual) !== JSON.stringify(WASM_EXPORTS)
  )
    throw TypeError('Incompatible Svet WASM export shape');
  compiledModule = module;
}

// The base64 copy of the core is not imported here: entries that want the zero-setup behaviour pull in
// `wasm-embedded.ts`, which registers it. `svet-engine/slim` leaves it out of the bundle entirely.
let embeddedWasmSource: (() => Uint8Array) | null = null;

/** Internal: registers the embedded copy of the core as the fallback used when no module was installed. */
export function installEmbeddedSvetWasmSource(decode: () => Uint8Array): void {
  embeddedWasmSource ??= decode;
}

/**
 * Compiles the core from `svet-engine/core.wasm` and installs it. Call it once, before the first signal,
 * effect or element is created. Needed with `svet-engine/slim`, which ships without the embedded copy;
 * with the default entry it is optional and lets the browser stream-compile and cache the binary.
 *
 * ```ts
 * import wasmUrl from 'svet-engine/core.wasm?url';
 * await loadSvetWasm(fetch(wasmUrl));
 * ```
 */
export async function loadSvetWasm(
  source: string | URL | Response | Promise<Response> | BufferSource | WebAssembly.Module,
): Promise<void> {
  if (source instanceof WebAssembly.Module) {
    installSvetWasmModule(source);
    return;
  }
  if (source instanceof ArrayBuffer || ArrayBuffer.isView(source)) {
    installSvetWasmModule(await WebAssembly.compile(source as BufferSource));
    return;
  }
  const response = await (typeof source === 'string' || source instanceof URL
    ? fetch(source)
    : source);
  if (!response.ok) {
    throw new Error(
      `Svet WASM core request failed: ${response.status} ${response.statusText}`.trim(),
    );
  }
  // `compileStreaming` insists on `application/wasm`; fall back to buffering for servers that send another type.
  const streamable =
    typeof WebAssembly.compileStreaming === 'function' &&
    (response.headers.get('content-type') ?? '').toLowerCase().startsWith('application/wasm');
  installSvetWasmModule(
    streamable
      ? await WebAssembly.compileStreaming(response)
      : await WebAssembly.compile(await response.arrayBuffer()),
  );
}

function getCompiledModule(): WebAssembly.Module {
  if (!compiledModule) {
    if (!embeddedWasmSource) {
      throw new Error(
        'Svet WASM core is not loaded. `svet-engine/slim` ships without the embedded binary: ' +
          'call `await loadSvetWasm(...)` with `svet-engine/core.wasm` (or `installSvetWasmModule`) before using the engine, ' +
          'or import `svet-engine` for the self-contained build.',
      );
    }
    compiledModule = new WebAssembly.Module(embeddedWasmSource() as BufferSource);
  }
  return compiledModule;
}

/**
 * Creates a brand-new isolated WebAssembly instance of the Svet reactive engine.
 * Ideal for concurrent SSR requests or multi-tenant sandboxes.
 */
export function createWasmCoreInstance(): SvetWasmExports {
  const instance = new WebAssembly.Instance(getCompiledModule(), {});
  return instance.exports as unknown as SvetWasmExports;
}

/**
 * Returns the currently active WASM core (either the isolated SSR context or the default singleton).
 */
export function getWasmCore(): SvetWasmExports {
  const activeCore = coreOverrides.length ? coreOverrides[coreOverrides.length - 1] : undefined;
  if (activeCore) return activeCore;
  const requestCore = asyncCoreResolver?.();
  if (requestCore) return requestCore;
  if (!defaultCore) {
    defaultCore = createWasmCoreInstance();
  }
  return defaultCore;
}

/**
 * Temporarily switches the active WASM core to `customCore` for the duration of `fn`.
 */
export function withWasmCore<T>(customCore: SvetWasmExports, fn: () => T): T {
  coreOverrides.push(customCore);
  try {
    return fn();
  } finally {
    coreOverrides.pop();
  }
}

const EMPTY_U32 = new Uint32Array(0);

/**
 * Reads the flushed effect IDs directly from WASM linear memory without per-item FFI calls.
 */
export function readFlushedEffects(core: SvetWasmExports): Uint32Array {
  const count = core.svet_prepare_flush();
  if (count === 0) {
    return EMPTY_U32;
  }
  const ptr = core.svet_get_flush_ptr();
  return new Uint32Array(core.memory.buffer, ptr, count);
}

/**
 * Computes the Longest Increasing Subsequence (LIS) in WASM linear memory in O(n log n) time.
 * Ignore marker: `0xFFFFFFFF` (`u32::MAX`) represents newly inserted items.
 */
export function computeLisWasm(indices: Uint32Array | number[]): Uint32Array {
  const core = getWasmCore();
  const len = indices.length;
  if (len === 0) return EMPTY_U32;

  const inPtr = core.svet_prepare_lis_input(len);
  const wasmInput = new Uint32Array(core.memory.buffer, inPtr, len);
  wasmInput.set(indices);
  const lisLen = core.svet_compute_lis_buffered(len);
  if (lisLen === 0) return EMPTY_U32;
  const outPtr = core.svet_get_lis_ptr();
  return new Uint32Array(core.memory.buffer, outPtr, lisLen);
}
