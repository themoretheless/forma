/** @vitest-environment jsdom */
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Canvas3D, Viewport3D, compileFormaComponent, validateFormaSource, configureGpuSubsystem, createMesh3D, createRoot, createTransform3D, resetReactiveSystem, signal, batch, getGraphTelemetry, pickNearestVertex3D, runWithIsolatedContext, effect, createWasmCoreInstance, withWasmCore, getWasmCore, onCleanup, getGpuTelemetry } from '../src/engine';

function renderDevice(options: { failTexture?: boolean; failIndexWrite?: boolean; failView?: boolean; failVertexAllocation?: boolean } = {}) {
  const buffers: any[] = [], textures: any[] = [], uploads: Uint8Array[] = [], pipelines: any[] = [], indexFormats: string[] = [], indexedCounts: number[] = [], plainCounts: number[] = [];
  let draws = 0;
  const device = {
    buffers, textures, uploads, pipelines, indexFormats, indexedCounts, plainCounts, get draws() { return draws; },
    createShaderModule: () => ({}), createRenderPipeline: (descriptor: any) => { pipelines.push(descriptor); return {}; },
    createBuffer({ size, usage }: { size: number; usage: number }) {
      if (options.failVertexAllocation && (usage & 0x20)) throw new Error('vertex allocation failed');
      const buffer = { usage, bytes: new Uint8Array(size), destroyed: false, destroy() { this.destroyed = true; } };
      buffers.push(buffer); return buffer;
    },
    createTexture(descriptor: any) {
      if (options.failTexture) throw new Error('texture failed');
      const texture = { descriptor, destroyed: false, createView: () => { if (options.failView) throw new Error('view failed'); return {}; }, destroy() { this.destroyed = true; } };
      textures.push(texture); return texture;
    },
    queue: { writeBuffer(buffer: any, offset: number, data: ArrayBuffer, dataOffset = 0, size = 0) {
      if (options.failIndexWrite && (buffer.usage & 0x10)) throw new Error('index upload failed');
      if (!buffers.includes(buffer) || buffer.destroyed || size % 4 || offset + size > buffer.bytes.length) throw new Error('invalid render upload');
      const bytes = new Uint8Array(data, dataOffset, size); buffer.bytes.set(bytes, offset); uploads.push(bytes.slice());
    }, submit() {} },
    createCommandEncoder: () => ({ beginRenderPass: () => ({
      setPipeline() {}, setVertexBuffer(_slot: number, buffer: any) { if (!buffers.includes(buffer)) throw new Error('foreign vertex buffer'); },
      setIndexBuffer(_buffer: any, type: string) { indexFormats.push(type); }, draw(count: number) { draws++; plainCounts.push(count); }, drawIndexed(count: number) { draws++; indexedCounts.push(count); }, end() {},
    }), finish: () => ({}) }),
  };
  return device;
}
function scene() {
  const transform = createTransform3D();
  const mesh = createMesh3D({ positions: [0, 0, 0, 1, 0, 0, 0, 1, 0], indices: [0, 1, 2], transform });
  return { transform, mesh, dispose() { mesh.dispose(); transform.dispose(); } };
}
const contexts: Array<{ configure: ReturnType<typeof vi.fn>; unconfigure: ReturnType<typeof vi.fn> }> = [];
beforeEach(() => {
  resetReactiveSystem(); configureGpuSubsystem({ device: null }); contexts.length = 0;
  const bound = new WeakMap<HTMLCanvasElement, { kind: string; ctx: any }>();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function(this: HTMLCanvasElement, kind: string): any {
    const existing = bound.get(this); if (existing) return existing.kind === kind ? existing.ctx : null;
    const ctx = kind === 'webgpu' ? { configure: vi.fn(), unconfigure: vi.fn(), getCurrentTexture: () => ({ createView: () => ({}) }) }
      : { clearRect() {}, beginPath() {}, arc: vi.fn(), fill() {}, moveTo: vi.fn(), lineTo: vi.fn(), stroke() {}, closePath() {} };
    if (ctx.configure && ctx.unconfigure) contexts.push({ configure: ctx.configure, unconfigure: ctx.unconfigure });
    bound.set(this, { kind, ctx }); return ctx;
  } as any);
});
afterEach(() => vi.restoreAllMocks());

it('releases vertex/index buffers, depth texture and canvas configuration on owner teardown', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh }); }); stop();
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(device.textures.every(texture => texture.destroyed)).toBe(true);
  expect(contexts[0].unconfigure).toHaveBeenCalledOnce(); s.dispose();
});

it('pads a three-index uint16 upload to a four-byte boundary', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh: s.mesh }); });
  expect(device.draws).toBe(1); expect(canvas.dataset.renderBackend).toBe('webgpu-depth24plus');
  expect(Array.from(device.uploads[0])).toEqual([0, 0, 1, 0, 2, 0, 0, 0]); stop(); s.dispose();
});

it('device replacement frees old resources and renders with the new device without a mesh mutation', () => {
  const first = renderDevice(), second = renderDevice(); configureGpuSubsystem({ device: first }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh }); }); configureGpuSubsystem({ device: second });
  expect(first.buffers.every(buffer => buffer.destroyed)).toBe(true); expect(first.textures.every(texture => texture.destroyed)).toBe(true);
  expect(second.draws).toBe(1); expect(contexts[0].configure).toHaveBeenLastCalledWith(expect.objectContaining({ device: second }));
  stop(); s.dispose();
});

it('partial initialization failure destroys already-created resources', () => {
  const device = renderDevice({ failTexture: true }); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh }); });
  expect(device.buffers.length).toBeGreaterThan(0); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(contexts[0].unconfigure).toHaveBeenCalled(); stop(); s.dispose();
});

it('does not retain GPU allocations after device detachment', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh: s.mesh }); });
  configureGpuSubsystem({ device: null });
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(canvas.dataset.renderBackend).toBe('unavailable'); stop(); s.dispose();
});

it('device loss releases resources and a replacement can render again', async () => {
  let lose!: (info: { message: string }) => void;
  const first = Object.assign(renderDevice(), { lost: new Promise<{ message: string }>(resolve => { lose = resolve; }) });
  configureGpuSubsystem({ device: first }); const s = scene(); let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh: s.mesh }); });
  lose({ message: 'test loss' }); await Promise.resolve(); await Promise.resolve();
  expect(first.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(first.textures.every(texture => texture.destroyed)).toBe(true);
  expect(canvas.dataset.renderBackend).toBe('unavailable');
  const second = renderDevice(); configureGpuSubsystem({ device: second });
  expect(second.draws).toBe(1); expect(canvas.dataset.renderBackend).toBe('webgpu-depth24plus');
  stop(); s.dispose();
});

it('reset releases renderer resources and does not resurrect old meshes on reconfiguration', () => {
  const first = renderDevice(); configureGpuSubsystem({ device: first }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh }); });
  resetReactiveSystem();
  expect(first.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(first.textures.every(texture => texture.destroyed)).toBe(true);
  const second = renderDevice(); configureGpuSubsystem({ device: second });
  expect(second.draws).toBe(0); stop();
});

it('initialization can be retried after a transient texture failure', () => {
  const options = { failTexture: true }; const device = renderDevice(options);
  configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh: s.mesh }); });
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  options.failTexture = false;
  s.transform.rotation.set([0.2, 0, 0]);
  expect(device.draws).toBe(1); expect(canvas.dataset.renderBackend).toBe('webgpu-depth24plus');
  stop(); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); s.dispose();
});


