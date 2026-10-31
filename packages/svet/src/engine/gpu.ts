/**
 * WASM-backed tensor storage, reactive host kernels and WebGPU arena uploads.
 * Tensor views share WASM memory with JavaScript. WebGPU queue.writeBuffer copies
 * data to device buffers in both upload policies; physical UMA is not proof of
 * zero-copy browser GPU access. Architecture selection is a policy/hint.
 */

import {
  effect,
  batch,
  untrack,
  flushEffects,
  invalidateSingleSignalFastPath,
  getReactiveGeneration,
  registerReactiveResetCleanup,
  getCurrentOwner,
  isOwnerScopeDisposed,
  runWithCapturedOwner,
  runUntrackedInCore,
  onCleanup,
  type ReadonlySignal,
  signal,
  type Signal,
  trackNodeRead,
  triggerNodeWrite,
  disposeTrackedNode,
} from './reactivity.ts';
import { getWasmCore, withWasmCore, withoutAsyncCoreContext, KIND_SIGNAL, type SvetWasmExports } from './wasm-bridge.ts';

export type MemoryArchitecture = 'UMA' | 'DUMA';

export interface GpuComputeQueueTelemetry {
  /** Device-wide unfinished hardware calls, including cancelled queue entries awaiting drainage. */
  pendingComputeCount: number;
  /** Logical input/private-output snapshot bytes reserved by those calls; not total GPU/process memory. */
  pendingComputeSnapshotBytes: number;
  maxPendingComputeCount: number;
  maxPendingComputeBytes: number;
}

export interface GpuDeviceTelemetry extends GpuComputeQueueTelemetry {
  architecture: MemoryArchitecture;
  webgpuAvailable: boolean;
  arenaFloatCount: number;
  arenaByteLength: number;
  lastSyncBytesTransferred: number;
  totalSyncCount: number;
  totalBytesTransferred: number;
}

export interface GpuTensorSignal extends ReadonlySignal<Float32Array> {
  /** Starting float index in the WASM `gpu_f32_arena` */
  readonly offset: number;
  /** Number of `f32` elements in this tensor */
  readonly length: number;
  /** Byte offset inside the GPU arena buffer */
  readonly byteOffset: number;
  /** Logical byte length; the arena allocation is padded to 16 bytes. */
  readonly byteLength: number;
  /** Returns a live zero-copy `Float32Array` view into WASM linear memory */
  view(): Float32Array;
  /** Writes values into the tensor at `startIndex`, marks dirty cache lines in WASM, and triggers reactive dependents */
  write(values: ArrayLike<number>, startIndex?: number): void;
  /** Mutates a single scalar element in-place, marks its 128-byte cache line dirty, and triggers dependents */
  setAt(index: number, value: number): void;
}

export interface WebGpuQueueLike {
  writeBuffer(
    buffer: any,
    bufferOffset: number,
    data: BufferSource | SharedArrayBuffer,
    dataOffset?: number,
    size?: number
  ): void;
  submit?(commandBuffers: any[]): void;
  onSubmittedWorkDone?(): PromiseLike<void>;
}

export interface WebGpuDeviceLike {
  queue: WebGpuQueueLike;
  createBuffer?(descriptor: { size: number; usage: number; mappedAtCreation?: boolean }): any;
  createShaderModule?(descriptor: { code: string }): any;
  createComputePipeline?(descriptor: any): any;
  createRenderPipeline?(descriptor: any): any;
  createTexture?(descriptor: any): any;
  createCommandEncoder?(): any;
  createBindGroup?(descriptor: any): any;
  pushErrorScope?(filter: 'validation' | 'out-of-memory' | 'internal'): void;
  popErrorScope?(): PromiseLike<{ message: string } | null>;
  lost?: PromiseLike<{ reason?: string; message?: string }>;
  destroy?(): void;
  limits?: { maxComputeWorkgroupsPerDimension?: number; maxBufferSize?: number; maxStorageBufferBindingSize?: number; maxTextureDimension2D?: number };
}

export interface GpuSubsystemConfig {
  /** Shared device snapshot budget; default 256 MiB. Existing work is never evicted. */
  maxPendingComputeBytes?: number;
  architecture?: MemoryArchitecture;
  device?: WebGpuDeviceLike | null;
}

interface GpuState {
  renderers: Set<(reset: boolean) => void>;
  configurationCleanup: { owner: NonNullable<ReturnType<typeof getCurrentOwner>>; callback: () => void } | null;
  lossSubscription: { device: WebGpuDeviceLike; reference: WeakRef<GpuState> } | null;
  contextLifetime: AbortController;
  lifetime: AbortController;
  version: number;
  initializationVersion: number;
  ownsDevice: boolean;
  generation: number;
  architecture: MemoryArchitecture;
  device: WebGpuDeviceLike | null;
  buffer: any;
  bufferSize: number;
  forceUpload: boolean;
  lastBytes: number;
  syncCount: number;
  totalBytes: number;
}
const gpuStates = new WeakMap<SvetWasmExports, GpuState>();
function unregisterConfiguration(state: GpuState): void {
  const registration = state.configurationCleanup;
  state.configurationCleanup = null;
  if (registration && !registration.owner.disposed) {
    const index = registration.owner.disposables.indexOf(registration.callback);
    if (index >= 0) registration.owner.disposables.splice(index, 1);
  }
}
function notifyRenderers(state: GpuState, reset = false): void {
  for (const listener of Array.from(state.renderers)) {
    try { listener(reset); } catch { /* one renderer must not prevent other cleanup */ }
  }
}
function detachDevice(state: GpuState, reason: Error, notify = true): void {
  unregisterConfiguration(state);
  state.lifetime.abort(reason);
  if (state.lossSubscription) {
    deviceLossHubs.get(state.lossSubscription.device)?.states.delete(state.lossSubscription.reference);
    state.lossSubscription = null;
  }
  const buffer = state.buffer, device = state.device, owned = state.ownsDevice;
  state.buffer = null; state.bufferSize = 0; state.forceUpload = true; state.device = null; state.ownsDevice = false;
  try { if (notify) notifyRenderers(state); } finally {
    try { buffer?.destroy?.(); } finally { if (owned) device?.destroy?.(); }
  }
}
type DeviceLossHub = { states: Set<WeakRef<GpuState>>; message?: string };
const deviceLossHubs = new WeakMap<WebGpuDeviceLike, DeviceLossHub>();
function subscribeDeviceLoss(device: WebGpuDeviceLike, hub: DeviceLossHub): void {
  const lost = (message: string) => {
    hub.message = message;
    for (const reference of Array.from(hub.states)) {
      const state = reference.deref();
      if (state?.device === device) {
        try { detachDevice(state, new Error(`GPU device lost: ${message}`)); } catch { /* notify other contexts too */ }
      } else hub.states.delete(reference);
    }
  };
  withoutAsyncCoreContext(() => {
    Promise.resolve(device.lost!).then(info => lost(info.message || info.reason || 'unknown'), error => lost(String(error))).catch(() => {});
  });
}
function watchDeviceLoss(state: GpuState): void {
  const device = state.device;
  if (!device?.lost || state.lossSubscription?.device === device) return;
  let hub = deviceLossHubs.get(device);
  if (!hub) {
    hub = { states: new Set() }; deviceLossHubs.set(device, hub); subscribeDeviceLoss(device, hub);
  }
  if (hub.message !== undefined) { detachDevice(state, new Error(`GPU device lost: ${hub.message}`)); return; }
  for (const reference of hub.states) if (!reference.deref()) hub.states.delete(reference);
  const reference = new WeakRef(state); hub.states.add(reference);
  state.lossSubscription = { device, reference };
}
function resetGpuState(state: GpuState, core: SvetWasmExports): void {
  notifyRenderers(state, true);
  state.lifetime.abort(new Error('GPU context reset'));
  state.contextLifetime.abort(new Error('GPU context reset'));
  state.lifetime = new AbortController();
  state.contextLifetime = new AbortController();
  const buffer = state.buffer;
  Object.assign(state, { generation: getReactiveGeneration(core), buffer: null, bufferSize: 0, forceUpload: true,
    lastBytes: 0, syncCount: 0, totalBytes: 0 });
  buffer?.destroy?.();
}
function gpuState(core = getWasmCore()): GpuState {
  const generation = getReactiveGeneration(core);
  let state = gpuStates.get(core);
  if (!state) {
    state = { renderers: new Set(), configurationCleanup: null, lossSubscription: null, contextLifetime: new AbortController(), lifetime: new AbortController(), version: 0, initializationVersion: 0, ownsDevice: false, generation, architecture: 'UMA', device: null, buffer: null, bufferSize: 0,
      forceUpload: true, lastBytes: 0, syncCount: 0, totalBytes: 0 };
    gpuStates.set(core, state);
    const created = state;
    registerReactiveResetCleanup(core, () => resetGpuState(created, core));
  } else if (state.generation !== generation) resetGpuState(state, core);
  return state;
}

/** Configures an upload policy and optional device for the current WASM context. */
export function configureGpuSubsystem(config: GpuSubsystemConfig = {}): GpuDeviceTelemetry {
  const owner = getCurrentOwner();
  if (isOwnerScopeDisposed(owner)) throw new Error('GPU configuration owner disposed');
  const core = getWasmCore(), generation = getReactiveGeneration(core), state = gpuState(core);
  const initialization = state.initializationVersion;
  // Read user-controlled fields once before publishing any configuration change.
  config = { architecture: config.architecture, device: config.device, maxPendingComputeBytes: config.maxPendingComputeBytes };
  if (generation !== getReactiveGeneration(core)) throw new Error('GPU context reset during configuration');
  if (isOwnerScopeDisposed(owner)) throw new Error('GPU configuration owner disposed');
  if (initialization !== state.initializationVersion) throw new Error('GPU configuration changed during configuration');
  if (config.architecture !== undefined && config.architecture !== 'UMA' && config.architecture !== 'DUMA') {
    throw new TypeError('Invalid GPU memory architecture');
  }
  if (config.maxPendingComputeBytes !== undefined) {
    if (!Number.isSafeInteger(config.maxPendingComputeBytes) || config.maxPendingComputeBytes < 0) {
      throw new RangeError('maxPendingComputeBytes must be a nonnegative safe integer');
    }
    const device = config.device === undefined ? state.device : config.device;
    if (!device) throw new Error('A GPU device is required to configure the compute snapshot budget');
    computeSnapshotBudgets.set(device, config.maxPendingComputeBytes);
  }
  unregisterConfiguration(state);
  state.initializationVersion++;
  if (config.architecture) state.architecture = config.architecture;
  else if (!config.device) {
    const appleHint = typeof navigator !== 'undefined' && /Mac|Apple/.test(navigator.platform || navigator.userAgent || '');
    state.architecture = appleHint ? 'UMA' : 'DUMA';
  }
  const deviceChanged = config.device !== undefined && config.device !== state.device;
  if (deviceChanged) {
    detachDevice(state, new Error('GPU device configuration changed'), false);
    state.lifetime = new AbortController();
    state.device = config.device!;
    watchDeviceLoss(state);
  }
  if (deviceChanged) notifyRenderers(state);
  const version = ++state.version;
  if (owner && !owner.disposed) {
    const callback = () => {
      if (state.version !== version) return;
      detachDevice(state, new Error('GPU configuration owner disposed'));
      state.version++;
    };
    state.configurationCleanup = { owner, callback };
    owner.disposables.push(callback);
  }
  return getGpuTelemetry();
}

export function resetGpuTelemetry(): void {
  const state = gpuState();
  state.lastBytes = 0; state.syncCount = 0; state.totalBytes = 0;
}

const tensorRecords = new WeakMap<GpuTensorSignal, { core: SvetWasmExports; revision: number }>();
function reviseTensor(tensor: GpuTensorSignal): void {
  const record = tensorRecords.get(tensor);
  if (record) record.revision++;
}

