/**
 * # 3D GPU Scene Graph, Camera, Transform3D & Render Pipeline (`gpu-scene3d.ts`)
 *
 * Reactive 3D transforms, perspective cameras, indexed/wireframe meshes, Z-aware WASM
 * vertex picking, and dual WebGPU (`depth24plus`) + Canvas2D Z-sort viewports.
 */

import {
  batch,
  effect,
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  onCleanup,
  runUntrackedInCore,
  runWithCapturedOwner,
  signal,
  triggerNodeWrite,
  untrack,
  type Signal,
} from './reactivity.ts';
import {
  getWasmCore,
  withWasmCore,
  withoutAsyncCoreContext,
  type SvetWasmExports,
} from './wasm-bridge.ts';
import {
  assertResidentRecord,
  residentVertexRecord,
  snapshotResidentVertices,
  type GpuResidentRecord,
  type GpuResidentTensor,
} from './gpu-resident-bridge.ts';
import {
  createGpuTensor,
  gpuState,
  reviseTensor,
  snapshotGpuOptionMetadata,
  type GpuTensorSignal,
  type WebGpuDeviceLike,
} from './gpu.ts';

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

type SceneDisposable = { dispose(): void };
/** Release unpublished scene resources while preserving the initialization failure. */
function constructSceneResource<T>(
  build: (retain: <R extends SceneDisposable>(resource: R) => R) => T,
): T {
  const resources: SceneDisposable[] = [];
  const retain = <R extends SceneDisposable>(resource: R): R => {
    resources.push(resource);
    return resource;
  };
  try {
    return build(retain);
  } catch (failure) {
    for (let i = resources.length - 1; i >= 0; i--) {
      const res = resources[i];
      try {
        res?.dispose();
      } catch {
        /* Continue releasing partial state. */
      }
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
  const core = getWasmCore(),
    generation = getReactiveGeneration(core),
    owner = getCurrentOwner();
  const assertCurrent = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('Transform construction owner disposed');
    if (generation !== getReactiveGeneration(core))
      throw new Error('Transform context reset during construction');
  };
  assertCurrent();
  options = options ? snapshotGpuOptionMetadata(options, assertCurrent) : undefined;
  const initialPosition = options?.position,
    initialRotation = options?.rotation,
    initialScale = options?.scale;
  const parentMatrix = options?.parent?.worldMatrix;
  assertCurrent();
  parentMatrix?.peek();
  assertCurrent();

  return constructSceneResource(retain => {
    const position = retain(signal<Vec3Tuple>(initialPosition ?? [0, 0, 0]));
    const rotation = retain(signal<Vec3Tuple>(initialRotation ?? [0, 0, 0]));
    const scale = retain(signal<Vec3Tuple>(initialScale ?? [1, 1, 1]));
    const localMatrix = retain(createGpuTensor(16));
    const worldMatrix = retain(createGpuTensor(16));
    const computation = retain(
      effect(() => {
        const [tx, ty, tz] = position();
        const [rx, ry, rz] = rotation();
        const [sx, sy, sz] = scale();
        // Complete user-controlled iteration/coercion before any native matrix write.
        const fTx = Math.fround(tx),
          fTy = Math.fround(ty),
          fTz = Math.fround(tz),
          fRx = Math.fround(rx),
          fRy = Math.fround(ry),
          fRz = Math.fround(rz),
          fSx = Math.fround(sx),
          fSy = Math.fround(sy),
          fSz = Math.fround(sz);
        assertCurrent();
        localMatrix.peek();
        worldMatrix.peek();
        parentMatrix?.();
        assertCurrent();
        localMatrix.peek();
        worldMatrix.peek();
        core.svet_gpu_mat4_compose_trs(
          localMatrix.offset,
          fTx,
          fTy,
          fTz,
          fRx,
          fRy,
          fRz,
          fSx,
          fSy,
          fSz,
        );
        reviseTensor(localMatrix);
        triggerNodeWrite(localMatrix.id);
        localMatrix.peek();
        worldMatrix.peek();
        parentMatrix?.peek();
        if (parentMatrix)
          core.svet_gpu_mat4_mul(parentMatrix.offset, localMatrix.offset, worldMatrix.offset);
        else {
          worldMatrix.view().set(localMatrix.view());
          core.svet_gpu_mark_dirty(worldMatrix.offset, 16);
        }
        reviseTensor(worldMatrix);
        triggerNodeWrite(worldMatrix.id);
      }),
    );
    assertCurrent();
    return {
      position,
      rotation,
      scale,
      localMatrix,
      worldMatrix,
      dispose: ownSceneDisposal(() => {
        computation.dispose();
        localMatrix.dispose();
        worldMatrix.dispose();
        position.dispose();
        rotation.dispose();
        scale.dispose();
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
export function createPerspectiveCamera3D(
  options?: PerspectiveCamera3DOptions,
): PerspectiveCamera3D {
  const core = getWasmCore(),
    generation = getReactiveGeneration(core),
    owner = getCurrentOwner();
  const validate = (values: [number, number, number, number]) => {
    const [v0, v1, v2, v3] = values;
    if (
      !values.every(value => typeof value === 'number' && Number.isFinite(value)) ||
      !core.svet_gpu_valid_perspective(v0, v1, v2, v3)
    ) {
      throw new RangeError('Invalid PerspectiveCamera3D projection parameters');
    }
  };
  const assertCurrent = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('Camera construction owner disposed');
    if (generation !== getReactiveGeneration(core))
      throw new Error('Camera context reset during construction');
  };
  assertCurrent();
  const read = (key: keyof PerspectiveCamera3DOptions, fallback: number) => {
    const value = options?.[key] ?? fallback;
    assertCurrent();
    return value;
  };
  const initial: [number, number, number, number] = [
    read('fovYRad', Math.PI / 3),
    read('aspect', 16 / 9),
    read('near', 0.1),
    read('far', 1000),
  ];
  validate(initial);
  assertCurrent();
  return constructSceneResource(retain => {
    const fovYRad = retain(signal(initial[0])),
      aspect = retain(signal(initial[1])),
      near = retain(signal(initial[2])),
      far = retain(signal(initial[3]));
    const projectionMatrix = retain(createGpuTensor(16));

    const parameters: [Signal<number>, Signal<number>, Signal<number>, Signal<number>] = [
      fovYRad,
      aspect,
      near,
      far,
    ];
    const setters: [
      Signal<number>['set'],
      Signal<number>['set'],
      Signal<number>['set'],
      Signal<number>['set'],
    ] = [fovYRad.set, aspect.set, near.set, far.set];
    ([0, 1, 2, 3] as const).forEach(index => {
      const parameter = parameters[index];
      const setter = setters[index];
      parameter.set = next =>
        setter(previous => {
          const value = typeof next === 'function' ? next(previous) : next;
          projectionMatrix.peek();
          const candidate: [number, number, number, number] = [
            parameters[0].peek(),
            parameters[1].peek(),
            parameters[2].peek(),
            parameters[3].peek(),
          ];
          candidate[index] = value;
          validate(candidate);
          return value;
        });
    });

    const computation = retain(
      effect(() => {
        core.svet_gpu_mat4_perspective(projectionMatrix.offset, fovYRad(), aspect(), near(), far());
        reviseTensor(projectionMatrix);
        triggerNodeWrite(projectionMatrix.id);
      }),
    );
    assertCurrent();

    return {
      fovYRad,
      aspect,
      near,
      far,
      projectionMatrix,
      setProjection(projOptions) {
        projectionMatrix.peek();
        const candidate: [number, number, number, number] = [
          projOptions.fovYRad ?? fovYRad.peek(),
          projOptions.aspect ?? aspect.peek(),
          projOptions.near ?? near.peek(),
          projOptions.far ?? far.peek(),
        ];
        validate(candidate);
        projectionMatrix.peek();
        batch(() => {
          setters[0](candidate[0]);
          setters[1](candidate[1]);
          setters[2](candidate[2]);
          setters[3](candidate[3]);
        });
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
  /** Maximum bytes in one privately copied index array; fixed at construction. */
  readonly maxIndexBytes: number;
  /** Optional triangle index buffer; its element width selects the WebGPU index format. */
  readonly indices?: Uint16Array | Uint32Array | undefined;
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

function copyMeshIndices(
  values: ArrayLike<number> | undefined,
  vertexCount: number,
  indexFormat: 'uint16' | 'uint32',
  maxIndexBytes: number,
  assertCurrent: () => void,
): Uint16Array | Uint32Array | undefined {
  if (indexFormat !== 'uint16' && indexFormat !== 'uint32')
    throw new RangeError('Invalid Mesh3D index format');
  const maxIndex = indexFormat === 'uint32' ? 0xffffffff : 65535;
  let indices: Uint16Array | Uint32Array | undefined;
  if (values) {
    const length = values.length;
    assertCurrent();
    if (!Number.isSafeInteger(length) || length < 0 || length % 3 !== 0) {
      throw new RangeError('Mesh3D triangle indices must contain complete triples');
    }
    const bytesPerIndex = indexFormat === 'uint32' ? 4 : 2;
    if (length > Math.floor(maxIndexBytes / bytesPerIndex)) {
      throw new RangeError('Mesh3D index snapshot exceeds maxIndexBytes');
    }
    indices = indexFormat === 'uint32' ? new Uint32Array(length) : new Uint16Array(length);
    for (let i = 0; i < length; i++) {
      const index = values[i];
      assertCurrent();
      if (
        typeof index !== 'number' ||
        !Number.isInteger(index) ||
        index < 0 ||
        index > maxIndex ||
        index >= vertexCount
      ) {
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
  indices?: ArrayLike<number>; // Optional triangle indices (3 per triangle)
  /** Defaults to uint16; select uint32 explicitly for indices above 65535. */
  indexFormat?: 'uint16' | 'uint32';
  /** Per-snapshot CPU index byte budget; defaults to the native arena byte limit
   * (currently 256 MiB). This is independent of GPU allocation limits. */
  maxIndexBytes?: number;
  transform: ReactiveTransform3D;
  camera?: PerspectiveCamera3D | undefined;
}): Mesh3D {
  const core = getWasmCore(),
    generation = getReactiveGeneration(core),
    owner = getCurrentOwner();
  const assertConstruction = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('Mesh3D construction owner disposed');
    if (generation !== getReactiveGeneration(core))
      throw new Error('Mesh3D context reset during construction');
  };
  assertConstruction();
  options = snapshotGpuOptionMetadata(options, assertConstruction);
  const maxIndexBytes =
    options.maxIndexBytes ?? core.svet_gpu_get_arena_limit_f32() * Float32Array.BYTES_PER_ELEMENT;
  if (!Number.isSafeInteger(maxIndexBytes) || maxIndexBytes < 0) {
    throw new RangeError('Mesh3D maxIndexBytes must be a nonnegative safe integer');
  }
  const worldMatrix = options.transform.worldMatrix;
  assertConstruction();
  worldMatrix.peek();
  assertConstruction();
  const projectionMatrix = options.camera?.projectionMatrix;
  assertConstruction();
  projectionMatrix?.peek();
  assertConstruction();
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
  const indices = copyMeshIndices(
    options.indices,
    vertexCount,
    indexFormat,
    maxIndexBytes,
    assertConstruction,
  );
  const homogeneous = new Float32Array(vertexCount * 4);
  for (let i = 0; i < vertexCount; i++) {
    for (let axis = 0; axis < 3; axis++) {
      const rawCoord = options.positions[i * 3 + axis];
      const value = Math.fround(rawCoord ?? NaN);
      assertConstruction();
      if (!Number.isFinite(value))
        throw new RangeError('Mesh3D positions must be finite float32 values');
      homogeneous[i * 4 + axis] = value;
    }
    homogeneous[i * 4 + 3] = 1.0;
  }
  // Borrowed dependency access above may be instrumented by user code.
  worldMatrix.peek();
  projectionMatrix?.peek();
  assertConstruction();
  return constructSceneResource(retain => {
    const vertices = retain(createGpuTensor(homogeneous));
    const transformedVertices = retain(createGpuTensor(vertexCount * 4));
    const mvpMatrix = retain(createGpuTensor(16));
    const computation = retain(
      effect(() => {
        worldMatrix();
        vertices();
        projectionMatrix?.();
        assertConstruction();
        vertices.peek();
        transformedVertices.peek();
        mvpMatrix.peek();
        worldMatrix.peek();
        projectionMatrix?.peek();
        if (projectionMatrix) {
          core.svet_gpu_mat4_mul(projectionMatrix.offset, worldMatrix.offset, mvpMatrix.offset);
        } else {
          const source = worldMatrix.view();
          assertConstruction();
          worldMatrix.peek();
          mvpMatrix.view().set(source);
          core.svet_gpu_mark_dirty(mvpMatrix.offset, 16);
        }
        reviseTensor(mvpMatrix);
        triggerNodeWrite(mvpMatrix.id);
        assertConstruction();
        mvpMatrix.peek();
        vertices.peek();
        transformedVertices.peek();
        core.svet_gpu_transform_positions_vec4(
          mvpMatrix.offset,
          vertices.offset,
          transformedVertices.offset,
          vertexCount,
        );
        reviseTensor(transformedVertices);
        triggerNodeWrite(transformedVertices.id);
      }),
    );
    assertConstruction();
    const topology = retain(signal<Uint16Array | Uint32Array | undefined>(indices));
    let disposed = false,
      topologyVersion = 0;
    const assertLive = () => {
      if (disposed || isOwnerScopeDisposed(owner) || generation !== getReactiveGeneration(core))
        throw new Error('Mesh3D has been disposed or invalidated');
      if (getWasmCore() !== core) throw new Error('Mesh3D belongs to a different reactive context');
    };
    return {
      vertexCount,
      get maxIndexBytes() {
        return maxIndexBytes;
      },
      get indices() {
        assertLive();
        return topology();
      },
      updateIndices(values, format = indexFormat) {
        assertLive();
        const version = topologyVersion;
        const assertCandidate = () => {
          assertLive();
          if (version !== topologyVersion) throw new Error('Mesh3D topology changed during update');
        };
        const next = copyMeshIndices(values, vertexCount, format, maxIndexBytes, assertCandidate);
        assertCandidate();
        // Failed candidates never advance the commit version. Observers may
        // publish another topology during set; no older state is written after it.
        topologyVersion++;
        indexFormat = format;
        topology.set(next);
      },
      vertices,
      transformedVertices,
      mvpMatrix,
      dispose: ownSceneDisposal(() => {
        disposed = true;
        topology.dispose();
        computation.dispose();
        vertices.dispose();
        transformedVertices.dispose();
        mvpMatrix.dispose();
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
  camera?: PerspectiveCamera3D | undefined;
}): {
  mesh: Mesh3D;
  edges: Array<[number, number]>;
  faces: Array<[number, number, number, number]>;
} {
  const s = (options.size ?? 1.2) * 0.5;
  const positions = [
    -s,
    -s,
    -s,
    s,
    -s,
    -s,
    s,
    s,
    -s,
    -s,
    s,
    -s,
    -s,
    -s,
    s,
    s,
    -s,
    s,
    s,
    s,
    s,
    -s,
    s,
    s,
  ];
  const edges: Array<[number, number]> = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
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
    0,
    1,
    2,
    0,
    2,
    3, // Back
    4,
    5,
    6,
    4,
    6,
    7, // Front
    0,
    1,
    5,
    0,
    5,
    4, // Bottom
    3,
    2,
    6,
    3,
    6,
    7, // Top
    0,
    3,
    7,
    0,
    7,
    4, // Left
    1,
    2,
    6,
    1,
    6,
    5, // Right
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
  },
): { vertexIndex: number; distancePx: number; ndcZ: number } | null {
  const core = getWasmCore();
  mesh.transformedVertices();
  const vw = options?.viewportWidth ?? 360;
  const vh = options?.viewportHeight ?? 200;
  const maxRadius = options?.maxRadiusPx ?? 28;
  if (
    ![vw, vh, clickX, clickY, maxRadius].every(Number.isFinite) ||
    vw <= 0 ||
    vh <= 0 ||
    maxRadius < 0
  )
    return null;

  const indices = mesh.indices;
  let pickedIdx: number;
  if (options?.occlusion !== 'none' && indices?.length) {
    const pointer = core.svet_prepare_gpu_pick_indices(indices.length) >>> 0;
    new Uint32Array(core.memory.buffer, pointer, indices.length).set(indices);
    pickedIdx = core.svet_gpu_pick_visible_vertex_3d(
      mesh.transformedVertices.offset,
      mesh.vertexCount,
      indices.length,
      vw,
      vh,
      clickX,
      clickY,
      maxRadius,
    );
  } else {
    pickedIdx = core.svet_gpu_pick_nearest_vertex_3d(
      mesh.transformedVertices.offset,
      mesh.vertexCount,
      vw,
      vh,
      clickX,
      clickY,
      maxRadius,
    );
  }
  if (pickedIdx < 0 || pickedIdx >= mesh.vertexCount) {
    return null;
  }

  const clipVerts = mesh.transformedVertices.view();
  const base = pickedIdx * 4;
  const x = clipVerts[base] ?? 0;
  const y = clipVerts[base + 1] ?? 0;
  const z = clipVerts[base + 2] ?? 0;
  const w = clipVerts[base + 3] ?? 1;
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
  /** GPU source identity for resident frames; mesh host positions are not its rendered data. */
  gpuVertices?: GpuResidentTensor;
  id: number;
  width: number;
  height: number;
  backend: 'webgpu-depth24plus' | 'canvas2d-zsort';
}
interface VertexPickSnapshot3D extends VertexPickFrame3D {
  vertices: Float32Array;
  resident?: GpuResidentTensor | undefined;
  indices?: Uint16Array | Uint32Array | undefined;
  vertexCount: number;
}
function pickFrameSnapshot(
  core: SvetWasmExports,
  frame: VertexPickSnapshot3D,
  x: number,
  y: number,
  occlude: boolean,
) {
  if (!frame.vertexCount) return null;
  const pointer = core.svet_prepare_gpu_pick_vertices(frame.vertices.length) >>> 0;
  if (!pointer) throw new RangeError('Vertex picking scratch capacity exceeded');
  new Float32Array(core.memory.buffer, pointer, frame.vertices.length).set(frame.vertices);
  if (occlude && frame.indices?.length) {
    const indexPointer = core.svet_prepare_gpu_pick_indices(frame.indices.length) >>> 0;
    new Uint32Array(core.memory.buffer, indexPointer, frame.indices.length).set(frame.indices);
  }
  const index = core.svet_gpu_pick_snapshot_3d(
    frame.vertexCount,
    frame.indices?.length ?? 0,
    occlude ? 1 : 0,
    frame.width,
    frame.height,
    x,
    y,
    28,
  );
  if (index < 0 || index >= frame.vertexCount) return null;
  const base = index * 4,
    vx = frame.vertices[base] ?? 0,
    vy = frame.vertices[base + 1] ?? 0,
    vz = frame.vertices[base + 2] ?? 0,
    w = frame.vertices[base + 3] ?? 1;
  const px = frame.width * 0.5 * (1 + vx / w),
    py = frame.height * 0.5 * (1 - vy / w);
  return {
    vertexIndex: index,
    distancePx: Math.hypot(px - x, py - y),
    ndcZ: vz / w,
  };
}

export interface Canvas3DProps {
  /** Mesh lifetime remains caller-owned; an accessor supports reactive replacement. */
  mesh: Mesh3D | (() => Mesh3D);
  /** Optional immutable clip-space float4 buffer, matching mesh.vertexCount.
   * Draws directly on WebGPU. Picking reads a snapshot only on click; no 2D substitute. */
  gpuVertices?: GpuResidentTensor | (() => GpuResidentTensor | null) | null;
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
  renderMode?: 'sync' | 'animation-frame' | undefined;
  /** Numeric hit plus source identity/dimensions of the last drawn/submitted frame. */
  onVertexPick?: (
    vertexIndex: number,
    distancePx: number,
    ndcZ: number,
    frame: VertexPickFrame3D,
  ) => void;
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
  primary.style.display =
    viewport.dataset.renderBackend === 'webgpu-depth24plus' ? 'block' : 'none';
  fallback.style.display = viewport.dataset.renderBackend === 'canvas2d-zsort' ? 'block' : 'none';
  viewport.style.width = `${primary.width + 2}px`;
  viewport.append(primary, fallback);
  return viewport;
}

type RenderValidationTarget = {
  settle(frame: number, error: { message: string } | null): void;
  queue(frame: number, error: { message: string } | null): void;
};
function observeRenderValidation(
  results: Array<{ filter: string; result: PromiseLike<{ message: string } | null> }>,
  target: WeakRef<RenderValidationTarget>,
  frame: number,
): void {
  let remaining = results.length,
    failed = false;
  const fail = (filter: string, message: string) => {
    if (failed) return;
    failed = true;
    target
      .deref()
      ?.settle(frame, { message: filter === 'validation' ? message : `${filter}: ${message}` });
  };
  withoutAsyncCoreContext(() => {
    for (const { filter, result } of results) {
      Promise.resolve(result)
        .then(
          error => {
            if (error) fail(filter, error.message);
            else if (--remaining === 0 && !failed) target.deref()?.settle(frame, null);
          },
          error => fail(filter, error instanceof Error ? error.message : String(error)),
        )
        .catch(() => {});
    }
  });
}

function observeRenderQueue(
  result: PromiseLike<void>,
  target: WeakRef<RenderValidationTarget>,
  frame: number,
): void {
  withoutAsyncCoreContext(() => {
    Promise.resolve(result)
      .then(
        () => target.deref()?.queue(frame, null),
        error =>
          target
            .deref()
            ?.queue(frame, { message: error instanceof Error ? error.message : String(error) }),
      )
      .catch(() => {});
  });
}

interface DestroyableResource {
  destroy?(): void;
}

interface WebGpuTextureViewHandle {
  readonly __brand?: unknown;
}

interface WebGpuTextureHandle extends DestroyableResource {
  createView?(): WebGpuTextureViewHandle;
}

interface WebGpuRenderPassHandle {
  setPipeline(pipeline: unknown): void;
  setVertexBuffer(slot: number, buffer: unknown): void;
  setIndexBuffer?(buffer: unknown, indexFormat: 'uint16' | 'uint32'): void;
  drawIndexed?(
    indexCount: number,
    instanceCount?: number,
    firstIndex?: number,
    baseVertex?: number,
    firstInstance?: number,
  ): void;
  draw(
    vertexCount: number,
    instanceCount?: number,
    firstVertex?: number,
    firstInstance?: number,
  ): void;
  end(): void;
}

interface WebGpuCommandEncoderHandle {
  beginRenderPass(descriptor: unknown): WebGpuRenderPassHandle;
  finish(): unknown;
}

interface WebGpuCanvasContextHandle {
  configure(config: { device: WebGpuDeviceLike; format: string; alphaMode: string }): void;
  unconfigure?(): void;
  getCurrentTexture(): { createView(): WebGpuTextureViewHandle };
}

function createCanvas3D(
  props: Canvas3DProps,
  surfaces?: { viewport: HTMLDivElement; fallback: HTMLCanvasElement },
): HTMLCanvasElement {
  const renderOwner = getCurrentOwner(),
    renderGeneration = getReactiveGeneration();
  const currentMesh = () => (typeof props.mesh === 'function' ? props.mesh() : props.mesh);
  const currentResident = () =>
    typeof props.gpuVertices === 'function' ? props.gpuVertices() : (props.gpuVertices ?? null);
  const dimension = (value: number | (() => number) | undefined, fallback: number) => {
    const result = typeof value === 'function' ? value() : (value ?? fallback);
    if (!Number.isInteger(result) || result < 1 || result > 0xffffffff) {
      throw new RangeError('Canvas3D dimensions must be positive uint32 integers');
    }
    return result;
  };
  let width = untrack(() => dimension(props.width, 360));
  let height = untrack(() => dimension(props.height, 200));
  if (
    props.renderMode !== undefined &&
    props.renderMode !== 'sync' &&
    props.renderMode !== 'animation-frame'
  )
    throw new RangeError('Invalid renderMode');
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
  if (surfaces) {
    surfaces.fallback.width = width;
    surfaces.fallback.height = height;
  }
  const selectBackend = (backend: string) => {
    if (backend === 'unavailable') clearPickFrame();
    canvas.dataset.renderBackend = backend;
    if (surfaces) {
      surfaces.viewport.dataset.renderBackend = backend;
      canvas.style.display = backend === 'webgpu-depth24plus' ? 'block' : 'none';
      surfaces.fallback.style.display = backend === 'canvas2d-zsort' ? 'block' : 'none';
    }
  };

  let pickSnapshot: VertexPickSnapshot3D | null = null;
  let pendingPick: AbortController | null = null;
  function clearPickFrame() {
    if (pendingPick) {
      pendingPick.abort(new Error('GPU pick frame retired'));
      for (const surface of surfaces ? [canvas, surfaces.viewport] : [canvas])
        surface.dataset.pickReadback = 'cancelled';
    }
    pendingPick = null;
    pickSnapshot = null;
  }
  let pickSequence = 0;
  const capturePickFrame = (
    mesh: Mesh3D,
    vertices: Float32Array | null,
    indices: Mesh3D['indices'],
    backend: VertexPickFrame3D['backend'],
    resident?: GpuResidentTensor,
  ) => {
    clearPickFrame();
    if (!props.onVertexPick) return;
    pickSnapshot = {
      mesh,
      id: ++pickSequence,
      width,
      height,
      backend,
      vertexCount: mesh.vertexCount,
      vertices: vertices?.slice() ?? new Float32Array(0),
      resident,
      indices: indices?.slice(),
    };
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
      tr.scale.set([
        clamped,
        Math.max(0.2, Math.min(5.0, sy * factor)),
        Math.max(0.2, Math.min(5.0, sz * factor)),
      ]);
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
      const pickingCanvas =
        surfaces && canvas.style.display === 'none' ? surfaces.fallback : canvas;
      const rect = pickingCanvas.getBoundingClientRect();
      const clickX = (e.clientX - rect.left) * (rect.width ? frame.width / rect.width : 1);
      const clickY = (e.clientY - rect.top) * (rect.height ? frame.height / rect.height : 1);
      const occlusion =
        props.pickOcclusion ?? (frame.backend === 'webgpu-depth24plus' ? 'indexed' : 'none');
      const publishPick = (vertices: Float32Array) => {
        if (
          rendererDisposed ||
          pickSnapshot !== frame ||
          renderGeneration !== getReactiveGeneration(renderCore) ||
          isOwnerScopeDisposed(renderOwner)
        )
          return;
        const hit = withWasmCore(renderCore, () =>
          pickFrameSnapshot(
            renderCore,
            { ...frame, vertices },
            clickX,
            clickY,
            occlusion === 'indexed',
          ),
        );
        if (hit) {
          const baseInfo: VertexPickFrame3D = {
            mesh: frame.mesh,
            id: frame.id,
            width: frame.width,
            height: frame.height,
            backend: frame.backend,
          };
          const info: VertexPickFrame3D = frame.resident
            ? { ...baseInfo, gpuVertices: frame.resident }
            : baseInfo;
          runUntrackedInCore(renderCore, () =>
            runWithCapturedOwner(renderCore, renderOwner, () => {
              const callback = props.onVertexPick;
              if (
                !rendererDisposed &&
                pickSnapshot === frame &&
                !isOwnerScopeDisposed(renderOwner)
              ) {
                callback?.(hit.vertexIndex, hit.distancePx, hit.ndcZ, info);
              }
            }),
          );
        }
      };
      if (!frame.resident) {
        publishPick(frame.vertices);
        return;
      }
      const residentFrame = frame.resident;
      pendingPick?.abort(new Error('GPU pick superseded'));
      const cancellation = new AbortController();
      pendingPick = cancellation;
      for (const surface of surfaces ? [canvas, surfaces.viewport] : [canvas]) {
        surface.dataset.pickReadback = 'pending';
        delete surface.dataset.pickError;
      }
      const request = runUntrackedInCore(renderCore, () =>
        runWithCapturedOwner(renderCore, renderOwner, () =>
          snapshotResidentVertices(residentFrame, cancellation.signal),
        ),
      );
      void request
        .then(vertices => {
          if (pendingPick !== cancellation || cancellation.signal.aborted) return;
          publishPick(vertices);
          if (pendingPick !== cancellation || cancellation.signal.aborted || rendererDisposed)
            return;
          for (const surface of surfaces ? [canvas, surfaces.viewport] : [canvas])
            surface.dataset.pickReadback = 'passed';
        })
        .catch((error: unknown) => {
          if (pendingPick !== cancellation || cancellation.signal.aborted || rendererDisposed)
            return;
          for (const surface of surfaces ? [canvas, surfaces.viewport] : [canvas]) {
            surface.dataset.pickReadback = 'failed';
            surface.dataset.pickError = String(error);
          }
        })
        .finally(() => {
          if (pendingPick === cancellation) pendingPick = null;
        });
    };
    interaction.addEventListener('click', onClickPick);
    onCleanup(() => interaction.removeEventListener('click', onClickPick));
  }

  // Optional Hardware WebGPU Render Pipeline state (lazy-initialized if WebGPU device & context exist)
  let webgpuCtx: WebGpuCanvasContextHandle | null = null;
  let webgpuPipeline: unknown = null;
  let webgpuVertexBuffer: DestroyableResource | null = null;
  let webgpuVertexCapacity = 0;
  let webgpuIndexBuffer: DestroyableResource | null = null;
  let webgpuIndexCount = 0;
  let webgpuIndexCapacity = 0;
  let uploadedIndices: Mesh3D['indices'] | null = null;
  let pipelineIndexed: boolean | null = null;
  let renderFormat = 'bgra8unorm';
  let webgpuDepthView: WebGpuTextureViewHandle | null = null;
  let webgpuDepthTexture: WebGpuTextureHandle | null = null;
  let depthWidth = 0,
    depthHeight = 0;
  const renderCore = getWasmCore();
  const renderState = gpuState(renderCore);
  let renderDevice: WebGpuDeviceLike | null = null;
  let renderEpoch = 0,
    validationFloor = 0,
    lastValidatedFrame = 0,
    rendererDisposed = false;
  const scheduledMode =
    props.renderMode === 'animation-frame' && Boolean(view?.requestAnimationFrame);
  let frameRequest: number | null = null,
    dirty = false,
    initialDraw = true;
  let drainingFrame: number | null = null;
  let submission: { frame: number; queueDone: boolean; validationDone: boolean } | null = null;
  const cancelFrameRequest = () => {
    if (frameRequest !== null) view?.cancelAnimationFrame(frameRequest);
    frameRequest = null;
  };
  const scheduleLatest = () => {
    if (
      !scheduledMode ||
      rendererDisposed ||
      !dirty ||
      submission ||
      drainingFrame !== null ||
      frameRequest !== null ||
      !view
    )
      return;
    frameRequest = view.requestAnimationFrame(() => {
      frameRequest = null;
      if (rendererDisposed || submission || drainingFrame !== null || !dirty) return;
      dirty = false;
      try {
        withWasmCore(renderCore, () => untrack(() => draw()));
      } catch (error) {
        releaseRenderResources();
        setValidation('failed', String(error));
        selectBackend('unavailable');
      }
    });
  };
  const finishSubmission = () => {
    if (submission?.queueDone && submission.validationDone) {
      submission = null;
      scheduleLatest();
    }
  };
  const failFrame = (message: string, queueFinished = false) => {
    if (!queueFinished && submission && !submission.queueDone) drainingFrame = submission.frame;
    releaseRenderResources();
    setValidation('failed', message);
    try {
      withWasmCore(renderCore, () => untrack(() => draw(true)));
    } catch {
      selectBackend('unavailable');
    }
    scheduleLatest();
  };
  const setValidation = (status: string, message?: string) => {
    for (const surface of surfaces ? [canvas, surfaces.viewport] : [canvas]) {
      surface.dataset.renderValidation = status;
      if (message) surface.dataset.renderError = message;
      else delete surface.dataset.renderError;
    }
  };
  const validationTarget: RenderValidationTarget = {
    settle(frame, error) {
      if (rendererDisposed || frame <= validationFloor) return;
      if (!error) {
        lastValidatedFrame = Math.max(lastValidatedFrame, frame);
        if (frame === renderEpoch) setValidation('passed');
        if (submission?.frame === frame) {
          submission.validationDone = true;
          finishSubmission();
        }
        return;
      }
      // Do not hide a failing stream merely because another frame is pending.
      if (frame <= lastValidatedFrame) return;
      failFrame(error.message);
    },
    queue(frame, error) {
      if (rendererDisposed) return;
      if (drainingFrame === frame) {
        drainingFrame = null;
        scheduleLatest();
        return;
      }
      if (submission?.frame !== frame || frame <= validationFloor) return;
      if (error) {
        failFrame(`GPU queue: ${error.message}`, true);
        return;
      }
      submission.queueDone = true;
      finishSubmission();
    },
  };
  const validationReference = new WeakRef(validationTarget);
  let selectedResident: GpuResidentRecord | null = null;
  let releaseResidentWatch = () => {};
  const watchResident = (record: GpuResidentRecord | null) => {
    if (selectedResident === record) return;
    releaseResidentWatch();
    releaseResidentWatch = () => {};
    selectedResident = record;
    if (!record) return;
    const stop = () => {
      if (rendererDisposed) return;
      // Preserve scheduled backpressure and resume the newest dirty source only
      // after the retired frame's queue completion has been observed.
      failFrame('Resident vertices disposed');
    };
    record.lifetime.signal.addEventListener('abort', stop, { once: true });
    releaseResidentWatch = () => record.lifetime.signal.removeEventListener('abort', stop);
  };
  const releaseRenderResources = () => {
    releaseResidentWatch();
    releaseResidentWatch = () => {};
    selectedResident = null;
    validationFloor = ++renderEpoch;
    clearPickFrame();
    submission = null;
    const resources: Array<DestroyableResource | null> = [
      webgpuVertexBuffer,
      webgpuIndexBuffer,
      webgpuDepthTexture,
    ];
    const ctx = webgpuCtx;
    webgpuVertexBuffer =
      webgpuIndexBuffer =
      webgpuDepthTexture =
      webgpuDepthView =
      webgpuPipeline =
      webgpuCtx =
        null;
    webgpuInitialized = false;
    renderDevice = null;
    webgpuIndexCount = 0;
    webgpuIndexCapacity = 0;
    uploadedIndices = null;
    pipelineIndexed = null;
    depthWidth = depthHeight = 0;
    webgpuVertexCapacity = 0;
    for (const resource of resources) {
      try {
        resource?.destroy?.();
      } catch {
        /* finish releasing other allocations */
      }
    }
    try {
      ctx?.unconfigure?.();
    } catch {
      /* device may already be lost */
    }
  };
  let webgpuInitialized = false;
  let lastRenderFailure: string | undefined;

  const tryRenderWebGpu = (
    clipVerts: Float32Array | null,
    mesh: Mesh3D,
    indices: Mesh3D['indices'],
    resident: GpuResidentRecord | null = null,
    residentHandle?: GpuResidentTensor,
  ): boolean => {
    lastRenderFailure = undefined;
    const activeDevice = renderState.device;
    if (activeDevice !== renderDevice) releaseRenderResources();
    if (props.preferWebGpu === false || !activeDevice || !activeDevice.createRenderPipeline) {
      return false;
    }
    watchResident(resident);
    const vertexBytes = resident?.bytes ?? clipVerts?.byteLength ?? 0;
    const frame = ++renderEpoch;
    const scopes: Array<'validation' | 'out-of-memory' | 'internal'> = [];
    try {
      if (activeDevice.pushErrorScope && activeDevice.popErrorScope) {
        for (const filter of ['validation', 'out-of-memory', 'internal'] as const) {
          activeDevice.pushErrorScope(filter);
          scopes.push(filter);
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
      checkLimit('maxBufferSize', resident ? vertexBytes : Math.max(64, vertexBytes));
      if (indices) checkLimit('maxBufferSize', Math.max(16, Math.ceil(indices.byteLength / 4) * 4));
      if (!webgpuInitialized) {
        const getCtx = (canvas as unknown as { getContext?(id: string): unknown }).getContext;
        const ctx = getCtx?.call(canvas, 'webgpu') as WebGpuCanvasContextHandle | null | undefined;
        if (!ctx) return false;
        webgpuCtx = ctx;
        renderDevice = activeDevice;
        const navGpu =
          typeof navigator !== 'undefined'
            ? (
                navigator as unknown as {
                  gpu?: { getPreferredCanvasFormat?(): string | undefined };
                }
              ).gpu
            : undefined;
        const format = navGpu?.getPreferredCanvasFormat?.() || 'bgra8unorm';
        renderFormat = format;
        ctx.configure({
          device: activeDevice,
          format,
          alphaMode: 'premultiplied',
        });
        webgpuInitialized = true;
      }
      if (resident && webgpuVertexBuffer) {
        webgpuVertexBuffer.destroy?.();
        webgpuVertexBuffer = null;
        webgpuVertexCapacity = 0;
      }
      if (!resident && (!webgpuVertexBuffer || vertexBytes > webgpuVertexCapacity)) {
        const capacity = Math.max(64, vertexBytes);
        const next = activeDevice.createBuffer?.({
          size: capacity,
          usage: 0x0020 | 0x0008,
        }) as DestroyableResource | null | undefined;
        if (!next) throw new Error('Incomplete render resources');
        const previous = webgpuVertexBuffer;
        webgpuVertexBuffer = next;
        webgpuVertexCapacity = capacity;
        previous?.destroy?.();
      }
      if (!webgpuDepthView || depthWidth !== width || depthHeight !== height) {
        const texture = activeDevice.createTexture?.({
          size: [width, height, 1],
          format: 'depth24plus',
          usage: 0x10,
        }) as WebGpuTextureHandle | null | undefined;
        let texView: WebGpuTextureViewHandle | undefined;
        try {
          texView = texture?.createView?.();
          if (!texView) throw new Error('Incomplete depth attachment');
        } catch (error) {
          texture?.destroy?.();
          throw error;
        }
        const previous = webgpuDepthTexture;
        webgpuDepthTexture = texture ?? null;
        webgpuDepthView = texView;
        depthWidth = width;
        depthHeight = height;
        previous?.destroy?.();
      }

      const vertexBuffer: unknown = resident?.buffer ?? webgpuVertexBuffer;
      if (!webgpuCtx || !vertexBuffer) return false;
      if (!webgpuPipeline || pipelineIndexed !== hasIndices) {
        const shaderModule: unknown = activeDevice.createShaderModule?.({
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
            const next = activeDevice.createBuffer?.({
              size: capacity,
              usage: 0x0010 | 0x0008,
            }) as DestroyableResource | null | undefined;
            if (!next) throw new Error('Unable to allocate index buffer');
            const previous = webgpuIndexBuffer;
            webgpuIndexBuffer = next;
            webgpuIndexCapacity = capacity;
            previous?.destroy?.();
          }
          if (alignedBytes) {
            const padded = new Uint8Array(alignedBytes);
            padded.set(new Uint8Array(indices.buffer, indices.byteOffset, indices.byteLength));
            activeDevice.queue.writeBuffer(webgpuIndexBuffer, 0, padded.buffer, 0, alignedBytes);
          }
          webgpuIndexCount = indices.length;
        } else {
          webgpuIndexBuffer?.destroy?.();
          webgpuIndexBuffer = null;
          webgpuIndexCapacity = 0;
          webgpuIndexCount = 0;
        }
        uploadedIndices = indices;
      }

      if (!resident && clipVerts)
        activeDevice.queue.writeBuffer(
          webgpuVertexBuffer,
          0,
          clipVerts.buffer,
          clipVerts.byteOffset,
          vertexBytes,
        );
      const encoder = activeDevice.createCommandEncoder?.() as
        WebGpuCommandEncoderHandle | null | undefined;
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
      pass.setVertexBuffer(0, vertexBuffer);
      if (webgpuIndexBuffer && pass.setIndexBuffer && pass.drawIndexed) {
        pass.setIndexBuffer(
          webgpuIndexBuffer,
          indices?.BYTES_PER_ELEMENT === 4 ? 'uint32' : 'uint16',
        );
        pass.drawIndexed(webgpuIndexCount, 1, 0, 0, 0);
      } else {
        pass.draw(mesh.vertexCount, 1, 0, 0);
      }
      pass.end();
      if (!activeDevice.queue.submit) throw new Error('Missing GPU queue submit');
      if (scheduledMode && !activeDevice.queue.onSubmittedWorkDone)
        throw new Error('GPU queue completion is unavailable');
      if (scheduledMode)
        submission = { frame, queueDone: false, validationDone: scopes.length === 0 };
      if (resident) assertResidentRecord(resident);
      activeDevice.queue.submit([encoder.finish()]);
      if (resident) {
        assertResidentRecord(resident);
        if (
          isOwnerScopeDisposed(renderOwner) ||
          renderGeneration !== getReactiveGeneration(renderCore)
        )
          throw new Error('Resident renderer lifetime ended');
      }
      if (scheduledMode && activeDevice.queue.onSubmittedWorkDone)
        observeRenderQueue(activeDevice.queue.onSubmittedWorkDone(), validationReference, frame);
      selectBackend('webgpu-depth24plus');
      capturePickFrame(mesh, clipVerts, indices, 'webgpu-depth24plus', residentHandle);
      return true;
    } catch (error) {
      releaseRenderResources();
      lastRenderFailure = error instanceof Error ? error.message : String(error);
      setValidation('failed', lastRenderFailure);
      return false;
    } finally {
      if (scopes.length && activeDevice.popErrorScope) {
        const popScope = activeDevice.popErrorScope.bind(activeDevice);
        const results = scopes.reverse().map(filter => {
          try {
            return { filter, result: popScope() };
          } catch (error) {
            return { filter, result: Promise.reject(error) };
          }
        });
        observeRenderValidation(results, validationReference, frame);
      }
    }
  };

  let screenCoords = new Float32Array(0);
  const draw = (force2d = false) => {
    const nextWidth = dimension(props.width, 360),
      nextHeight = dimension(props.height, 200);
    if (nextWidth !== width || nextHeight !== height) {
      clearPickFrame();
      width = nextWidth;
      height = nextHeight;
      for (const surface of surfaces ? [canvas, surfaces.fallback] : [canvas]) {
        surface.width = width;
        surface.height = height;
        surface.style.width = `${width}px`;
        surface.style.height = `${height}px`;
      }
      if (surfaces) surfaces.viewport.style.width = `${width + 2}px`;
    }
    const mesh = currentMesh(),
      residentHandle = currentResident();
    const indices = mesh.indices;
    if (residentHandle) {
      // A resident frame has no authoritative host replica for forced recovery.
      // Keep the original GPU diagnostic rather than replacing it with a fallback error.
      if (force2d) {
        selectBackend('unavailable');
        return;
      }
      try {
        const resident = residentVertexRecord(
          residentHandle,
          renderCore,
          renderState.device,
          mesh.vertexCount,
        );
        if (tryRenderWebGpu(null, mesh, indices, resident, residentHandle)) return;
        throw new Error(
          lastRenderFailure ?? 'Resident vertices require an available WebGPU renderer',
        );
      } catch (error) {
        releaseRenderResources();
        setValidation('failed', error instanceof Error ? error.message : String(error));
        selectBackend('unavailable');
        return;
      }
    }
    watchResident(null);
    const clipVerts = mesh.transformedVertices();
    const halfW = width * 0.5;
    const halfH = height * 0.5;
    const count = mesh.vertexCount;
    if (!force2d && tryRenderWebGpu(clipVerts, mesh, indices)) return;

    if (screenCoords.length < count * 2) {
      screenCoords = new Float32Array(count * 2);
    }
    for (let i = 0; i < count; i++) {
      const x = clipVerts[i * 4] ?? 0;
      const y = clipVerts[i * 4 + 1] ?? 0;
      const w = clipVerts[i * 4 + 3] ?? 1;
      const invW = 1.0 / w;
      screenCoords[i * 2] = halfW + x * invW * halfW;
      screenCoords[i * 2 + 1] = halfH - y * invW * halfH;
    }

    const ctx = (surfaces?.fallback ?? canvas).getContext?.('2d');
    selectBackend(ctx ? 'canvas2d-zsort' : 'unavailable');
    if (!ctx) return;

    clearPickFrame();
    ctx.clearRect(0, 0, width, height);

    // Rust clips homogeneous primitives before JS transports finite screen points.
    const clipPrimitive = (primIndices: readonly number[]): Array<[number, number, number]> => {
      const i0 = primIndices[0],
        i1 = primIndices[1];
      if (i0 === undefined || i1 === undefined) return [];
      for (const index of primIndices) {
        if (!Number.isInteger(index) || index < 0 || index >= count) return [];
      }
      const size = renderCore.svet_gpu_clip_primitive_3d(
        mesh.transformedVertices.offset,
        count,
        i0,
        i1,
        primIndices[2] ?? 0,
        primIndices[3] ?? 0,
        primIndices.length,
      );
      if (size === 0) return [];
      // Clipping may grow WASM memory; construct the view after the call.
      const data = new Float32Array(
        renderCore.memory.buffer,
        renderCore.svet_gpu_clip_output_ptr() >>> 0,
        size * 4,
      );
      const points: Array<[number, number, number]> = [];
      for (let i = 0; i < size; i++) {
        const dx = data[i * 4] ?? 0;
        const dy = data[i * 4 + 1] ?? 0;
        const dz = data[i * 4 + 2] ?? 0;
        const w = data[i * 4 + 3] ?? 0;
        const px = halfW * (1 + dx / w);
        const py = halfH * (1 - dy / w);
        const pz = dz / w;
        if (!Number.isFinite(px) || !Number.isFinite(py) || !Number.isFinite(pz)) return [];
        points.push([px, py, pz]);
      }
      return points;
    };
    if (props.faces?.length) {
      const sortedFaces: Array<{ points: Array<[number, number, number]>; avgZ: number }> = [];
      for (const face of props.faces) {
        const points = clipPrimitive(face);
        if (points.length >= 3) {
          let sumZ = 0;
          for (const pt of points) sumZ += pt[2];
          sortedFaces.push({ points, avgZ: sumZ / points.length });
        }
      }
      sortedFaces.sort((a, b) => b.avgZ - a.avgZ);
      ctx.fillStyle = props.fillStyle ?? 'rgba(56, 189, 248, 0.14)';
      for (const face of sortedFaces) {
        const first = face.points[0];
        if (!first) continue;
        ctx.beginPath();
        ctx.moveTo(first[0], first[1]);
        for (let j = 1; j < face.points.length; j++) {
          const pt = face.points[j];
          if (pt) ctx.lineTo(pt[0], pt[1]);
        }
        ctx.closePath();
        ctx.fill();
      }
    }
    if (props.edges?.length) {
      ctx.strokeStyle = props.strokeStyle ?? '#38bdf8';
      ctx.lineWidth = 2;
      ctx.beginPath();
      for (const edge of props.edges) {
        const points = clipPrimitive(edge);
        const p0 = points[0],
          p1 = points[1];
        if (points.length !== 2 || !p0 || !p1) continue;
        ctx.moveTo(p0[0], p0[1]);
        ctx.lineTo(p1[0], p1[1]);
      }
      ctx.stroke();
    }

    if (props.showVertices !== false) {
      // Scratch allocation can grow WASM memory and detach the original view.
      const markerVertices = mesh.transformedVertices.view();
      ctx.fillStyle = props.pointColor ?? '#34d399';
      for (let i = 0; i < count; i++) {
        const base = i * 4;
        const x = markerVertices[base] ?? NaN,
          y = markerVertices[base + 1] ?? NaN,
          z = markerVertices[base + 2] ?? NaN,
          w = markerVertices[base + 3] ?? NaN;
        if (
          !Number.isFinite(x) ||
          !Number.isFinite(y) ||
          !Number.isFinite(z) ||
          !Number.isFinite(w) ||
          w <= 0 ||
          Math.abs(x) > w ||
          Math.abs(y) > w ||
          z < 0 ||
          z > w
        )
          continue;
        const px = screenCoords[i * 2] ?? 0;
        const py = screenCoords[i * 2 + 1] ?? 0;
        ctx.beginPath();
        ctx.arc(px, py, 3.5, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    capturePickFrame(mesh, mesh.transformedVertices.view(), indices, 'canvas2d-zsort');
  };
  const render = () => {
    if (!scheduledMode) {
      draw();
      return;
    }
    // Track the latest sources now; the deferred draw runs without graph tracking.
    const mesh = currentMesh();
    if (!currentResident()) mesh.transformedVertices();
    void mesh.indices;
    dimension(props.width, 360);
    dimension(props.height, 200);
    if (initialDraw) {
      initialDraw = false;
      draw();
      return;
    }
    dirty = true;
    scheduleLatest();
  };
  const stopRender = effect(render);
  const onDeviceChange = (reset: boolean) => {
    cancelFrameRequest();
    dirty = false;
    drainingFrame = null;
    releaseRenderResources();
    setValidation('inactive');
    if (reset) {
      validationTarget.settle = validationTarget.queue = () => {};
      rendererDisposed = true;
      selectBackend('unavailable');
      stopRender.dispose();
      renderState.renderers.delete(onDeviceChange);
      return;
    }
    try {
      withWasmCore(renderCore, () => untrack(() => draw()));
    } catch (error) {
      setValidation('failed', error instanceof Error ? error.message : String(error));
      selectBackend('unavailable');
    }
  };
  const disposeRenderer = () => {
    validationTarget.settle = validationTarget.queue = () => {};
    rendererDisposed = true;
    cancelFrameRequest();
    dirty = false;
    drainingFrame = null;
    renderState.renderers.delete(onDeviceChange);
    releaseRenderResources();
    setValidation('disposed');
  };
  if (isOwnerScopeDisposed(renderOwner) || renderGeneration !== getReactiveGeneration(renderCore)) {
    disposeRenderer();
    selectBackend('unavailable');
  } else {
    renderState.renderers.add(onDeviceChange);
    onCleanup(disposeRenderer);
  }

  return canvas;
}
