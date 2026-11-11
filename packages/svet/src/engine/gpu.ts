/**
 * WASM-backed tensor storage, reactive host kernels and WebGPU arena uploads.
 * Tensor views share WASM memory with JavaScript. WebGPU queue.writeBuffer copies
 * data to device buffers in both upload policies; physical UMA is not proof of
 * zero-copy browser GPU access. Architecture selection is a policy/hint.
 */

import {
  invalidateSingleSignalFastPath,
  allocateReactiveNode,
  getReactiveGeneration,
  registerReactiveResetCleanup,
  getCurrentOwner,
  isOwnerScopeDisposed,
  runWithCapturedOwner,
  runUntrackedInCore,
  onCleanup,
  type ReadonlySignal,
  trackNodeRead,
  triggerNodeWrite,
  disposeTrackedNode,
} from './reactivity.ts';
import {
  getWasmCore,
  withoutAsyncCoreContext,
  KIND_SIGNAL,
  type SvetWasmExports,
} from './wasm-bridge.ts';

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
  /** Persistent-result reservations, including pending creation; excludes transient GPU buffers. */
  residentComputeBytes: number;
  residentComputeCount: number;
  maxResidentComputeBytes: number;

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
    size?: number,
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
  createBindGroupLayout?(descriptor: any): any;
  createPipelineLayout?(descriptor: any): any;
  pushErrorScope?(filter: 'validation' | 'out-of-memory' | 'internal'): void;
  popErrorScope?(): PromiseLike<{ message: string } | null>;
  lost?: PromiseLike<{ reason?: string; message?: string }>;
  destroy?(): void;
  limits?: {
    maxComputeWorkgroupsPerDimension?: number;
    maxBufferSize?: number;
    maxStorageBufferBindingSize?: number;
    maxTextureDimension2D?: number;
  };
}

export interface GpuSubsystemConfig {
  /** Shared device snapshot budget; default 256 MiB. Existing work is never evicted. */
  maxPendingComputeBytes?: number | undefined;
  /** Shared persistent compute-result budget; default 256 MiB. Existing results are retained. */
  maxResidentComputeBytes?: number | undefined;
  architecture?: MemoryArchitecture | undefined;
  device?: WebGpuDeviceLike | null | undefined;
}