/**
 * Allocates a 16-byte aligned reactive GPU Tensor Signal directly in the Rust WASM `gpu_f32_arena`.
 * Zero JS Heap array copies: `tensor()` and `tensor.view()` return a direct `Float32Array` view over WASM memory.
 */
export function createGpuTensor(initialOrLength: number | ArrayLike<number>): GpuTensorSignal {
  const core = getWasmCore(), owner = getCurrentOwner();
  const generation = getReactiveGeneration(core);
  const assertConstructionCurrent = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('GPU tensor construction owner disposed');
    if (generation !== getReactiveGeneration(core)) throw new Error('GPU context reset during tensor initialization');
  };
  assertConstructionCurrent();
  const length = typeof initialOrLength === 'number' ? initialOrLength : initialOrLength.length;
  assertConstructionCurrent();
  if (!Number.isInteger(length) || length < 0 || length > core.svet_gpu_get_arena_limit_f32()) throw new RangeError('Invalid GPU tensor length');
  // Snapshot before arena allocation: input may borrow WASM memory that arena
  // growth detaches, and ArrayLike getters may throw or reset the graph.
  let initialValues: Float32Array | undefined;
  if (typeof initialOrLength !== 'number') {
    initialValues = new Float32Array(length);
    if (ArrayBuffer.isView(initialOrLength)) initialValues.set(initialOrLength);
    else for (let i = 0; i < length; i++) initialValues[i] = initialOrLength[i];
    assertConstructionCurrent();
  }
  const offset = core.svet_gpu_alloc_f32(length) >>> 0;
  if (offset === 0xffffffff) throw new RangeError('GPU tensor arena capacity exceeded');
  const id = core.svet_create_node(KIND_SIGNAL);
  invalidateSingleSignalFastPath();
  let disposed = false;
  const assertLive = () => {
    if (disposed || isOwnerScopeDisposed(owner) || generation !== getReactiveGeneration(core)) throw new Error('GPU tensor has been disposed or invalidated');
    if (getWasmCore() !== core) throw new Error('GPU tensor belongs to a different reactive context');
  };

  let cachedBuffer: ArrayBufferLike | null = null;
  let cachedPtr = -1;
  let cachedArray: Float32Array | null = null;

  const getLiveView = (): Float32Array => {
    assertLive();
    const currentBuffer = core.memory.buffer;
    const arenaPtr = core.svet_gpu_get_arena_ptr();
    if (
      cachedArray !== null &&
      cachedBuffer === currentBuffer &&
      cachedPtr === arenaPtr &&
      currentBuffer.byteLength > 0
    ) {
      return cachedArray;
    }
    const byteStart = arenaPtr + offset * 4;
    cachedBuffer = currentBuffer;
    cachedPtr = arenaPtr;
    cachedArray = new Float32Array(currentBuffer, byteStart, length);
    return cachedArray;
  };

  if (initialValues) {
    getLiveView().set(initialValues);
    core.svet_gpu_mark_dirty(offset, length);
  }

  const getter = (() => {
    assertLive();
    trackNodeRead(id);
    return getLiveView();
  }) as GpuTensorSignal;

  tensorRecords.set(getter, { core, revision: 0 });
  (getter as any).id = id;
  (getter as any).offset = offset;
  Object.defineProperty(getter, 'length', { value: length, enumerable: true });
  (getter as any).byteOffset = offset * 4;
  (getter as any).byteLength = length * 4;

  getter.peek = () => getLiveView();
  getter.view = () => getLiveView();

  getter.write = (values: ArrayLike<number>, startIndex = 0) => {
    assertLive();
    const sourceLength = values.length;
    if (!Number.isInteger(startIndex) || startIndex < 0 || startIndex > length || !Number.isInteger(sourceLength) || sourceLength < 0) throw new RangeError('Invalid GPU tensor write range');
    const writeLen = Math.min(sourceLength, length - startIndex);
    if (writeLen <= 0) return;

    let source: ArrayLike<number>;
    if (ArrayBuffer.isView(values)) {
      source = writeLen === sourceLength ? values : (values as Float32Array).subarray(0, writeLen);
    } else {
      // ArrayLike access and numeric conversion may throw, grow WASM memory or
      // dispose this tensor. Complete them privately before touching live storage.
      const snapshot = new Float32Array(writeLen);
      for (let i = 0; i < writeLen; i++) snapshot[i] = values[i];
      source = snapshot;
    }
    // Reacquire and revalidate only after all user-defined source conversion.
    // TypedArray.set also preserves overlapping borrowed-view copy semantics.
    getLiveView().set(source, startIndex);
    core.svet_gpu_mark_dirty(offset + startIndex, writeLen);
    reviseTensor(getter);
    triggerNodeWrite(id);
  };

  getter.setAt = (index: number, value: number) => {
    assertLive();
    if (!Number.isInteger(index) || index < 0 || index >= length) throw new RangeError('Invalid GPU tensor index');
    const next = Math.fround(value);
    // Numeric conversion can dispose/reallocate the tensor or grow WASM memory.
    // Validate lifetime and acquire the current buffer only after it completes.
    const view = getLiveView();
    if (!Object.is(view[index], next)) {
      view[index] = next;
      core.svet_gpu_mark_dirty(offset + index, 1);
      reviseTensor(getter);
      triggerNodeWrite(id);
    }
  };

  const cleanup = () => getter.dispose();
  getter.dispose = () => {
    if (disposed) return;
    disposed = true;
    if (owner && !owner.disposed) {
      const index = owner.disposables.indexOf(cleanup);
      if (index >= 0) owner.disposables.splice(index, 1);
    }
    cachedArray = null; cachedBuffer = null;
    reviseTensor(getter);
    if (generation !== getReactiveGeneration(core)) return;
    core.svet_gpu_free_f32(offset, length);
    disposeTrackedNode(core, id);
  };

  onCleanup(cleanup);

  return getter;
}

/**
 * Synchronizes the WASM GPU Arena with the hardware GPU / VRAM buffer:
 * Both policies use queue.writeBuffer. UMA uploads the dirty float window;
 * DUMA aligns partial transfers and collects disjoint dirty cache-line spans.
 * A new/grown buffer receives the full arena so unchanged data is preserved.
 */
export function syncGpuArena(): {
  architecture: MemoryArchitecture;
  dirtyFloatMin: number;
  dirtyFloatMax: number;
  bytesTransferred: number;
  spansTransferred?: number;
} {
  const core = getWasmCore();
  const state = gpuState(core);
  const arenaBytes = core.svet_gpu_get_arena_len() * 4;
  if (state.device && arenaBytes && (!state.buffer || state.bufferSize < arenaBytes)) {
    if (!state.device.createBuffer) throw new TypeError('GPU device does not provide createBuffer');
    const buffer = state.device.createBuffer({ size: arenaBytes, usage: 0x80 | 0x08 | 0x04 });
    if (!buffer) throw new Error('GPU buffer allocation failed');
    state.buffer?.destroy?.();
    state.buffer = buffer; state.bufferSize = arenaBytes; state.forceUpload = true;
  }
  if (state.device && state.forceUpload && arenaBytes) core.svet_gpu_mark_dirty(0, arenaBytes / 4);
  const minFloat = core.svet_gpu_get_dirty_min() >>> 0;
  const maxFloat = core.svet_gpu_get_dirty_max();
  state.lastBytes = 0;
  if (minFloat >= maxFloat || minFloat === 0xffffffff) {
    return { architecture: state.architecture, dirtyFloatMin: 0, dirtyFloatMax: 0, bytesTransferred: 0, spansTransferred: 0 };
  }
  let spansCount = 0;
  try {
    if (state.device) {
      let spans: Array<[number, number]>;
      if (state.forceUpload) spans = [[0, arenaBytes]];
      else if (state.architecture === 'UMA') spans = [[minFloat * 4, (maxFloat - minFloat) * 4]];
      else {
        const count = core.svet_gpu_collect_dirty_spans();
        if (count > 1) {
          const data = new Uint32Array(core.memory.buffer, core.svet_get_lis_ptr(), count * 2);
          spans = Array.from({ length: count }, (_, i) => [data[i * 2] * 4, data[i * 2 + 1] * 4]);
        } else spans = [[minFloat * 4, (maxFloat - minFloat) * 4]];
        spans = spans.map(([start, length]) => {
          const aligned = Math.floor(start / 16) * 16;
          return [aligned, Math.min(Math.ceil((start + length) / 16) * 16, arenaBytes) - aligned];
        });
      }
      // Construct views after WASM span collection, which can grow linear memory.
      const arena = new Uint8Array(core.memory.buffer, core.svet_gpu_get_arena_ptr(), arenaBytes);
      for (const [start, length] of spans) {
        state.device.queue.writeBuffer(state.buffer, start, arena, start, length);
        state.lastBytes += length; state.totalBytes += length; spansCount++;
      }
    }
    core.svet_gpu_clear_dirty();
    state.forceUpload = false;
  } finally {
    state.syncCount++;
  }
  return { architecture: state.architecture, dirtyFloatMin: minFloat, dirtyFloatMax: maxFloat,
    bytesTransferred: state.lastBytes, spansTransferred: spansCount };
}

/**
 * Executes a reactive WASM SIMD multiply/add (with separate float32 rounding) (`output[i] = input[i] * scale + bias`)
 * directly over the UMA/DUMA arena and triggers any reactive subscribers of `output`.
 */
export function executeGpuVectorFma(
  input: GpuTensorSignal,
  output: GpuTensorSignal,
  scale: number,
  bias: number
): void {
  const core = getWasmCore();
  const len = Math.min(input.length, output.length);
  // Read input so calling this inside `effect()` tracks `input` automatically!
  input();
  output.peek();
  reviseTensor(output);
  core.svet_gpu_vector_fma(input.offset, output.offset, len, scale, bias);
  triggerNodeWrite(output.id);
}

export interface WebGpuComputeDispatchOptions {
  /** WGSL source used by the async entry; the synchronous entry uses fallbackKernel. */
  wgsl: string;
  /** Input tensor signals passed to the host fallback. */
  inputs: GpuTensorSignal[];
  /** Output tensor signal written by the host fallback. */
  output: GpuTensorSignal;
  /** Hardware workgroup count for the async entry; ignored by the synchronous host entry. */
  workgroups?: [number, number?, number?];
  /** High-speed CPU/WASM fallback kernel executed when WebGPU hardware device is absent */
  fallbackKernel?: (inputs: Float32Array[], output: Float32Array) => void;
}

/** Snapshot enumerable metadata, stopping between user-controlled accesses. */
function snapshotGpuOptionMetadata<T extends object>(options: T, assertCurrent: () => void): T {
  const snapshot = {} as T;
  const keys = Reflect.ownKeys(options); assertCurrent();
  for (const key of keys) {
    const descriptor = Reflect.getOwnPropertyDescriptor(options, key); assertCurrent();
    if (!descriptor?.enumerable) continue;
    const value = Reflect.get(options, key); assertCurrent();
    Object.defineProperty(snapshot, key, {value, enumerable: true, writable: true, configurable: true});
  }
  return snapshot;
}

function snapshotGpuComputeOptions<T extends WebGpuComputeDispatchOptions>(options: T, assertCurrent: () => void): T {
  const snapshot = snapshotGpuOptionMetadata(options, assertCurrent);
  // The previous entry also accepted inherited/nonenumerable inputs.
  const inputs = Object.hasOwn(snapshot, 'inputs') ? snapshot.inputs : Reflect.get(options, 'inputs');
  assertCurrent();
  const copied: GpuTensorSignal[] = [];
  for (const tensor of inputs) { assertCurrent(); copied.push(tensor); }
  assertCurrent();
  Object.defineProperty(snapshot, 'inputs', {value: copied, enumerable: true, writable: true, configurable: true});
  return snapshot;
}

