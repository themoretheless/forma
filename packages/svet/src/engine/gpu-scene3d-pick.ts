/**
 * Picking the vertex under a point of the viewport, live or against the frame last presented.
 */

import { getWasmCore, type SvetWasmExports } from './wasm-bridge.ts';
import type { GpuResidentTensor } from './gpu-resident-bridge.ts';
import type { Mesh3D } from './gpu-scene3d-model.ts';

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
export interface VertexPickSnapshot3D extends VertexPickFrame3D {
  vertices: Float32Array;
  resident?: GpuResidentTensor | undefined;
  indices?: Uint16Array | Uint32Array | undefined;
  vertexCount: number;
}
export function pickFrameSnapshot(
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
