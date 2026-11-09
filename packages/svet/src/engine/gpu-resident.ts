/** Immutable device buffers for compute graphs with explicit host publication. */
import {
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  runUntrackedInCore,
} from './reactivity.ts';
import { getWasmCore } from './wasm-bridge.ts';
import {
  gpuState,
  tensorRecords,
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
  type GpuTensorSignal,
  type WebGpuDeviceLike,
  type GpuComputeResult,
} from './gpu.ts';

import {
  residentRecords as records,
  assertResidentRecord as assertRecord,
  type GpuResidentRecord as ResidentRecord,
  type GpuResidentTensor,
} from './gpu-resident-bridge.ts';
export type { GpuResidentTensor } from './gpu-resident-bridge.ts';

export interface GpuResidentComputeOptions {
  wgsl: string;
  inputs: readonly (GpuTensorSignal | GpuResidentTensor)[];
  outputLength: number;
  entryPoint?: string;
  workgroups?: [number, number?, number?];
  signal?: AbortSignal;
}
export class GpuResidentCapacityError extends Error {
  constructor(
    readonly requestedBytes: number,
    readonly reservedBytes: number,
    readonly maxBytes: number,
  ) {
    super('WebGPU persistent compute budget exceeded (bytes or 1024 results)');
    this.name = 'GpuResidentCapacityError';
  }
}