it('adaptive viewport starts in 2D, upgrades to GPU and recovers 2D after detachment', () => {
  const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  const [primary, fallback] = Array.from(viewport.children) as HTMLCanvasElement[];
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort');
  expect(primary.style.display).toBe('none'); expect(fallback.style.display).toBe('block');
  const device = renderDevice(); configureGpuSubsystem({ device });
  expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus'); expect(device.draws).toBe(1);
  expect(primary.style.display).toBe('block'); expect(fallback.style.display).toBe('none');
  configureGpuSubsystem({ device: null });
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort');
  expect(primary.style.display).toBe('none'); expect(fallback.style.display).toBe('block');
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  const replacement = renderDevice(); configureGpuSubsystem({ device: replacement });
  expect(replacement.draws).toBe(1); expect(viewport.children[0]).toBe(primary);
  stop(); s.dispose();
});

it('adaptive viewport retains the 2D surface after GPU initialization fails', () => {
  const device = renderDevice({ failTexture: true }); configureGpuSubsystem({ device });
  const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort');
  expect((viewport.children[1] as HTMLElement).style.display).toBe('block');
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); stop(); s.dispose();
});


it('orbit controls work through the fallback canvas and stop after owner disposal', () => {
  const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, orbitTransform: s.transform }); });
  const fallback = viewport.children[1];
  fallback.dispatchEvent(new WheelEvent('wheel', { deltaY: -1, bubbles: true, cancelable: true }));
  expect(s.transform.scale.peek()[0]).toBeCloseTo(1.08);
  const device = renderDevice(); configureGpuSubsystem({ device });
  viewport.children[0].dispatchEvent(new MouseEvent('mousedown', { clientX: 10, clientY: 10, bubbles: true }));
  window.dispatchEvent(new MouseEvent('mousemove', { clientX: 20, clientY: 20 }));
  window.dispatchEvent(new MouseEvent('mouseup'));
  expect(s.transform.rotation.peek()[0]).toBeCloseTo(0.12);
  stop();
  fallback.dispatchEvent(new WheelEvent('wheel', { deltaY: -1, bubbles: true }));
  expect(s.transform.scale.peek()[0]).toBeCloseTo(1.08); s.dispose();
});

it('Forma exposes adaptive Viewport3D while preserving legacy Scene3D canvas nodes', () => {
  const source = `component Adaptive { Column { Viewport3D { key: 'adaptive'; width: 240; height: 140; } Scene3D { key: 'legacy'; } } }`;
  expect(validateFormaSource(source).has3DViewport).toBe(true);
  const instance = compileFormaComponent(source);
  const mounted = instance.mount(document.createElement('div'));
  const adaptive = instance.getElementByKey('adaptive')!;
  expect(adaptive.tagName.toLowerCase()).toBe('div'); expect(adaptive.children).toHaveLength(2);
  expect(adaptive.dataset.renderBackend).toBe('canvas2d-zsort');
  expect(instance.getElementByKey('legacy')!.tagName.toLowerCase()).toBe('canvas');
  mounted.unmount(); instance.dispose();
});


it('reactive resize updates both surfaces and replaces only the depth attachment', () => {
  const device = renderDevice(); configureGpuSubsystem({ device });
  const s = scene(), width = signal(240), height = signal(140); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, width, height }); });
  const originalBuffers = device.buffers.slice(), originalTextures = device.textures.slice();
  batch(() => { width.set(480); height.set(280); });
  for (const canvas of Array.from(viewport.children) as HTMLCanvasElement[]) {
    expect([canvas.width, canvas.height]).toEqual([480, 280]);
    expect([canvas.style.width, canvas.style.height]).toEqual(['480px', '280px']);
  }
  expect(viewport.style.width).toBe('482px'); expect(device.draws).toBe(2);
  expect(originalBuffers.every(buffer => !buffer.destroyed)).toBe(true);
  expect(device.buffers).toEqual(originalBuffers); expect(device.pipelines).toHaveLength(1);
  expect(contexts[0].configure).toHaveBeenCalledOnce();
  expect(originalTextures.every(texture => texture.destroyed)).toBe(true);
  expect(device.textures.at(-1).descriptor.size).toEqual([480, 280, 1]);
  configureGpuSubsystem({ device: null }); expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort');
  stop(); width.dispose(); height.dispose(); s.dispose();
});

it('unchanged dimensions do not recreate GPU resources during a transform update', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh, width: () => 240 }); });
  s.transform.rotation.set([0.1, 0, 0]);
  expect(device.buffers).toHaveLength(2); expect(device.textures).toHaveLength(1); expect(device.draws).toBe(2);
  stop(); s.dispose();
});

it('rejects invalid dimensions before allocating a render surface', () => {
  const s = scene();
  for (const width of [0, -1, 1.5, NaN, Infinity, 2 ** 32]) {
    expect(() => Canvas3D({ mesh: s.mesh, width })).toThrow(RangeError);
  }
  s.dispose();
});


it('Forma viewport tracks dimension expressions and preserves its stable wrapper', () => {
  const source = `component Sized { state w: Number = 240; state h: Number = 140; Viewport3D { key: 'view'; width: state.w; height: state.h; } }`;
  const instance = compileFormaComponent(source), mounted = instance.mount(document.createElement('div'));
  const viewport = instance.getElementByKey('view')!;
  batch(() => { instance.state.w.set(400); instance.state.h.set(250); });
  expect(instance.getElementByKey('view')).toBe(viewport);
  for (const canvas of Array.from(viewport.children) as HTMLCanvasElement[]) expect([canvas.width, canvas.height]).toEqual([400, 250]);
  mounted.unmount(); instance.dispose();
});

it('invalid reactive dimensions leave the previous resources intact and a later valid size recovers', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(), width = signal(240); let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh: s.mesh, width }); });
  const original = device.buffers.slice();
  expect(() => width.set(0)).toThrow(RangeError);
  expect(canvas.width).toBe(240); expect(original.every(buffer => !buffer.destroyed)).toBe(true);
  width.set(320); expect(canvas.width).toBe(320); expect(device.draws).toBe(2);
  stop(); width.dispose(); s.dispose();
});

it('2D picking uses resized drawing-buffer dimensions and the visible canvas CSS scale', () => {
  const s = scene(), width = signal(240), height = signal(140), pick = vi.fn(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, width, height, onVertexPick: pick }); });
  batch(() => { width.set(480); height.set(280); });
  const fallback = viewport.children[1] as HTMLCanvasElement;
  vi.spyOn(fallback, 'getBoundingClientRect').mockReturnValue({ left: 10, top: 20, width: 960, height: 560 } as DOMRect);
  fallback.dispatchEvent(new MouseEvent('click', { clientX: 490, clientY: 300, bubbles: true }));
  expect(pick.mock.calls[0].slice(0,3)).toEqual([0,0,0]);
  expect(pick.mock.calls[0][3].mesh).toBe(s.mesh);
  stop(); width.dispose(); height.dispose(); s.dispose();
});


