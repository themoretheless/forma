/** @vitest-environment jsdom */
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  Canvas3D,
  Viewport3D,
  configureGpuSubsystem,
  createGpuTensor,
  createMesh3D,
  createRoot,
  createTransform3D,
  createWasmCoreInstance,
  withWasmCore,
  dispatchGpuComputeResident,
  getCurrentOwner,
  getGraphTelemetry,
  getWasmCore,
  readGpuResidentSnapshot,
  resetReactiveSystem,
  signal,
  type GpuResidentTensor,
} from '../src/engine';
import { gpuState } from '../src/engine/gpu';
import { device } from './helpers/resident-device';
const wgsl = 'identity compute mock; native fixture supplies real WGSL';
const triangle = [0, 0, 0.4, 1, -0.8, -0.6, 0.4, 1, 0.8, -0.6, 0.4, 1];
const contexts: any[] = [],
  fallbackDraw = vi.fn();
beforeEach(() => {
  resetReactiveSystem();
  configureGpuSubsystem({ device: null });
  contexts.length = 0;
  fallbackDraw.mockClear();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (
    this: HTMLCanvasElement,
    kind: string,
  ): any {
    if (kind === 'webgpu') {
      const ctx = {
        configure: vi.fn(),
        unconfigure: vi.fn(),
        getCurrentTexture: () => ({ createView: () => ({}) }),
      };
      contexts.push(ctx);
      return ctx;
    }
    return {
      clearRect: fallbackDraw,
      beginPath() {},
      arc() {},
      fill() {},
      moveTo() {},
      lineTo() {},
      stroke() {},
    };
  } as any);
});
afterEach(() => vi.restoreAllMocks());
function scene(count = 3) {
  const transform = createTransform3D();
  const positions = Array.from({ length: count }, (_, i) => [0.8 + i * 0.01, 0.8, 0.5]).flat();
  const mesh = createMesh3D({ positions, indices: [0, 1, 2], transform });
  return {
    mesh,
    dispose() {
      mesh.dispose();
      transform.dispose();
    },
  };
}
async function result(gpu: ReturnType<typeof device>, values = triangle) {
  gpu.compute = (inputs, output) => output.set(inputs[0]);
  const source = createGpuTensor(values);
  try {
    return await dispatchGpuComputeResident({
      wgsl,
      inputs: [source],
      outputLength: values.length,
    });
  } finally {
    source.dispose();
  }
}
async function drain() {
  for (let i = 0; i < 80; i++) await Promise.resolve();
}
function click(node: HTMLElement) {
  node.dispatchEvent(new MouseEvent('click', { clientX: 100, clientY: 100, bubbles: true }));
}

it('binds resident vertex storage directly without host uploads, maps or transformed-vertex reads', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const handle = await result(gpu),
    s = scene();
  const hostReads = vi.spyOn(s.mesh, 'transformedVertices'),
    writes = vi.spyOn(gpu.queue, 'writeBuffer');
  let stop!: () => void;
  const canvas = createRoot(dispose => {
    stop = dispose;
    return Canvas3D({ mesh: s.mesh, gpuVertices: handle });
  });
  expect(canvas.dataset.renderBackend).toBe('webgpu-depth24plus');
  expect(hostReads).not.toHaveBeenCalled();
  expect(gpu.renderDraws[0].values).toEqual(triangle.map(Math.fround));
  expect(gpu.renderDraws[0].buffer.usage & 0x80).toBeTruthy();
  expect(writes.mock.calls.map(args => args[4])).toEqual([8]); // Padded uint16 topology only.
  expect(gpu.buffers.filter(buffer => buffer.usage & 0x20 && !(buffer.usage & 0x80))).toHaveLength(
    0,
  );
  expect(gpu.buffers.reduce((sum, buffer) => sum + buffer.maps, 0)).toBe(0);
  stop();
  expect(handle.active).toBe(true);
  expect(gpu.renderDraws[0].buffer.destroyed).toBe(false);
  handle.dispose();
  s.dispose();
});

it('reads GPU vertex data only on click and reports the actual GPU frame with its owner', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const handle = await result(gpu),
    s = scene();
  const pick = vi.fn(),
    nodes = getGraphTelemetry().nodes;
  let stop!: () => void, owner: unknown;
  const viewport = createRoot(dispose => {
    stop = dispose;
    owner = getCurrentOwner();
    return Viewport3D({
      mesh: s.mesh,
      gpuVertices: handle,
      width: 200,
      height: 200,
      onVertexPick: (...args) => {
        expect(getCurrentOwner()).toBe(owner);
        pick(...args);
      },
    });
  });
  expect(gpu.buffers.reduce((sum, buffer) => sum + buffer.maps, 0)).toBe(0);
  click(viewport);
  await drain();
  expect(viewport.dataset.pickReadback).toBe('passed');
  expect(pick.mock.calls[0].slice(0, 3)).toEqual([0, 0, expect.closeTo(0.4)]);
  expect(pick.mock.calls[0][3]).toMatchObject({
    mesh: s.mesh,
    width: 200,
    height: 200,
    backend: 'webgpu-depth24plus',
  });
  expect(pick.mock.calls[0][3].gpuVertices).toBe(handle);
  expect(Object.keys(pick.mock.calls[0][3]).sort()).toEqual([
    'backend',
    'gpuVertices',
    'height',
    'id',
    'mesh',
    'width',
  ]);
  expect(gpu.buffers.reduce((sum, buffer) => sum + buffer.maps, 0)).toBe(1);
  stop();
  expect(getGraphTelemetry().nodes).toBe(nodes);
  handle.dispose();
  s.dispose();
});