const DEFAULT_RESIDENT_BYTES = 256 * 1024 * 1024;
function reserveResident(device: WebGpuDeviceLike, bytes: number): () => void {
  const used = residentComputeUsage.get(device) ?? { bytes: 0, count: 0 };
  const budget = residentComputeBudgets.get(device) ?? DEFAULT_RESIDENT_BYTES;
  if (bytes > budget - used.bytes || used.count >= 1024)
    throw new GpuResidentCapacityError(bytes, used.bytes, budget);
  residentComputeUsage.set(device, { bytes: used.bytes + bytes, count: used.count + 1 });
  let released = false;
  return () => {
    if (released) return;
    released = true;
    const current = residentComputeUsage.get(device)!;
    if (current.count === 1) residentComputeUsage.delete(device);
    else
      residentComputeUsage.set(device, { bytes: current.bytes - bytes, count: current.count - 1 });
  };
}
function context(signal?: AbortSignal) {
  const core = getWasmCore(),
    generation = getReactiveGeneration(core),
    owner = getCurrentOwner(),
    state = gpuState(core);
  const device = state.device,
    deviceLifetime = state.lifetime,
    contextLifetime = state.contextLifetime;
  if (
    !device?.createBuffer ||
    !device.createShaderModule ||
    !device.createComputePipeline ||
    !device.createBindGroup ||
    !device.createCommandEncoder ||
    !device.createBindGroupLayout ||
    !device.createPipelineLayout ||
    !device.queue.submit ||
    !device.queue.onSubmittedWorkDone ||
    !device.pushErrorScope ||
    !device.popErrorScope
  )
    throw new Error('GPU resident compute requires a complete WebGPU device');
  const cancellation = new AbortController();
  const stop = () => cancellation.abort(new Error('GPU resident computation owner disposed'));
  const signals = [
    cancellation.signal,
    deviceLifetime.signal,
    contextLifetime.signal,
    ...(signal ? [signal] : []),
  ];
  const assertCurrent = () => {
    checkGpuSignals(signals);
    if (state.device !== device || state.lifetime !== deviceLifetime)
      throw new Error('GPU resident device configuration changed');
    if (isOwnerScopeDisposed(owner)) throw new Error('GPU resident computation owner disposed');
    if (generation !== getReactiveGeneration(core)) throw new Error('GPU resident context reset');
  };
  assertCurrent();
  owner?.disposables.push(stop);
  const release = () => {
    const index = owner?.disposables.indexOf(stop) ?? -1;
    if (index >= 0) owner!.disposables.splice(index, 1);
  };
  return { core, generation, owner, state, device, signals, assertCurrent, release };
}
type Context = ReturnType<typeof context>;
function validateBytes(length: number, device: WebGpuDeviceLike): number {
  if (
    !Number.isSafeInteger(length) ||
    length < 1 ||
    length > Math.floor(Number.MAX_SAFE_INTEGER / 4)
  ) {
    throw new RangeError('Invalid resident outputLength');
  }
  const bytes = length * 4;
  if (
    bytes > (device.limits?.maxBufferSize ?? Infinity) ||
    bytes > (device.limits?.maxStorageBufferBindingSize ?? Infinity)
  ) {
    throw new RangeError('Invalid resident storage binding size');
  }
  return bytes;
}
/** Same per-device queue and credits as mapped compute; queued cancellation retains credits. */
async function enqueue<T>(
  ctx: Context,
  releaseAdmission: () => void,
  run: () => Promise<T>,
): Promise<T> {
  let started = false;
  const queued = (computeQueues.get(ctx.device) ?? Promise.resolve()).then(() => {
    started = true;
    return run();
  });
  const tail = queued.then(releaseAdmission, releaseAdmission);
  computeQueues.set(ctx.device, tail);
  void tail.then(() => {
    if (computeQueues.get(ctx.device) === tail) computeQueues.delete(ctx.device);
  });
  try {
    return await waitForGpu(queued, ctx.signals);
  } catch (failure) {
    if (started) {
      try {
        await queued;
      } catch {
        /* Preserve cancellation. */
      }
    }
    throw failure;
  }
}
async function validated<T>(ctx: Context, body: () => Promise<T>): Promise<T> {
  let failed = false,
    failure: unknown,
    result!: T;
  const filters: Array<'validation' | 'out-of-memory' | 'internal'> = [];
  try {
    for (const filter of ['validation', 'out-of-memory', 'internal'] as const) {
      ctx.device.pushErrorScope!(filter);
      filters.push(filter);
    }
    result = await body();
  } catch (error) {
    failed = true;
    failure = error;
  } finally {
    const pending = filters.reverse().map(filter => {
      try {
        return Promise.resolve(ctx.device.popErrorScope!()).then(error => {
          if (error) throw new Error(`WebGPU ${filter}: ${error.message}`);
        });
      } catch (error) {
        return Promise.reject(error);
      }
    });
    try {
      await waitForGpu(Promise.all(pending), ctx.signals);
    } catch (error) {
      if (!failed) {
        failed = true;
        failure = error;
      }
    }
  }
  if (failed) throw failure;
  ctx.assertCurrent();
  return result;
}
type ResidentLifetimeHub = { disposers: Set<() => void>; abort: () => void };
const residentLifetimeHubs = new WeakMap<AbortSignal, ResidentLifetimeHub>();
function subscribeResidentLifetime(signal: AbortSignal, dispose: () => void): () => void {
  signal.throwIfAborted();
  let hub = residentLifetimeHubs.get(signal);
  if (!hub) {
    const created: ResidentLifetimeHub = {
      disposers: new Set(),
      abort: () => {
        const pending = Array.from(created.disposers);
        created.disposers.clear();
        residentLifetimeHubs.delete(signal);
        for (const stop of pending) {
          try {
            stop();
          } catch {
            /* Drain all resident buffers. */
          }
        }
      },
    };
    hub = created;
    residentLifetimeHubs.set(signal, hub);
    signal.addEventListener('abort', hub.abort, { once: true });
  }
  hub.disposers.add(dispose);
  const subscribed = hub;
  return () => {
    subscribed.disposers.delete(dispose);
    if (!subscribed.disposers.size) {
      signal.removeEventListener('abort', subscribed.abort);
      if (residentLifetimeHubs.get(signal) === subscribed) residentLifetimeHubs.delete(signal);
    }
  };
}
function publishResident(
  ctx: Context,
  buffer: any,
  bytes: number,
  uploadedBytes: number,
  release: () => void,
): GpuResidentTensor {
  const record: ResidentRecord = {
    core: ctx.core,
    device: ctx.device,
    buffer,
    bytes,
    disposed: false,
    owner: ctx.owner,
    generation: ctx.generation,
    lifetime: new AbortController(),
    readSnapshot: readGpuResidentSnapshot,
  };
  const lifetimeSignals = [ctx.signals[1], ctx.signals[2]];
  const subscriptions: Array<() => void> = [];
  const dispose = () => {
    // Also unlink registrations added during reentrant lifetime cancellation.
    for (const unsubscribe of subscriptions.splice(0)) unsubscribe();
    const index = ctx.owner?.disposables.indexOf(dispose) ?? -1;
    if (index >= 0) ctx.owner!.disposables.splice(index, 1);
    if (record.disposed) return;
    record.disposed = true;
    record.lifetime.abort(new Error('GPU resident result disposed'));
    try {
      buffer.destroy?.();
    } finally {
      release();
    }
  };
  const handle: GpuResidentTensor = Object.freeze({
    length: bytes / 4,
    byteLength: bytes,
    uploadedBytes,
    get active() {
      return (
        !record.disposed &&
        !isOwnerScopeDisposed(record.owner) &&
        record.generation === getReactiveGeneration(record.core)
      );
    },
    dispose,
  });
  records.set(handle, record);
  try {
    for (const signal of lifetimeSignals)
      subscriptions.push(subscribeResidentLifetime(signal, dispose));
    ctx.owner?.disposables.push(dispose);
    ctx.assertCurrent();
  } catch (failure) {
    dispose();
    throw failure;
  }
  return handle;
}

