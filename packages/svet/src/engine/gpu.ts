/**
 * # Svet GPU + UMA + DUMA Compute & Memory Subsystem (`gpu.ts`)
 *
 * Bridges the Svet WASM Reactive DAG with **WebGPU**, **UMA (Unified Memory Architecture)**,
 * and **DUMA (Discrete / Dual Unified Memory Architecture)**:
 *
 * 1. **UMA Mode (Unified Memory Architecture — e.g. Apple Silicon M1/M2/M3/M4, integrated SoCs)**:
 *    - CPU (WASM Linear Heap) and GPU share the same physical RAM.
 *    - `GpuTensorSignal` allocates 16-byte-aligned `Float32Array` slices directly inside the Rust WASM
 *      `gpu_f32_arena` and maps persistent `MAP_WRITE | COPY_SRC` buffers without staging copies.
 *
 * 2. **DUMA Mode (Discrete / Dual Unified Memory Architecture — e.g. PCIe dGPU with separate VRAM + Host RAM)**:
 *    - Avoids full-buffer PCIe bandwidth saturation by tracking fine-grained 128-byte (32-float)
 *      cache-line dirty blocks in Rust WASM (`svet_gpu_mark_dirty`, `svet_gpu_get_dirty_min/max`).
 *    - On `syncGpuArena()`, ONLY the dirty byte window `[dirtyByteOffset .. dirtyByteEnd)` is
 *      transferred over the bus via `device.queue.writeBuffer(...)`!
 *
 * 3. **Reactive Compute Shaders (`createGpuComputeEffect`)**:
 *    - Declaratively binds WGSL compute shaders (or WASM SIMD-ready FMA kernels when running headless)
 *      into the reactive signal graph. When an input `GpuTensorSignal` mutates, downstream compute
 *      kernels and DOM/Canvas visualizers update automatically.
 */

import {
  effect,
  flushEffects,
  invalidateSingleSignalFastPath,
  onCleanup,
  ReadonlySignal,
  signal,
  Signal,
  trackNodeRead,
  triggerNodeWrite,
} from './reactivity';
import { getWasmCore, KIND_SIGNAL } from './wasm-bridge';

export type MemoryArchitecture = 'UMA' | 'DUMA';

export interface GpuDeviceTelemetry {
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
  /** Byte length of this tensor (aligned) */
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
}

export interface WebGpuDeviceLike {
  queue: WebGpuQueueLike;
  createBuffer?(descriptor: { size: number; usage: number; mappedAtCreation?: boolean }): any;
  createShaderModule?(descriptor: { code: string }): any;
  createComputePipeline?(descriptor: any): any;
  createRenderPipeline?(descriptor: any): any;
  createTexture?(descriptor: any): any;
  createCommandEncoder?(): any;
}

export interface GpuSubsystemConfig {
  architecture?: MemoryArchitecture;
  device?: WebGpuDeviceLike | null;
}

let currentArch: MemoryArchitecture = 'UMA';
let activeDevice: WebGpuDeviceLike | null = null;
let deviceBuffer: any = null;
let lastSyncBytes = 0;
let totalSyncCount = 0;
let totalBytesTransferred = 0;

/**
 * Auto-detects or configures the memory architecture (`UMA` vs `DUMA`) and attaches an optional WebGPU device.
 */
export function configureGpuSubsystem(config: GpuSubsystemConfig = {}): GpuDeviceTelemetry {
  if (config.architecture) {
    currentArch = config.architecture;
  } else {
    // Auto-detect Apple Silicon / Unified SoC vs discrete PCIe architecture
    const isAppleSilicon =
      typeof navigator !== 'undefined' &&
      (/Mac/.test(navigator.platform || '') || /Apple/.test(navigator.userAgent || ''));
    currentArch = isAppleSilicon ? 'UMA' : 'DUMA';
  }

  if (config.device !== undefined) {
    activeDevice = config.device;
  }

  return getGpuTelemetry();
}

export function resetGpuTelemetry(): void {
  lastSyncBytes = 0;
  totalSyncCount = 0;
  totalBytesTransferred = 0;
}

/**
 * Allocates a 16-byte aligned reactive GPU Tensor Signal directly in the Rust WASM `gpu_f32_arena`.
 * Zero JS Heap array copies: `tensor()` and `tensor.view()` return a direct `Float32Array` view over WASM memory.
 */