it('mesh replacement grows buffers, changes topology and detaches old dependencies', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const first = scene();
  const transform = createTransform3D();
  const second = createMesh3D({ positions: Array.from({ length: 36 }, (_, i) => i % 3 === 2 ? 0.2 : i / 100), transform });
  const mesh = signal(first.mesh); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh }); });
  const originalBuffers = device.buffers.slice(), originalTextures = device.textures.slice();
  mesh.set(second);
  expect(device.draws).toBe(2); expect(device.buffers.at(-1).bytes.length).toBe(192);
  expect(originalBuffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(originalTextures.every(texture => !texture.destroyed)).toBe(true);
  expect(device.textures).toHaveLength(1); expect(contexts[0].configure).toHaveBeenCalledOnce();
  expect(device.pipelines.map(pipeline => pipeline.primitive.topology)).toEqual(['triangle-list', 'line-strip']);
  first.transform.rotation.set([0.2, 0, 0]); expect(device.draws).toBe(2);
  transform.rotation.set([0.2, 0, 0]); expect(device.draws).toBe(3);
  mesh.set(first.mesh); expect(device.draws).toBe(4);
  expect(device.pipelines.at(-1).primitive.topology).toBe('triangle-list');
  expect(viewport.children).toHaveLength(2);
  stop(); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  mesh.dispose(); first.dispose(); second.dispose(); transform.dispose();
});

it('picking resolves the replacement mesh rather than the initially rendered vertices', () => {
  const first = scene(), transform = createTransform3D();
  const second = createMesh3D({ positions: [1, 1, 0.1, 0, 0, 0.2, -1, -1, 0.3], transform });
  const mesh = signal(first.mesh), pick = vi.fn(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh, onVertexPick: pick, width: 200, height: 200 }); });
  mesh.set(second);
  viewport.children[1].dispatchEvent(new MouseEvent('click', { clientX: 100, clientY: 100, bubbles: true }));
  expect(pick.mock.calls[0][0]).toBe(1);
  stop(); mesh.dispose(); first.dispose(); second.dispose(); transform.dispose();
});


it('rejects malformed geometry before allocating reactive mesh nodes', () => {
  const transform = createTransform3D(), baseline = getGraphTelemetry().nodes;
  for (const positions of [[0, 1], [0, NaN, 0], [0, Infinity, 0], [0, 1e100, 0]]) {
    expect(() => createMesh3D({ positions, transform })).toThrow(RangeError);
    expect(getGraphTelemetry().nodes).toBe(baseline);
  }
  for (const indices of [[0], [0, 1, 3], [0, -1, 2], [0, 1.5, 2], [0, NaN, 2]]) {
    expect(() => createMesh3D({ positions: [0,0,0, 1,0,0, 0,1,0], indices, transform })).toThrow(RangeError);
    expect(getGraphTelemetry().nodes).toBe(baseline);
  }
  transform.dispose();
});

it('never silently truncates a uint32-sized vertex index to uint16', () => {
  const transform = createTransform3D();
  expect(() => createMesh3D({ positions: new Float32Array(65537 * 3), indices: [0, 1, 65536], transform })).toThrow(/uint16/);
  const mesh = createMesh3D({ positions: new Float32Array(65536 * 3), indices: [0, 1, 65535], transform });
  expect(Array.from(mesh.indices!)).toEqual([0, 1, 65535]);
  mesh.dispose(); transform.dispose();
});


it('2D vertex projection and Rust picking use the WebGPU viewport scale', () => {
  const transform = createTransform3D(), mesh = createMesh3D({ positions: [0.8, 0.4, 0.2], transform });
  let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh, width: 200, height: 200 }); });
  const ctx = canvas.getContext('2d')!;
  const arc = vi.mocked(ctx.arc).mock.calls[0];
  expect(arc[0]).toBeCloseTo(180); expect(arc[1]).toBeCloseTo(60);
  const pick = pickNearestVertex3D(mesh, 180, 60, { viewportWidth: 200, viewportHeight: 200, maxRadiusPx: 1 });
  expect(pick?.vertexIndex).toBe(0); expect(pick?.distancePx).toBeCloseTo(0, 4);
  expect(pickNearestVertex3D(mesh, 152, 74, { viewportWidth: 200, viewportHeight: 200, maxRadiusPx: 1 })).toBeNull();
  stop(); mesh.dispose(); transform.dispose();
});

it('2D vertex markers and picking exclude vertices outside the homogeneous clip volume', () => {
  const transform = createTransform3D();
  const mesh = createMesh3D({ positions: [0,0,-0.1, 0,0,1.1, 1.1,0,0.2, 0,1.1,0.2, 0,0,0.5], transform });
  let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh, width: 200, height: 200 }); });
  expect(vi.mocked(canvas.getContext('2d')!.arc)).toHaveBeenCalledOnce();
  expect(pickNearestVertex3D(mesh, 100, 100, { viewportWidth: 200, viewportHeight: 200 })?.vertexIndex).toBe(4);
  stop(); mesh.dispose(); transform.dispose();
});


it('clips 2D edges at near and side planes before perspective divide', () => {
  const transform = createTransform3D();
  const mesh = createMesh3D({ positions: [-0.5,0,-0.5, 0.5,0,0.5, -2,0,0.5, 2,0,0.5], transform });
  let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh, width: 200, height: 200, edges: [[0,1],[2,3],[-1,1],[0,100]], showVertices: false }); });
  const ctx = canvas.getContext('2d')!;
  expect(vi.mocked(ctx.moveTo).mock.calls).toEqual([[100,100],[0,100]]);
  expect(vi.mocked(ctx.lineTo).mock.calls).toEqual([[150,100],[200,100]]);
  stop(); mesh.dispose(); transform.dispose();
});

it('clips 2D convex faces and does not emit paths for wholly invisible faces', () => {
  const transform = createTransform3D();
  const mesh = createMesh3D({ positions: [-0.5,-0.5,-0.5, 0.5,-0.5,0.5, 0.5,0.5,0.5, -0.5,0.5,-0.5], transform });
  let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh, width: 200, height: 200, faces: [[0,1,2,3]], showVertices: false }); });
  const ctx = canvas.getContext('2d')!;
  const points = [...vi.mocked(ctx.moveTo).mock.calls, ...vi.mocked(ctx.lineTo).mock.calls];
  expect(points).toHaveLength(4);
  expect(points.every(([x,y]) => x >= 100 && x <= 150 && y >= 50 && y <= 150)).toBe(true);
  vi.mocked(ctx.moveTo).mockClear(); vi.mocked(ctx.lineTo).mockClear();
  mesh.vertices.write([-0.5,-0.5,-1,1, 0.5,-0.5,-1,1, 0.5,0.5,-1,1, -0.5,0.5,-1,1]);
  expect(ctx.moveTo).not.toHaveBeenCalled(); expect(ctx.lineTo).not.toHaveBeenCalled();
  stop(); mesh.dispose(); transform.dispose();
});


it('uploads uint32 indices above 65535 without truncation and binds uint32 format', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const transform = createTransform3D();
  const mesh = createMesh3D({ positions: new Float32Array(65537 * 3), indices: [0,1,65536], indexFormat: 'uint32', transform });
  expect(mesh.indices).toBeInstanceOf(Uint32Array); expect(Array.from(mesh.indices!)).toEqual([0,1,65536]);
  let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh }); });
  expect(device.indexFormats).toEqual(['uint32']); expect(device.draws).toBe(1);
  expect(Array.from(new Uint32Array(device.uploads[0].buffer))).toEqual([0,1,65536]);
  stop(); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); mesh.dispose(); transform.dispose();
});

it('reactive replacement can switch between uint16 and uint32 index formats', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const first = scene();
  const second = createMesh3D({ positions: [0,0,0, 1,0,0, 0,1,0], indices: [0,1,2], indexFormat: 'uint32', transform: first.transform });
  const selected = signal(first.mesh); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: selected }); });
  selected.set(second); selected.set(first.mesh);
  expect(device.indexFormats).toEqual(['uint16','uint32','uint16']);
  stop(); selected.dispose(); second.dispose(); first.dispose();
});

