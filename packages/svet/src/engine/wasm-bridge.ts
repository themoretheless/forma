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

import { decodeWasmBinary } from './wasm-binary.ts';

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
    bias: number
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
    far: number
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
    sz: number
  ): void;
  svet_gpu_transform_positions_vec4(
    matOffset: number,
    vertsOffset: number,
    dstOffset: number,
    vertexCount: number
  ): void;
  svet_gpu_clip_primitive_3d(offset: number, count: number, a: number, b: number, c: number, d: number, size: number): number;
  svet_gpu_clip_output_ptr(): number;
  svet_prepare_gpu_pick_vertices(length: number): number;
  svet_gpu_pick_snapshot_3d(count: number, indices: number, occlude: number, vw: number, vh: number, x: number, y: number, radius: number): number;
  svet_prepare_gpu_pick_indices(length: number): number;
  svet_gpu_pick_visible_vertex_3d(offset: number, count: number, indices: number, vw: number, vh: number, x: number, y: number, radius: number): number;
  svet_gpu_pick_nearest_vertex_3d(
    clipVertsOffset: number,
    vertexCount: number,
    viewportW: number,
    viewportH: number,
    clickX: number,
    clickY: number,
    maxRadiusPx: number
  ): number;
}

let compiledModule: WebAssembly.Module | null = null;
let defaultCore: SvetWasmExports | null = null;
let activeCore: SvetWasmExports | null = null;
let asyncCoreResolver: (() => SvetWasmExports | undefined) | undefined;
let escapeAsyncContext: (<T>(callback: () => T) => T) | undefined;

/** Keep long-lived device event subscriptions outside a request's async store. */
export function withoutAsyncCoreContext<T>(callback: () => T): T {
  return escapeAsyncContext ? escapeAsyncContext(callback) : callback();
}

/** Internal adapter hook; the browser entry has no Node dependency. */
export function installAsyncCoreResolver(resolver: () => SvetWasmExports | undefined, escape?: <T>(callback: () => T) => T): void {
  asyncCoreResolver = resolver;
  escapeAsyncContext = escape;
}

function getCompiledModule(): WebAssembly.Module {
  if (!compiledModule) {
    const bytes = decodeWasmBinary();
    compiledModule = new WebAssembly.Module(bytes as BufferSource);
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
  if (activeCore) {
    return activeCore;
  }
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
  const prev = activeCore;
  activeCore = customCore;
  try {
    return fn();
  } finally {
    activeCore = prev;
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