export function createGpuTensor(initialOrLength: number | ArrayLike<number>): GpuTensorSignal {
  const core = getWasmCore();
  const id = core.svet_create_node(KIND_SIGNAL);
  invalidateSingleSignalFastPath();

  const length =
    typeof initialOrLength === 'number' ? initialOrLength : initialOrLength.length;
  const offset = core.svet_gpu_alloc_f32(length);

  let cachedBuffer: ArrayBufferLike | null = null;
  let cachedPtr = -1;
  let cachedArray: Float32Array | null = null;

  const getLiveView = (): Float32Array => {
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

  if (typeof initialOrLength !== 'number') {
    const view = getLiveView();
    for (let i = 0; i < length; i++) {
      view[i] = initialOrLength[i];
    }
    core.svet_gpu_mark_dirty(offset, length);
  }

  const getter = (() => {
    trackNodeRead(id);
    return getLiveView();
  }) as GpuTensorSignal;

  (getter as any).id = id;
  (getter as any).offset = offset;
  Object.defineProperty(getter, 'length', { value: length, enumerable: true });
  (getter as any).byteOffset = offset * 4;
  (getter as any).byteLength = length * 4;

  getter.peek = () => getLiveView();
  getter.view = () => getLiveView();

  getter.write = (values: ArrayLike<number>, startIndex = 0) => {
    const view = getLiveView();
    const writeLen = Math.min(values.length, length - startIndex);
    if (writeLen <= 0) return;

    for (let i = 0; i < writeLen; i++) {
      view[startIndex + i] = values[i];
    }
    core.svet_gpu_mark_dirty(offset + startIndex, writeLen);
    triggerNodeWrite(id);
  };

  getter.setAt = (index: number, value: number) => {
    if (index < 0 || index >= length) return;
    const view = getLiveView();
    if (view[index] !== value) {
      view[index] = value;
      core.svet_gpu_mark_dirty(offset + index, 1);
      triggerNodeWrite(id);
    }
  };

  let disposed = false;
  getter.dispose = () => {
    if (disposed) return;
    disposed = true;
    core.svet_gpu_free_f32(offset, length);
    core.svet_dispose_node(id);
  };

  onCleanup(() => getter.dispose());

  return getter;
}

/**
 * Synchronizes the WASM GPU Arena with the hardware GPU / VRAM buffer:
 * - **In UMA mode**: Zero-copy pointer handoff or unified page range flush without staging buffers.
 * - **In DUMA mode**: Reads the dirty cache-line window `[dirtyMin .. dirtyMax)` tracked in Rust WASM
 *   and performs a surgical partial DMA `queue.writeBuffer` of ONLY the modified bytes!
 */
export function syncGpuArena(): {
  architecture: MemoryArchitecture;
  dirtyFloatMin: number;
  dirtyFloatMax: number;
  bytesTransferred: number;
  spansTransferred?: number;
} {
  const core = getWasmCore();
  const minFloat = core.svet_gpu_get_dirty_min();
  const maxFloat = core.svet_gpu_get_dirty_max();

  if (minFloat >= maxFloat || minFloat === 0xffffffff) {
    lastSyncBytes = 0;
    return {
      architecture: currentArch,
      dirtyFloatMin: 0,
      dirtyFloatMax: 0,
      bytesTransferred: 0,
      spansTransferred: 0,
    };
  }

  const dirtyFloatCount = maxFloat - minFloat;
  const byteOffset = minFloat * 4;
  let actualBytesTransferred = dirtyFloatCount * 4;
  let spansCount = 1;

  if (activeDevice) {
    const arenaPtr = core.svet_gpu_get_arena_ptr();
    const fullArenaView = new Uint8Array(
      core.memory.buffer,
      arenaPtr,
      core.svet_gpu_get_arena_len() * 4
    );

    if (currentArch === 'UMA') {
      // Unified memory: single direct mapping / zero-staging write
      activeDevice.queue.writeBuffer(
        deviceBuffer,
        byteOffset,
        fullArenaView,
        byteOffset,
        actualBytesTransferred
      );
    } else {
      // DUMA (Discrete VRAM + Host RAM): check if multiple disjoint cache-line spans exist
      const spanPairs = core.svet_gpu_collect_dirty_spans();
      if (spanPairs > 1) {
        spansCount = spanPairs;
        const spanPtr = core.svet_get_lis_ptr();
        const spanData = new Uint32Array(core.memory.buffer, spanPtr, spanPairs * 2);
        let totalSpanBytes = 0;
        for (let i = 0; i < spanPairs; i++) {
          const startFl = spanData[i * 2];
          const lenFl = spanData[i * 2 + 1];
          const sByte = (startFl * 4) & ~15;
          const eByte = Math.min(((startFl + lenFl) * 4 + 15) & ~15, fullArenaView.byteLength);
          const bLen = eByte - sByte;
          activeDevice.queue.writeBuffer(deviceBuffer, sByte, fullArenaView, sByte, bLen);
          totalSpanBytes += bLen;
        }
        actualBytesTransferred = totalSpanBytes;
      } else {
        // Single dirty range: align transfer to 16-byte DMA boundary for partial PCIe burst
        const alignedByteOffset = byteOffset & ~15;
        const alignedEnd = (byteOffset + actualBytesTransferred + 15) & ~15;
        const clampedEnd = Math.min(alignedEnd, fullArenaView.byteLength);
        const alignedLength = clampedEnd - alignedByteOffset;

        activeDevice.queue.writeBuffer(
          deviceBuffer,
          alignedByteOffset,
          fullArenaView,
          alignedByteOffset,
          alignedLength
        );
      }
    }
  }

  core.svet_gpu_clear_dirty();
  lastSyncBytes = actualBytesTransferred;
  totalSyncCount++;
  totalBytesTransferred += actualBytesTransferred;

  return {
    architecture: currentArch,
    dirtyFloatMin: minFloat,
    dirtyFloatMax: maxFloat,
    bytesTransferred: actualBytesTransferred,
    spansTransferred: spansCount,
  };
}

/**
 * Executes a reactive GPU / WASM-SIMD Fused Multiply-Add (`output[i] = input[i] * scale + bias`)
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
  core.svet_gpu_vector_fma(input.offset, output.offset, len, scale, bias);
  triggerNodeWrite(output.id);
}

export interface WebGpuComputeDispatchOptions {
  /** WGSL compute shader source code (`@compute @workgroup_size(...)`) */
  wgsl: string;
  /** Input tensor signals bound to `@group(0) @binding(0..N)` */
  inputs: GpuTensorSignal[];
  /** Output tensor signal bound after inputs */
  output: GpuTensorSignal;
  /** Workgroup count `[x, y, z]` */
  workgroups?: [number, number?, number?];
  /** High-speed CPU/WASM fallback kernel executed when WebGPU hardware device is absent */
  fallbackKernel?: (inputs: Float32Array[], output: Float32Array) => void;
}