it('keeps indexed occlusion picking on GPU positions and supports explicit through-surface picking', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const handle = await result(
      gpu,
      [-0.8, -0.8, 0.2, 1, 0.8, -0.8, 0.2, 1, 0, 0.8, 0.2, 1, 0, 0, 0.8, 1],
    ),
    s = scene(4);
  const visible = vi.fn(),
    through = vi.fn();
  let stop!: () => void;
  const views = createRoot(dispose => {
    stop = dispose;
    return [
      Viewport3D({
        mesh: s.mesh,
        gpuVertices: handle,
        width: 200,
        height: 200,
        onVertexPick: visible,
      }),
      Viewport3D({
        mesh: s.mesh,
        gpuVertices: handle,
        width: 200,
        height: 200,
        pickOcclusion: 'none',
        onVertexPick: through,
      }),
    ];
  });
  click(views[0]);
  await drain();
  expect(visible).not.toHaveBeenCalled();
  click(views[1]);
  await drain();
  expect(through.mock.calls[0][0]).toBe(3);
  stop();
  handle.dispose();
  s.dispose();
});

it('cancels a pending pick when a new resident frame replaces it', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const a = await result(gpu),
    b = await result(gpu),
    s = scene();
  const selected = signal<GpuResidentTensor | null>(a),
    pick = vi.fn();
  let stop!: () => void;
  const view = createRoot(dispose => {
    stop = dispose;
    return Viewport3D({
      mesh: s.mesh,
      gpuVertices: selected,
      width: 200,
      height: 200,
      onVertexPick: pick,
    });
  });
  gpu.pauseMap = true;
  click(view);
  await gpu.mapping.promise;
  selected.set(b);
  await drain();
  expect(view.dataset.pickReadback).toBe('cancelled');
  expect(pick).not.toHaveBeenCalled();
  expect(gpu.buffers.filter(buffer => buffer.maps).every(buffer => buffer.destroyed)).toBe(true);
  gpu.mapped.resolve();
  await drain();
  expect(pick).not.toHaveBeenCalled();
  stop();
  selected.dispose();
  a.dispose();
  b.dispose();
  s.dispose();
});

it('cancels pending pick on renderer teardown without disposing a caller-owned result', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const handle = await result(gpu),
    s = scene(),
    pick = vi.fn();
  let stop!: () => void;
  const view = createRoot(dispose => {
    stop = dispose;
    return Viewport3D({
      mesh: s.mesh,
      gpuVertices: handle,
      width: 200,
      height: 200,
      onVertexPick: pick,
    });
  });
  gpu.pauseMap = true;
  click(view);
  await gpu.mapping.promise;
  stop();
  await drain();
  expect(pick).not.toHaveBeenCalled();
  expect(handle.active).toBe(true);
  expect(gpu.buffers.filter(buffer => buffer.maps).every(buffer => buffer.destroyed)).toBe(true);
  gpu.mapped.resolve();
  handle.dispose();
  s.dispose();
});

it('source disposal invalidates rendering and picking without substituting CPU geometry', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const handle = await result(gpu),
    s = scene(),
    pick = vi.fn();
  let stop!: () => void;
  const view = createRoot(dispose => {
    stop = dispose;
    return Viewport3D({ mesh: s.mesh, gpuVertices: handle, onVertexPick: pick });
  });
  handle.dispose();
  expect(view.dataset.renderBackend).toBe('unavailable');
  expect(fallbackDraw).not.toHaveBeenCalled();
  click(view);
  await drain();
  expect(pick).not.toHaveBeenCalled();
  stop();
  s.dispose();
});