it('rejects invalid uint32 indices and unsupported runtime formats before creating mesh nodes', () => {
  const transform = createTransform3D(), baseline = getGraphTelemetry().nodes;
  for (const index of [-1, 0.5, NaN, Infinity, 2 ** 32, 3]) {
    expect(() => createMesh3D({ positions: [0,0,0, 1,0,0, 0,1,0], indices: [0,1,index], indexFormat: 'uint32', transform })).toThrow(RangeError);
    expect(getGraphTelemetry().nodes).toBe(baseline);
  }
  expect(() => createMesh3D({ positions: [], indexFormat: 'uint8' as any, transform })).toThrow(/format/);
  transform.dispose();
});


it('updates topology reactively, grows its buffer, copies input and retains the chosen format', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh }); });
  const original = device.buffers.slice();
  const source = [0,1,2, 2,1,0, 1,0,2, 2,0,1, 0,2,1, 1,2,0];
  s.mesh.updateIndices(source, 'uint32'); source[0] = 2;
  expect(s.mesh.indices![0]).toBe(0); expect(device.draws).toBe(2);
  expect(device.indexFormats).toEqual(['uint16','uint32']);
  expect(device.indexedCounts).toEqual([3,18]);
  expect(device.buffers.at(-1).bytes.length).toBe(72);
  expect(original.find(buffer => buffer.usage & 0x10).destroyed).toBe(true);
  expect(original.find(buffer => buffer.usage & 0x20).destroyed).toBe(false);
  expect(device.textures).toHaveLength(1); expect(device.pipelines).toHaveLength(1);
  s.mesh.updateIndices([2,1,0]); expect(s.mesh.indices).toBeInstanceOf(Uint32Array);
  expect(device.indexedCounts.at(-1)).toBe(3);
  stop(); s.dispose();
});

it('distinguishes empty indexed topology from nonindexed lines', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh }); });
  s.mesh.updateIndices([]);
  expect(device.indexedCounts).toEqual([3,0]); expect(device.plainCounts).toEqual([]);
  expect(device.pipelines.at(-1).primitive.topology).toBe('triangle-list');
  s.mesh.updateIndices(undefined);
  expect(device.plainCounts).toEqual([3]); expect(device.pipelines.at(-1).primitive.topology).toBe('line-strip');
  s.mesh.updateIndices([0,1,2]); expect(device.indexedCounts.at(-1)).toBe(3);
  stop(); s.dispose();
});

it('invalid topology updates preserve the last topology and allocations', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh }); });
  const previous = s.mesh.indices, buffers = device.buffers.slice();
  for (const input of [[0], [0,1,3], [0,-1,2], [0,1.5,2]]) expect(() => s.mesh.updateIndices(input)).toThrow(RangeError);
  expect(() => s.mesh.updateIndices([0,1,2], 'uint8' as any)).toThrow(RangeError);
  expect(s.mesh.indices).toBe(previous); expect(device.draws).toBe(1);
  expect(buffers.every(buffer => !buffer.destroyed)).toBe(true);
  stop(); s.dispose();
});

it('topology handles reject writes after disposal, owner teardown and reactive reset', () => {
  const first = scene(); first.mesh.dispose(); expect(() => first.mesh.updateIndices([])).toThrow(/disposed/); first.transform.dispose();
  let owned!: ReturnType<typeof createMesh3D>, stop!: () => void;
  createRoot(dispose => { stop = dispose; const transform = createTransform3D(); owned = createMesh3D({ positions: [], transform }); });
  stop(); expect(() => owned.updateIndices([])).toThrow(/disposed/);
  const stale = scene(); resetReactiveSystem(); expect(() => stale.mesh.updateIndices([])).toThrow(/invalidated/);
});


it('topology reads track reactively and batched writes expose the final validated snapshot', () => {
  const s = scene(); const lengths: number[] = [], originalNodes = getGraphTelemetry().nodes;
  const watcher = effect(() => { lengths.push(s.mesh.indices?.length ?? -1); });
  batch(() => { s.mesh.updateIndices([]); s.mesh.updateIndices([0,1,2, 2,1,0]); });
  expect(lengths).toEqual([3,6]);
  watcher.dispose(); expect(getGraphTelemetry().nodes).toBe(originalNodes); s.dispose();
});

it('topology updates reject a foreign reactive context and leave the owning mesh unchanged', () => {
  const s = scene(), previous = s.mesh.indices;
  runWithIsolatedContext(() => {
    expect(() => s.mesh.updateIndices([])).toThrow(/different reactive context/);
    expect(() => s.mesh.indices).toThrow(/different reactive context/);
  });
  expect(s.mesh.indices).toBe(previous); s.dispose();
});


it('reuses index capacity, vertex buffer, depth texture and pipeline for fitting topology updates', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh }); });
  const original = device.buffers.slice();
  s.mesh.updateIndices([2,1,0], 'uint32');
  expect(device.buffers).toEqual(original); expect(original.every(buffer => !buffer.destroyed)).toBe(true);
  expect(device.indexFormats).toEqual(['uint16','uint32']);
  expect(device.textures).toHaveLength(1); expect(device.pipelines).toHaveLength(1);
  expect(contexts[0].configure).toHaveBeenCalledOnce();
  const uploads = device.uploads.length;
  s.transform.rotation.set([0.1,0,0]);
  expect(device.uploads.length).toBe(uploads + 1); // Only vertices, indices unchanged.
  s.mesh.updateIndices([]); s.mesh.updateIndices([0,1,2]);
  expect(device.buffers).toEqual(original); expect(device.pipelines).toHaveLength(1);
  stop(); expect(original.every(buffer => buffer.destroyed)).toBe(true); s.dispose();
});

it('only replaces the pipeline and index allocation when switching between triangles and lines', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh }); });
  const vertex = device.buffers.find(buffer => buffer.usage & 0x20), index = device.buffers.find(buffer => buffer.usage & 0x10);
  s.mesh.updateIndices(undefined);
  expect(index.destroyed).toBe(true); expect(vertex.destroyed).toBe(false); expect(device.textures).toHaveLength(1);
  expect(device.pipelines.map(pipeline => pipeline.primitive.topology)).toEqual(['triangle-list','line-strip']);
  s.mesh.updateIndices([0,1,2]);
  expect(device.buffers.filter(buffer => buffer.usage & 0x20)).toEqual([vertex]);
  expect(device.textures).toHaveLength(1); expect(contexts[0].configure).toHaveBeenCalledOnce();
  stop(); s.dispose();
});

it('cleans cached allocations after a failed reused-buffer upload and retries from the current topology', () => {
  const options = { failIndexWrite: false }, device = renderDevice(options);
  configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const canvas = createRoot(dispose => { stop = dispose; return Canvas3D({ mesh: s.mesh }); });
  const initial = device.buffers.slice(); options.failIndexWrite = true;
  s.mesh.updateIndices([2,1,0]);
  expect(initial.every(buffer => buffer.destroyed)).toBe(true); expect(canvas.dataset.renderBackend).toBe('unavailable');
  options.failIndexWrite = false; s.transform.rotation.set([0.1,0,0]);
  expect(device.draws).toBe(2); expect(canvas.dataset.renderBackend).toBe('webgpu-depth24plus');
  expect(Array.from(new Uint16Array(device.uploads.at(-2)!.buffer))).toEqual([2,1,0,0]);
  stop(); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); s.dispose();
});


