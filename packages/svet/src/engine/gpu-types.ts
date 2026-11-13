/**
 * The shapes the GPU subsystem exchanges: telemetry, tensor signals and the parts of the WebGPU
 * device it relies on.
 */
import type { ReadonlySignal } from './reactivity.ts';

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
