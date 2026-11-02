/** Internal typed bridge; GPU rendering does not import the compute executor. */
import { getReactiveGeneration, isOwnerScopeDisposed, type OwnerScope } from './reactivity.ts';
import type { SvetWasmExports } from './wasm-bridge.ts';
import type { WebGpuDeviceLike } from './gpu.ts';

export interface GpuResidentTensor {
  readonly length: number;
  readonly byteLength: number;
  readonly uploadedBytes: number;
  readonly active: boolean;
  dispose(): void;
}
export interface GpuResidentRecord {
  core: SvetWasmExports;
  device: WebGpuDeviceLike;
  buffer: any;
  bytes: number;
  disposed: boolean;
  lifetime: AbortController;
  owner: OwnerScope | null;
  generation: number;
  readSnapshot(input: GpuResidentTensor, options: {signal?: AbortSignal}): Promise<Float32Array>;
}
export const residentRecords = new WeakMap<GpuResidentTensor, GpuResidentRecord>();
export function assertResidentRecord(record: GpuResidentRecord): void {
  if (record.disposed || isOwnerScopeDisposed(record.owner)) throw new Error('GPU resident result disposed');
  if (record.generation !== getReactiveGeneration(record.core)) throw new Error('GPU resident context reset');
}
export function residentVertexRecord(handle: GpuResidentTensor, core: SvetWasmExports,
  device: WebGpuDeviceLike | null, vertexCount: number): GpuResidentRecord {
  const record = residentRecords.get(handle);
  if (!record) throw new TypeError('Invalid GPU resident vertices');
  assertResidentRecord(record);
  if (record.core !== core || record.device !== device) throw new Error('Resident vertices belong to another context or device');
  if (record.bytes !== vertexCount * 16) throw new RangeError('Resident vertices must contain one clip-space float4 per mesh vertex');
  return record;
}
export function snapshotResidentVertices(input: GpuResidentTensor, signal: AbortSignal): Promise<Float32Array> {
  const record = residentRecords.get(input);
  if (!record) return Promise.reject(new TypeError('Invalid GPU resident result'));
  return record.readSnapshot(input, {signal});
}