it.each(['unsupported', 'pipeline', 'length'] as const)(
  'refuses %s resident drawing without a stale 2D substitute',
  async mode => {
    const gpu = device();
    configureGpuSubsystem({ device: gpu });
    const handle = await result(gpu, mode === 'length' ? triangle.slice(0, 4) : triangle),
      s = scene();
    let stop!: () => void;
    if (mode === 'pipeline') gpu.renderFailure = true;
    const view = createRoot(dispose => {
      stop = dispose;
      return Viewport3D({
        mesh: s.mesh,
        gpuVertices: handle,
        preferWebGpu: mode !== 'unsupported',
      });
    });
    expect(view.dataset.renderBackend).toBe('unavailable');
    expect(fallbackDraw).not.toHaveBeenCalled();
    if (mode === 'pipeline') expect(view.dataset.renderError).toContain('render failed');
    stop();
    handle.dispose();
    s.dispose();
  },
);

it('switches explicitly between GPU-only vertices and the normal host renderer', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const handle = await result(gpu),
    s = scene(),
    selected = signal<GpuResidentTensor | null>(handle);
  let stop!: () => void;
  createRoot(dispose => {
    stop = dispose;
    Viewport3D({ mesh: s.mesh, gpuVertices: selected });
  });
  selected.set(null);
  expect(gpu.renderDraws.at(-1)!.values).not.toEqual(triangle.map(Math.fround));
  const hostBuffer = gpu.renderDraws.at(-1)!.buffer;
  selected.set(handle);
  expect(hostBuffer.destroyed).toBe(true);
  expect(gpu.renderDraws.at(-1)!.values).toEqual(triangle.map(Math.fround));
  stop();
  handle.dispose();
  selected.dispose();
  s.dispose();
});

it('an initial-submit teardown releases the renderer registration and allocations', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const handle = await result(gpu),
    s = scene();
  let view!: HTMLDivElement;
  createRoot(dispose => {
    gpu.onSubmit = dispose;
    view = Viewport3D({ mesh: s.mesh, gpuVertices: handle });
  });
  gpu.onSubmit = undefined;
  expect(gpuState(getWasmCore()).renderers.size).toBe(0);
  expect(view.dataset.renderBackend).toBe('unavailable');
  expect(gpu.textures.every(texture => texture.destroyed)).toBe(true);
  expect(handle.active).toBe(true);
  handle.dispose();
  s.dispose();
});

it('raw snapshots are private CPU copies and allocate no WASM output nodes', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const handle = await result(gpu),
    nodes = getGraphTelemetry().nodes;
  const first = await readGpuResidentSnapshot(handle);
  first[0] = 99;
  const second = await readGpuResidentSnapshot(handle);
  expect(Array.from(second)).toEqual(triangle.map(Math.fround));
  expect(getGraphTelemetry().nodes).toBe(nodes);
  handle.dispose();
});

it('rejects a resident source from another core without a CPU substitute', async () => {
  const gpu = device(),
    foreign = createWasmCoreInstance();
  const handle = await withWasmCore(foreign, () => {
    configureGpuSubsystem({ device: gpu });
    return result(gpu);
  });
  configureGpuSubsystem({ device: gpu });
  const s = scene();
  let stop!: () => void;
  const view = createRoot(dispose => {
    stop = dispose;
    return Viewport3D({ mesh: s.mesh, gpuVertices: handle });
  });
  expect(view.dataset.renderBackend).toBe('unavailable');
  expect(view.dataset.renderError).toContain('another context');
  expect(fallbackDraw).not.toHaveBeenCalled();
  stop();
  handle.dispose();
  s.dispose();
});

it('retiring a scheduled source drains its submitted frame before drawing the latest result', async () => {
  const gpu = device();
  configureGpuSubsystem({ device: gpu });
  const first = await result(gpu),
    second = await result(gpu),
    s = scene();
  let finish!: () => void;
  const completion = new Promise<void>(resolve => {
    finish = resolve;
  });
  vi.spyOn(gpu.queue, 'onSubmittedWorkDone')
    .mockImplementationOnce(() => completion)
    .mockResolvedValue(undefined);
  const callbacks = new Map<number, FrameRequestCallback>();
  let id = 0;
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => {
    callbacks.set(++id, callback);
    return id;
  });
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(handle => {
    callbacks.delete(handle);
  });
  const selected = signal<GpuResidentTensor | null>(first);
  let stop!: () => void;
  const view = createRoot(dispose => {
    stop = dispose;
    return Viewport3D({ mesh: s.mesh, gpuVertices: selected, renderMode: 'animation-frame' });
  });
  selected.set(second);
  first.dispose();
  await drain();
  expect(callbacks.size).toBe(0);
  expect(gpu.renderDraws).toHaveLength(1);
  expect(view.dataset.renderBackend).toBe('unavailable');
  finish();
  await drain();
  expect(callbacks.size).toBe(1);
  const pending = Array.from(callbacks.values());
  callbacks.clear();
  pending.forEach(callback => callback(0));
  await drain();
  expect(gpu.renderDraws).toHaveLength(2);
  expect(view.dataset.renderBackend).toBe('webgpu-depth24plus');
  stop();
  second.dispose();
  selected.dispose();
  s.dispose();
});
