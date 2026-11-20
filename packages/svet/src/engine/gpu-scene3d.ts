/**
 * # 3D GPU Scene Graph, Camera, Transform3D & Render Pipeline (`gpu-scene3d.ts`)
 *
 * Reactive 3D transforms, perspective cameras, indexed/wireframe meshes, Z-aware WASM
 * vertex picking, and dual WebGPU (`depth24plus`) + Canvas2D Z-sort viewports.
 */

import {
  effect,
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  onCleanup,
  untrack,
} from './reactivity.ts';
import { getWasmCore, withWasmCore } from './wasm-bridge.ts';
import { residentVertexRecord, type GpuResidentTensor } from './gpu-resident-bridge.ts';
import { gpuState } from './gpu-state.ts';

import type { Mesh3D, ReactiveTransform3D } from './gpu-scene3d-model.ts';
import type { VertexPickFrame3D } from './gpu-scene3d-pick.ts';
import { attachOrbitControls3D, createScene3DPickController } from './gpu-scene3d-controls.ts';
import { createScene3DWebGpuController } from './gpu-scene3d-webgpu.ts';
import { createScene3DCanvas2DRenderer } from './gpu-scene3d-canvas2d.ts';

export { DEFAULT_3D_WGSL_SHADER } from './gpu-scene3d-webgpu.ts';
export {
  createCubeMesh3D,
  createMesh3D,
  createPerspectiveCamera3D,
  createTransform3D,
  type Mesh3D,
  type PerspectiveCamera3D,
  type PerspectiveCamera3DOptions,
  type ReactiveTransform3D,
  type Vec3Tuple,
} from './gpu-scene3d-model.ts';
export { pickNearestVertex3D, type VertexPickFrame3D } from './gpu-scene3d-pick.ts';

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

function readCanvas3DDimension(
  value: number | (() => number) | undefined,
  fallback: number,
): number {
  const result = typeof value === 'function' ? value() : (value ?? fallback);
  if (!Number.isInteger(result) || result < 1 || result > 0xffffffff) {
    throw new RangeError('Canvas3D dimensions must be positive uint32 integers');
  }
  return result;
}

