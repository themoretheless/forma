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
  runUntrackedInCore,
  runWithCapturedOwner,
  untrack,
} from './reactivity.ts';
import { getWasmCore, withWasmCore, withoutAsyncCoreContext } from './wasm-bridge.ts';
import {
  assertResidentRecord,
  residentVertexRecord,
  snapshotResidentVertices,
  type GpuResidentRecord,
  type GpuResidentTensor,
} from './gpu-resident-bridge.ts';
import { gpuState, type WebGpuDeviceLike } from './gpu.ts';

import type { Mesh3D, ReactiveTransform3D } from './gpu-scene3d-model.ts';
import {
  pickFrameSnapshot,
  type VertexPickFrame3D,
  type VertexPickSnapshot3D,
} from './gpu-scene3d-pick.ts';

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