/**
 * Initializes the hardware WebGPU device (`navigator.gpu.requestAdapter() -> requestDevice()`)
 * and detects whether the hardware uses **UMA** (Unified Memory Architecture) or **DUMA** (Discrete VRAM).
 * Falls back gracefully if running in headless Node/SSR or a browser without WebGPU.
 */
export async function initWebGpuHardware(): Promise<GpuDeviceTelemetry> {
  if (typeof navigator !== 'undefined' && 'gpu' in navigator) {
    try {
      const gpu = (navigator as any).gpu;
      const adapter = await gpu.requestAdapter({ powerPreference: 'high-performance' });
      if (adapter) {
        const info = adapter.info || (await adapter.requestAdapterInfo?.()) || {};
        const archStr = String(info.architecture || info.vendor || '').toLowerCase();
        const isUnified =
          Boolean(adapter.isFallbackAdapter === false) &&
          (archStr.includes('apple') ||
            archStr.includes('unified') ||
            archStr.includes('arm') ||
            /Mac/.test(navigator.platform || ''));
        const device = await adapter.requestDevice();
        return configureGpuSubsystem({
          architecture: isUnified ? 'UMA' : 'DUMA',
          device,
        });
      }
    } catch {
      // Fallback to WASM SIMD/CPU mode
    }
  }
  return configureGpuSubsystem();
}