it('resize failure cleans previous resources, uses 2D and retries at the new size', () => {
  const options = { failTexture: false }, device = renderDevice(options); configureGpuSubsystem({ device });
  const s = scene(), width = signal(240); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, width }); });
  options.failTexture = true; width.set(480);
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(device.textures.every(texture => texture.destroyed)).toBe(true);
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort');
  options.failTexture = false; s.transform.rotation.set([0.1,0,0]);
  expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus');
  expect(device.textures.at(-1).descriptor.size).toEqual([480,200,1]);
  stop(); width.dispose(); s.dispose();
});

it('failed resized depth view creation destroys both the new texture and old attachment', () => {
  const options = { failView: false }, device = renderDevice(options); configureGpuSubsystem({ device });
  const s = scene(), height = signal(140); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, height }); });
  options.failView = true; height.set(280);
  expect(device.textures).toHaveLength(2); expect(device.textures.every(texture => texture.destroyed)).toBe(true);
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort');
  options.failView = false; height.set(200);
  expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus');
  stop(); expect(device.textures.every(texture => texture.destroyed)).toBe(true); height.dispose(); s.dispose();
});


function scopedRenderDevice(options: { failTexture?: boolean } = {}) {
  type Deferred = { resolve(value: { message: string } | null): void; reject(error: Error): void };
  type Group = Deferred & { byFilter: Record<string, Deferred> };
  const scopes: Group[] = [], stack: Array<{ filter: string; group: Group }> = [];
  let group: Group;
  return Object.assign(renderDevice(options), {
    scopes,
    pushErrorScope: vi.fn((filter: string) => {
      if (filter === 'validation') {
        const byFilter: Record<string, Deferred> = {};
        group = { byFilter, resolve(value) { for (const [name, entry] of Object.entries(byFilter)) entry.resolve(name === 'validation' ? value : null); },
          reject(error) { for (const [name, entry] of Object.entries(byFilter)) { if (name === 'validation') entry.reject(error); else entry.resolve(null); } } };
        scopes.push(group);
      }
      stack.push({ filter, group });
    }),
    popErrorScope: vi.fn(() => {
      const entry = stack.pop(); if (!entry) throw new Error('empty scope stack');
      return new Promise<{ message: string } | null>((resolve, reject) => { entry.group.byFilter[entry.filter] = { resolve, reject }; });
    }),
  });
}
const drainValidation = async () => { await Promise.resolve(); await Promise.resolve(); };

it('async render validation failure releases GPU resources and draws the independent 2D surface', async () => {
  const device = scopedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  expect(viewport.dataset.renderValidation).toBe('pending');
  device.scopes[0].resolve({ message: 'invalid pipeline' }); await drainValidation();
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(viewport.dataset.renderValidation).toBe('failed');
  expect(viewport.dataset.renderError).toBe('invalid pipeline');
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); expect(device.textures.every(texture => texture.destroyed)).toBe(true);
  s.transform.rotation.set([0.1,0,0]); device.scopes[1].resolve(null); await drainValidation();
  expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus'); expect(viewport.dataset.renderValidation).toBe('passed');
  expect(viewport.dataset.renderError).toBeUndefined(); stop(); s.dispose();
});

it('a late old-frame validation error cannot destroy a newer validated frame', async () => {
  const device = scopedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  s.transform.rotation.set([0.1,0,0]); device.scopes[1].resolve(null); await drainValidation();
  device.scopes[0].resolve({ message: 'old frame' }); await drainValidation();
  expect(viewport.dataset.renderValidation).toBe('passed'); expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus');
  expect(device.buffers.every(buffer => !buffer.destroyed)).toBe(true); stop(); s.dispose();
});

it('scope rejection after owner disposal cannot redraw or mutate the disposed viewport', async () => {
  const device = scopedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  stop(); const draws = device.draws;
  device.scopes[0].reject(new Error('late rejection')); await drainValidation();
  expect(viewport.dataset.renderValidation).toBe('disposed'); expect(device.draws).toBe(draws); s.dispose();
});

it('device replacement isolates late validation errors from the previous device', async () => {
  const first = scopedRenderDevice(), second = scopedRenderDevice(); configureGpuSubsystem({ device: first }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  configureGpuSubsystem({ device: second }); second.scopes[0].resolve(null); await drainValidation();
  first.scopes[0].resolve({ message: 'old device' }); await drainValidation();
  expect(viewport.dataset.renderValidation).toBe('passed'); expect(second.buffers.every(buffer => !buffer.destroyed)).toBe(true);
  stop(); s.dispose();
});

it('a synchronous initialization failure pops each opened render error scope exactly once', async () => {
  const device = scopedRenderDevice({ failTexture: true }); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  expect(device.pushErrorScope).toHaveBeenCalledTimes(3); expect(device.popErrorScope).toHaveBeenCalledTimes(3);
  device.scopes[0].resolve(null); await drainValidation();
  expect(viewport.dataset.renderValidation).toBe('failed'); expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort');
  stop(); s.dispose();
});


it('reset invalidates pending render validation without reading recycled mesh handles', async () => {
  const device = scopedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  resetReactiveSystem(); const replacement = scene();
  device.scopes[0].resolve({ message: 'pre-reset frame' }); await drainValidation();
  expect(viewport.dataset.renderBackend).toBe('unavailable'); expect(viewport.dataset.renderValidation).toBe('inactive');
  expect(replacement.mesh.indices).toEqual(new Uint16Array([0,1,2])); stop(); replacement.dispose();
});

it('active render scope rejection is handled as a validation failure', async () => {
  const device = scopedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  device.scopes[0].reject(new Error('scope rejected')); await drainValidation();
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(viewport.dataset.renderError).toBe('scope rejected');
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); stop(); s.dispose();
});

it('a device without queue submission cannot report a rendered GPU frame', () => {
  const device = renderDevice(); delete (device.queue as any).submit; configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(viewport.dataset.renderValidation).toBe('failed');
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); stop(); s.dispose();
});


it('a stream of newer pending frames cannot hide an unresolved validation failure', async () => {
  const device = scopedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  s.transform.rotation.set([0.1,0,0]);
  device.scopes[0].resolve({ message: 'invalid cached resource' }); await drainValidation();
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort');
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  device.scopes[1].resolve(null); await drainValidation();
  expect(viewport.dataset.renderValidation).toBe('failed');
  s.transform.rotation.set([0.2,0,0]); device.scopes[2].resolve(null); await drainValidation();
  expect(viewport.dataset.renderValidation).toBe('passed'); stop(); s.dispose();
});


it('rejects texture dimensions beyond device limits before any GPU allocation and recovers at the limit', () => {
  const device = Object.assign(renderDevice(), { limits: { maxTextureDimension2D: 256, maxBufferSize: 1024 } });
  configureGpuSubsystem({ device }); const s = scene(), width = signal(257); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, width, height: 128 }); });
  expect(device.buffers).toHaveLength(0); expect(device.textures).toHaveLength(0); expect(device.pipelines).toHaveLength(0);
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(viewport.dataset.renderError).toContain('maxTextureDimension2D');
  width.set(256); expect(device.draws).toBe(1); expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus');
  stop(); width.dispose(); s.dispose();
});

it('rejects oversized vertex allocations before creating a GPU canvas context', () => {
  const device = Object.assign(renderDevice(), { limits: { maxBufferSize: 128 } }); configureGpuSubsystem({ device });
  const transform = createTransform3D(), mesh = createMesh3D({ positions: new Float32Array(12 * 3), transform }); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh }); });
  expect(device.buffers).toHaveLength(0); expect(contexts).toHaveLength(0);
  expect(viewport.dataset.renderError).toContain('requested 192, limit 128');
  stop(); mesh.dispose(); transform.dispose();
});

