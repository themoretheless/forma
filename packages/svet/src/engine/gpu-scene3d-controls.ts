import {
  getReactiveGeneration,
  isOwnerScopeDisposed,
  onCleanup,
  type OwnerScope,
  runUntrackedInCore,
  runWithCapturedOwner,
} from './reactivity.ts';
import { withWasmCore, type SvetWasmExports } from './wasm-bridge.ts';
import { snapshotResidentVertices, type GpuResidentTensor } from './gpu-resident-bridge.ts';
import type { Mesh3D, ReactiveTransform3D } from './gpu-scene3d-model.ts';
import {
  pickFrameSnapshot,
  type VertexPickFrame3D,
  type VertexPickSnapshot3D,
} from './gpu-scene3d-pick.ts';

export function attachOrbitControls3D(
  interaction: HTMLElement,
  view: Window | null,
  transform: ReactiveTransform3D,
): void {
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
    const [rx, ry, rz] = transform.rotation.peek();
    transform.rotation.set([rx + dy * 0.012, ry + dx * 0.012, rz]);
  };
  const onUp = () => {
    dragging = false;
    interaction.style.cursor = 'grab';
  };
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.08 : 0.92;
    const [sx, sy, sz] = transform.scale.peek();
    const clamped = Math.max(0.2, Math.min(5.0, sx * factor));
    transform.scale.set([
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

export type VertexPickCallback = (
  vertexIndex: number,
  distancePx: number,
  ndcZ: number,
  frame: VertexPickFrame3D,
) => void;

export interface CapturePickFrameInput {
  mesh: Mesh3D;
  vertices: Float32Array | null;
  indices: Mesh3D['indices'];
  width: number;
  height: number;
  backend: VertexPickFrame3D['backend'];
  resident?: GpuResidentTensor | undefined;
}

export interface Scene3DPickController {
  clearPickFrame(): void;
  capturePickFrame(input: CapturePickFrameInput): void;
  disposePickBuffers(): void;
}

export function createScene3DPickController(options: {
  interaction: HTMLElement;
  surfaceElements: readonly HTMLElement[];
  pickingCanvas(): HTMLCanvasElement;
  renderCore: SvetWasmExports;
  renderOwner: OwnerScope | null;
  renderGeneration: number;
  isDisposed(): boolean;
  pickOcclusion?: 'indexed' | 'none' | undefined;
  onVertexPick?: VertexPickCallback | undefined;
}): Scene3DPickController {
  let pickSnapshot: VertexPickSnapshot3D | null = null;
  let pendingPick: AbortController | null = null;
  let pickSequence = 0;
  let pickVertexBacking = new Float32Array(0);
  let pickIndexBacking = new Uint32Array(0);

  const clearPickFrame = () => {
    if (pendingPick) {
      pendingPick.abort(new Error('GPU pick frame retired'));
      for (const surface of options.surfaceElements) surface.dataset.pickReadback = 'cancelled';
    }
    pendingPick = null;
    pickSnapshot = null;
  };

  const capturePickFrame = ({
    mesh,
    vertices,
    indices,
    width,
    height,
    backend,
    resident,
  }: CapturePickFrameInput) => {
    clearPickFrame();
    if (!options.onVertexPick) return;
    let verticesView: Float32Array;
    if (vertices && vertices.length > 0) {
      if (pickVertexBacking.length < vertices.length) {
        pickVertexBacking = new Float32Array(vertices.length);
      }
      pickVertexBacking.set(vertices);
      verticesView = pickVertexBacking.subarray(0, vertices.length);
    } else {
      verticesView = pickVertexBacking.subarray(0, 0);
    }
    let indicesView: Uint32Array | undefined;
    if (indices && indices.length > 0) {
      if (pickIndexBacking.length < indices.length) {
        pickIndexBacking = new Uint32Array(indices.length);
      }
      pickIndexBacking.set(indices);
      indicesView = pickIndexBacking.subarray(0, indices.length);
    } else if (indices) {
      indicesView = pickIndexBacking.subarray(0, 0);
    }
    pickSnapshot = {
      mesh,
      id: ++pickSequence,
      width,
      height,
      backend,
      vertexCount: mesh.vertexCount,
      vertices: verticesView,
      resident,
      indices: indicesView,
    };
  };

  if (options.onVertexPick) {
    const onClickPick = (e: MouseEvent) => {
      const frame = pickSnapshot;
      if (!frame) return;
      const targetCanvas = options.pickingCanvas();
      const rect = targetCanvas.getBoundingClientRect();
      const clickX = (e.clientX - rect.left) * (rect.width ? frame.width / rect.width : 1);
      const clickY = (e.clientY - rect.top) * (rect.height ? frame.height / rect.height : 1);
      const occlusion =
        options.pickOcclusion ?? (frame.backend === 'webgpu-depth24plus' ? 'indexed' : 'none');

      const publishPick = (vertices: Float32Array) => {
        if (
          options.isDisposed() ||
          pickSnapshot !== frame ||
          options.renderGeneration !== getReactiveGeneration(options.renderCore) ||
          isOwnerScopeDisposed(options.renderOwner)
        )
          return;
        const hit = withWasmCore(options.renderCore, () =>
          pickFrameSnapshot(
            options.renderCore,
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
          runUntrackedInCore(options.renderCore, () =>
            runWithCapturedOwner(options.renderCore, options.renderOwner, () => {
              const callback = options.onVertexPick;
              if (
                !options.isDisposed() &&
                pickSnapshot === frame &&
                !isOwnerScopeDisposed(options.renderOwner)
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
      for (const surface of options.surfaceElements) {
        surface.dataset.pickReadback = 'pending';
        delete surface.dataset.pickError;
      }
      const request = runUntrackedInCore(options.renderCore, () =>
        runWithCapturedOwner(options.renderCore, options.renderOwner, () =>
          snapshotResidentVertices(residentFrame, cancellation.signal),
        ),
      );
      void request
        .then(vertices => {
          if (pendingPick !== cancellation || cancellation.signal.aborted) return;
          publishPick(vertices);
          if (pendingPick !== cancellation || cancellation.signal.aborted || options.isDisposed())
            return;
          for (const surface of options.surfaceElements) surface.dataset.pickReadback = 'passed';
        })
        .catch((error: unknown) => {
          if (pendingPick !== cancellation || cancellation.signal.aborted || options.isDisposed())
            return;
          for (const surface of options.surfaceElements) {
            surface.dataset.pickReadback = 'failed';
            surface.dataset.pickError = String(error);
          }
        })
        .finally(() => {
          if (pendingPick === cancellation) pendingPick = null;
        });
    };
    options.interaction.addEventListener('click', onClickPick);
    onCleanup(() => options.interaction.removeEventListener('click', onClickPick));
  }

  return {
    clearPickFrame,
    capturePickFrame,
    disposePickBuffers() {
      pickVertexBacking = new Float32Array(0);
      pickIndexBacking = new Uint32Array(0);
    },
  };
}