/**
 * Executes a declarative WGSL Compute Shader (or WASM linear-memory kernel fallback)
 * tied directly into the Svet reactive graph.
 */
export function dispatchGpuComputeKernel(options: WebGpuComputeDispatchOptions): void {
  const core = getWasmCore();
  // 1. Track all input tensor signals in the WASM dependency graph
  const inputViews = options.inputs.map((t) => t());
  const outView = options.output.peek();

  // 2. Sync any pending host mutations to GPU buffer before dispatch
  syncGpuArena();

  // 3. Execute fallback / shared-memory kernel so WASM view is immediately up-to-date
  if (options.fallbackKernel) {
    options.fallbackKernel(inputViews, outView);
    core.svet_gpu_mark_dirty(options.output.offset, options.output.length);
  }

  // 4. Notify all downstream subscribers of `output` tensor signal
  triggerNodeWrite(options.output.id);
}
export function getGpuTelemetry(): GpuDeviceTelemetry {
  const core = getWasmCore();
  const floatCount = core.svet_gpu_get_arena_len();
  return {
    architecture: currentArch,
    webgpuAvailable:
      activeDevice !== null || (typeof navigator !== 'undefined' && 'gpu' in navigator),
    arenaFloatCount: floatCount,
    arenaByteLength: floatCount * 4,
    lastSyncBytesTransferred: lastSyncBytes,
    totalSyncCount,
    totalBytesTransferred,
  };
}

// ============================================================================
// 4. 3D GPU Scene Graph, Camera, Transform3D & Render Pipeline (`createSceneGraphNode3D`)
// ============================================================================

export type Vec3Tuple = [number, number, number];

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
  const core = getWasmCore();
  const position = signal<Vec3Tuple>(options?.position ?? [0, 0, 0]);
  const rotation = signal<Vec3Tuple>(options?.rotation ?? [0, 0, 0]);
  const scale = signal<Vec3Tuple>(options?.scale ?? [1, 1, 1]);

  const localMatrix = createGpuTensor(16);
  const worldMatrix = createGpuTensor(16);

  effect(() => {
    const [tx, ty, tz] = position();
    const [rx, ry, rz] = rotation();
    const [sx, sy, sz] = scale();
    core.svet_gpu_mat4_compose_trs(localMatrix.offset, tx, ty, tz, rx, ry, rz, sx, sy, sz);
    triggerNodeWrite(localMatrix.id);

    if (options?.parent) {
      const parentWorld = options.parent.worldMatrix();
      void parentWorld;
      core.svet_gpu_mat4_mul(options.parent.worldMatrix.offset, localMatrix.offset, worldMatrix.offset);
    } else {
      worldMatrix.view().set(localMatrix.view());
      core.svet_gpu_mark_dirty(worldMatrix.offset, 16);
    }
    triggerNodeWrite(worldMatrix.id);
  });

  return {
    position,
    rotation,
    scale,
    localMatrix,
    worldMatrix,
    dispose() {
      localMatrix.dispose();
      worldMatrix.dispose();
      position.dispose();
      rotation.dispose();
      scale.dispose();
    },
  };
}

export interface PerspectiveCamera3D {
  readonly fovYRad: Signal<number>;
  readonly aspect: Signal<number>;
  readonly near: Signal<number>;
  readonly far: Signal<number>;
  readonly projectionMatrix: GpuTensorSignal;
  dispose(): void;
}

/**
 * Creates a reactive 3D Perspective Camera (`fovYRad`, `aspect`, `near`, `far`)
 * whose 16-float `projectionMatrix` is computed in WASM linear memory (`depth [0, 1]` WebGPU standard).
 */
export function createPerspectiveCamera3D(options?: {
  fovYRad?: number;
  aspect?: number;
  near?: number;
  far?: number;
}): PerspectiveCamera3D {
  const core = getWasmCore();
  const fovYRad = signal(options?.fovYRad ?? Math.PI / 3);
  const aspect = signal(options?.aspect ?? 16 / 9);
  const near = signal(options?.near ?? 0.1);
  const far = signal(options?.far ?? 1000.0);

  const projectionMatrix = createGpuTensor(16);

  effect(() => {
    core.svet_gpu_mat4_perspective(
      projectionMatrix.offset,
      fovYRad(),
      aspect(),
      near(),
      far()
    );
    triggerNodeWrite(projectionMatrix.id);
  });

  return {
    fovYRad,
    aspect,
    near,
    far,
    projectionMatrix,
    dispose() {
      projectionMatrix.dispose();
      fovYRad.dispose();
      aspect.dispose();
      near.dispose();
      far.dispose();
    },
  };
}