it('oversized topology cleans existing allocations and a fitting update recovers', () => {
  const device = Object.assign(renderDevice(), { limits: { maxBufferSize: 64 } }); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  expect(device.draws).toBe(1); const allocated = device.buffers.length;
  s.mesh.updateIndices(Array.from({ length: 18 }, (_, i) => i % 3), 'uint32');
  expect(device.buffers).toHaveLength(allocated); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(viewport.dataset.renderError).toContain('maxBufferSize');
  s.mesh.updateIndices([0,1,2]); expect(device.draws).toBe(2); expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus');
  stop(); s.dispose();
});

it('over-limit resize destroys old resources without attempting an invalid texture allocation', () => {
  const device = Object.assign(renderDevice(), { limits: { maxTextureDimension2D: 400 } }); configureGpuSubsystem({ device });
  const s = scene(), height = signal(200); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, height }); });
  height.set(401); expect(device.textures).toHaveLength(1); expect(device.textures[0].destroyed).toBe(true);
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort');
  height.set(400); expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus');
  stop(); height.dispose(); s.dispose();
});


it.each(['out-of-memory', 'internal'])('a %s scope error immediately frees resources even if other scopes are pending', async filter => {
  const device = scopedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  device.scopes[0].byFilter[filter].resolve({ message: 'injected failure' }); await drainValidation();
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(viewport.dataset.renderError).toContain(filter);
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  device.scopes[0].resolve(null); await drainValidation(); expect(viewport.dataset.renderValidation).toBe('failed');
  stop(); s.dispose();
});

it('success waits for all three error filters regardless of resolution order', async () => {
  const device = scopedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  expect(device.pushErrorScope.mock.calls.map(call => call[0])).toEqual(['validation','out-of-memory','internal']);
  device.scopes[0].byFilter.validation.resolve(null); device.scopes[0].byFilter.internal.resolve(null); await drainValidation();
  expect(viewport.dataset.renderValidation).toBe('pending');
  device.scopes[0].byFilter['out-of-memory'].resolve(null); await drainValidation();
  expect(viewport.dataset.renderValidation).toBe('passed'); stop(); s.dispose();
});

it('partial scope setup failure still pops all scopes successfully opened before it', async () => {
  const device = scopedRenderDevice(), push = device.pushErrorScope.getMockImplementation()!;
  device.pushErrorScope.mockImplementation(filter => { if (filter === 'internal') throw new Error('unsupported filter'); push(filter); });
  configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh }); });
  expect(device.popErrorScope).toHaveBeenCalledTimes(2); expect(device.buffers).toHaveLength(0);
  device.scopes[0].resolve(null); await drainValidation(); expect(viewport.dataset.renderValidation).toBe('failed');
  stop(); s.dispose();
});


it('indexed picking excludes hidden vertices and finds a nearby visible candidate', () => {
  const transform = createTransform3D(), mesh = createMesh3D({ positions: [-0.8,-0.8,0.2, 0.8,-0.8,0.2, 0,0.8,0.2, 0,0,0.8, 0.15,0,0.1], indices: [0,1,2], transform });
  const options = { viewportWidth: 200, viewportHeight: 200, maxRadiusPx: 20 };
  expect(pickNearestVertex3D(mesh,100,100,{ ...options, occlusion: 'none' })?.vertexIndex).toBe(3);
  expect(pickNearestVertex3D(mesh,100,100,options)?.vertexIndex).toBe(4);
  mesh.updateIndices([]); expect(pickNearestVertex3D(mesh,100,100,options)?.vertexIndex).toBe(3);
  mesh.dispose(); transform.dispose();
});

it('visible picking transports uint32 triangle indices without truncation', () => {
  const positions = new Float32Array(65537 * 3).fill(2);
  positions.set([0,0,0.8]); positions.set([-0.8,-0.8,0.2, 0.8,-0.8,0.2, 0,0.8,0.2], 65534 * 3);
  const transform = createTransform3D(), mesh = createMesh3D({ positions, indices: [65534,65535,65536], indexFormat: 'uint32', transform });
  const options = { viewportWidth: 200, viewportHeight: 200, maxRadiusPx: 1 };
  expect(pickNearestVertex3D(mesh,100,100,{ ...options, occlusion: 'none' })?.vertexIndex).toBe(0);
  expect(pickNearestVertex3D(mesh,100,100,options)).toBeNull(); mesh.dispose(); transform.dispose();
});

it('viewport click handlers honor explicit through-surface picking', () => {
  const transform = createTransform3D(), mesh = createMesh3D({ positions: [-0.8,-0.8,0.2, 0.8,-0.8,0.2, 0,0.8,0.2, 0,0,0.8], indices: [0,1,2], transform });
  const visible = vi.fn(), through = vi.fn(), wireframe = vi.fn(); let stop!: () => void;
  const views = createRoot(dispose => { stop = dispose; return [Viewport3D({ mesh, width: 200, height: 200, onVertexPick: visible, pickOcclusion: 'indexed' }), Viewport3D({ mesh, width: 200, height: 200, onVertexPick: through, pickOcclusion: 'none' }), Viewport3D({ mesh, width: 200, height: 200, onVertexPick: wireframe })]; });
  for (const viewport of views) viewport.children[1].dispatchEvent(new MouseEvent('click',{ clientX:100, clientY:100, bubbles:true }));
  expect(visible).not.toHaveBeenCalled(); expect(through.mock.calls[0][0]).toBe(3); expect(wireframe.mock.calls[0][0]).toBe(3);
  stop(); mesh.dispose(); transform.dispose();
});

it('WASM picking handles tiny positive homogeneous w without reciprocal overflow', () => {
  const transform = createTransform3D(), mesh = createMesh3D({ positions: [0,0,0], transform });
  mesh.transformedVertices.write([0,0,0,1e-45]);
  const hit = pickNearestVertex3D(mesh,100,100,{ viewportWidth:200,viewportHeight:200,maxRadiusPx:0 });
  expect(hit?.vertexIndex).toBe(0); expect(hit?.distancePx).toBe(0); mesh.dispose(); transform.dispose();
});


it('mesh replacement within capacity reuses geometry buffers, depth texture and pipeline', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const first = scene();
  const transform = createTransform3D(), second = createMesh3D({ positions: [0,0,0.2, 0.5,0,0.2, 0,0.5,0.2], indices: [2,1,0], indexFormat: 'uint32', transform });
  const selected = signal(first.mesh); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: selected }); });
  const buffers = device.buffers.slice(); selected.set(second);
  expect(device.buffers).toEqual(buffers); expect(buffers.every(buffer => !buffer.destroyed)).toBe(true);
  expect(device.textures).toHaveLength(1); expect(device.pipelines).toHaveLength(1); expect(device.indexFormats).toEqual(['uint16','uint32']);
  expect(Array.from(new Uint32Array(device.uploads.at(-2)!.buffer))).toEqual([2,1,0]);
  stop(); selected.dispose(); second.dispose(); transform.dispose(); first.dispose();
});

