/**
 * Bookkeeping around asynchronous GPU compute: the pipeline cache, admission limits per device
 * and waiting that honours abort signals.
 */
import type { GpuComputeQueueTelemetry, WebGpuDeviceLike } from './gpu-types.ts';

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

export const computeQueues = new WeakMap<WebGpuDeviceLike, Promise<void>>();
export type ComputePipelineCache = { entries: Map<string, any>; keyBytes: number };
export const computePipelineCaches = new WeakMap<WebGpuDeviceLike, ComputePipelineCache>();
export const MAX_COMPUTE_PIPELINES = 32;
export const MAX_COMPUTE_PIPELINE_KEY_BYTES = 1024 * 1024;
export function computePipelineKey(
  wgsl: string,
  entryPoint: string,
  namespace = '',
): string | undefined {
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
export function findComputePipeline(device: WebGpuDeviceLike, key: string | undefined): any {
  if (key === undefined) return;
  const cache = computePipelineCaches.get(device),
    pipeline = cache?.entries.get(key);
  if (pipeline) {
    cache!.entries.delete(key);
    cache!.entries.set(key, pipeline);
  }
  return pipeline;
}
export function forgetComputePipeline(device: WebGpuDeviceLike, key: string | undefined): void {
  if (key === undefined) return;
  const cache = computePipelineCaches.get(device);
  if (cache?.entries.delete(key)) cache.keyBytes -= key.length * 2;
}
/** Stores a pipeline, evicting the oldest entries past the count and key-size limits. */
export function cacheComputePipeline(
  device: WebGpuDeviceLike,
  key: string | undefined,
  pipeline: any,
): void {
  if (key === undefined) return;
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
export const pendingComputations = new WeakMap<WebGpuDeviceLike, number>();
export const MAX_PENDING_COMPUTATIONS = 32;
export const pendingSnapshotBytes = new WeakMap<WebGpuDeviceLike, number>();
export const computeSnapshotBudgets = new WeakMap<WebGpuDeviceLike, number>();
export const DEFAULT_COMPUTE_SNAPSHOT_BUDGET = 256 * 1024 * 1024;
export const residentComputeBudgets = new WeakMap<WebGpuDeviceLike, number>();
export const residentComputeUsage = new WeakMap<
  WebGpuDeviceLike,
  { bytes: number; count: number }
>();
export function computeQueueTelemetry(device: WebGpuDeviceLike | null): GpuComputeQueueTelemetry {
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
export function reserveGpuComputeAdmission(
  device: WebGpuDeviceLike,
  snapshotBytes: number,
): () => void {
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

export function checkGpuSignals(signals: AbortSignal[]): void {
  for (const signal of signals) signal.throwIfAborted();
}
export async function waitForGpu<T>(promise: PromiseLike<T>, signals: AbortSignal[]): Promise<T> {
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