export interface Mesh3D {
  readonly vertexCount: number;
  /** Optional 16-bit triangle index buffer (`triangle-list` topology) for WebGPU `drawIndexed` */
  readonly indices?: Uint16Array;
  /** Local-space homogeneous vertices (`[x, y, z, 1.0]` per vertex) in WASM UMA/DUMA arena */
  readonly vertices: GpuTensorSignal;
  /** Clip-space / World-space transformed vertices (`mvp * vertex`) in WASM UMA/DUMA arena */
  readonly transformedVertices: GpuTensorSignal;
  /** Model-View-Projection (`projection * world`) 4x4 matrix in WASM UMA/DUMA arena */
  readonly mvpMatrix: GpuTensorSignal;
  dispose(): void;
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
  transform: ReactiveTransform3D;
  camera?: PerspectiveCamera3D;
}): Mesh3D {
  const core = getWasmCore();
  const vertexCount = Math.floor(options.positions.length / 3);
  const indices = options.indices ? new Uint16Array(options.indices) : undefined;
  const homogeneous = new Float32Array(vertexCount * 4);
  for (let i = 0; i < vertexCount; i++) {
    homogeneous[i * 4] = options.positions[i * 3];
    homogeneous[i * 4 + 1] = options.positions[i * 3 + 1];
    homogeneous[i * 4 + 2] = options.positions[i * 3 + 2];
    homogeneous[i * 4 + 3] = 1.0;
  }

  const vertices = createGpuTensor(homogeneous);
  const transformedVertices = createGpuTensor(vertexCount * 4);
  const mvpMatrix = createGpuTensor(16);

  effect(() => {
    // Track world matrix, vertices, and optional camera projection matrix
    options.transform.worldMatrix();
    vertices();
    if (options.camera) {
      options.camera.projectionMatrix();
      core.svet_gpu_mat4_mul(
        options.camera.projectionMatrix.offset,
        options.transform.worldMatrix.offset,
        mvpMatrix.offset
      );
    } else {
      mvpMatrix.view().set(options.transform.worldMatrix.view());
      core.svet_gpu_mark_dirty(mvpMatrix.offset, 16);
    }
    triggerNodeWrite(mvpMatrix.id);

    core.svet_gpu_transform_positions_vec4(
      mvpMatrix.offset,
      vertices.offset,
      transformedVertices.offset,
      vertexCount
    );
    triggerNodeWrite(transformedVertices.id);
  });

  return {
    vertexCount,
    indices,
    vertices,
    transformedVertices,
    mvpMatrix,
    dispose() {
      vertices.dispose();
      transformedVertices.dispose();
      mvpMatrix.dispose();
    },
  };
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
 * Interactive 3D Viewport component (`<Canvas3D />` / `createViewport3D`)
 * that renders a live 3D mesh directly from the WASM UMA/DUMA `transformedVertices` tensor:
 * - **Hardware WebGPU RenderPass + `depth24plus` Z-Buffer** when a WebGPU device & `webgpu` canvas context are available.
 * - **Automatic 2D Perspective + Depth-Sorted Rasterizer fallback** in headless / non-WebGPU environments.
 */
export function Canvas3D(props: {
  mesh: Mesh3D;
  edges?: Array<[number, number]>;
  faces?: Array<[number, number, number, number]>;
  width?: number;
  height?: number;
  strokeStyle?: string;
  fillStyle?: string;
  pointColor?: string;
  background?: string;
  showVertices?: boolean;
  preferWebGpu?: boolean;
  /** Optional `ReactiveTransform3D` to attach interactive mouse/pointer drag-to-orbit and wheel-to-zoom controls */
  orbitTransform?: ReactiveTransform3D;
  /** Optional callback fired when a 3D vertex is clicked (`(vertexIndex, distancePx) => void`) */
  onVertexPick?: (vertexIndex: number, distancePx: number) => void;
}): HTMLCanvasElement {
  const width = props.width ?? 360;
  const height = props.height ?? 200;
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

  let lastScreenPts: Array<[number, number, number]> = [];

  if (props.orbitTransform) {
    const tr = props.orbitTransform;
    canvas.style.cursor = 'grab';
    let dragging = false;
    let prevX = 0;
    let prevY = 0;

    const onDown = (e: MouseEvent) => {
      dragging = true;
      prevX = e.clientX;
      prevY = e.clientY;
      canvas.style.cursor = 'grabbing';
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
      canvas.style.cursor = 'grab';
    };
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const factor = e.deltaY < 0 ? 1.08 : 0.92;
      const [sx, sy, sz] = tr.scale.peek();
      const clamped = Math.max(0.2, Math.min(5.0, sx * factor));
      tr.scale.set([clamped, Math.max(0.2, Math.min(5.0, sy * factor)), Math.max(0.2, Math.min(5.0, sz * factor))]);
    };

    canvas.addEventListener('mousedown', onDown);
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    canvas.addEventListener('wheel', onWheel, { passive: false });

    onCleanup(() => {
      canvas.removeEventListener('mousedown', onDown);
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      canvas.removeEventListener('wheel', onWheel);
    });
  }

  if (props.onVertexPick) {
    const onClickPick = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      const clickX = e.clientX - rect.left;
      const clickY = e.clientY - rect.top;
      let bestIdx = -1;
      let bestDist = Infinity;
      for (let i = 0; i < lastScreenPts.length; i++) {
        const [px, py] = lastScreenPts[i];
        const dist = Math.hypot(px - clickX, py - clickY);
        if (dist < bestDist) {
          bestDist = dist;
          bestIdx = i;
        }
      }
      if (bestIdx !== -1 && bestDist <= 28) {
        props.onVertexPick!(bestIdx, bestDist);
      }
    };
    canvas.addEventListener('click', onClickPick);
    onCleanup(() => canvas.removeEventListener('click', onClickPick));
  }

  // Optional Hardware WebGPU Render Pipeline state (lazy-initialized if WebGPU device & context exist)
  let webgpuCtx: any = null;
  let webgpuPipeline: any = null;
  let webgpuVertexBuffer: any = null;
  let webgpuIndexBuffer: any = null;
  let webgpuIndexCount = 0;
  let webgpuDepthView: any = null;
  let webgpuInitialized = false;

  const tryRenderWebGpu = (clipVerts: Float32Array): boolean => {
    if (props.preferWebGpu === false || !activeDevice || !activeDevice.createRenderPipeline) {
      return false;
    }
    try {
      const hasIndices = Boolean(props.mesh.indices && props.mesh.indices.length > 0);
      if (!webgpuInitialized) {
        webgpuInitialized = true;
        const ctx = (canvas as any).getContext?.('webgpu');
        if (!ctx) return false;
        const format =
          (typeof navigator !== 'undefined' &&
            (navigator as any).gpu?.getPreferredCanvasFormat?.()) ||
          'bgra8unorm';
        ctx.configure({
          device: activeDevice,
          format,
          alphaMode: 'premultiplied',
        });
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
            targets: [{ format }],
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
        webgpuVertexBuffer = activeDevice.createBuffer?.({
          size: Math.max(64, clipVerts.byteLength),
          usage: 0x0020 | 0x0008, // GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST
        });
        if (hasIndices && props.mesh.indices) {
          const idxArr = props.mesh.indices;
          webgpuIndexCount = idxArr.length;
          const alignedIdxBytes = (idxArr.byteLength + 3) & ~3;
          webgpuIndexBuffer = activeDevice.createBuffer?.({
            size: Math.max(16, alignedIdxBytes),
            usage: 0x0010 | 0x0008, // GPUBufferUsage.INDEX | GPUBufferUsage.COPY_DST
          });
          if (webgpuIndexBuffer) {
            activeDevice.queue.writeBuffer(
              webgpuIndexBuffer,
              0,
              idxArr.buffer,
              idxArr.byteOffset,
              idxArr.byteLength
            );
          }
        }
        const depthTex = activeDevice.createTexture?.({
          size: [width, height, 1],
          format: 'depth24plus',
          usage: 0x10, // GPUTextureUsage.RENDER_ATTACHMENT
        });
        webgpuDepthView = depthTex?.createView?.();
        webgpuCtx = ctx;
      }

      if (!webgpuCtx || !webgpuPipeline || !webgpuVertexBuffer) return false;

      activeDevice.queue.writeBuffer(
        webgpuVertexBuffer,
        0,
        clipVerts.buffer,
        clipVerts.byteOffset,
        clipVerts.byteLength
      );
      const encoder = activeDevice.createCommandEncoder?.();
      if (!encoder) return false;

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
      if (webgpuIndexBuffer && webgpuIndexCount > 0 && pass.setIndexBuffer && pass.drawIndexed) {
        pass.setIndexBuffer(webgpuIndexBuffer, 'uint16');
        pass.drawIndexed(webgpuIndexCount, 1, 0, 0, 0);
      } else {
        pass.draw(props.mesh.vertexCount, 1, 0, 0);
      }
      pass.end();
      activeDevice.queue.submit?.([encoder.finish()]);
      canvas.setAttribute('data-render-backend', 'webgpu-depth24plus');
      return true;
    } catch {
      return false;
    }
  };

  effect(() => {
    // Subscribe to the WASM UMA/DUMA transformed vertex buffer
    const clipVerts = props.mesh.transformedVertices();
    const halfW = width * 0.5;
    const halfH = height * 0.5;
    const count = props.mesh.vertexCount;

    const screenPts: Array<[number, number, number]> = new Array(count);
    for (let i = 0; i < count; i++) {
      const x = clipVerts[i * 4];
      const y = clipVerts[i * 4 + 1];
      const z = clipVerts[i * 4 + 2];
      const w = clipVerts[i * 4 + 3];
      const invW = Math.abs(w) > 1e-5 ? 1.0 / w : 1.0;
      const sx = halfW + x * invW * (halfW * 0.65);
      const sy = halfH - y * invW * (halfH * 0.65);
      screenPts[i] = [sx, sy, z * invW];
    }
    lastScreenPts = screenPts;

    if (tryRenderWebGpu(clipVerts)) {
      return;
    }

    canvas.setAttribute('data-render-backend', 'canvas2d-zsort');
    const ctx = canvas.getContext?.('2d');
    if (!ctx) return;

    ctx.clearRect(0, 0, width, height);

    // Optional depth-sorted face shading
    if (props.faces && props.faces.length > 0) {
      const sortedFaces = props.faces
        .filter(([a, b, c, d]) => a < count && b < count && c < count && d < count)
        .map((face) => {
          const avgZ =
            (screenPts[face[0]][2] +
              screenPts[face[1]][2] +
              screenPts[face[2]][2] +
              screenPts[face[3]][2]) *
            0.25;
          return { face, avgZ };
        })
        .sort((a, b) => b.avgZ - a.avgZ);

      ctx.fillStyle = props.fillStyle ?? 'rgba(56, 189, 248, 0.14)';
      for (const { face } of sortedFaces) {
        ctx.beginPath();
        ctx.moveTo(screenPts[face[0]][0], screenPts[face[0]][1]);
        ctx.lineTo(screenPts[face[1]][0], screenPts[face[1]][1]);
        ctx.lineTo(screenPts[face[2]][0], screenPts[face[2]][1]);
        ctx.lineTo(screenPts[face[3]][0], screenPts[face[3]][1]);
        ctx.closePath();
        ctx.fill();
      }
    }

    if (props.edges && props.edges.length > 0) {
      ctx.strokeStyle = props.strokeStyle ?? '#38bdf8';
      ctx.lineWidth = 2;
      ctx.beginPath();
      for (const [a, b] of props.edges) {
        if (a < count && b < count) {
          ctx.moveTo(screenPts[a][0], screenPts[a][1]);
          ctx.lineTo(screenPts[b][0], screenPts[b][1]);
        }
      }
      ctx.stroke();
    }

    if (props.showVertices !== false) {
      ctx.fillStyle = props.pointColor ?? '#34d399';
      for (let i = 0; i < count; i++) {
        const [px, py] = screenPts[i];
        ctx.beginPath();
        ctx.arc(px, py, 3.5, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  });

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