it('shrinking a nonindexed mesh reuses vertex capacity but draws only the current vertices', () => {
  const device = renderDevice(); configureGpuSubsystem({ device }); const transform = createTransform3D();
  const large = createMesh3D({ positions: new Float32Array(12 * 3), transform }), small = createMesh3D({ positions: [0,0,0.2, 0.5,0,0.2], transform });
  const selected = signal(large); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: selected }); }); selected.set(small);
  expect(device.plainCounts).toEqual([12,2]); expect(device.buffers).toHaveLength(1);
  expect(device.buffers[0].bytes.length).toBe(192); expect(device.textures).toHaveLength(1); expect(device.pipelines).toHaveLength(1);
  stop(); selected.dispose(); small.dispose(); large.dispose(); transform.dispose();
});

it('failed vertex-buffer growth frees retained resources and a fitting replacement retries', () => {
  const options = { failVertexAllocation: false }, device = renderDevice(options); configureGpuSubsystem({ device }); const first = scene();
  const transform = createTransform3D(), large = createMesh3D({ positions: new Float32Array(12 * 3), transform });
  const selected = signal(first.mesh); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: selected }); });
  options.failVertexAllocation = true; selected.set(large);
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(device.textures.every(texture => texture.destroyed)).toBe(true);
  options.failVertexAllocation = false; selected.set(first.mesh);
  expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus'); stop(); selected.dispose(); large.dispose(); transform.dispose(); first.dispose();
});

function animationFrames() {
  const callbacks = new Map<number, FrameRequestCallback>(); let id = 0;
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => { callbacks.set(++id, callback); return id; });
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(handle => { callbacks.delete(handle); });
  return { callbacks, flush() { const pending = Array.from(callbacks.values()); callbacks.clear(); for (const callback of pending) callback(0); } };
}
function queuedRenderDevice() {
  const device = scopedRenderDevice();
  const completion: Array<{ resolve(): void; reject(error: Error): void }> = [];
  Object.assign(device.queue, { onSubmittedWorkDone: () => new Promise<void>((resolve,reject) => { completion.push({ resolve, reject }); }) });
  return Object.assign(device, { completion });
}

it('animation-frame mode coalesces changes and waits for both GPU queue and all scopes', async () => {
  const frames = animationFrames(), device = queuedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh, renderMode: 'animation-frame' }); });
  for (let i = 1; i <= 20; i++) s.transform.rotation.set([i / 100,0,0]);
  expect(device.draws).toBe(1); expect(frames.callbacks.size).toBe(0);
  device.completion[0].resolve(); await drainValidation(); expect(frames.callbacks.size).toBe(0);
  device.scopes[0].resolve(null); await drainValidation(); expect(frames.callbacks.size).toBe(1);
  frames.flush(); expect(device.draws).toBe(2);
  const vertex = device.buffers.find(buffer => buffer.usage & 0x20);
  expect(Array.from(new Float32Array(vertex.bytes.buffer).subarray(0,12))).toEqual(Array.from(s.mesh.transformedVertices.peek()).slice(0,12));
  stop(); s.dispose();
});

it('scheduled CPU drawing coalesces resize updates and cancels a pending frame on teardown', () => {
  const frames = animationFrames(), s = scene(), width = signal(200); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, width, renderMode: 'animation-frame' }); });
  width.set(240); width.set(300); expect((viewport.children[1] as HTMLCanvasElement).width).toBe(200);
  expect(frames.callbacks.size).toBe(1); frames.flush(); expect((viewport.children[1] as HTMLCanvasElement).width).toBe(300);
  width.set(320); stop(); expect(frames.callbacks.size).toBe(0); frames.flush(); width.dispose(); s.dispose();
});

it('queue rejection after scope success frees resources and permits a later scheduled retry', async () => {
  const frames = animationFrames(), device = queuedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, renderMode: 'animation-frame' }); });
  device.scopes[0].resolve(null); await drainValidation();
  device.completion[0].reject(new Error('queue failed')); await drainValidation();
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  s.transform.rotation.set([0.1,0,0]); frames.flush();
  device.completion[1].resolve(); device.scopes[1].resolve(null); await drainValidation();
  expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus'); stop(); s.dispose();
});

it('late queue completion cannot redraw a disposed scheduled viewport', async () => {
  const frames = animationFrames(), device = queuedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh, renderMode: 'animation-frame' }); });
  s.transform.rotation.set([0.1,0,0]); stop(); device.completion[0].resolve(); device.scopes[0].resolve(null); await drainValidation();
  expect(frames.callbacks.size).toBe(0); expect(device.draws).toBe(1); s.dispose();
});

it('device replacement discards old queue credit without unlocking the new pending submission', async () => {
  const frames = animationFrames(), first = queuedRenderDevice(), second = queuedRenderDevice(); configureGpuSubsystem({ device: first }); const s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh, renderMode: 'animation-frame' }); });
  configureGpuSubsystem({ device: second });
  first.completion[0].resolve(); first.scopes[0].resolve(null); await drainValidation();
  s.transform.rotation.set([0.1,0,0]); expect(frames.callbacks.size).toBe(0);
  second.completion[0].resolve(); second.scopes[0].resolve(null); await drainValidation();
  expect(frames.callbacks.size).toBe(1); frames.flush(); expect(first.draws).toBe(1); expect(second.draws).toBe(2);
  stop(); s.dispose();
});

it('reactive reset cancels deferred CPU drawing without reading stale tensors', () => {
  const frames = animationFrames(), s = scene(); let stop!: () => void;
  createRoot(dispose => { stop = dispose; Canvas3D({ mesh: s.mesh, renderMode: 'animation-frame' }); });
  s.transform.rotation.set([0.1,0,0]); expect(frames.callbacks.size).toBe(1);
  resetReactiveSystem(); expect(frames.callbacks.size).toBe(0); expect(() => frames.flush()).not.toThrow(); stop();
});

it('scheduled GPU mode requires queue completion support and falls back cleanly if absent', () => {
  animationFrames(); const device = renderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, renderMode: 'animation-frame' }); });
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(viewport.dataset.renderError).toContain('queue completion');
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); stop(); s.dispose();
});

it('Forma exposes frame scheduling and cancels its pending callback on unmount', () => {
  const frames = animationFrames();
  const instance = compileFormaComponent(`component Scheduled { state w: Number = 200; Viewport3D { key: 'view'; width: state.w; renderMode: 'animation-frame'; } }`);
  const mounted = instance.mount(document.createElement('div')), viewport = instance.getElementByKey('view')!;
  instance.state.w.set(300); expect((viewport.children[1] as HTMLCanvasElement).width).toBe(200);
  frames.flush(); expect((viewport.children[1] as HTMLCanvasElement).width).toBe(300);
  instance.state.w.set(320); mounted.unmount(); expect(frames.callbacks.size).toBe(0); instance.dispose();
});

it('invalid scheduling modes fail before canvas initialization', () => {
  const s = scene(); expect(() => Canvas3D({ mesh: s.mesh, renderMode: 'queued' as any })).toThrow(/renderMode/); expect(contexts).toHaveLength(0); s.dispose();
});


it('a failed scheduled frame retains backpressure until its submitted GPU work settles', async () => {
  const frames = animationFrames(), device = queuedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, renderMode: 'animation-frame' }); });
  s.transform.rotation.set([0.1,0,0]); device.scopes[0].resolve({ message: 'frame failed' }); await drainValidation();
  expect(viewport.dataset.renderBackend).toBe('canvas2d-zsort'); expect(frames.callbacks.size).toBe(0);
  s.transform.rotation.set([0.2,0,0]); expect(frames.callbacks.size).toBe(0); expect(device.draws).toBe(1);
  device.completion[0].resolve(); await drainValidation(); expect(frames.callbacks.size).toBe(1);
  frames.flush(); expect(device.draws).toBe(2); stop(); s.dispose();
});