interface GpuState {
  renderers: Set<(reset: boolean) => void>;
  configurationCleanup: {
    owner: NonNullable<ReturnType<typeof getCurrentOwner>>;
    callback: () => void;
  } | null;
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
    try {
      listener(reset);
    } catch {
      /* one renderer must not prevent other cleanup */
    }
  }
}
function detachDevice(state: GpuState, reason: Error, notify = true): void {
  unregisterConfiguration(state);
  state.lifetime.abort(reason);
  if (state.lossSubscription) {
    deviceLossHubs
      .get(state.lossSubscription.device)
      ?.states.delete(state.lossSubscription.reference);
    state.lossSubscription = null;
  }
  const buffer = state.buffer,
    device = state.device,
    owned = state.ownsDevice;
  state.buffer = null;
  state.bufferSize = 0;
  state.forceUpload = true;
  state.device = null;
  state.ownsDevice = false;
  try {
    if (notify) notifyRenderers(state);
  } finally {
    try {
      buffer?.destroy?.();
    } finally {
      if (owned && device) {
        computePipelineCaches.delete(device);
        device.destroy?.();
      }
    }
  }
}
type DeviceLossHub = { states: Set<WeakRef<GpuState>>; message?: string };
const deviceLossHubs = new WeakMap<WebGpuDeviceLike, DeviceLossHub>();
function subscribeDeviceLoss(device: WebGpuDeviceLike, hub: DeviceLossHub): void {
  const lost = (message: string) => {
    hub.message = message;
    computePipelineCaches.delete(device);
    for (const reference of Array.from(hub.states)) {
      const state = reference.deref();
      if (state?.device === device) {
        try {
          detachDevice(state, new Error(`GPU device lost: ${message}`));
        } catch {
          /* notify other contexts too */
        }
      } else hub.states.delete(reference);
    }
  };
  withoutAsyncCoreContext(() => {
    Promise.resolve(device.lost!)
      .then(
        info => lost(info.message || info.reason || 'unknown'),
        error => lost(String(error)),
      )
      .catch(() => {});
  });
}
function watchDeviceLoss(state: GpuState): void {
  const device = state.device;
  if (!device?.lost || state.lossSubscription?.device === device) return;
  let hub = deviceLossHubs.get(device);
  if (!hub) {
    hub = { states: new Set() };
    deviceLossHubs.set(device, hub);
    subscribeDeviceLoss(device, hub);
  }
  if (hub.message !== undefined) {
    detachDevice(state, new Error(`GPU device lost: ${hub.message}`));
    return;
  }
  for (const reference of hub.states) if (!reference.deref()) hub.states.delete(reference);
  const reference = new WeakRef(state);
  hub.states.add(reference);
  state.lossSubscription = { device, reference };
}
function resetGpuState(state: GpuState, core: SvetWasmExports): void {
  notifyRenderers(state, true);
  state.lifetime.abort(new Error('GPU context reset'));
  state.contextLifetime.abort(new Error('GPU context reset'));
  state.lifetime = new AbortController();
  state.contextLifetime = new AbortController();
  const buffer = state.buffer;
  Object.assign(state, {
    generation: getReactiveGeneration(core),
    buffer: null,
    bufferSize: 0,
    forceUpload: true,
    lastBytes: 0,
    syncCount: 0,
    totalBytes: 0,
  });
  buffer?.destroy?.();
}
function gpuState(core = getWasmCore()): GpuState {
  const generation = getReactiveGeneration(core);
  let state = gpuStates.get(core);
  if (!state) {
    state = {
      renderers: new Set(),
      configurationCleanup: null,
      lossSubscription: null,
      contextLifetime: new AbortController(),
      lifetime: new AbortController(),
      version: 0,
      initializationVersion: 0,
      ownsDevice: false,
      generation,
      architecture: 'UMA',
      device: null,
      buffer: null,
      bufferSize: 0,
      forceUpload: true,
      lastBytes: 0,
      syncCount: 0,
      totalBytes: 0,
    };
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
  const core = getWasmCore(),
    generation = getReactiveGeneration(core),
    state = gpuState(core);
  const initialization = state.initializationVersion;
  // Read user-controlled fields once before publishing any configuration change.
  config = {
    architecture: config.architecture,
    device: config.device,
    maxPendingComputeBytes: config.maxPendingComputeBytes,
    maxResidentComputeBytes: config.maxResidentComputeBytes,
  };
  if (generation !== getReactiveGeneration(core))
    throw new Error('GPU context reset during configuration');
  if (isOwnerScopeDisposed(owner)) throw new Error('GPU configuration owner disposed');
  if (initialization !== state.initializationVersion)
    throw new Error('GPU configuration changed during configuration');
  if (
    config.architecture !== undefined &&
    config.architecture !== 'UMA' &&
    config.architecture !== 'DUMA'
  ) {
    throw new TypeError('Invalid GPU memory architecture');
  }
  const budgetDevice = config.device === undefined ? state.device : config.device;
  for (const key of ['maxPendingComputeBytes', 'maxResidentComputeBytes'] as const) {
    const value = config[key];
    if (value === undefined) continue;
    if (!Number.isSafeInteger(value) || value < 0)
      throw new RangeError(`${key} must be a nonnegative safe integer`);
    if (!budgetDevice) throw new Error('A GPU device is required to configure a compute budget');
  }
  if (config.maxPendingComputeBytes !== undefined)
    computeSnapshotBudgets.set(budgetDevice!, config.maxPendingComputeBytes);
  if (config.maxResidentComputeBytes !== undefined)
    residentComputeBudgets.set(budgetDevice!, config.maxResidentComputeBytes);
  unregisterConfiguration(state);
  state.initializationVersion++;
  if (config.architecture) state.architecture = config.architecture;
  else if (!config.device) {
    const appleHint =
      typeof navigator !== 'undefined' &&
      /Mac|Apple/.test(navigator.platform || navigator.userAgent || '');
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
  state.lastBytes = 0;
  state.syncCount = 0;
  state.totalBytes = 0;
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
  const core = getWasmCore(),
    owner = getCurrentOwner();
  const generation = getReactiveGeneration(core);
  const assertConstructionCurrent = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('GPU tensor construction owner disposed');
    if (generation !== getReactiveGeneration(core))
      throw new Error('GPU context reset during tensor initialization');
  };
  assertConstructionCurrent();
  const length = typeof initialOrLength === 'number' ? initialOrLength : initialOrLength.length;
  assertConstructionCurrent();
  if (!Number.isInteger(length) || length < 0 || length > core.svet_gpu_get_arena_limit_f32())
    throw new RangeError('Invalid GPU tensor length');
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
  const id = allocateReactiveNode(core, KIND_SIGNAL);
  invalidateSingleSignalFastPath();
  let disposed = false;
  const assertLive = () => {
    if (disposed || isOwnerScopeDisposed(owner) || generation !== getReactiveGeneration(core))
      throw new Error('GPU tensor has been disposed or invalidated');
    if (getWasmCore() !== core)
      throw new Error('GPU tensor belongs to a different reactive context');
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
    if (
      !Number.isInteger(startIndex) ||
      startIndex < 0 ||
      startIndex > length ||
      !Number.isInteger(sourceLength) ||
      sourceLength < 0
    )
      throw new RangeError('Invalid GPU tensor write range');
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
    if (!Number.isInteger(index) || index < 0 || index >= length)
      throw new RangeError('Invalid GPU tensor index');
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
    cachedArray = null;
    cachedBuffer = null;
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
    state.buffer = buffer;
    state.bufferSize = arenaBytes;
    state.forceUpload = true;
  }
  if (state.device && state.forceUpload && arenaBytes) core.svet_gpu_mark_dirty(0, arenaBytes / 4);
  const minFloat = core.svet_gpu_get_dirty_min() >>> 0;
  const maxFloat = core.svet_gpu_get_dirty_max();
  state.lastBytes = 0;
  if (minFloat >= maxFloat || minFloat === 0xffffffff) {
    return {
      architecture: state.architecture,
      dirtyFloatMin: 0,
      dirtyFloatMax: 0,
      bytesTransferred: 0,
      spansTransferred: 0,
    };
  }
  let spansCount = 0;
  try {
    if (state.device) {
      const queue = state.device.queue;
      const buffer = state.buffer;
      const uploadSpan = (arena: Uint8Array<ArrayBuffer>, start: number, length: number) => {
        const aligned = start & ~15;
        const end = Math.min((start + length + 15) & ~15, arenaBytes);
        const alignedLen = end - aligned;
        queue.writeBuffer(buffer, aligned, arena, aligned, alignedLen);
        state.lastBytes += alignedLen;
        state.totalBytes += alignedLen;
        spansCount++;
      };
      if (state.forceUpload) {
        const arena = new Uint8Array(core.memory.buffer, core.svet_gpu_get_arena_ptr(), arenaBytes);
        uploadSpan(arena, 0, arenaBytes);
      } else if (state.architecture === 'UMA') {
        const arena = new Uint8Array(core.memory.buffer, core.svet_gpu_get_arena_ptr(), arenaBytes);
        uploadSpan(arena, minFloat * 4, (maxFloat - minFloat) * 4);
      } else {
        const count = core.svet_gpu_collect_dirty_spans();
        // Construct views after WASM span collection, which can grow linear memory.
        const arena = new Uint8Array(core.memory.buffer, core.svet_gpu_get_arena_ptr(), arenaBytes);
        if (count > 1) {
          const data = new Uint32Array(core.memory.buffer, core.svet_get_lis_ptr(), count * 2);
          for (let i = 0; i < count; i++) {
            uploadSpan(arena, data[i * 2] * 4, data[i * 2 + 1] * 4);
          }
        } else {
          uploadSpan(arena, minFloat * 4, (maxFloat - minFloat) * 4);
        }
      }
    }
    core.svet_gpu_clear_dirty();
    state.forceUpload = false;
  } finally {
    state.syncCount++;
  }
  return {
    architecture: state.architecture,
    dirtyFloatMin: minFloat,
    dirtyFloatMax: maxFloat,
    bytesTransferred: state.lastBytes,
    spansTransferred: spansCount,
  };
}

/**
 * Executes a reactive WASM SIMD multiply/add (with separate float32 rounding) (`output[i] = input[i] * scale + bias`)
 * directly over the UMA/DUMA arena and triggers any reactive subscribers of `output`.
 */
export function executeGpuVectorFma(
  input: GpuTensorSignal,
  output: GpuTensorSignal,
  scale: number,
  bias: number,
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
  const keys = Reflect.ownKeys(options);
  assertCurrent();
  for (const key of keys) {
    const descriptor = Reflect.getOwnPropertyDescriptor(options, key);
    assertCurrent();
    if (!descriptor?.enumerable) continue;
    const value = Reflect.get(options, key);
    assertCurrent();
    Object.defineProperty(snapshot, key, {
      value,
      enumerable: true,
      writable: true,
      configurable: true,
    });
  }
  return snapshot;
}

function snapshotGpuComputeOptions<T extends WebGpuComputeDispatchOptions>(
  options: T,
  assertCurrent: () => void,
): T {
  const snapshot = snapshotGpuOptionMetadata(options, assertCurrent);
  // The previous entry also accepted inherited/nonenumerable inputs.
  const inputs = Object.hasOwn(snapshot, 'inputs')
    ? snapshot.inputs
    : Reflect.get(options, 'inputs');
  assertCurrent();
  const copied: GpuTensorSignal[] = [];
  for (const tensor of inputs) {
    assertCurrent();
    copied.push(tensor);
  }
  assertCurrent();
  Object.defineProperty(snapshot, 'inputs', {
    value: copied,
    enumerable: true,
    writable: true,
    configurable: true,
  });
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
  const core = getWasmCore(),
    state = gpuState(core);
  const generation = getReactiveGeneration(core),
    initialization = ++state.initializationVersion;
  const current = () =>
    !isOwnerScopeDisposed(owner) &&
    generation === getReactiveGeneration(core) &&
    state.initializationVersion === initialization;
  let created: WebGpuDeviceLike | undefined;
  const releaseCreated = () => {
    const device = created;
    created = undefined;
    try {
      device?.destroy?.();
    } catch {
      /* Preserve cancellation/fallback when device teardown fails. */
    }
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
      if (!created || !created.createBuffer || !created.queue)
        throw new Error('Invalid GPU device returned by initialization');
      if (!current()) throw new Error('GPU initialization cancelled');
      return runWithCapturedOwner(core, owner, () => {
        const report = configureGpuSubsystem({
          architecture: unifiedHint ? 'UMA' : 'DUMA',
          device: created,
        });
        if (state.device === created) {
          state.ownsDevice = true;
          created = undefined;
        } else releaseCreated();
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
  const core = getWasmCore(),
    generation = getReactiveGeneration(core),
    owner = getCurrentOwner();
  const assertContext = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
    if (generation !== getReactiveGeneration(core))
      throw new Error('GPU context reset during computation');
  };
  assertContext();
  options = snapshotGpuComputeOptions(options, assertContext);
  if (!options.fallbackKernel)
    throw new Error(
      'Synchronous compute requires a fallbackKernel; use dispatchGpuComputeKernelAsync for WGSL',
    );
  const inputViews = options.inputs.map(t => t().slice());
  const workingOutput = options.output.peek().slice();
  const record = tensorRecords.get(options.output);
  if (record?.core !== core) throw new Error('GPU output belongs to a different reactive context');
  assertContext();
  const revision = record.revision;
  const returned: unknown = runUntrackedInCore(core, () => {
    assertContext();
    return runWithCapturedOwner(core, owner, () =>
      options.fallbackKernel!(inputViews, workingOutput),
    );
  });
  const then = returned == null ? undefined : (returned as PromiseLike<unknown>).then;
  if (typeof then === 'function') {
    // Cache the user accessor once and preserve its receiver during native
    // assimilation. Observe late rejection without exposing working arrays.
    void Promise.resolve({ then: then.bind(returned) }).catch(() => {});
    throw new TypeError(
      'Synchronous compute received an asynchronous callback; use dispatchGpuComputeKernelAsync',
    );
  }
  if (generation !== getReactiveGeneration(core))
    throw new Error('GPU context reset during computation');
  if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
  if (record.revision !== revision) throw new Error('GPU output changed during computation');
  options.output.write(workingOutput);
}

export interface GpuComputeChainStage {
  wgsl: string;
  entryPoint?: string | undefined;
  workgroups?: [number, number?, number?];
  /** Private stage output length in floats; defaults to the final tensor length. */
  outputLength?: number;
}
export interface AsyncWebGpuComputeDispatchOptions extends Omit<
  WebGpuComputeDispatchOptions,
  'fallbackKernel'
> {
  /** Entry point defaults to main. Inputs use bindings 0..N-1, output uses N. */
  entryPoint?: string | undefined;
  signal?: AbortSignal | undefined;
  /** First private output length; the last stage must match output.length. */
  outputLength?: number;
  /** Up to 31 additional GPU stages. Each reads the previous result
   * at binding 0 and writes binding 1, seeded with the previous result's prefix.
   * One submission/readback/publication covers the whole chain. */
  chain?: readonly GpuComputeChainStage[];
  /** When hardware is absent, this callback implements the entire chain. */
  fallbackKernel?: (inputs: Float32Array[], output: Float32Array) => void | PromiseLike<void>;
}
export interface GpuComputeResult {
  backend: 'webgpu' | 'fallback';
  elements: number;
  uploadedBytes: number;
  readbackBytes: number;
}

/** An admission failure; no tensor snapshots or output revisions were reserved. */
export class GpuComputeCapacityError extends Error {
  readonly queue: Readonly<GpuComputeQueueTelemetry>;
  constructor(
    readonly reason: 'count' | 'snapshot-bytes',
    queue: GpuComputeQueueTelemetry,
    /** Null when the count limit rejects before tensor inspection. */
    readonly requestedSnapshotBytes: number | null = null,
  ) {
    super(
      reason === 'count'
        ? 'WebGPU compute queue is full (32 pending computations)'
        : 'WebGPU compute snapshot byte budget exceeded',
    );
    this.name = 'GpuComputeCapacityError';
    this.queue = Object.freeze({ ...queue });
  }
}

const computeQueues = new WeakMap<WebGpuDeviceLike, Promise<void>>();
type ComputePipelineCache = { entries: Map<string, any>; keyBytes: number };
const computePipelineCaches = new WeakMap<WebGpuDeviceLike, ComputePipelineCache>();
const MAX_COMPUTE_PIPELINES = 32;
const MAX_COMPUTE_PIPELINE_KEY_BYTES = 1024 * 1024;
function computePipelineKey(wgsl: string, entryPoint: string, namespace = ''): string | undefined {
  // Do not coerce user objects or retain unbounded shader source in cache keys.
  if (
    typeof wgsl !== 'string' ||
    typeof entryPoint !== 'string' ||
    (wgsl.length + entryPoint.length) * 2 > MAX_COMPUTE_PIPELINE_KEY_BYTES
  )
    return;
  const key = `${namespace}${entryPoint.length}:${entryPoint}${wgsl}`;
  return key.length * 2 <= MAX_COMPUTE_PIPELINE_KEY_BYTES ? key : undefined;
}
function findComputePipeline(device: WebGpuDeviceLike, key: string | undefined): any {
  if (key === undefined) return;
  const cache = computePipelineCaches.get(device),
    pipeline = cache?.entries.get(key);
  if (pipeline) {
    cache!.entries.delete(key);
    cache!.entries.set(key, pipeline);
  }
  return pipeline;
}
function forgetComputePipeline(device: WebGpuDeviceLike, key: string | undefined): void {
  if (key === undefined) return;
  const cache = computePipelineCaches.get(device);
  if (cache?.entries.delete(key)) cache.keyBytes -= key.length * 2;
}
function retainComputePipeline(
  device: WebGpuDeviceLike,
  key: string | undefined,
  pipeline: any,
): void {
  if (key === undefined || deviceLossHubs.get(device)?.message !== undefined) return;
  let cache = computePipelineCaches.get(device);
  if (!cache) {
    cache = { entries: new Map(), keyBytes: 0 };
    computePipelineCaches.set(device, cache);
  }
  forgetComputePipeline(device, key);
  while (
    cache.entries.size >= MAX_COMPUTE_PIPELINES ||
    cache.keyBytes + key.length * 2 > MAX_COMPUTE_PIPELINE_KEY_BYTES
  ) {
    forgetComputePipeline(device, cache.entries.keys().next().value!);
  }
  cache.entries.set(key, pipeline);
  cache.keyBytes += key.length * 2;
}
// Includes running work and cancelled entries whose queue closures still retain snapshots.
const pendingComputations = new WeakMap<WebGpuDeviceLike, number>();
const MAX_PENDING_COMPUTATIONS = 32;
const pendingSnapshotBytes = new WeakMap<WebGpuDeviceLike, number>();
const computeSnapshotBudgets = new WeakMap<WebGpuDeviceLike, number>();
const DEFAULT_COMPUTE_SNAPSHOT_BUDGET = 256 * 1024 * 1024;
const residentComputeBudgets = new WeakMap<WebGpuDeviceLike, number>();
const residentComputeUsage = new WeakMap<WebGpuDeviceLike, { bytes: number; count: number }>();
function computeQueueTelemetry(device: WebGpuDeviceLike | null): GpuComputeQueueTelemetry {
  return {
    pendingComputeCount: device ? (pendingComputations.get(device) ?? 0) : 0,
    pendingComputeSnapshotBytes: device ? (pendingSnapshotBytes.get(device) ?? 0) : 0,
    maxPendingComputeCount: MAX_PENDING_COMPUTATIONS,
    maxPendingComputeBytes: device
      ? (computeSnapshotBudgets.get(device) ?? DEFAULT_COMPUTE_SNAPSHOT_BUDGET)
      : DEFAULT_COMPUTE_SNAPSHOT_BUDGET,
  };
}

/** @internal Reserve before copying host data; retain queued credits until drainage. */
function reserveGpuComputeAdmission(device: WebGpuDeviceLike, snapshotBytes: number): () => void {
  const pending = pendingComputations.get(device) ?? 0;
  if (pending >= MAX_PENDING_COMPUTATIONS)
    throw new GpuComputeCapacityError('count', computeQueueTelemetry(device));
  const retained = pendingSnapshotBytes.get(device) ?? 0;
  if (
    !Number.isSafeInteger(snapshotBytes) ||
    snapshotBytes < 0 ||
    snapshotBytes >
      (computeSnapshotBudgets.get(device) ?? DEFAULT_COMPUTE_SNAPSHOT_BUDGET) - retained
  ) {
    throw new GpuComputeCapacityError(
      'snapshot-bytes',
      computeQueueTelemetry(device),
      snapshotBytes,
    );
  }
  pendingComputations.set(device, pending + 1);
  pendingSnapshotBytes.set(device, retained + snapshotBytes);
  let released = false;
  return () => {
    if (released) return;
    released = true;
    const remaining = pendingComputations.get(device)! - 1;
    const bytes = pendingSnapshotBytes.get(device)! - snapshotBytes;
    if (remaining) pendingComputations.set(device, remaining);
    else pendingComputations.delete(device);
    if (bytes) pendingSnapshotBytes.set(device, bytes);
    else pendingSnapshotBytes.delete(device);
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
  try {
    return await Promise.race([operation, cancelled]);
  } finally {
    for (const cleanup of remove) cleanup();
  }
}

/**
 * Executes WGSL on a configured device and atomically publishes mapped readback.
 * Inputs are snapshots captured at call time. Default workgroups assume 64 lanes;
 * supply workgroups when the shader uses a different workgroup size.
 * Missing compute support uses an explicit host fallback, never a silent no-op.
 */
export async function dispatchGpuComputeKernelAsync(
  options: AsyncWebGpuComputeDispatchOptions,
): Promise<GpuComputeResult> {
  const core = getWasmCore(),
    generation = getReactiveGeneration(core),
    owner = getCurrentOwner();
  const assertContext = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
    if (generation !== getReactiveGeneration(core))
      throw new Error('GPU context reset during computation');
  };
  assertContext();
  options = snapshotGpuComputeOptions(options, assertContext);
  const chain: GpuComputeChainStage[] = [];
  if (options.chain !== undefined) {
    if (!Array.isArray(options.chain))
      throw new RangeError('GPU compute chain supports at most 31 additional stages');
    const length = options.chain.length;
    assertContext();
    if (!Number.isInteger(length) || length < 0 || length > 31)
      throw new RangeError('GPU compute chain supports at most 31 additional stages');
    for (const value of options.chain) {
      assertContext();
      if (chain.length >= 31)
        throw new RangeError('GPU compute chain supports at most 31 additional stages');
      const stage = snapshotGpuOptionMetadata(value, assertContext);
      if (
        typeof stage.wgsl !== 'string' ||
        (stage.entryPoint !== undefined && typeof stage.entryPoint !== 'string')
      ) {
        throw new TypeError('Invalid GPU compute chain shader or entry point');
      }
      chain.push(stage);
    }
    assertContext();
  }
  options.signal?.throwIfAborted();
  assertContext();
  const state = gpuState(core);
  const admissionDevice = state.device;
  const hardware =
    admissionDevice?.createComputePipeline &&
    admissionDevice.createShaderModule &&
    admissionDevice.createBindGroup &&
    admissionDevice.createBuffer &&
    admissionDevice.createCommandEncoder &&
    admissionDevice.queue.submit;
  assertContext();
  let releaseAdmission = () => {};
  if (hardware) {
    const pending = pendingComputations.get(admissionDevice!) ?? 0;
    if (pending >= MAX_PENDING_COMPUTATIONS)
      throw new GpuComputeCapacityError('count', computeQueueTelemetry(admissionDevice!));
    let snapshotBytes = 0;
    for (const tensor of [...options.inputs, options.output]) {
      if (tensorRecords.get(tensor)?.core !== core)
        throw new Error('GPU tensor belongs to a different reactive context');
      snapshotBytes += tensor.length * Float32Array.BYTES_PER_ELEMENT;
    }
    const retained = pendingSnapshotBytes.get(admissionDevice!) ?? 0;
    const budget = computeSnapshotBudgets.get(admissionDevice!) ?? DEFAULT_COMPUTE_SNAPSHOT_BUDGET;
    if (!Number.isSafeInteger(snapshotBytes) || snapshotBytes > budget - retained) {
      throw new GpuComputeCapacityError(
        'snapshot-bytes',
        computeQueueTelemetry(admissionDevice!),
        snapshotBytes,
      );
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
      if (tensorRecords.get(tensor)?.core !== core)
        throw new Error('GPU tensor belongs to a different reactive context');
      return tensor().slice();
    });
    const outputRecord = tensorRecords.get(options.output);
    if (outputRecord?.core !== core)
      throw new Error('GPU output belongs to a different reactive context');
    const workingOutput = options.output.peek().slice();
    const candidate = state.device;
    const kernels = [
      {
        wgsl: options.wgsl,
        entryPoint: options.entryPoint,
        workgroups: options.workgroups,
        outputLength: options.outputLength,
      },
      ...chain,
    ];
    const outputLengths = kernels.map(kernel => {
      const length = kernel.outputLength ?? workingOutput.length;
      // GPU-only intermediates are independent of the WASM arena limit.
      if (
        !Number.isSafeInteger(length) ||
        length < 0 ||
        length > Math.floor(Number.MAX_SAFE_INTEGER / Float32Array.BYTES_PER_ELEMENT)
      ) {
        throw new RangeError('Invalid GPU compute stage outputLength');
      }
      return length;
    });
    if (outputLengths.at(-1) !== workingOutput.length)
      throw new RangeError('Final GPU compute stage must match output tensor length');
    const outputByteLengths = outputLengths.map(length => length * Float32Array.BYTES_PER_ELEMENT);
    const maxOutputBytes = Math.max(...outputByteLengths);
    const initialOutputBytes = Math.min(workingOutput.byteLength, outputByteLengths[0]);
    const dimensions: number[][] = [];
    if (
      candidate?.createComputePipeline &&
      candidate.createShaderModule &&
      candidate.createBindGroup &&
      candidate.createBuffer &&
      candidate.createCommandEncoder &&
      candidate.queue.submit
    ) {
      for (let stage = 0; stage < kernels.length; stage++) {
        const kernel = kernels[stage];
        const groups = kernel.workgroups ?? [Math.ceil(outputLengths[stage] / 64), 1, 1];
        if (!Array.isArray(groups)) throw new RangeError('Invalid GPU workgroup dimensions');
        const length = groups.length;
        assertContext();
        if (!Number.isInteger(length) || length < 1 || length > 3)
          throw new RangeError('Invalid GPU workgroup dimensions');
        const counts: number[] = [];
        for (let i = 0; i < 3; i++) {
          const count = groups[i];
          assertContext();
          counts.push(i === 0 ? count! : (count ?? 1));
        }
        if (
          counts.some(
            n =>
              !Number.isInteger(n) ||
              n < 1 ||
              n > (candidate.limits?.maxComputeWorkgroupsPerDimension ?? 65535),
          )
        ) {
          throw new RangeError('Invalid GPU workgroup count');
        }
        dimensions.push(counts);
      }
      for (const byteLength of [...inputViews.map(view => view.byteLength), ...outputByteLengths]) {
        if (
          !byteLength ||
          byteLength > (candidate.limits?.maxBufferSize ?? Infinity) ||
          byteLength > (candidate.limits?.maxStorageBufferBindingSize ?? Infinity)
        )
          throw new RangeError('Invalid GPU storage binding size');
      }
    } else if (!options.fallbackKernel)
      throw new Error('WebGPU compute is unavailable and no fallbackKernel was supplied');
    assertContext();
    const revision = ++outputRecord.revision;
    const cancellation = new AbortController();
    const cancel = () => cancellation.abort(new Error('GPU computation owner disposed'));
    if (owner && !owner.disposed) owner.disposables.push(cancel);
    const signals = [
      cancellation.signal,
      state.contextLifetime.signal,
      ...(options.signal ? [options.signal] : []),
    ];
    const assertCurrent = () => {
      if (isOwnerScopeDisposed(owner)) throw new Error('GPU computation owner disposed');
      checkGpuSignals(signals);
      if (generation !== getReactiveGeneration(core))
        throw new Error('GPU context reset during computation');
      if (outputRecord.revision !== revision)
        throw new Error('GPU output changed during computation');
    };
    const publish = (values: Float32Array, backend: GpuComputeResult['backend']) =>
      runUntrackedInCore(core, () => {
        assertCurrent();
        options.output.write(values);
        return {
          backend,
          elements: values.length,
          uploadedBytes:
            backend === 'webgpu'
              ? inputViews.reduce((sum, input) => sum + input.byteLength, initialOutputBytes)
              : 0,
          readbackBytes: backend === 'webgpu' ? values.byteLength : 0,
        };
      });
    try {
      const device = state.device;
      if (
        !device?.createComputePipeline ||
        !device.createShaderModule ||
        !device.createBindGroup ||
        !device.createBuffer ||
        !device.createCommandEncoder ||
        !device.queue.submit
      ) {
        if (!options.fallbackKernel)
          throw new Error('WebGPU compute is unavailable and no fallbackKernel was supplied');
        await waitForGpu(
          Promise.resolve().then(() =>
            runUntrackedInCore(core, () => {
              assertCurrent();
              return runWithCapturedOwner(core, owner, () =>
                options.fallbackKernel!(inputViews, workingOutput),
              );
            }),
          ),
          signals,
        );
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
        let mapped = false,
          staging: any,
          result: Float32Array | undefined;
        const pipelines: Array<{ key: string | undefined; pipeline: any }> = [];
        let failed = false,
          failure: unknown;
        try {
          if (device.pushErrorScope && device.popErrorScope) {
            for (const filter of ['validation', 'out-of-memory', 'internal'] as const) {
              device.pushErrorScope(filter);
              scopes.push(filter);
            }
          }
          for (const kernel of kernels) {
            const entryPoint = kernel.entryPoint ?? 'main';
            const key = computePipelineKey(kernel.wgsl, entryPoint);
            let pipeline =
              findComputePipeline(device, key) ??
              (key === undefined ? undefined : pipelines.find(item => item.key === key)?.pipeline);
            // Remember attempted keys as well, so a later failure evicts all
            // pipelines used by this transaction before any result is published.
            const record = { key, pipeline };
            pipelines.push(record);
            if (!pipeline) {
              const shader = device.createShaderModule!({ code: kernel.wgsl });
              if (shader.getCompilationInfo) {
                const info = await waitForGpu(
                  shader.getCompilationInfo() as PromiseLike<{
                    messages: Array<{
                      type: string;
                      message: string;
                      lineNum: number;
                      linePos: number;
                    }>;
                  }>,
                  signals,
                );
                const error = info.messages.find(message => message.type === 'error');
                if (error)
                  throw new Error(`WGSL ${error.lineNum}:${error.linePos}: ${error.message}`);
              }
              assertCurrent();
              pipeline = device.createComputePipeline!({
                layout: 'auto',
                compute: { module: shader, entryPoint },
              });
              record.pipeline = pipeline;
            }
          }
          assertCurrent();
          const views = [...inputViews, workingOutput];
          const storage = views.map((view, binding) => {
            const isOutput = binding === inputViews.length;
            const size = isOutput ? outputByteLengths[0] : view.byteLength;
            const buffer = device.createBuffer!({
              size: isOutput ? maxOutputBytes : size,
              usage: 0x80 | 0x08 | (isOutput ? 0x04 : 0),
            });
            if (!buffer) throw new Error('GPU compute buffer allocation failed');
            buffers.push(buffer);
            const uploaded = Math.min(view.byteLength, size);
            if (uploaded)
              device.queue.writeBuffer(buffer, 0, view.buffer, view.byteOffset, uploaded);
            return { binding, resource: { buffer, size } };
          });
          staging = device.createBuffer!({ size: workingOutput.byteLength, usage: 0x01 | 0x08 });
          if (!staging) throw new Error('GPU readback allocation failed');
          buffers.push(staging);
          const encoder = device.createCommandEncoder!();
          let previous = storage[inputViews.length].resource.buffer;
          let alternate: any;
          if (chain.length) {
            alternate = device.createBuffer!({ size: maxOutputBytes, usage: 0x80 | 0x08 | 0x04 });
            if (!alternate) throw new Error('GPU compute chain buffer allocation failed');
            buffers.push(alternate);
          }
          for (let i = 0; i < pipelines.length; i++) {
            assertCurrent();
            const pipeline = pipelines[i].pipeline;
            let entries = storage;
            if (i > 0) {
              const copiedBytes = Math.min(outputByteLengths[i - 1], outputByteLengths[i]);
              if (outputByteLengths[i] > copiedBytes)
                encoder.clearBuffer(alternate, copiedBytes, outputByteLengths[i] - copiedBytes);
              encoder.copyBufferToBuffer(previous, 0, alternate, 0, copiedBytes);
              entries = [previous, alternate].map((buffer, binding) => ({
                binding,
                resource: { buffer, size: outputByteLengths[i - 1 + binding] },
              }));
            }
            const group = device.createBindGroup!({
              layout: pipeline.getBindGroupLayout(0),
              entries,
            });
            const pass = encoder.beginComputePass();
            pass.setPipeline(pipeline);
            pass.setBindGroup(0, group);
            pass.dispatchWorkgroups(...dimensions[i]);
            pass.end();
            if (i > 0) [previous, alternate] = [alternate, previous];
          }
          encoder.copyBufferToBuffer(previous, 0, staging, 0, workingOutput.byteLength);
          device.queue.submit!([encoder.finish()]);
          await waitForGpu(staging.mapAsync(0x01, 0, workingOutput.byteLength), signals);
          mapped = true;
          result = new Float32Array(staging.getMappedRange(0, workingOutput.byteLength)).slice();
        } catch (error) {
          failed = true;
          failure = error;
        } finally {
          if (mapped) {
            try {
              staging.unmap();
            } catch (error) {
              if (!failed) {
                failed = true;
                failure = error;
              }
            }
          }
          for (const buffer of buffers) {
            try {
              buffer.destroy?.();
            } catch (error) {
              if (!failed) {
                failed = true;
                failure = error;
              }
            }
          }
          if (scopes.length) {
            // Pop the whole group synchronously before awaiting any result.
            const results = scopes.reverse().map(filter => {
              try {
                return Promise.resolve(device.popErrorScope!()).then(error => {
                  if (error) throw new Error(`WebGPU ${filter}: ${error.message}`);
                });
              } catch (error) {
                return Promise.reject(error);
              }
            });
            try {
              await waitForGpu(Promise.all(results), signals);
            } catch (error) {
              if (!failed) {
                failed = true;
                failure = error;
              }
            }
          }
        }
        if (failed) {
          for (const { key } of pipelines) forgetComputePipeline(device, key);
          throw failure;
        }
        assertCurrent();
        // Error scopes must finish successfully before publishing to this cache.
        for (const { key, pipeline } of pipelines) retainComputePipeline(device, key, pipeline);
        return publish(result!, 'webgpu');
      };
      // Error scopes belong to a device, so serialize complete validation/readback
      // transactions on that device rather than interleave their scope stacks.
      const queued = (computeQueues.get(device) ?? Promise.resolve()).then(execute);
      const tail = queued.then(releaseAdmission, releaseAdmission);
      enqueued = true;
      computeQueues.set(device, tail);
      void tail.then(() => {
        if (computeQueues.get(device) === tail) computeQueues.delete(device);
      });
      try {
        return await waitForGpu(queued, signals);
      } catch (error) {
        // If work started, cancellation settles only after its resources are freed.
        if (started) {
          try {
            await queued;
          } catch {
            /* preserve cancellation */
          }
        }
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
  return {
    architecture: state.architecture,
    webgpuAvailable: state.device !== null,
    arenaFloatCount: floatCount,
    arenaByteLength: floatCount * 4,
    lastSyncBytesTransferred: state.lastBytes,
    totalSyncCount: state.syncCount,
    totalBytesTransferred: state.totalBytes,
    residentComputeBytes: state.device ? (residentComputeUsage.get(state.device)?.bytes ?? 0) : 0,
    residentComputeCount: state.device ? (residentComputeUsage.get(state.device)?.count ?? 0) : 0,
    maxResidentComputeBytes: state.device
      ? (residentComputeBudgets.get(state.device) ?? DEFAULT_COMPUTE_SNAPSHOT_BUDGET)
      : DEFAULT_COMPUTE_SNAPSHOT_BUDGET,
    ...computeQueueTelemetry(state.device),
  };
}

// Internal executor primitives shared with immutable resident-result and 3D scene operations.
export {
  gpuState,
  tensorRecords,
  reviseTensor,
  computeQueues,
  computePipelineKey,
  findComputePipeline,
  forgetComputePipeline,
  retainComputePipeline,
  snapshotGpuOptionMetadata,
  waitForGpu,
  checkGpuSignals,
  reserveGpuComputeAdmission,
  residentComputeBudgets,
  residentComputeUsage,
};