/**
 * Initializes the hardware WebGPU device (`navigator.gpu.requestAdapter() -> requestDevice()`)
 * and selects an upload policy from adapter/browser hints, not a proven physical memory topology.
 * Clears the configured device if no adapter is available or initialization fails.
 */
export async function initWebGpuHardware(): Promise<GpuDeviceTelemetry> {
  const owner = getCurrentOwner();
  if (isOwnerScopeDisposed(owner)) throw new Error('GPU initialization owner disposed');
  const core = getWasmCore(), state = gpuState(core);
  const generation = getReactiveGeneration(core), initialization = ++state.initializationVersion;
  const current = () => !isOwnerScopeDisposed(owner) && generation === getReactiveGeneration(core) && state.initializationVersion === initialization;
  let created: WebGpuDeviceLike | undefined;
  const releaseCreated = () => {
    const device = created; created = undefined;
    try { device?.destroy?.(); } catch { /* Preserve cancellation/fallback when device teardown fails. */ }
  };
  try {
    const gpu = typeof navigator !== 'undefined' ? (navigator as any).gpu : undefined;
    const adapter = gpu ? await gpu.requestAdapter({ powerPreference: 'high-performance' }) : null;
    if (!current()) throw new Error('GPU initialization cancelled');
    if (adapter) {
      const info = adapter.info || (await adapter.requestAdapterInfo?.()) || {};
      const hint = String(info.architecture || info.vendor || '').toLowerCase();
      const unifiedHint = !adapter.isFallbackAdapter && /apple|unified|arm/.test(hint);
      if (!current()) throw new Error('GPU initialization cancelled');
      created = await adapter.requestDevice();
      if (!created || !created.createBuffer || !created.queue) throw new Error('Invalid GPU device returned by initialization');
      if (!current()) throw new Error('GPU initialization cancelled');
      return runWithCapturedOwner(core, owner, () => {
        const report = configureGpuSubsystem({ architecture: unifiedHint ? 'UMA' : 'DUMA', device: created });
        if (state.device === created) { state.ownsDevice = true; created = undefined; }
        else releaseCreated();
        return report;
      });
    }
  } catch (error) {
    releaseCreated();
    if (!current()) throw new Error('GPU initialization cancelled', { cause: error });
  }
  return runWithCapturedOwner(core, owner, () => configureGpuSubsystem({ device: null }));
}

/** Executes the synchronous host fallback, publishing its output to the graph.
 * WGSL execution is not implemented by this synchronous compatibility entry.
 */
export function dispatchGpuComputeKernel(options: WebGpuComputeDispatchOptions): void {
  const core = getWasmCore(), generation = getReactiveGeneration(core), owner = getCurrentOwner();
  const assertContext = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
    if (generation !== getReactiveGeneration(core)) throw new Error('GPU context reset during computation');
  };
  assertContext();
  options = snapshotGpuComputeOptions(options, assertContext);
  if (!options.fallbackKernel) throw new Error('Synchronous compute requires a fallbackKernel; use dispatchGpuComputeKernelAsync for WGSL');
  const inputViews = options.inputs.map(t => t().slice());
  const workingOutput = options.output.peek().slice();
  const record = tensorRecords.get(options.output);
  if (record?.core !== core) throw new Error('GPU output belongs to a different reactive context');
  assertContext();
  const revision = record.revision;
  const returned: unknown = runUntrackedInCore(core, () => {
    assertContext();
    return runWithCapturedOwner(core, owner, () => options.fallbackKernel!(inputViews, workingOutput));
  });
  if (returned != null && typeof (returned as PromiseLike<unknown>).then === 'function') {
    // Observe late rejection while keeping all callback-owned arrays private.
    void Promise.resolve(returned).catch(() => {});
    throw new TypeError('Synchronous compute received an asynchronous callback; use dispatchGpuComputeKernelAsync');
  }
  if (generation !== getReactiveGeneration(core)) throw new Error('GPU context reset during computation');
  if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
  if (record.revision !== revision) throw new Error('GPU output changed during computation');
  options.output.write(workingOutput);
}

export interface AsyncWebGpuComputeDispatchOptions extends Omit<WebGpuComputeDispatchOptions, 'fallbackKernel'> {
  /** Entry point defaults to main. Inputs use bindings 0..N-1, output uses N. */
  entryPoint?: string;
  signal?: AbortSignal;
  fallbackKernel?: (inputs: Float32Array[], output: Float32Array) => void | PromiseLike<void>;
}
export interface GpuComputeResult { backend: 'webgpu' | 'fallback'; elements: number; uploadedBytes: number; readbackBytes: number }

/** An admission failure; no tensor snapshots or output revisions were reserved. */
export class GpuComputeCapacityError extends Error {
  readonly queue: Readonly<GpuComputeQueueTelemetry>;
  constructor(
    readonly reason: 'count' | 'snapshot-bytes',
    queue: GpuComputeQueueTelemetry,
    /** Null when the count limit rejects before tensor inspection. */
    readonly requestedSnapshotBytes: number | null = null,
  ) {
    super(reason === 'count' ? 'WebGPU compute queue is full (32 pending computations)' : 'WebGPU compute snapshot byte budget exceeded');
    this.name = 'GpuComputeCapacityError';
    this.queue = Object.freeze({ ...queue });
  }
}


const computeQueues = new WeakMap<WebGpuDeviceLike, Promise<void>>();
// Includes running work and cancelled entries whose queue closures still retain snapshots.
const pendingComputations = new WeakMap<WebGpuDeviceLike, number>();
const MAX_PENDING_COMPUTATIONS = 32;
const pendingSnapshotBytes = new WeakMap<WebGpuDeviceLike, number>();
const computeSnapshotBudgets = new WeakMap<WebGpuDeviceLike, number>();
const DEFAULT_COMPUTE_SNAPSHOT_BUDGET = 256 * 1024 * 1024;
function computeQueueTelemetry(device: WebGpuDeviceLike | null): GpuComputeQueueTelemetry {
  return {
    pendingComputeCount: device ? pendingComputations.get(device) ?? 0 : 0,
    pendingComputeSnapshotBytes: device ? pendingSnapshotBytes.get(device) ?? 0 : 0,
    maxPendingComputeCount: MAX_PENDING_COMPUTATIONS,
    maxPendingComputeBytes: device ? computeSnapshotBudgets.get(device) ?? DEFAULT_COMPUTE_SNAPSHOT_BUDGET : DEFAULT_COMPUTE_SNAPSHOT_BUDGET,
  };
}

function checkGpuSignals(signals: AbortSignal[]): void {
  for (const signal of signals) signal.throwIfAborted();
}
async function waitForGpu<T>(promise: PromiseLike<T>, signals: AbortSignal[]): Promise<T> {
  // Always observe the operation, including when cancellation has already occurred.
  const operation = Promise.resolve(promise);
  operation.catch(() => {});
  checkGpuSignals(signals);
  const remove: Array<() => void> = [];
  const cancelled = new Promise<never>((_, reject) => {
    for (const signal of signals) {
      const abort = () => reject(signal.reason);
      signal.addEventListener('abort', abort, { once: true });
      remove.push(() => signal.removeEventListener('abort', abort));
    }
  });
  try { return await Promise.race([operation, cancelled]); }
  finally { for (const cleanup of remove) cleanup(); }
}

/**
 * Executes WGSL on a configured device and atomically publishes mapped readback.
 * Inputs are snapshots captured at call time. Default workgroups assume 64 lanes;
 * supply workgroups when the shader uses a different workgroup size.
 * Missing compute support uses an explicit host fallback, never a silent no-op.
 */