it('deferred CPU picking uses drawn vertices until the next animation frame', () => {
  const frames = animationFrames(), s = scene(), pick = vi.fn(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh: s.mesh, width:200, height:200, renderMode:'animation-frame', onVertexPick:pick }); });
  s.mesh.vertices.setAt(0,0.5);
  viewport.children[1].dispatchEvent(new MouseEvent('click',{ clientX:100,clientY:100,bubbles:true }));
  expect(pick.mock.calls[0][0]).toBe(0); expect(pick.mock.calls[0][1]).toBe(0); const id = pick.mock.calls[0][3].id;
  frames.flush();
  viewport.children[1].dispatchEvent(new MouseEvent('click',{ clientX:100,clientY:100,bubbles:true })); expect(pick).toHaveBeenCalledOnce();
  viewport.children[1].dispatchEvent(new MouseEvent('click',{ clientX:150,clientY:100,bubbles:true }));
  expect(pick.mock.calls[1][3].id).toBeGreaterThan(id); stop(); s.dispose();
});

it('queued GPU picking retains the submitted geometry without changing live arena or graph nodes', async () => {
  const frames = animationFrames(), device = queuedRenderDevice(); configureGpuSubsystem({ device }); const s = scene(), pick = vi.fn(); let stop!: () => void;
  const viewport = createRoot(dispose => { stop = dispose; return Viewport3D({ mesh:s.mesh,width:200,height:200,renderMode:'animation-frame',onVertexPick:pick }); });
  s.mesh.vertices.setAt(0,0.5); const nodes = getGraphTelemetry().nodes, arenaBytes = getGpuTelemetry().arenaByteLength;
  const before = Array.from(s.mesh.transformedVertices.peek());
  viewport.children[0].dispatchEvent(new MouseEvent('click',{clientX:100,clientY:100,bubbles:true}));
  expect(pick.mock.calls[0][0]).toBe(0); expect(pick.mock.calls[0][3].backend).toBe('webgpu-depth24plus');
  expect(getGraphTelemetry().nodes).toBe(nodes); expect(getGpuTelemetry().arenaByteLength).toBe(arenaBytes); expect(Array.from(s.mesh.transformedVertices.peek())).toEqual(before);
  device.completion[0].resolve(); device.scopes[0].resolve(null); await drainValidation(); frames.flush();
  viewport.children[0].dispatchEvent(new MouseEvent('click',{clientX:100,clientY:100,bubbles:true})); expect(pick).toHaveBeenCalledOnce();
  stop(); s.dispose();
});

it('a pending mesh replacement picks the old drawn identity even after that mesh is disposed', () => {
  const frames = animationFrames(), first = scene(), transform = createTransform3D();
  const second = createMesh3D({positions:[1,1,0.1, 0,0,0.2, -1,-1,0.3],transform}), selected = signal(first.mesh), pick = vi.fn(); let stop!: () => void;
  const viewport = createRoot(dispose => {stop=dispose;return Viewport3D({mesh:selected,width:200,height:200,renderMode:'animation-frame',onVertexPick:pick});});
  selected.set(second); first.dispose();
  viewport.children[1].dispatchEvent(new MouseEvent('click',{clientX:100,clientY:100,bubbles:true}));
  expect(pick.mock.calls[0][0]).toBe(0); expect(pick.mock.calls[0][3].mesh).toBe(first.mesh);
  frames.flush(); viewport.children[1].dispatchEvent(new MouseEvent('click',{clientX:100,clientY:100,bubbles:true}));
  expect(pick.mock.calls[1][0]).toBe(1); expect(pick.mock.calls[1][3].mesh).toBe(second);
  expect(Object.keys(pick.mock.calls[1][3]).sort()).toEqual(['backend','height','id','mesh','width']);
  stop(); selected.dispose(); second.dispose(); transform.dispose();
});

it('queued picking uses drawn triangle occlusion rather than pending index updates', async () => {
  const frames = animationFrames(), device = queuedRenderDevice(); configureGpuSubsystem({device}); const transform = createTransform3D();
  const mesh = createMesh3D({positions:[-0.8,-0.8,0.2, 0.8,-0.8,0.2, 0,0.8,0.2, 0,0,0.8],indices:[0,1,2],transform}); const pick=vi.fn();let stop!:()=>void;
  const viewport=createRoot(dispose=>{stop=dispose;return Viewport3D({mesh,width:200,height:200,renderMode:'animation-frame',onVertexPick:pick});});
  const click=()=>viewport.children[0].dispatchEvent(new MouseEvent('click',{clientX:100,clientY:100,bubbles:true}));
  mesh.updateIndices([]); click(); expect(pick).not.toHaveBeenCalled();
  device.completion[0].resolve();device.scopes[0].resolve(null);await drainValidation();frames.flush();click();expect(pick.mock.calls[0][0]).toBe(3);
  mesh.updateIndices([0,1,2]);click();expect(pick).toHaveBeenCalledTimes(2);
  device.completion[1].resolve();device.scopes[1].resolve(null);await drainValidation();frames.flush();click();expect(pick).toHaveBeenCalledTimes(2);
  stop();mesh.dispose();transform.dispose();
});

it('an invalid size update preserves picking for the unchanged drawn canvas', () => {
  const s=scene(),width=signal(200),pick=vi.fn();let stop!:()=>void;
  const viewport=createRoot(dispose=>{stop=dispose;return Viewport3D({mesh:s.mesh,width,height:200,onVertexPick:pick});});
  expect(()=>width.set(0)).toThrow(RangeError);
  viewport.children[1].dispatchEvent(new MouseEvent('click',{clientX:100,clientY:100,bubbles:true}));
  expect(pick.mock.calls[0][0]).toBe(0);expect(pick.mock.calls[0][3].width).toBe(200);stop();width.dispose();s.dispose();
});


it('snapshot picking invokes the callback in its captured WASM context', () => {
  const core = createWasmCoreInstance(); let observed: unknown, stop!: () => void;
  const viewport = withWasmCore(core, () => createRoot(dispose => {
    stop = dispose; const s = scene(); onCleanup(() => s.dispose());
    return Viewport3D({mesh:s.mesh,width:200,height:200,onVertexPick:(_index,_distance,_z,frame)=>{
      observed = getWasmCore(); expect(frame.mesh.transformedVertices.view()).toHaveLength(12);
    }});
  }));
  viewport.children[1].dispatchEvent(new MouseEvent('click',{clientX:100,clientY:100,bubbles:true}));
  expect(observed).toBe(core); stop();
});


it('a failed device-change redraw clears frame picking instead of exposing stale GPU geometry', () => {
  const first=renderDevice(),second=renderDevice();configureGpuSubsystem({device:first});const s=scene(),width=signal(200),pick=vi.fn();let stop!:()=>void;
  const viewport=createRoot(dispose=>{stop=dispose;return Viewport3D({mesh:s.mesh,width,height:200,onVertexPick:pick});});
  expect(()=>width.set(0)).toThrow(RangeError);configureGpuSubsystem({device:second});
  expect(viewport.dataset.renderBackend).toBe('unavailable');expect(viewport.dataset.renderValidation).toBe('failed');
  viewport.dispatchEvent(new MouseEvent('click',{clientX:100,clientY:100,bubbles:true}));expect(pick).not.toHaveBeenCalled();
  width.set(200);expect(viewport.dataset.renderBackend).toBe('webgpu-depth24plus');stop();width.dispose();s.dispose();
});