/** Hardware-only immutable result. Output starts zeroed; no readback or WASM output allocation. */
export async function dispatchGpuComputeResident(
  options: GpuResidentComputeOptions,
): Promise<GpuResidentTensor> {
  // Snapshot metadata before retaining an owner cancellation hook.
  const core = getWasmCore(),
    generation = getReactiveGeneration(core),
    owner = getCurrentOwner();
  const preflight = () => {
    if (generation !== getReactiveGeneration(core)) throw new Error('GPU resident context reset');
    if (isOwnerScopeDisposed(owner)) throw new Error('GPU resident computation owner disposed');
  };
  preflight();
  options = snapshotGpuOptionMetadata(options, preflight);
  const ctx = context(options.signal);
  let releaseAdmission: (() => void) | undefined, releaseBudget: (() => void) | undefined;
  let enqueued = false,
    published = false;
  const temporary: any[] = [];
  let outputBuffer: any;
  let pipelineKey: string | undefined;
  try {
    if (
      typeof options.wgsl !== 'string' ||
      (options.entryPoint !== undefined && typeof options.entryPoint !== 'string')
    )
      throw new TypeError('Invalid resident shader or entry point');
    const bytes = validateBytes(options.outputLength, ctx.device);
    const inputs: Array<{
      record?: ResidentRecord;
      tensor?: GpuTensorSignal;
      bytes: number;
      snapshot?: Float32Array;
    }> = [];
    let snapshotBytes = 0;
    for (const input of options.inputs) {
      ctx.assertCurrent();
      const record = records.get(input as GpuResidentTensor);
      if (record) {
        assertRecord(record);
        if (record.core !== core || record.device !== ctx.device)
          throw new Error('GPU resident input belongs to another context or device');
        inputs.push({ record, bytes: record.bytes });
        ctx.signals.push(record.lifetime.signal);
      } else {
        const tensor = input as GpuTensorSignal;
        if (tensorRecords.get(tensor)?.core !== core)
          throw new Error('GPU tensor belongs to a different reactive context');
        const inputBytes = validateBytes(tensor.length, ctx.device);
        snapshotBytes += inputBytes;
        inputs.push({ tensor, bytes: inputBytes });
      }
    }
    const groups = options.workgroups ?? [Math.ceil(options.outputLength / 64)];
    if (!Array.isArray(groups) || groups.length < 1 || groups.length > 3)
      throw new RangeError('Invalid GPU workgroup dimensions');
    const dimensions = [groups[0], groups[1] ?? 1, groups[2] ?? 1];
    ctx.assertCurrent();
    if (
      dimensions.some(
        n =>
          !Number.isInteger(n) ||
          n! < 1 ||
          n! > (ctx.device.limits?.maxComputeWorkgroupsPerDimension ?? 65535),
      )
    )
      throw new RangeError('Invalid GPU workgroup count');
    ctx.assertCurrent();
    releaseAdmission = reserveGpuComputeAdmission(ctx.device, snapshotBytes);
    releaseBudget = reserveResident(ctx.device, bytes);
    for (const input of inputs)
      if (input.tensor) {
        input.snapshot = input.tensor().slice();
        ctx.assertCurrent();
      }
    pipelineKey = computePipelineKey(
      options.wgsl,
      options.entryPoint ?? 'main',
      `resident/${inputs.length}/`,
    );
    const execute = async () => {
      try {
        ctx.assertCurrent();
        let pipeline: any;
        await validated(ctx, async () => {
          pipeline = findComputePipeline(ctx.device, pipelineKey);
          if (!pipeline) {
            const shader = ctx.device.createShaderModule!({ code: options.wgsl });
            if (shader.getCompilationInfo) {
              const info: any = await waitForGpu(shader.getCompilationInfo(), ctx.signals);
              const error = info.messages.find((message: any) => message.type === 'error');
              if (error)
                throw new Error(`WGSL ${error.lineNum}:${error.linePos}: ${error.message}`);
            }
            ctx.assertCurrent();
            // Enforce immutable inputs at WebGPU validation, regardless of WGSL declarations.
            const entries = Array.from({ length: inputs.length + 1 }, (_, binding) => ({
              binding,
              visibility: 0x04,
              buffer: { type: binding === inputs.length ? 'storage' : 'read-only-storage' },
            }));
            const groupLayout = ctx.device.createBindGroupLayout!({ entries });
            const layout = ctx.device.createPipelineLayout!({ bindGroupLayouts: [groupLayout] });
            pipeline = ctx.device.createComputePipeline!({
              layout,
              compute: { module: shader, entryPoint: options.entryPoint ?? 'main' },
            });
          }
          ctx.assertCurrent();
          const entries = inputs.map((input, binding) => {
            let buffer = input.record?.buffer;
            if (!buffer) {
              buffer = ctx.device.createBuffer!({ size: input.bytes, usage: 0x80 | 0x08 });
              if (!buffer) throw new Error('GPU resident upload allocation failed');
              temporary.push(buffer);
              ctx.device.queue.writeBuffer(
                buffer,
                0,
                input.snapshot!.buffer,
                input.snapshot!.byteOffset,
                input.bytes,
              );
            }
            return { binding, resource: { buffer, size: input.bytes } };
          });
          outputBuffer = ctx.device.createBuffer!({ size: bytes, usage: 0x80 | 0x04 | 0x20 });
          if (!outputBuffer) throw new Error('GPU resident output allocation failed');
          entries.push({ binding: inputs.length, resource: { buffer: outputBuffer, size: bytes } });
          ctx.assertCurrent();
          const group = ctx.device.createBindGroup!({
            layout: pipeline.getBindGroupLayout(0),
            entries,
          });
          const encoder = ctx.device.createCommandEncoder!(),
            pass = encoder.beginComputePass();
          pass.setPipeline(pipeline);
          pass.setBindGroup(0, group);
          pass.dispatchWorkgroups(...dimensions);
          pass.end();
          ctx.assertCurrent();
          ctx.device.queue.submit!([encoder.finish()]);
          await waitForGpu(ctx.device.queue.onSubmittedWorkDone!(), ctx.signals);
        });
        ctx.assertCurrent();
        for (const buffer of temporary) buffer.destroy?.();
        temporary.length = 0;
        ctx.assertCurrent();
        const handle = publishResident(ctx, outputBuffer, bytes, snapshotBytes, releaseBudget!);
        published = true;
        retainComputePipeline(ctx.device, pipelineKey, pipeline);
        return handle;
      } catch (failure) {
        forgetComputePipeline(ctx.device, pipelineKey);
        throw failure;
      } finally {
        for (const buffer of temporary) {
          try {
            buffer.destroy?.();
          } catch {
            /* Drain all buffers. */
          }
        }
        if (!published) {
          try {
            outputBuffer?.destroy?.();
          } finally {
            releaseBudget?.();
          }
        }
      }
    };
    enqueued = true;
    return await enqueue(ctx, releaseAdmission, execute);
  } finally {
    ctx.release();
    if (!enqueued) {
      releaseAdmission?.();
      releaseBudget?.();
    }
  }
}