export async function dispatchGpuComputeKernelAsync(options: AsyncWebGpuComputeDispatchOptions): Promise<GpuComputeResult> {
  const core = getWasmCore(), generation = getReactiveGeneration(core), owner = getCurrentOwner();
  const assertContext = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
    if (generation !== getReactiveGeneration(core)) throw new Error('GPU context reset during computation');
  };
  assertContext();
  options = snapshotGpuComputeOptions(options, assertContext);
  options.signal?.throwIfAborted();
  assertContext();
  const state = gpuState(core);
  const admissionDevice = state.device;
  const hardware = admissionDevice?.createComputePipeline && admissionDevice.createShaderModule &&
    admissionDevice.createBindGroup && admissionDevice.createBuffer && admissionDevice.createCommandEncoder && admissionDevice.queue.submit;
  assertContext();
  let releaseAdmission = () => {};
  if (hardware) {
    const pending = pendingComputations.get(admissionDevice!) ?? 0;
    if (pending >= MAX_PENDING_COMPUTATIONS) throw new GpuComputeCapacityError('count', computeQueueTelemetry(admissionDevice!));
    let snapshotBytes = 0;
    for (const tensor of [...options.inputs, options.output]) {
      if (tensorRecords.get(tensor)?.core !== core) throw new Error('GPU tensor belongs to a different reactive context');
      snapshotBytes += tensor.length * Float32Array.BYTES_PER_ELEMENT;
    }
    const retained = pendingSnapshotBytes.get(admissionDevice!) ?? 0;
    const budget = computeSnapshotBudgets.get(admissionDevice!) ?? DEFAULT_COMPUTE_SNAPSHOT_BUDGET;
    if (!Number.isSafeInteger(snapshotBytes) || snapshotBytes > budget - retained) {
      throw new GpuComputeCapacityError('snapshot-bytes', computeQueueTelemetry(admissionDevice!), snapshotBytes);
    }
    pendingSnapshotBytes.set(admissionDevice!, retained + snapshotBytes);
    pendingComputations.set(admissionDevice!, pending + 1);
    let released = false;
    releaseAdmission = () => {
      if (released) return;
      released = true;
      const retainedBytes = pendingSnapshotBytes.get(admissionDevice!)! - snapshotBytes;
      if (retainedBytes) pendingSnapshotBytes.set(admissionDevice!, retainedBytes);
      else pendingSnapshotBytes.delete(admissionDevice!);
      const remaining = pendingComputations.get(admissionDevice!)! - 1;
      if (remaining) pendingComputations.set(admissionDevice!, remaining);
      else pendingComputations.delete(admissionDevice!);
    };
  }
  let enqueued = false;
  try {
    const inputViews = options.inputs.map(tensor => {
      if (tensorRecords.get(tensor)?.core !== core) throw new Error('GPU tensor belongs to a different reactive context');
      return tensor().slice();
    });
    const outputRecord = tensorRecords.get(options.output);
    if (outputRecord?.core !== core) throw new Error('GPU output belongs to a different reactive context');
    const workingOutput = options.output.peek().slice();
    const candidate = state.device;
    let dimensions: number[] = [];
    if (candidate?.createComputePipeline && candidate.createShaderModule && candidate.createBindGroup &&
        candidate.createBuffer && candidate.createCommandEncoder && candidate.queue.submit) {
      const groups = options.workgroups ?? [Math.ceil(workingOutput.length / 64), 1, 1];
      if (!Array.isArray(groups) || groups.length < 1 || groups.length > 3) throw new RangeError('Invalid GPU workgroup dimensions');
      dimensions = [groups[0], groups[1] ?? 1, groups[2] ?? 1];
      if (dimensions.some(n => !Number.isInteger(n) || n < 1 || n > (candidate.limits?.maxComputeWorkgroupsPerDimension ?? 65535))) {
        throw new RangeError('Invalid GPU workgroup count');
      }
      for (const view of [...inputViews, workingOutput]) {
        if (!view.byteLength || view.byteLength > (candidate.limits?.maxBufferSize ?? Infinity) ||
            view.byteLength > (candidate.limits?.maxStorageBufferBindingSize ?? Infinity)) throw new RangeError('Invalid GPU storage binding size');
      }
    } else if (!options.fallbackKernel) throw new Error('WebGPU compute is unavailable and no fallbackKernel was supplied');
    assertContext();
    const revision = ++outputRecord.revision;
    const cancellation = new AbortController();
    const cancel = () => cancellation.abort(new Error('GPU computation owner disposed'));
    if (owner && !owner.disposed) owner.disposables.push(cancel);
    const signals = [cancellation.signal, state.contextLifetime.signal, ...(options.signal ? [options.signal] : [])];
    const assertCurrent = () => {
      if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
      checkGpuSignals(signals);
      if (generation !== getReactiveGeneration(core)) throw new Error('GPU context reset during computation');
      if (outputRecord.revision !== revision) throw new Error('GPU output changed during computation');
    };
    const publish = (values: Float32Array, backend: GpuComputeResult['backend']) => runUntrackedInCore(core, () => {
      assertCurrent();
      options.output.write(values);
      return { backend, elements: values.length,
        uploadedBytes: backend === 'webgpu' ? inputViews.reduce((sum, input) => sum + input.byteLength, values.byteLength) : 0,
        readbackBytes: backend === 'webgpu' ? values.byteLength : 0 };
    });
    try {
      const device = state.device;
      if (!device?.createComputePipeline || !device.createShaderModule || !device.createBindGroup ||
          !device.createBuffer || !device.createCommandEncoder || !device.queue.submit) {
        if (!options.fallbackKernel) throw new Error('WebGPU compute is unavailable and no fallbackKernel was supplied');
        await waitForGpu(Promise.resolve().then(() => runUntrackedInCore(core, () => {
          assertCurrent();
          return runWithCapturedOwner(core, owner, () => options.fallbackKernel!(inputViews, workingOutput));
        })), signals);
        return publish(workingOutput, 'fallback');
      }
      const lifetime = state.lifetime;
      signals.push(lifetime.signal);
      let started = false;
      const execute = async (): Promise<GpuComputeResult> => {
        started = true;
        assertCurrent();
        const buffers: any[] = [];
        const scopes: Array<'validation' | 'out-of-memory' | 'internal'> = [];
        let mapped = false, staging: any, result: Float32Array | undefined;
        let failed = false, failure: unknown;
        try {
          if (device.pushErrorScope && device.popErrorScope) {
            for (const filter of ['validation', 'out-of-memory', 'internal'] as const) { device.pushErrorScope(filter); scopes.push(filter); }
          }
          const shader = device.createShaderModule!({ code: options.wgsl });
          if (shader.getCompilationInfo) {
            const info = await waitForGpu(shader.getCompilationInfo() as PromiseLike<{ messages: Array<{ type: string; message: string; lineNum: number; linePos: number }> }>, signals);
            const error = info.messages.find(message => message.type === 'error');
            if (error) throw new Error(`WGSL ${error.lineNum}:${error.linePos}: ${error.message}`);
          }
          assertCurrent();
          const pipeline = device.createComputePipeline!({ layout: 'auto', compute: { module: shader, entryPoint: options.entryPoint ?? 'main' } });
          const views = [...inputViews, workingOutput];
          const storage = views.map((view, binding) => {
            const buffer = device.createBuffer!({ size: view.byteLength, usage: 0x80 | 0x08 | (binding === inputViews.length ? 0x04 : 0) });
            if (!buffer) throw new Error('GPU compute buffer allocation failed');
            buffers.push(buffer);
            device.queue.writeBuffer(buffer, 0, view.buffer, view.byteOffset, view.byteLength);
            return { binding, resource: { buffer, size: view.byteLength } };
          });
          const group = device.createBindGroup!({ layout: pipeline.getBindGroupLayout(0), entries: storage });
          staging = device.createBuffer!({ size: workingOutput.byteLength, usage: 0x01 | 0x08 });
          if (!staging) throw new Error('GPU readback allocation failed');
          buffers.push(staging);
          const encoder = device.createCommandEncoder!(), pass = encoder.beginComputePass();
          pass.setPipeline(pipeline); pass.setBindGroup(0, group); pass.dispatchWorkgroups(...dimensions); pass.end();
          encoder.copyBufferToBuffer(storage[inputViews.length].resource.buffer, 0, staging, 0, workingOutput.byteLength);
          device.queue.submit!([encoder.finish()]);
          await waitForGpu(staging.mapAsync(0x01, 0, workingOutput.byteLength), signals);
          mapped = true;
          result = new Float32Array(staging.getMappedRange(0, workingOutput.byteLength)).slice();
        } catch (error) { failed = true; failure = error; }
        finally {
          if (mapped) { try { staging.unmap(); } catch (error) { if (!failed) { failed = true; failure = error; } } }
          for (const buffer of buffers) { try { buffer.destroy?.(); } catch (error) { if (!failed) { failed = true; failure = error; } } }
          if (scopes.length) {
            // Pop the whole group synchronously before awaiting any result.
            const results = scopes.reverse().map(filter => {
              try {
                return Promise.resolve(device.popErrorScope!()).then(error => {
                  if (error) throw new Error(`WebGPU ${filter}: ${error.message}`);
                });
              } catch (error) { return Promise.reject(error); }
            });
            try { await waitForGpu(Promise.all(results), signals); }
            catch (error) { if (!failed) { failed = true; failure = error; } }
          }
        }
        if (failed) throw failure;
        return publish(result!, 'webgpu');
      };
      // Error scopes belong to a device, so serialize complete validation/readback
      // transactions on that device rather than interleave their scope stacks.
      const queued = (computeQueues.get(device) ?? Promise.resolve()).then(execute);
      const tail = queued.then(releaseAdmission, releaseAdmission);
      enqueued = true;
      computeQueues.set(device, tail);
      void tail.then(() => { if (computeQueues.get(device) === tail) computeQueues.delete(device); });
      try { return await waitForGpu(queued, signals); }
      catch (error) {
        // If work started, cancellation settles only after its resources are freed.
        if (started) { try { await queued; } catch { /* preserve cancellation */ } }
        throw error;
      }
    } finally {
      const index = owner?.disposables.indexOf(cancel) ?? -1;
      if (index >= 0) owner!.disposables.splice(index, 1);
    }
  } finally {
    // Queued cancellation must retain its credit until the closure actually drains.
    if (!enqueued) releaseAdmission();
  }
}

export function getGpuTelemetry(): GpuDeviceTelemetry {
  const core = getWasmCore();
  const state = gpuState(core);
  const floatCount = core.svet_gpu_get_arena_len();
  return { architecture: state.architecture, webgpuAvailable: state.device !== null,
    arenaFloatCount: floatCount, arenaByteLength: floatCount * 4,
    lastSyncBytesTransferred: state.lastBytes, totalSyncCount: state.syncCount,
    totalBytesTransferred: state.totalBytes,
    ...computeQueueTelemetry(state.device) };
}

// ============================================================================
// 4. 3D GPU Scene Graph, Camera, Transform3D & Render Pipeline (`createSceneGraphNode3D`)
// ============================================================================

export type Vec3Tuple = [number, number, number];

/** Own the complete scene resource, including ordinary parameter signals. */
function ownSceneDisposal(cleanup: () => void): () => void {
  const owner = getCurrentOwner();
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    if (owner && !owner.disposed) {
      const index = owner.disposables.indexOf(dispose);
      if (index >= 0) owner.disposables.splice(index, 1);
    }
    cleanup();
  };
  onCleanup(dispose);
  return dispose;
}

type SceneDisposable = {dispose(): void};
/** Release unpublished scene resources while preserving the initialization failure. */
function constructSceneResource<T>(build: (retain: <R extends SceneDisposable>(resource: R) => R) => T): T {
  const resources: SceneDisposable[] = [];
  const retain = <R extends SceneDisposable>(resource: R): R => { resources.push(resource); return resource; };
  try { return build(retain); }
  catch (failure) {
    for (let i = resources.length - 1; i >= 0; i--) {
      try { resources[i].dispose(); } catch { /* Continue releasing partial state. */ }
    }
    throw failure;
  }
}

export interface ReactiveTransform3D {
  readonly position: Signal<Vec3Tuple>;
  readonly rotation: Signal<Vec3Tuple>;
  readonly scale: Signal<Vec3Tuple>;
  /** 16-float (`mat4x4<f32>`) Local Transform Matrix inside WASM UMA/DUMA Arena */
  readonly localMatrix: GpuTensorSignal;
  /** 16-float (`mat4x4<f32>`) World Transform Matrix (`parent.world * local`) inside WASM UMA/DUMA Arena */
  readonly worldMatrix: GpuTensorSignal;
  dispose(): void;
}

/**
 * Creates a reactive 3D Transform node (`position`, `rotation`, `scale`) backed by
 * 16-float `mat4x4<f32>` tensors in the Rust WASM UMA/DUMA arena.
 * If `parent` is provided, `worldMatrix` automatically recomputes `parent.worldMatrix * localMatrix`
 * in WASM whenever either parent or child transform signals change!
 */
export function createTransform3D(options?: {
  position?: Vec3Tuple;
  rotation?: Vec3Tuple;
  scale?: Vec3Tuple;
  parent?: ReactiveTransform3D;
}): ReactiveTransform3D {
  const core = getWasmCore(), generation = getReactiveGeneration(core), owner = getCurrentOwner();
  const assertCurrent = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('Transform construction owner disposed');
    if (generation !== getReactiveGeneration(core)) throw new Error('Transform context reset during construction');
  };
  assertCurrent();
  options = options ? snapshotGpuOptionMetadata(options, assertCurrent) : undefined;
  const initialPosition = options?.position, initialRotation = options?.rotation, initialScale = options?.scale;
  const parentMatrix = options?.parent?.worldMatrix;
  assertCurrent(); parentMatrix?.peek(); assertCurrent();

  return constructSceneResource(retain => {
    const position = retain(signal<Vec3Tuple>(initialPosition ?? [0, 0, 0]));
    const rotation = retain(signal<Vec3Tuple>(initialRotation ?? [0, 0, 0]));
    const scale = retain(signal<Vec3Tuple>(initialScale ?? [1, 1, 1]));
    const localMatrix = retain(createGpuTensor(16));
    const worldMatrix = retain(createGpuTensor(16));
    const computation = retain(effect(() => {
      const [tx, ty, tz] = position();
      const [rx, ry, rz] = rotation();
      const [sx, sy, sz] = scale();
      // Complete user-controlled iteration/coercion before any native matrix write.
      const values = [tx, ty, tz, rx, ry, rz, sx, sy, sz].map(value => Math.fround(value));
      assertCurrent(); localMatrix.peek(); worldMatrix.peek(); parentMatrix?.();
      assertCurrent(); localMatrix.peek(); worldMatrix.peek();
      core.svet_gpu_mat4_compose_trs(localMatrix.offset, ...values as [number, number, number, number, number, number, number, number, number]);
      reviseTensor(localMatrix);
      triggerNodeWrite(localMatrix.id);
      localMatrix.peek(); worldMatrix.peek(); parentMatrix?.peek();
      if (parentMatrix) core.svet_gpu_mat4_mul(parentMatrix.offset, localMatrix.offset, worldMatrix.offset);
      else {
        worldMatrix.view().set(localMatrix.view());
        core.svet_gpu_mark_dirty(worldMatrix.offset, 16);
      }
      reviseTensor(worldMatrix);
      triggerNodeWrite(worldMatrix.id);
    }));
    assertCurrent();
    return {
      position, rotation, scale, localMatrix, worldMatrix,
      dispose: ownSceneDisposal(() => {
        computation.dispose(); localMatrix.dispose(); worldMatrix.dispose();
        position.dispose(); rotation.dispose(); scale.dispose();
      }),
    };
  });
}

export interface PerspectiveCamera3DOptions {
  fovYRad?: number;
  aspect?: number;
  near?: number;
  far?: number;
}

