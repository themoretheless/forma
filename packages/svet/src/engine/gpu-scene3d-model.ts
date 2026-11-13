/**
 * The reactive model of a 3D scene: transforms, perspective cameras and meshes, and who disposes
 * the resources each of them owns.
 */

import {
  batch,
  effect,
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  onCleanup,
  signal,
  triggerNodeWrite,
  type Signal,
} from './reactivity.ts';
import { getWasmCore } from './wasm-bridge.ts';
import {
  createGpuTensor,
  reviseTensor,
  snapshotGpuOptionMetadata,
  type GpuTensorSignal,
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