/** Map only on explicit request; publish once after validation and revision checks. */
async function readResident(
  input: GpuResidentTensor,
  options: { signal?: AbortSignal },
  output?: GpuTensorSignal,
): Promise<GpuComputeResult | Float32Array> {
  const ctx = context(options.signal);
  let releaseAdmission: (() => void) | undefined,
    enqueued = false;
  try {
    const record = records.get(input);
    if (!record) throw new TypeError('Invalid GPU resident result');
    assertRecord(record);
    if (record.core !== ctx.core || record.device !== ctx.device)
      throw new Error('GPU resident input belongs to another context or device');
    const target = output ? tensorRecords.get(output) : undefined;
    if (output) {
      if (target?.core !== ctx.core)
        throw new Error('GPU output belongs to a different reactive context');
      output.peek();
      if (output.byteLength !== record.bytes)
        throw new RangeError('GPU readback output length mismatch');
    }
    ctx.signals.push(record.lifetime.signal);
    ctx.assertCurrent();
    releaseAdmission = reserveGpuComputeAdmission(ctx.device, record.bytes);
    const revision = target ? ++target.revision : 0;
    const assertOutput = () => {
      ctx.assertCurrent();
      if (target && target.revision !== revision)
        throw new Error('GPU output changed during computation');
    };
    const execute = async () => {
      let staging: any,
        mapped = false;
      try {
        const values = await validated(ctx, async () => {
          assertOutput();
          staging = ctx.device.createBuffer!({ size: record.bytes, usage: 0x01 | 0x08 });
          if (!staging) throw new Error('GPU readback allocation failed');
          const encoder = ctx.device.createCommandEncoder!();
          encoder.copyBufferToBuffer(record.buffer, 0, staging, 0, record.bytes);
          ctx.device.queue.submit!([encoder.finish()]);
          await waitForGpu(staging.mapAsync(0x01, 0, record.bytes), ctx.signals);
          mapped = true;
          return new Float32Array(staging.getMappedRange(0, record.bytes)).slice();
        });
        if (mapped) {
          staging.unmap();
          mapped = false;
        }
        staging.destroy?.();
        staging = undefined;
        return runUntrackedInCore(ctx.core, () => {
          assertOutput();
          if (!output) return values;
          output.write(values);
          return {
            backend: 'webgpu' as const,
            elements: values.length,
            uploadedBytes: 0,
            readbackBytes: record.bytes,
          };
        });
      } finally {
        if (mapped) {
          try {
            staging.unmap();
          } catch {
            /* Still destroy staging. */
          }
        }
        staging?.destroy?.();
      }
    };
    enqueued = true;
    return await enqueue(ctx, releaseAdmission, execute);
  } finally {
    ctx.release();
    if (!enqueued) releaseAdmission?.();
  }
}

/** Explicit mapped readback into a private CPU array; no WASM tensor or reactive write. */
export function readGpuResidentSnapshot(
  input: GpuResidentTensor,
  options: { signal?: AbortSignal } = {},
): Promise<Float32Array> {
  return readResident(input, options) as Promise<Float32Array>;
}
/** Explicit mapped readback with one revision-checked host tensor publication. */
export function readGpuResidentTensor(
  input: GpuResidentTensor,
  output: GpuTensorSignal,
  options: { signal?: AbortSignal } = {},
): Promise<GpuComputeResult> {
  return readResident(input, options, output) as Promise<GpuComputeResult>;
}