export interface PerspectiveCamera3D {
  readonly fovYRad: Signal<number>;
  readonly aspect: Signal<number>;
  readonly near: Signal<number>;
  readonly far: Signal<number>;
  readonly projectionMatrix: GpuTensorSignal;
  /** Validate the final configuration and publish all parameter changes together. */
  setProjection(options: PerspectiveCamera3DOptions): void;
  dispose(): void;
}

/**
 * Creates a reactive 3D Perspective Camera (`fovYRad`, `aspect`, `near`, `far`)
 * whose 16-float `projectionMatrix` is computed in WASM linear memory (`depth [0, 1]` WebGPU standard).
 */
export function createPerspectiveCamera3D(options?: PerspectiveCamera3DOptions): PerspectiveCamera3D {
  const core = getWasmCore(), generation = getReactiveGeneration(core), owner = getCurrentOwner();
  const validate = (values: number[]) => {
    if (!values.every(value => typeof value === 'number' && Number.isFinite(value)) ||
        !core.svet_gpu_valid_perspective(values[0], values[1], values[2], values[3])) {
      throw new RangeError('Invalid PerspectiveCamera3D projection parameters');
    }
  };
  const assertCurrent = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('Camera construction owner disposed');
    if (generation !== getReactiveGeneration(core)) throw new Error('Camera context reset during construction');
  };
  assertCurrent();
  const read = (key: keyof PerspectiveCamera3DOptions, fallback: number) => {
    const value = options?.[key] ?? fallback; assertCurrent(); return value;
  };
  const initial = [read('fovYRad', Math.PI / 3), read('aspect', 16 / 9), read('near', 0.1), read('far', 1000)];
  validate(initial); assertCurrent();
  return constructSceneResource(retain => {
    const fovYRad = retain(signal(initial[0])), aspect = retain(signal(initial[1])), near = retain(signal(initial[2])), far = retain(signal(initial[3]));
    const projectionMatrix = retain(createGpuTensor(16));

    const parameters = [fovYRad, aspect, near, far];
    const setters = parameters.map(parameter => parameter.set);
    parameters.forEach((parameter, index) => {
      parameter.set = next => setters[index](previous => {
        const value = typeof next === 'function' ? next(previous) : next;
        projectionMatrix.peek();
        const candidate = parameters.map(parameter => parameter.peek()); candidate[index] = value;
        validate(candidate);
        return value;
      });
    });

    const computation = retain(effect(() => {
      core.svet_gpu_mat4_perspective(
        projectionMatrix.offset,
        fovYRad(),
        aspect(),
        near(),
        far()
      );
      reviseTensor(projectionMatrix);
      triggerNodeWrite(projectionMatrix.id);
    }));
    assertCurrent();

    return {
      fovYRad,
      aspect,
      near,
      far,
      projectionMatrix,
      setProjection(options) {
        projectionMatrix.peek();
        const candidate = [options.fovYRad ?? fovYRad.peek(), options.aspect ?? aspect.peek(),
          options.near ?? near.peek(), options.far ?? far.peek()];
        validate(candidate); projectionMatrix.peek();
        batch(() => setters.forEach((set, index) => set(candidate[index])));
      },
      dispose: ownSceneDisposal(() => {
        computation.dispose();
        projectionMatrix.dispose();
        fovYRad.dispose();
        aspect.dispose();
        near.dispose();
        far.dispose();
      }),
    };
  });
}

export interface Mesh3D {
  readonly vertexCount: number;
  /** Optional triangle index buffer; its element width selects the WebGPU index format. */
  readonly indices?: Uint16Array | Uint32Array;
  /** Copy/validate new topology; undefined selects nonindexed lines, [] selects no triangles. */
  updateIndices(values: ArrayLike<number> | undefined, format?: 'uint16' | 'uint32'): void;
  /** Local-space homogeneous vertices (`[x, y, z, 1.0]` per vertex) in WASM UMA/DUMA arena */
  readonly vertices: GpuTensorSignal;
  /** Clip-space / World-space transformed vertices (`mvp * vertex`) in WASM UMA/DUMA arena */
  readonly transformedVertices: GpuTensorSignal;
  /** Model-View-Projection (`projection * world`) 4x4 matrix in WASM UMA/DUMA arena */
  readonly mvpMatrix: GpuTensorSignal;
  dispose(): void;
}

function copyMeshIndices(values: ArrayLike<number> | undefined, vertexCount: number, indexFormat: 'uint16' | 'uint32', assertCurrent: () => void): Uint16Array | Uint32Array | undefined {
  if (indexFormat !== 'uint16' && indexFormat !== 'uint32') throw new RangeError('Invalid Mesh3D index format');
  const maxIndex = indexFormat === 'uint32' ? 0xffffffff : 65535;
  let indices: Uint16Array | Uint32Array | undefined;
  if (values) {
    const length = values.length;
    assertCurrent();
    if (!Number.isSafeInteger(length) || length < 0 || length % 3 !== 0) {
      throw new RangeError('Mesh3D triangle indices must contain complete triples');
    }
    indices = indexFormat === 'uint32' ? new Uint32Array(length) : new Uint16Array(length);
    for (let i = 0; i < length; i++) {
      const index = values[i];
      assertCurrent();
      if (!Number.isInteger(index) || index < 0 || index > maxIndex || index >= vertexCount) {
        throw new RangeError(`Mesh3D index must reference a vertex and fit ${indexFormat}`);
      }
      indices[i] = index;
    }
  }
  return indices;
}

/**
 * Creates a reactive 3D Mesh bound to a `ReactiveTransform3D` and optional `PerspectiveCamera3D`.
 * Whenever the mesh's transform, its parent transform, or the camera changes, WASM automatically
 * updates `mvpMatrix` and transforms all 3D vertices in linear memory, marking only the touched
 * cache lines dirty for WebGPU UMA/DUMA rendering!
 */
export function createMesh3D(options: {
  positions: ArrayLike<number>; // [x0,y0,z0, x1,y1,z1, ...] (3 floats per vertex)
  indices?: ArrayLike<number>;  // Optional triangle indices (3 per triangle)
  /** Defaults to uint16; select uint32 explicitly for indices above 65535. */
  indexFormat?: 'uint16' | 'uint32';
  transform: ReactiveTransform3D;
  camera?: PerspectiveCamera3D;
}): Mesh3D {
  const core = getWasmCore(), generation = getReactiveGeneration(core), owner = getCurrentOwner();
  const assertConstruction = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('Mesh3D construction owner disposed');
    if (generation !== getReactiveGeneration(core)) throw new Error('Mesh3D context reset during construction');
  };
  assertConstruction();
  options = snapshotGpuOptionMetadata(options, assertConstruction);
  const worldMatrix = options.transform.worldMatrix;
  assertConstruction(); worldMatrix.peek(); assertConstruction();
  const projectionMatrix = options.camera?.projectionMatrix;
  assertConstruction(); projectionMatrix?.peek(); assertConstruction();
  const positionLength = options.positions.length;
  assertConstruction();
  if (!Number.isSafeInteger(positionLength) || positionLength < 0 || positionLength % 3 !== 0) {
    throw new RangeError('Mesh3D positions must contain complete xyz triples');
  }
  const vertexCount = positionLength / 3;
  // Two vec4 buffers plus one mat4 must fit even an otherwise empty arena.
  // This necessary bound does not assume availability or lack of fragmentation.
  if (vertexCount * 8 + 16 > core.svet_gpu_get_arena_limit_f32()) {
    throw new RangeError('Mesh3D storage exceeds GPU tensor arena capacity');
  }
  let indexFormat = options.indexFormat ?? 'uint16';
  const indices = copyMeshIndices(options.indices, vertexCount, indexFormat, assertConstruction);
  const homogeneous = new Float32Array(vertexCount * 4);
  for (let i = 0; i < vertexCount; i++) {
    for (let axis = 0; axis < 3; axis++) {
      const value = Math.fround(options.positions[i * 3 + axis]);
      assertConstruction();
      if (!Number.isFinite(value)) throw new RangeError('Mesh3D positions must be finite float32 values');
      homogeneous[i * 4 + axis] = value;
    }
    homogeneous[i * 4 + 3] = 1.0;
  }
  // Borrowed dependency access above may be instrumented by user code.
  worldMatrix.peek(); projectionMatrix?.peek(); assertConstruction();
  return constructSceneResource(retain => {
    const vertices = retain(createGpuTensor(homogeneous));
    const transformedVertices = retain(createGpuTensor(vertexCount * 4));
    const mvpMatrix = retain(createGpuTensor(16));
    const computation = retain(effect(() => {
      worldMatrix(); vertices(); projectionMatrix?.();
      assertConstruction(); vertices.peek(); transformedVertices.peek(); mvpMatrix.peek();
      worldMatrix.peek(); projectionMatrix?.peek();
      if (projectionMatrix) {
        core.svet_gpu_mat4_mul(projectionMatrix.offset, worldMatrix.offset, mvpMatrix.offset);
      } else {
        const source = worldMatrix.view();
        assertConstruction(); worldMatrix.peek(); mvpMatrix.view().set(source);
        core.svet_gpu_mark_dirty(mvpMatrix.offset, 16);
      }
      reviseTensor(mvpMatrix);
      triggerNodeWrite(mvpMatrix.id);
      assertConstruction(); mvpMatrix.peek(); vertices.peek(); transformedVertices.peek();
      core.svet_gpu_transform_positions_vec4(mvpMatrix.offset, vertices.offset, transformedVertices.offset, vertexCount);
      reviseTensor(transformedVertices);
      triggerNodeWrite(transformedVertices.id);
    }));
    assertConstruction();
    const topology = retain(signal<Uint16Array | Uint32Array | undefined>(indices));
    let disposed = false, topologyVersion = 0;
    const assertLive = () => {
      if (disposed || isOwnerScopeDisposed(owner) || generation !== getReactiveGeneration(core)) throw new Error('Mesh3D has been disposed or invalidated');
      if (getWasmCore() !== core) throw new Error('Mesh3D belongs to a different reactive context');
    };
    return {
      vertexCount,
      get indices() { assertLive(); return topology(); },
      updateIndices(values, format = indexFormat) {
        assertLive();
        const version = topologyVersion;
        const assertCandidate = () => {
          assertLive();
          if (version !== topologyVersion) throw new Error('Mesh3D topology changed during update');
        };
        const next = copyMeshIndices(values, vertexCount, format, assertCandidate);
        assertCandidate();
        // Failed candidates never advance the commit version. Observers may
        // publish another topology during set; no older state is written after it.
        topologyVersion++; indexFormat = format; topology.set(next);
      },
      vertices, transformedVertices, mvpMatrix,
      dispose: ownSceneDisposal(() => {
        disposed = true; topology.dispose(); computation.dispose();
        vertices.dispose(); transformedVertices.dispose(); mvpMatrix.dispose();
      }),
    };
  });
}

/**
 * Helper to create a standard 3D Cube mesh (8 vertices, 36 triangle indices, 12 wireframe edges, 6 quad faces)
 * inside the WASM UMA/DUMA arena, ready for `<Canvas3D />` or Forma `Scene3D`.
 */
