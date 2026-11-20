import {
  getReactiveGeneration,
  type OwnerScope,
  registerReactiveResetCleanup,
} from './reactivity.ts';
import { getWasmCore, withoutAsyncCoreContext, type SvetWasmExports } from './wasm-bridge.ts';
import { cacheComputePipeline, computePipelineCaches } from './gpu-compute-support.ts';
import type { GpuTensorSignal, MemoryArchitecture, WebGpuDeviceLike } from './gpu-types.ts';

export interface DestroyableGpuBuffer {
  destroy?(): void;
}

export interface GpuState {
  renderers: Set<(reset: boolean) => void>;
  configurationCleanup: {
    owner: OwnerScope;
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
  buffer: DestroyableGpuBuffer | null;
  bufferSize: number;
  forceUpload: boolean;
  lastBytes: number;
  syncCount: number;
  totalBytes: number;
}

const gpuStates = new WeakMap<SvetWasmExports, GpuState>();

export function unregisterConfiguration(state: GpuState): void {
  const registration = state.configurationCleanup;
  state.configurationCleanup = null;
  if (registration && !registration.owner.disposed) {
    const index = registration.owner.disposables.indexOf(registration.callback);
    if (index >= 0) registration.owner.disposables.splice(index, 1);
  }
}

export function notifyRenderers(state: GpuState, reset = false): void {
  for (const listener of Array.from(state.renderers)) {
    try {
      listener(reset);
    } catch {
      /* one renderer must not prevent other cleanup */
    }
  }
}

export function detachDevice(state: GpuState, reason: Error, notify = true): void {
  unregisterConfiguration(state);
  state.lifetime.abort(reason);
  if (state.lossSubscription) {
    deviceLossHubs
      .get(state.lossSubscription.device)
      ?.states.delete(state.lossSubscription.reference);
    state.lossSubscription = null;
  }
  const buffer = state.buffer;
  const device = state.device;
  const owned = state.ownsDevice;
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

/** A lost device keeps no pipelines: nothing compiled for it can run again. */
export function retainComputePipeline(
  device: WebGpuDeviceLike,
  key: string | undefined,
  pipeline: unknown,
): void {
  if (deviceLossHubs.get(device)?.message !== undefined) return;
  cacheComputePipeline(device, key, pipeline);
}

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
    void Promise.resolve(device.lost!)
      .then(
        info => lost(info.message || info.reason || 'unknown'),
        error => lost(String(error)),
      )
      .catch(() => {});
  });
}

export function watchDeviceLoss(state: GpuState): void {
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

export function gpuState(core = getWasmCore()): GpuState {
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

export const tensorRecords = new WeakMap<
  GpuTensorSignal,
  { core: SvetWasmExports; revision: number }
>();

export function reviseTensor(tensor: GpuTensorSignal): void {
  const record = tensorRecords.get(tensor);
  if (record) record.revision++;
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
export function snapshotGpuOptionMetadata<T extends object>(
  options: T,
  assertCurrent: () => void,
): T {
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

export function snapshotGpuComputeOptions<T extends WebGpuComputeDispatchOptions>(
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