function createCanvas3D(
  props: Canvas3DProps,
  surfaces?: { viewport: HTMLDivElement; fallback: HTMLCanvasElement },
): HTMLCanvasElement {
  const renderOwner = getCurrentOwner();
  const renderGeneration = getReactiveGeneration();
  const currentMesh = () => (typeof props.mesh === 'function' ? props.mesh() : props.mesh);
  const currentResident = () =>
    typeof props.gpuVertices === 'function' ? props.gpuVertices() : (props.gpuVertices ?? null);
  let width = untrack(() => readCanvas3DDimension(props.width, 360));
  let height = untrack(() => readCanvas3DDimension(props.height, 200));
  if (
    props.renderMode !== undefined &&
    props.renderMode !== 'sync' &&
    props.renderMode !== 'animation-frame'
  ) {
    throw new RangeError('Invalid renderMode');
  }
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
  const surfaceElements: HTMLElement[] = surfaces ? [canvas, surfaces.viewport] : [canvas];
  if (surfaces) {
    surfaces.fallback.width = width;
    surfaces.fallback.height = height;
  }

  let rendererDisposed = false;
  const renderCore = getWasmCore();
  const renderState = gpuState(renderCore);
  const scheduledMode =
    props.renderMode === 'animation-frame' && Boolean(view?.requestAnimationFrame);
  let initialDraw = true;

  const pick = createScene3DPickController({
    interaction,
    surfaceElements,
    pickingCanvas: () => (surfaces && canvas.style.display === 'none' ? surfaces.fallback : canvas),
    renderCore,
    renderOwner,
    renderGeneration,
    isDisposed: () => rendererDisposed,
    pickOcclusion: props.pickOcclusion,
    onVertexPick: props.onVertexPick,
  });

  const selectBackend = (backend: string) => {
    if (backend === 'unavailable') pick.clearPickFrame();
    canvas.dataset.renderBackend = backend;
    if (surfaces) {
      surfaces.viewport.dataset.renderBackend = backend;
      canvas.style.display = backend === 'webgpu-depth24plus' ? 'block' : 'none';
      surfaces.fallback.style.display = backend === 'canvas2d-zsort' ? 'block' : 'none';
    }
  };

  if (props.orbitTransform) {
    attachOrbitControls3D(interaction, view, props.orbitTransform);
  }

  const webgpu = createScene3DWebGpuController({
    canvas,
    view,
    surfaceElements,
    preferWebGpu: props.preferWebGpu,
    scheduledMode,
    renderCore,
    renderOwner,
    renderGeneration,
    getActiveDevice: () => renderState.device,
    isDisposed: () => rendererDisposed,
    selectBackend,
    onClearPick: pick.clearPickFrame,
    onCapturePick: pick.capturePickFrame,
    onDraw: (force2d = false) => draw(force2d),
  });

  const drawCanvas2D = createScene3DCanvas2DRenderer({
    renderCore,
    faces: props.faces,
    edges: props.edges,
    fillStyle: props.fillStyle,
    strokeStyle: props.strokeStyle,
    pointColor: props.pointColor,
    showVertices: props.showVertices,
    getTargetCanvas: () => surfaces?.fallback ?? canvas,
    selectBackend,
    onClearPick: pick.clearPickFrame,
    onCapturePick: pick.capturePickFrame,
  });

  const draw = (force2d = false) => {
    const nextWidth = readCanvas3DDimension(props.width, 360);
    const nextHeight = readCanvas3DDimension(props.height, 200);
    if (nextWidth !== width || nextHeight !== height) {
      pick.clearPickFrame();
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
    const mesh = currentMesh();
    const residentHandle = currentResident();
    const indices = mesh.indices;
    if (residentHandle) {
      // A resident frame has no authoritative host replica for forced recovery.
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
        if (
          webgpu.tryRender({
            clipVerts: null,
            mesh,
            indices,
            width,
            height,
            resident,
            residentHandle,
          })
        ) {
          return;
        }
        throw new Error(
          webgpu.getLastFailure() ?? 'Resident vertices require an available WebGPU renderer',
        );
      } catch (error) {
        webgpu.releaseResources();
        webgpu.setValidation('failed', error instanceof Error ? error.message : String(error));
        selectBackend('unavailable');
        return;
      }
    }
    webgpu.watchResident(null);
    const clipVerts = mesh.transformedVertices();
    if (!force2d && webgpu.tryRender({ clipVerts, mesh, indices, width, height })) return;
    drawCanvas2D(mesh, clipVerts, width, height);
  };

  const stopRender = effect(() => {
    if (!scheduledMode) {
      draw();
      return;
    }
    const mesh = currentMesh();
    if (!currentResident()) mesh.transformedVertices();
    void mesh.indices;
    readCanvas3DDimension(props.width, 360);
    readCanvas3DDimension(props.height, 200);
    if (initialDraw) {
      initialDraw = false;
      draw();
      return;
    }
    webgpu.requestScheduledDraw();
  });

  const onDeviceChange = (reset: boolean) => {
    webgpu.resetForDeviceChange();
    if (reset) {
      rendererDisposed = true;
      webgpu.dispose();
      selectBackend('unavailable');
      stopRender.dispose();
      renderState.renderers.delete(onDeviceChange);
      return;
    }
    try {
      withWasmCore(renderCore, () => untrack(() => draw()));
    } catch (error) {
      webgpu.setValidation('failed', error instanceof Error ? error.message : String(error));
      selectBackend('unavailable');
    }
  };

  const disposeRenderer = () => {
    rendererDisposed = true;
    renderState.renderers.delete(onDeviceChange);
    webgpu.dispose();
    pick.disposePickBuffers();
    webgpu.setValidation('disposed');
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