export function createCubeMesh3D(options: {
  size?: number;
  transform: ReactiveTransform3D;
  camera?: PerspectiveCamera3D;
}): {
  mesh: Mesh3D;
  edges: Array<[number, number]>;
  faces: Array<[number, number, number, number]>;
} {
  const s = (options.size ?? 1.2) * 0.5;
  const positions = [
    -s, -s, -s,
     s, -s, -s,
     s,  s, -s,
    -s,  s, -s,
    -s, -s,  s,
     s, -s,  s,
     s,  s,  s,
    -s,  s,  s,
  ];
  const edges: Array<[number, number]> = [
    [0, 1], [1, 2], [2, 3], [3, 0],
    [4, 5], [5, 6], [6, 7], [7, 4],
    [0, 4], [1, 5], [2, 6], [3, 7],
  ];
  const faces: Array<[number, number, number, number]> = [
    [0, 1, 2, 3], // Back
    [4, 5, 6, 7], // Front
    [0, 1, 5, 4], // Bottom
    [3, 2, 6, 7], // Top
    [0, 3, 7, 4], // Left
    [1, 2, 6, 5], // Right
  ];
  // 12 triangles (36 uint16 indices) for hardware WebGPU `triangle-list` + `drawIndexed`
  const indices = new Uint16Array([
    0, 1, 2,  0, 2, 3, // Back
    4, 5, 6,  4, 6, 7, // Front
    0, 1, 5,  0, 5, 4, // Bottom
    3, 2, 6,  3, 6, 7, // Top
    0, 3, 7,  0, 7, 4, // Left
    1, 2, 6,  1, 6, 5, // Right
  ]);
  const mesh = createMesh3D({
    positions,
    indices,
    transform: options.transform,
    camera: options.camera,
  });
  return { mesh, edges, faces };
}

export const DEFAULT_3D_WGSL_SHADER = /* wgsl */ `
struct VertexOut {
  @builtin(position) clip_pos: vec4<f32>,
  @location(0) depth_shade: f32,
};

@vertex
fn vs_main(@location(0) clip_in: vec4<f32>) -> VertexOut {
  var out: VertexOut;
  out.clip_pos = clip_in;
  out.depth_shade = clamp(1.0 - (clip_in.z / max(clip_in.w, 0.0001)) * 0.5, 0.25, 1.0);
  return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
  return vec4<f32>(0.22 * in.depth_shade, 0.74 * in.depth_shade, 0.97 * in.depth_shade, 0.92);
}
`;

/**
 * Z-Aware 3D Vertex Picker powered directly by the WASM UMA/DUMA linear arena (`svet_gpu_pick_nearest_vertex_3d`).
 * Projects all vertices of `mesh` onto `(viewportWidth, viewportHeight)` inside WASM and selects
 * the vertex closest to `(clickX, clickY)` within `maxRadiusPx`, prioritizing foreground vertices
 * (smaller `ndcZ`) when front and back vertices overlap in 2D screen space!
 */
export function pickNearestVertex3D(
  mesh: Mesh3D,
  clickX: number,
  clickY: number,
  options?: {
    viewportWidth?: number;
    viewportHeight?: number;
    maxRadiusPx?: number;
    /** Indexed triangles occlude candidates by default; none preserves through-surface picking. */
    occlusion?: 'indexed' | 'none';
  }
): { vertexIndex: number; distancePx: number; ndcZ: number } | null {
  const core = getWasmCore();
  mesh.transformedVertices();
  const vw = options?.viewportWidth ?? 360;
  const vh = options?.viewportHeight ?? 200;
  const maxRadius = options?.maxRadiusPx ?? 28;
  if (![vw,vh,clickX,clickY,maxRadius].every(Number.isFinite) || vw <= 0 || vh <= 0 || maxRadius < 0) return null;

  const indices = mesh.indices;
  let pickedIdx: number;
  if (options?.occlusion !== 'none' && indices?.length) {
    const pointer = core.svet_prepare_gpu_pick_indices(indices.length) >>> 0;
    new Uint32Array(core.memory.buffer, pointer, indices.length).set(indices);
    pickedIdx = core.svet_gpu_pick_visible_vertex_3d(mesh.transformedVertices.offset, mesh.vertexCount, indices.length, vw, vh, clickX, clickY, maxRadius);
  } else {
    pickedIdx = core.svet_gpu_pick_nearest_vertex_3d(mesh.transformedVertices.offset, mesh.vertexCount, vw, vh, clickX, clickY, maxRadius);
  }
  if (pickedIdx < 0 || pickedIdx >= mesh.vertexCount) {
    return null;
  }

  const clipVerts = mesh.transformedVertices.view();
  const base = pickedIdx * 4;
  const x = clipVerts[base];
  const y = clipVerts[base + 1];
  const z = clipVerts[base + 2];
  const w = clipVerts[base + 3];
  const invW = 1.0 / w;
  const halfW = vw * 0.5;
  const halfH = vh * 0.5;
  const sx = halfW + x * invW * halfW;
  const sy = halfH - y * invW * halfH;
  const distancePx = Math.hypot(sx - clickX, sy - clickY);

  return {
    vertexIndex: pickedIdx,
    distancePx,
    ndcZ: z * invW,
  };
}

/**
 * Interactive 3D Viewport component (`<Canvas3D />` / `createViewport3D`)
 * that renders a live 3D mesh directly from the WASM UMA/DUMA `transformedVertices` tensor:
 * - **Hardware WebGPU RenderPass + `depth24plus` Z-Buffer** when a WebGPU device & `webgpu` canvas context are available.
 * - **Automatic 2D Perspective + Depth-Sorted Rasterizer fallback** in headless / non-WebGPU environments.
 */
export interface VertexPickFrame3D {
  /** Caller-owned source mesh identity; it may have been replaced or disposed since drawing. */
  mesh: Mesh3D;
  id: number;
  width: number;
  height: number;
  backend: 'webgpu-depth24plus' | 'canvas2d-zsort';
}
interface VertexPickSnapshot3D extends VertexPickFrame3D {
  vertices: Float32Array;
  indices?: Uint16Array | Uint32Array;
  vertexCount: number;
}
function pickFrameSnapshot(core: SvetWasmExports, frame: VertexPickSnapshot3D, x: number, y: number, occlude: boolean) {
  if (!frame.vertexCount) return null;
  const pointer = core.svet_prepare_gpu_pick_vertices(frame.vertices.length) >>> 0;
  if (!pointer) throw new RangeError('Vertex picking scratch capacity exceeded');
  new Float32Array(core.memory.buffer, pointer, frame.vertices.length).set(frame.vertices);
  if (occlude && frame.indices?.length) {
    const indexPointer = core.svet_prepare_gpu_pick_indices(frame.indices.length) >>> 0;
    new Uint32Array(core.memory.buffer, indexPointer, frame.indices.length).set(frame.indices);
  }
  const index = core.svet_gpu_pick_snapshot_3d(frame.vertexCount, frame.indices?.length ?? 0, occlude ? 1 : 0, frame.width, frame.height, x, y, 28);
  if (index < 0 || index >= frame.vertexCount) return null;
  const base = index * 4, w = frame.vertices[base + 3];
  const px = frame.width * 0.5 * (1 + frame.vertices[base] / w), py = frame.height * 0.5 * (1 - frame.vertices[base + 1] / w);
  return { vertexIndex: index, distancePx: Math.hypot(px-x,py-y), ndcZ: frame.vertices[base + 2] / w };
}

export interface Canvas3DProps {
  /** Mesh lifetime remains caller-owned; an accessor supports reactive replacement. */
  mesh: Mesh3D | (() => Mesh3D);
  edges?: Array<[number, number]>;
  faces?: Array<[number, number, number, number]>;
  /** Positive integer drawing-buffer dimensions; accessors are tracked reactively. */
  width?: number | (() => number);
  height?: number | (() => number);
  strokeStyle?: string;
  fillStyle?: string;
  pointColor?: string;
  background?: string;
  showVertices?: boolean;
  preferWebGpu?: boolean;
  /** Optional `ReactiveTransform3D` to attach interactive mouse/pointer drag-to-orbit and wheel-to-zoom controls */
  orbitTransform?: ReactiveTransform3D;
  /** Override indexed-surface occlusion versus through-surface picking. */
  pickOcclusion?: 'indexed' | 'none';
  /** Initial draw is immediate; later updates can be coalesced with GPU backpressure. */
  renderMode?: 'sync' | 'animation-frame';
  /** Numeric hit plus source identity/dimensions of the last drawn/submitted frame. */
  onVertexPick?: (vertexIndex: number, distancePx: number, ndcZ: number, frame: VertexPickFrame3D) => void;
}

export function Canvas3D(props: Canvas3DProps): HTMLCanvasElement {
  return createCanvas3D(props);
}

/** Stable interaction surface with independent WebGPU and 2D canvas contexts. */
export function Viewport3D(props: Canvas3DProps): HTMLDivElement {
  const viewport = document.createElement('div');
  viewport.dataset.svetViewport3d = 'true';
  const fallback = document.createElement('canvas');
  const primary = createCanvas3D(props, { viewport, fallback });
  fallback.style.cssText = primary.style.cssText;
  // createCanvas3D selects visibility during its initial synchronous render.
  primary.style.display = viewport.dataset.renderBackend === 'webgpu-depth24plus' ? 'block' : 'none';
  fallback.style.display = viewport.dataset.renderBackend === 'canvas2d-zsort' ? 'block' : 'none';
  viewport.style.width = `${primary.width + 2}px`;
  viewport.append(primary, fallback);
  return viewport;
}

type RenderValidationTarget = { settle(frame: number, error: { message: string } | null): void; queue(frame: number, error: { message: string } | null): void };
function observeRenderValidation(results: Array<{ filter: string; result: PromiseLike<{ message: string } | null> }>, target: WeakRef<RenderValidationTarget>, frame: number): void {
  let remaining = results.length, failed = false;
  const fail = (filter: string, message: string) => {
    if (failed) return;
    failed = true;
    target.deref()?.settle(frame, { message: filter === 'validation' ? message : `${filter}: ${message}` });
  };
  withoutAsyncCoreContext(() => {
    for (const { filter, result } of results) {
      Promise.resolve(result).then(error => {
        if (error) fail(filter, error.message);
        else if (--remaining === 0 && !failed) target.deref()?.settle(frame, null);
      }, error => fail(filter, error instanceof Error ? error.message : String(error))).catch(() => {});
    }
  });
}

function observeRenderQueue(result: PromiseLike<void>, target: WeakRef<RenderValidationTarget>, frame: number): void {
  withoutAsyncCoreContext(() => {
    Promise.resolve(result).then(() => target.deref()?.queue(frame, null),
      error => target.deref()?.queue(frame, { message: error instanceof Error ? error.message : String(error) })).catch(() => {});
  });
}

