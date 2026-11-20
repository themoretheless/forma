import {
  getReactiveGeneration,
  isOwnerScopeDisposed,
  type OwnerScope,
  untrack,
} from './reactivity.ts';
import { type SvetWasmExports, withWasmCore, withoutAsyncCoreContext } from './wasm-bridge.ts';
import {
  assertResidentRecord,
  type GpuResidentRecord,
  type GpuResidentTensor,
} from './gpu-resident-bridge.ts';
import type { WebGpuDeviceLike } from './gpu-types.ts';
import type { Mesh3D } from './gpu-scene3d-model.ts';
import type { CapturePickFrameInput } from './gpu-scene3d-controls.ts';

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

type RenderValidationTarget = {
  settle(frame: number, error: { message: string } | null): void;
  queue(frame: number, error: { message: string } | null): void;
};

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

function observeRenderValidation(
  results: Array<{ filter: string; result: PromiseLike<{ message: string } | null> }>,
  target: WeakRef<RenderValidationTarget>,
  frame: number,
): void {
  let remaining = results.length;
  let failed = false;
  const fail = (filter: string, message: string) => {
    if (failed) return;
    failed = true;
    target
      .deref()
      ?.settle(frame, { message: filter === 'validation' ? message : `${filter}: ${message}` });
  };
  withoutAsyncCoreContext(() => {
    for (const { filter, result } of results) {
      void Promise.resolve(result)
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
    void Promise.resolve(result)
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

export interface Scene3DWebGpuController {
  watchResident(record: GpuResidentRecord | null): void;
  tryRender(input: {
    clipVerts: Float32Array | null;
    mesh: Mesh3D;
    indices: Mesh3D['indices'];
    width: number;
    height: number;
    resident?: GpuResidentRecord | null;
    residentHandle?: GpuResidentTensor | undefined;
  }): boolean;
  getLastFailure(): string | undefined;
  releaseResources(): void;
  setValidation(status: string, message?: string): void;
  requestScheduledDraw(): void;
  resetForDeviceChange(): void;
  dispose(): void;
}

export function createScene3DWebGpuController(options: {
  canvas: HTMLCanvasElement;
  view: Window | null;
  surfaceElements: readonly HTMLElement[];
  preferWebGpu?: boolean | undefined;
  scheduledMode: boolean;
  renderCore: SvetWasmExports;
  renderOwner: OwnerScope | null;
  renderGeneration: number;
  getActiveDevice(): WebGpuDeviceLike | null;
  isDisposed(): boolean;
  selectBackend(backend: string): void;
  onClearPick(): void;
  onCapturePick(frame: CapturePickFrameInput): void;
  onDraw(force2d?: boolean): void;
}): Scene3DWebGpuController {
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
  let depthWidth = 0;
  let depthHeight = 0;
  let renderDevice: WebGpuDeviceLike | null = null;
  let renderEpoch = 0;
  let validationFloor = 0;
  let lastValidatedFrame = 0;
  let webgpuInitialized = false;
  let lastRenderFailure: string | undefined;

  let frameRequest: number | null = null;
  let dirty = false;
  let drainingFrame: number | null = null;
  let submission: { frame: number; queueDone: boolean; validationDone: boolean } | null = null;

  let selectedResident: GpuResidentRecord | null = null;
  let releaseResidentWatch = () => {};

  const setValidation = (status: string, message?: string) => {
    for (const surface of options.surfaceElements) {
      surface.dataset.renderValidation = status;
      if (message) surface.dataset.renderError = message;
      else delete surface.dataset.renderError;
    }
  };

  const cancelFrameRequest = () => {
    if (frameRequest !== null) options.view?.cancelAnimationFrame(frameRequest);
    frameRequest = null;
  };

  const scheduleLatest = () => {
    if (
      !options.scheduledMode ||
      options.isDisposed() ||
      !dirty ||
      submission ||
      drainingFrame !== null ||
      frameRequest !== null ||
      !options.view
    )
      return;
    frameRequest = options.view.requestAnimationFrame(() => {
      frameRequest = null;
      if (options.isDisposed() || submission || drainingFrame !== null || !dirty) return;
      dirty = false;
      try {
        withWasmCore(options.renderCore, () => untrack(() => options.onDraw()));
      } catch (error) {
        releaseResources();
        setValidation('failed', String(error));
        options.selectBackend('unavailable');
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
    releaseResources();
    setValidation('failed', message);
    try {
      withWasmCore(options.renderCore, () => untrack(() => options.onDraw(true)));
    } catch {
      options.selectBackend('unavailable');
    }
    scheduleLatest();
  };

  const validationTarget: RenderValidationTarget = {
    settle(frame, error) {
      if (options.isDisposed() || frame <= validationFloor) return;
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
      if (options.isDisposed()) return;
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

  const watchResident = (record: GpuResidentRecord | null) => {
    if (selectedResident === record) return;
    releaseResidentWatch();
    releaseResidentWatch = () => {};
    selectedResident = record;
    if (!record) return;
    const stop = () => {
      if (options.isDisposed()) return;
      // Preserve scheduled backpressure and resume the newest dirty source only
      // after the retired frame's queue completion has been observed.
      failFrame('Resident vertices disposed');
    };
    record.lifetime.signal.addEventListener('abort', stop, { once: true });
    releaseResidentWatch = () => record.lifetime.signal.removeEventListener('abort', stop);
  };

  const releaseResources = () => {
    releaseResidentWatch();
    releaseResidentWatch = () => {};
    selectedResident = null;
    validationFloor = ++renderEpoch;
    options.onClearPick();
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

  const tryRender = ({
    clipVerts,
    mesh,
    indices,
    width,
    height,
    resident = null,
    residentHandle,
  }: {
    clipVerts: Float32Array | null;
    mesh: Mesh3D;
    indices: Mesh3D['indices'];
    width: number;
    height: number;
    resident?: GpuResidentRecord | null;
    residentHandle?: GpuResidentTensor | undefined;
  }): boolean => {
    lastRenderFailure = undefined;
    const activeDevice = options.getActiveDevice();
    if (activeDevice !== renderDevice) releaseResources();
    if (options.preferWebGpu === false || !activeDevice || !activeDevice.createRenderPipeline) {
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
        const getCtx = (options.canvas as unknown as { getContext?(id: string): unknown })
          .getContext;
        const ctx = getCtx?.call(options.canvas, 'webgpu') as
          WebGpuCanvasContextHandle | null | undefined;
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
      if (options.scheduledMode && !activeDevice.queue.onSubmittedWorkDone)
        throw new Error('GPU queue completion is unavailable');
      if (options.scheduledMode)
        submission = { frame, queueDone: false, validationDone: scopes.length === 0 };
      if (resident) assertResidentRecord(resident);
      activeDevice.queue.submit([encoder.finish()]);
      if (resident) {
        assertResidentRecord(resident);
        if (
          isOwnerScopeDisposed(options.renderOwner) ||
          options.renderGeneration !== getReactiveGeneration(options.renderCore)
        )
          throw new Error('Resident renderer lifetime ended');
      }
      if (options.scheduledMode && activeDevice.queue.onSubmittedWorkDone)
        observeRenderQueue(activeDevice.queue.onSubmittedWorkDone(), validationReference, frame);
      options.selectBackend('webgpu-depth24plus');
      options.onCapturePick({
        mesh,
        vertices: clipVerts,
        indices,
        width,
        height,
        backend: 'webgpu-depth24plus',
        resident: residentHandle,
      });
      return true;
    } catch (error) {
      releaseResources();
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

  return {
    watchResident,
    tryRender,
    getLastFailure: () => lastRenderFailure,
    releaseResources,
    setValidation,
    requestScheduledDraw() {
      dirty = true;
      scheduleLatest();
    },
    resetForDeviceChange() {
      cancelFrameRequest();
      dirty = false;
      drainingFrame = null;
      releaseResources();
      setValidation('inactive');
    },
    dispose() {
      validationTarget.settle = validationTarget.queue = () => {};
      cancelFrameRequest();
      dirty = false;
      drainingFrame = null;
      releaseResources();
    },
  };
}