function createCanvas3D(props: Canvas3DProps, surfaces?: { viewport: HTMLDivElement; fallback: HTMLCanvasElement }): HTMLCanvasElement {
  const currentMesh = () => typeof props.mesh === 'function' ? props.mesh() : props.mesh;
  const dimension = (value: number | (() => number) | undefined, fallback: number) => {
    const result = typeof value === 'function' ? value() : value ?? fallback;
    if (!Number.isInteger(result) || result < 1 || result > 0xffffffff) {
      throw new RangeError('Canvas3D dimensions must be positive uint32 integers');
    }
    return result;
  };
  let width = untrack(() => dimension(props.width, 360));
  let height = untrack(() => dimension(props.height, 200));
  if (props.renderMode !== undefined && props.renderMode !== 'sync' && props.renderMode !== 'animation-frame') throw new RangeError('Invalid renderMode');
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;
  canvas.style.borderRadius = '8px';
  canvas.style.display = 'block';
  canvas.style.background = props.background ?? '#070b14';
  canvas.style.border = '1px solid #23304d';
  canvas.setAttribute('data-svet-canvas3d', 'true');
  const interaction: HTMLElement = surfaces?.viewport ?? canvas;
  const view = canvas.ownerDocument.defaultView;
  if (surfaces) { surfaces.fallback.width = width; surfaces.fallback.height = height; }
  const selectBackend = (backend: string) => {
    if (backend === 'unavailable') pickSnapshot = null;
    canvas.dataset.renderBackend = backend;
    if (surfaces) {
      surfaces.viewport.dataset.renderBackend = backend;
      canvas.style.display = backend === 'webgpu-depth24plus' ? 'block' : 'none';
      surfaces.fallback.style.display = backend === 'canvas2d-zsort' ? 'block' : 'none';
    }
  };

  let pickSnapshot: VertexPickSnapshot3D | null = null;
  let pickSequence = 0;
  const capturePickFrame = (mesh: Mesh3D, vertices: Float32Array, indices: Mesh3D['indices'], backend: VertexPickFrame3D['backend']) => {
    if (!props.onVertexPick) return;
    pickSnapshot = { mesh, id: ++pickSequence, width, height, backend, vertexCount: mesh.vertexCount, vertices: vertices.slice(), indices: indices?.slice() };
  };

  if (props.orbitTransform) {
    const tr = props.orbitTransform;
    interaction.style.cursor = 'grab';
    let dragging = false;
    let prevX = 0;
    let prevY = 0;

    const onDown = (e: MouseEvent) => {
      dragging = true;
      prevX = e.clientX;
      prevY = e.clientY;
      interaction.style.cursor = 'grabbing';
    };
    const onMove = (e: MouseEvent) => {
      if (!dragging) return;
      const dx = e.clientX - prevX;
      const dy = e.clientY - prevY;
      prevX = e.clientX;
      prevY = e.clientY;
      const [rx, ry, rz] = tr.rotation.peek();
      tr.rotation.set([rx + dy * 0.012, ry + dx * 0.012, rz]);
    };
    const onUp = () => {
      dragging = false;
      interaction.style.cursor = 'grab';
    };
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const factor = e.deltaY < 0 ? 1.08 : 0.92;
      const [sx, sy, sz] = tr.scale.peek();
      const clamped = Math.max(0.2, Math.min(5.0, sx * factor));
      tr.scale.set([clamped, Math.max(0.2, Math.min(5.0, sy * factor)), Math.max(0.2, Math.min(5.0, sz * factor))]);
    };

    interaction.addEventListener('mousedown', onDown);
    view?.addEventListener('mousemove', onMove);
    view?.addEventListener('mouseup', onUp);
    interaction.addEventListener('wheel', onWheel, { passive: false });

    onCleanup(() => {
      interaction.removeEventListener('mousedown', onDown);
      view?.removeEventListener('mousemove', onMove);
      view?.removeEventListener('mouseup', onUp);
      interaction.removeEventListener('wheel', onWheel);
    });
  }

  if (props.onVertexPick) {
    const onClickPick = (e: MouseEvent) => {
      const frame = pickSnapshot;
      if (!frame) return;
      const pickingCanvas = surfaces && canvas.style.display === 'none' ? surfaces.fallback : canvas;
      const rect = pickingCanvas.getBoundingClientRect();
      const clickX = (e.clientX - rect.left) * (rect.width ? frame.width / rect.width : 1);
      const clickY = (e.clientY - rect.top) * (rect.height ? frame.height / rect.height : 1);
      const occlusion = props.pickOcclusion ?? (frame.backend === 'webgpu-depth24plus' ? 'indexed' : 'none');
      const hit = withWasmCore(renderCore, () => pickFrameSnapshot(renderCore, frame, clickX, clickY, occlusion === 'indexed'));
      if (hit) {
        const { vertices: _vertices, indices: _indices, vertexCount: _count, ...info } = frame;
        withWasmCore(renderCore, () => props.onVertexPick!(hit.vertexIndex, hit.distancePx, hit.ndcZ, info));
      }
    };
    interaction.addEventListener('click', onClickPick);
    onCleanup(() => interaction.removeEventListener('click', onClickPick));
  }

  // Optional Hardware WebGPU Render Pipeline state (lazy-initialized if WebGPU device & context exist)
  let webgpuCtx: any = null;
  let webgpuPipeline: any = null;
  let webgpuVertexBuffer: any = null;
  let webgpuVertexCapacity = 0;
  let webgpuIndexBuffer: any = null;
  let webgpuIndexCount = 0;
  let webgpuIndexCapacity = 0;
  let uploadedIndices: Mesh3D['indices'] | null = null;
  let pipelineIndexed: boolean | null = null;
  let renderFormat = 'bgra8unorm';
  let webgpuDepthView: any = null;
  let webgpuDepthTexture: any = null;
  let depthWidth = 0, depthHeight = 0;
  const renderCore = getWasmCore();
  const renderState = gpuState(renderCore);
  let renderDevice: WebGpuDeviceLike | null = null;
  let renderEpoch = 0, validationFloor = 0, lastValidatedFrame = 0, rendererDisposed = false;
  const scheduledMode = props.renderMode === 'animation-frame' && Boolean(view?.requestAnimationFrame);
  let frameRequest: number | null = null, dirty = false, initialDraw = true;
  let drainingFrame: number | null = null;
  let submission: { frame: number; queueDone: boolean; validationDone: boolean } | null = null;
  const cancelFrameRequest = () => { if (frameRequest !== null) view?.cancelAnimationFrame(frameRequest); frameRequest = null; };
  const scheduleLatest = () => {
    if (!scheduledMode || rendererDisposed || !dirty || submission || drainingFrame !== null || frameRequest !== null) return;
    frameRequest = view!.requestAnimationFrame(() => {
      frameRequest = null;
      if (rendererDisposed || submission || drainingFrame !== null || !dirty) return;
      dirty = false;
      try { withWasmCore(renderCore, () => untrack(() => draw())); }
      catch (error) { releaseRenderResources(); setValidation('failed', String(error)); selectBackend('unavailable'); }
    });
  };
  const finishSubmission = () => {
    if (submission?.queueDone && submission.validationDone) { submission = null; scheduleLatest(); }
  };
  const failFrame = (message: string, queueFinished = false) => {
    if (!queueFinished && submission && !submission.queueDone) drainingFrame = submission.frame;
    releaseRenderResources(); setValidation('failed', message);
    try { withWasmCore(renderCore, () => untrack(() => draw(true))); }
    catch { selectBackend('unavailable'); }
    scheduleLatest();
  };
  const setValidation = (status: string, message?: string) => {
    for (const surface of surfaces ? [canvas, surfaces.viewport] : [canvas]) {
      surface.dataset.renderValidation = status;
      if (message) surface.dataset.renderError = message;
      else delete surface.dataset.renderError;
    }
  };
  const validationTarget: RenderValidationTarget = { settle(frame, error) {
    if (rendererDisposed || frame <= validationFloor) return;
    if (!error) {
      lastValidatedFrame = Math.max(lastValidatedFrame, frame);
      if (frame === renderEpoch) setValidation('passed');
      if (submission?.frame === frame) { submission.validationDone = true; finishSubmission(); }
      return;
    }
    // Do not hide a failing stream merely because another frame is pending.
    if (frame <= lastValidatedFrame) return;
    failFrame(error.message);
  }, queue(frame, error) {
    if (rendererDisposed) return;
    if (drainingFrame === frame) { drainingFrame = null; scheduleLatest(); return; }
    if (submission?.frame !== frame || frame <= validationFloor) return;
    if (error) { failFrame(`GPU queue: ${error.message}`, true); return; }
    submission.queueDone = true; finishSubmission();
  } };
  const validationReference = new WeakRef(validationTarget);
  const releaseRenderResources = () => {
    validationFloor = ++renderEpoch;
    pickSnapshot = null;
    submission = null;
    const resources = [webgpuVertexBuffer, webgpuIndexBuffer, webgpuDepthTexture];
    const ctx = webgpuCtx;
    webgpuVertexBuffer = webgpuIndexBuffer = webgpuDepthTexture = webgpuDepthView = webgpuPipeline = webgpuCtx = null;
    webgpuInitialized = false; renderDevice = null; webgpuIndexCount = 0;
    webgpuIndexCapacity = 0; uploadedIndices = null; pipelineIndexed = null;
    depthWidth = depthHeight = 0; webgpuVertexCapacity = 0;
    for (const resource of resources) { try { resource?.destroy?.(); } catch { /* finish releasing other allocations */ } }
    try { ctx?.unconfigure?.(); } catch { /* device may already be lost */ }
  };
  let webgpuInitialized = false;

  const tryRenderWebGpu = (clipVerts: Float32Array, mesh: Mesh3D, indices: Mesh3D['indices']): boolean => {
    const activeDevice = renderState.device;
    if (activeDevice !== renderDevice) releaseRenderResources();
    if (props.preferWebGpu === false || !activeDevice || !activeDevice.createRenderPipeline) {
      return false;
    }
    const frame = ++renderEpoch;
    const scopes: Array<'validation' | 'out-of-memory' | 'internal'> = [];
    try {
      if (activeDevice.pushErrorScope && activeDevice.popErrorScope) {
        for (const filter of ['validation', 'out-of-memory', 'internal'] as const) {
          activeDevice.pushErrorScope(filter); scopes.push(filter);
        }
        setValidation('pending');
      } else setValidation('unsupported');
      const hasIndices = indices !== undefined;
      const checkLimit = (name: 'maxBufferSize' | 'maxTextureDimension2D', value: number) => {
        const limit = activeDevice.limits?.[name];
        if (limit !== undefined && (!Number.isFinite(limit) || limit < 1 || value > limit)) {
          throw new RangeError(`Canvas3D ${name} exceeded: requested ${value}, limit ${limit}`);
        }
      };
      checkLimit('maxTextureDimension2D', Math.max(width, height));
      checkLimit('maxBufferSize', Math.max(64, clipVerts.byteLength));
      if (indices) checkLimit('maxBufferSize', Math.max(16, Math.ceil(indices.byteLength / 4) * 4));
      if (!webgpuInitialized) {
        const ctx = (canvas as any).getContext?.('webgpu');
        if (!ctx) return false;
        webgpuCtx = ctx; renderDevice = activeDevice;
        const format =
          (typeof navigator !== 'undefined' &&
            (navigator as any).gpu?.getPreferredCanvasFormat?.()) ||
          'bgra8unorm';
        renderFormat = format;
        ctx.configure({
          device: activeDevice,
          format,
          alphaMode: 'premultiplied',
        });
        webgpuInitialized = true;
      }
      if (!webgpuVertexBuffer || clipVerts.byteLength > webgpuVertexCapacity) {
        const capacity = Math.max(64, clipVerts.byteLength);
        const next = activeDevice.createBuffer?.({ size: capacity, usage: 0x0020 | 0x0008 });
        if (!next) throw new Error('Incomplete render resources');
        const previous = webgpuVertexBuffer;
        webgpuVertexBuffer = next; webgpuVertexCapacity = capacity;
        previous?.destroy?.();
      }
      if (!webgpuDepthView || depthWidth !== width || depthHeight !== height) {
        const texture = activeDevice.createTexture?.({
          size: [width, height, 1], format: 'depth24plus', usage: 0x10,
        });
        let view: any;
        try {
          view = texture?.createView?.();
          if (!view) throw new Error('Incomplete depth attachment');
        } catch (error) { texture?.destroy?.(); throw error; }
        const previous = webgpuDepthTexture;
        webgpuDepthTexture = texture; webgpuDepthView = view;
        depthWidth = width; depthHeight = height;
        previous?.destroy?.();
      }

      if (!webgpuCtx || !webgpuVertexBuffer) return false;
      if (!webgpuPipeline || pipelineIndexed !== hasIndices) {
        const shaderModule = activeDevice.createShaderModule?.({
          code: DEFAULT_3D_WGSL_SHADER,
        });
        webgpuPipeline = activeDevice.createRenderPipeline({
          layout: 'auto',
          vertex: {
            module: shaderModule,
            entryPoint: 'vs_main',
            buffers: [
              {
                arrayStride: 16,
                attributes: [{ shaderLocation: 0, offset: 0, format: 'float32x4' }],
              },
            ],
          },
          fragment: {
            module: shaderModule,
            entryPoint: 'fs_main',
            targets: [{ format: renderFormat }],
          },
          primitive: {
            topology: hasIndices ? 'triangle-list' : 'line-strip',
          },
          depthStencil: {
            depthWriteEnabled: true,
            depthCompare: 'less',
            format: 'depth24plus',
          },
        });
        pipelineIndexed = hasIndices;
      }
      if (indices !== uploadedIndices) {
        if (indices) {
          const alignedBytes = Math.ceil(indices.byteLength / 4) * 4;
          if (!webgpuIndexBuffer || alignedBytes > webgpuIndexCapacity) {
            const capacity = Math.max(16, alignedBytes);
            const next = activeDevice.createBuffer?.({ size: capacity, usage: 0x0010 | 0x0008 });
            if (!next) throw new Error('Unable to allocate index buffer');
            const previous = webgpuIndexBuffer;
            webgpuIndexBuffer = next; webgpuIndexCapacity = capacity;
            previous?.destroy?.();
          }
          if (alignedBytes) {
            const padded = new Uint8Array(alignedBytes);
            padded.set(new Uint8Array(indices.buffer, indices.byteOffset, indices.byteLength));
            activeDevice.queue.writeBuffer(webgpuIndexBuffer, 0, padded.buffer, 0, alignedBytes);
          }
          webgpuIndexCount = indices.length;
        } else {
          webgpuIndexBuffer?.destroy?.(); webgpuIndexBuffer = null;
          webgpuIndexCapacity = 0; webgpuIndexCount = 0;
        }
        uploadedIndices = indices;
      }

      activeDevice.queue.writeBuffer(
        webgpuVertexBuffer,
        0,
        clipVerts.buffer,
        clipVerts.byteOffset,
        clipVerts.byteLength
      );
      const encoder = activeDevice.createCommandEncoder?.();
      if (!encoder) throw new Error('Missing GPU command encoder');

      const textureView = webgpuCtx.getCurrentTexture().createView();
      const pass = encoder.beginRenderPass({
        colorAttachments: [
          {
            view: textureView,
            clearValue: { r: 0.027, g: 0.043, b: 0.078, a: 1.0 },
            loadOp: 'clear',
            storeOp: 'store',
          },
        ],
        ...(webgpuDepthView
          ? {
              depthStencilAttachment: {
                view: webgpuDepthView,
                depthClearValue: 1.0,
                depthLoadOp: 'clear',
                depthStoreOp: 'store',
              },
            }
          : {}),
      });
      pass.setPipeline(webgpuPipeline);
      pass.setVertexBuffer(0, webgpuVertexBuffer);
      if (webgpuIndexBuffer && pass.setIndexBuffer && pass.drawIndexed) {
        pass.setIndexBuffer(webgpuIndexBuffer, indices?.BYTES_PER_ELEMENT === 4 ? 'uint32' : 'uint16');
        pass.drawIndexed(webgpuIndexCount, 1, 0, 0, 0);
      } else {
        pass.draw(mesh.vertexCount, 1, 0, 0);
      }
      pass.end();
      if (!activeDevice.queue.submit) throw new Error('Missing GPU queue submit');
      if (scheduledMode && !activeDevice.queue.onSubmittedWorkDone) throw new Error('GPU queue completion is unavailable');
      if (scheduledMode) submission = { frame, queueDone: false, validationDone: scopes.length === 0 };
      activeDevice.queue.submit([encoder.finish()]);
      if (scheduledMode) observeRenderQueue(activeDevice.queue.onSubmittedWorkDone!(), validationReference, frame);
      selectBackend('webgpu-depth24plus');
      capturePickFrame(mesh, clipVerts, indices, 'webgpu-depth24plus');
      return true;
    } catch (error) {
      releaseRenderResources();
      setValidation('failed', error instanceof Error ? error.message : String(error));
      return false;
    } finally {
      if (scopes.length) {
        const results = scopes.reverse().map(filter => {
          try { return { filter, result: activeDevice.popErrorScope!() }; }
          catch (error) { return { filter, result: Promise.reject(error) }; }
        });
        observeRenderValidation(results, validationReference, frame);
      }
    }
  };

  const draw = (force2d = false) => {
    const nextWidth = dimension(props.width, 360), nextHeight = dimension(props.height, 200);
    if (nextWidth !== width || nextHeight !== height) {
      pickSnapshot = null;
      width = nextWidth; height = nextHeight;
      for (const surface of surfaces ? [canvas, surfaces.fallback] : [canvas]) {
        surface.width = width; surface.height = height;
        surface.style.width = `${width}px`; surface.style.height = `${height}px`;
      }
      if (surfaces) surfaces.viewport.style.width = `${width + 2}px`;
    }
    // Subscribe to the WASM UMA/DUMA transformed vertex buffer
    const mesh = currentMesh();
    const clipVerts = mesh.transformedVertices();
    const indices = mesh.indices;
    const halfW = width * 0.5;
    const halfH = height * 0.5;
    const count = mesh.vertexCount;
    if (!force2d && tryRenderWebGpu(clipVerts, mesh, indices)) return;

    const screenPts: Array<[number, number, number]> = new Array(count);
    for (let i = 0; i < count; i++) {
      const x = clipVerts[i * 4];
      const y = clipVerts[i * 4 + 1];
      const z = clipVerts[i * 4 + 2];
      const w = clipVerts[i * 4 + 3];
      const invW = 1.0 / w;
      const sx = halfW + x * invW * halfW;
      const sy = halfH - y * invW * halfH;
      screenPts[i] = [sx, sy, z * invW];
    }

    const ctx = (surfaces?.fallback ?? canvas).getContext?.('2d');
    selectBackend(ctx ? 'canvas2d-zsort' : 'unavailable');
    if (!ctx) return;

    pickSnapshot = null;
    ctx.clearRect(0, 0, width, height);

    // Rust clips homogeneous primitives before JS transports finite screen points.
    const clipPrimitive = (indices: number[]): Array<[number, number, number]> => {
      if (indices.some(index => !Number.isInteger(index) || index < 0 || index >= count)) return [];
      const size = renderCore.svet_gpu_clip_primitive_3d(mesh.transformedVertices.offset, count,
        indices[0], indices[1], indices[2] ?? 0, indices[3] ?? 0, indices.length);
      // Clipping may grow WASM memory; construct the view after the call.
      const data = new Float32Array(renderCore.memory.buffer, renderCore.svet_gpu_clip_output_ptr() >>> 0, size * 4);
      const points: Array<[number, number, number]> = [];
      for (let i = 0; i < size; i++) {
        const w = data[i * 4 + 3];
        const point: [number, number, number] = [halfW * (1 + data[i * 4] / w), halfH * (1 - data[i * 4 + 1] / w), data[i * 4 + 2] / w];
        if (!point.every(Number.isFinite)) return [];
        points.push(point);
      }
      return points;
    };
    if (props.faces?.length) {
      const sortedFaces = props.faces.map(face => clipPrimitive(face))
        .filter(points => points.length >= 3)
        .map(points => ({ points, avgZ: points.reduce((sum, point) => sum + point[2], 0) / points.length }))
        .sort((a, b) => b.avgZ - a.avgZ);
      ctx.fillStyle = props.fillStyle ?? 'rgba(56, 189, 248, 0.14)';
      for (const { points } of sortedFaces) {
        ctx.beginPath(); ctx.moveTo(points[0][0], points[0][1]);
        for (const point of points.slice(1)) ctx.lineTo(point[0], point[1]);
        ctx.closePath(); ctx.fill();
      }
    }
    if (props.edges?.length) {
      ctx.strokeStyle = props.strokeStyle ?? '#38bdf8'; ctx.lineWidth = 2; ctx.beginPath();
      for (const edge of props.edges) {
        const points = clipPrimitive(edge);
        if (points.length !== 2) continue;
        ctx.moveTo(points[0][0], points[0][1]); ctx.lineTo(points[1][0], points[1][1]);
      }
      ctx.stroke();
    }

    if (props.showVertices !== false) {
      // Scratch allocation can grow WASM memory and detach the original view.
      const markerVertices = mesh.transformedVertices.view();
      ctx.fillStyle = props.pointColor ?? '#34d399';
      for (let i = 0; i < count; i++) {
        const base = i * 4;
        const x = markerVertices[base], y = markerVertices[base + 1], z = markerVertices[base + 2], w = markerVertices[base + 3];
        if (![x, y, z, w].every(Number.isFinite) || w <= 0 || Math.abs(x) > w || Math.abs(y) > w || z < 0 || z > w) continue;
        const [px, py] = screenPts[i];
        ctx.beginPath();
        ctx.arc(px, py, 3.5, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    capturePickFrame(mesh, mesh.transformedVertices.view(), indices, 'canvas2d-zsort');
  };
  const render = () => {
    if (!scheduledMode) { draw(); return; }
    // Track the latest sources now; the deferred draw runs without graph tracking.
    const mesh = currentMesh(); mesh.transformedVertices(); mesh.indices;
    dimension(props.width, 360); dimension(props.height, 200);
    if (initialDraw) { initialDraw = false; draw(); return; }
    dirty = true; scheduleLatest();
  };
  const stopRender = effect(render);
  const onDeviceChange = (reset: boolean) => {
    cancelFrameRequest(); dirty = false; drainingFrame = null;
    releaseRenderResources();
    setValidation('inactive');
    if (reset) {
      validationTarget.settle = validationTarget.queue = () => {};
      rendererDisposed = true;
      selectBackend('unavailable');
      stopRender.dispose(); renderState.renderers.delete(onDeviceChange); return;
    }
    try { withWasmCore(renderCore, () => untrack(() => draw())); }
    catch (error) { setValidation('failed', error instanceof Error ? error.message : String(error)); selectBackend('unavailable'); }
  };
  renderState.renderers.add(onDeviceChange);
  onCleanup(() => { validationTarget.settle = validationTarget.queue = () => {}; rendererDisposed = true; cancelFrameRequest(); dirty = false; drainingFrame = null; renderState.renderers.delete(onDeviceChange); releaseRenderResources(); setValidation('disposed'); });

  return canvas;
}

/**
 * High-speed GPU / WASM Arena Sparkline visualizer (`<GpuSparkline />`)
 * that renders a reactive `GpuTensorSignal` or numeric array directly onto a `<canvas>`.
 */
export function GpuSparkline(props: {
  tensor: GpuTensorSignal | ReadonlySignal<number[] | Float32Array>;
  width?: number;
  height?: number;
  strokeStyle?: string;
  fillStyle?: string;
  maxPoints?: number;
}): HTMLCanvasElement {
  const width = props.width ?? 320;
  const height = props.height ?? 72;
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;
  canvas.style.borderRadius = '6px';
  canvas.style.display = 'block';
  canvas.style.background = '#070b14';
  canvas.style.border = '1px solid #1e293b';
  canvas.setAttribute('data-svet-sparkline', 'true');

  effect(() => {
    const data = props.tensor();
    const ctx = canvas.getContext?.('2d');
    if (!ctx || !data || data.length === 0) return;

    ctx.clearRect(0, 0, width, height);
    const n = Math.min(data.length, props.maxPoints ?? 64);
    let minVal = Infinity;
    let maxVal = -Infinity;
    for (let i = 0; i < n; i++) {
      const v = data[i];
      if (v < minVal) minVal = v;
      if (v > maxVal) maxVal = v;
    }
    const span = maxVal - minVal || 1;
    const pad = 8;
    const usableH = height - pad * 2;

    ctx.beginPath();
    for (let i = 0; i < n; i++) {
      const x = (i / Math.max(1, n - 1)) * width;
      const y = height - pad - ((data[i] - minVal) / span) * usableH;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = props.strokeStyle ?? '#34d399';
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.lineTo(width, height);
    ctx.lineTo(0, height);
    ctx.closePath();
    ctx.fillStyle = props.fillStyle ?? 'rgba(52, 211, 153, 0.12)';
    ctx.fill();
  });

  return canvas;
}
