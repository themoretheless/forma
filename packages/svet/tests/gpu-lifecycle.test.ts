import { beforeEach, expect, it, vi } from 'vitest';
import { configureGpuSubsystem, createRoot, runWithOwnerScope, onCleanup, effect, executeGpuVectorFma, dispatchGpuComputeKernel, initWebGpuHardware, createGpuTensor, createTransform3D, createPerspectiveCamera3D, createMesh3D, flushEffects, getGraphTelemetry, getCurrentOwner, getWasmCore, getGpuTelemetry, resetGpuTelemetry, resetReactiveSystem, signal, runWithIsolatedContext, syncGpuArena } from '../src/engine';

beforeEach(() => { resetReactiveSystem(); configureGpuSubsystem({ architecture: 'DUMA', device: null }); resetGpuTelemetry(); });
function device() {
  const buffers: Array<{ bytes: Uint8Array; destroyed: boolean; destroy(): void }> = [];
  const writes: number[] = [];
  return { buffers, writes, createBuffer({ size }: { size: number }) {
    const buffer = { bytes: new Uint8Array(size), destroyed: false, destroy() { this.destroyed = true; } };
    buffers.push(buffer); return buffer;
  }, queue: { writeBuffer(buffer: any, offset: number, data: any, dataOffset = 0, size = 0) {
    if (!buffer || buffer.destroyed || offset + size > buffer.bytes.length || offset % 4 || size % 4) throw new Error('invalid GPU upload');
    const bytes = ArrayBuffer.isView(data) ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength) : new Uint8Array(data);
    buffer.bytes.set(bytes.subarray(dataOffset, dataOffset + size), offset); writes.push(size);
  } } };
}

it('creates a valid arena buffer and uploads the complete arena again after growth', () => {
  const gpu = device(); configureGpuSubsystem({ device: gpu });
  const a = createGpuTensor([1, 2, 3, 4]); syncGpuArena();
  const b = createGpuTensor([5, 6, 7, 8]); syncGpuArena();
  expect(gpu.buffers).toHaveLength(2);
  expect(gpu.buffers[0].destroyed).toBe(true);
  expect(Array.from(new Float32Array(gpu.buffers[1].bytes.buffer))).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  a.dispose(); b.dispose(); configureGpuSubsystem({ device: null });
});

it('reports the actual aligned bytes submitted to the queue', () => {
  const gpu = device(); configureGpuSubsystem({ device: gpu });
  const tensor = createGpuTensor(256); syncGpuArena(); gpu.writes.length = 0;
  tensor.write([1, 2], 100);
  const report = syncGpuArena();
  expect(report.bytesTransferred).toBe(gpu.writes.reduce((a, b) => a + b, 0));
  expect(report.bytesTransferred).toBe(16);
  configureGpuSubsystem({ device: null });
});

it('attaching a different device uploads existing data even when the arena was clean', () => {
  const tensor = createGpuTensor([1, 2, 3, 4]);
  const first = device(); configureGpuSubsystem({ device: first }); syncGpuArena();
  const second = device(); configureGpuSubsystem({ device: second }); syncGpuArena();
  expect(first.buffers[0].destroyed).toBe(true);
  expect(second.writes).toEqual([16]);
  expect(Array.from(new Float32Array(second.buffers[0].bytes.buffer))).toEqual(Array.from(tensor()));
  configureGpuSubsystem({ device: null });
});

it('does not report hardware transfers when no device is configured', () => {
  createGpuTensor([1, 2, 3, 4]);
  expect(syncGpuArena().bytesTransferred).toBe(0);
  expect(getGpuTelemetry().totalBytesTransferred).toBe(0);
});

it('rejects disposed tensor access so recycled storage cannot be overwritten', () => {
  const old = createGpuTensor([1, 2, 3, 4]); old.dispose();
  const next = createGpuTensor([5, 6, 7, 8]);
  expect(next.offset).toBe(old.offset);
  expect(() => old.setAt(0, 99)).toThrow();
  expect(() => old.write([99])).toThrow();
  expect(() => old.view()).toThrow();
  expect(Array.from(next())).toEqual([5, 6, 7, 8]);
});

it('old tensor disposal after graph reset cannot free a new allocation', () => {
  const old = createGpuTensor([1, 2, 3, 4]); resetReactiveSystem();
  const next = createGpuTensor([5, 6, 7, 8]); old.dispose();
  expect(getGraphTelemetry().nodes).toBe(1);
  const third = createGpuTensor([9, 10, 11, 12]);
  expect(third.offset).not.toBe(next.offset);
});

it('validates lengths and integer indices before mutating graph or arena', () => {
  for (const length of [-1, 1.5, NaN, Infinity, 67108865]) expect(() => createGpuTensor(length)).toThrow(RangeError);
  expect(getGraphTelemetry().nodes).toBe(0);
  const tensor = createGpuTensor([1, 2, 3, 4]);
  for (const index of [-1, 0.5, NaN, Infinity, 4]) expect(() => tensor.setAt(index, 99)).toThrow(RangeError);
  expect(() => tensor.write([9], -1)).toThrow(RangeError);
  expect(Array.from(tensor())).toEqual([1, 2, 3, 4]);
});

it('isolated contexts have independent devices, architecture and telemetry', () => {
  configureGpuSubsystem({ architecture: 'UMA', device: null });
  const isolated = runWithIsolatedContext(() => {
    configureGpuSubsystem({ architecture: 'DUMA', device: null });
    createGpuTensor([1, 2]); syncGpuArena();
    return getGpuTelemetry().architecture;
  });
  expect(isolated).toBe('DUMA');
  expect(getGpuTelemetry().architecture).toBe('UMA');
  expect(getGpuTelemetry().totalSyncCount).toBe(0);
});

it('disposing a top-level transform releases its reactive computation', () => {
  const baseline = getGraphTelemetry().nodes;
  const transform = createTransform3D(); transform.dispose();
  expect(getGraphTelemetry().nodes).toBe(baseline);
});

it('a clean arena performs no uploads on a repeated sync', () => {
  const gpu = device(); configureGpuSubsystem({ device: gpu });
  createGpuTensor([1, 2]); syncGpuArena(); gpu.writes.length = 0;
  expect(syncGpuArena().bytesTransferred).toBe(0);
  expect(gpu.writes).toEqual([]);
  configureGpuSubsystem({ device: null });
});

it('preserves dirty state after a queue failure so synchronization can be retried', () => {
  const gpu = device(); const write = gpu.queue.writeBuffer;
  let fail = true;
  gpu.queue.writeBuffer = (...args) => { if (fail) throw new Error('queue failed'); write(...args); };
  configureGpuSubsystem({ device: gpu }); createGpuTensor([1, 2, 3, 4]);
  expect(() => syncGpuArena()).toThrow('queue failed');
  fail = false;
  expect(syncGpuArena().bytesTransferred).toBe(16);
  expect(Array.from(new Float32Array(gpu.buffers[0].bytes.buffer))).toEqual([1, 2, 3, 4]);
  configureGpuSubsystem({ device: null });
});

it('requesting hardware where no API exists clears a previously configured device', async () => {
  configureGpuSubsystem({ device: device() });
  expect((await initWebGpuHardware()).webgpuAvailable).toBe(false);
});

it('owner teardown releases its configured arena buffer', () => {
  const gpu = device(); let stop!: () => void;
  createRoot(dispose => {
    stop = dispose; configureGpuSubsystem({ device: gpu });
    createGpuTensor([1, 2]); syncGpuArena();
  });
  stop();
  expect(gpu.buffers[0].destroyed).toBe(true);
  expect(getGpuTelemetry().webgpuAvailable).toBe(false);
});

it('does not pretend a shader ran when no host fallback was supplied', () => {
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  expect(() => dispatchGpuComputeKernel({ wgsl: '@compute @workgroup_size(1) fn main() {}', inputs: [input], output }))
    .toThrow('fallbackKernel');
  expect(Array.from(output())).toEqual([0]);
});

it('matches explicit float32 multiply/add rounding across SIMD chunks and scalar tails', () => {
  let seed = 123456789;
  const random = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
  for (const length of [1, 3, 4, 5, 17, 65, 129]) {
    const input = createGpuTensor(Array.from({ length }, () => (random() - 0.5) * 1000));
    const output = createGpuTensor(length);
    const scale = (random() - 0.5) * 10, bias = random() - 0.5;
    const expected = Array.from(input(), value => Math.fround(Math.fround(value * Math.fround(scale)) + Math.fround(bias)));
    executeGpuVectorFma(input, output, scale, bias);
    expect(Array.from(output())).toEqual(expected);
    input.dispose(); output.dispose();
  }
});

it('does not allow a compute operation to mix tensors from different WASM contexts', () => {
  const outside = createGpuTensor([1, 2]);
  runWithIsolatedContext(() => {
    const inside = createGpuTensor([0, 0]);
    expect(() => executeGpuVectorFma(outside, inside, 2, 1)).toThrow('different reactive context');
    expect(Array.from(inside())).toEqual([0, 0]);
    inside.dispose();
  });
});

it('compares stored float32 values including signed zero and NaN', () => {
  const tensor = createGpuTensor([0, NaN]); let runs = 0;
  const handle = effect(() => { tensor(); runs++; });
  tensor.setAt(0, -0);
  expect(Object.is(tensor()[0], -0)).toBe(true);
  expect(runs).toBe(2);
  tensor.setAt(1, NaN);
  expect(runs).toBe(2);
  tensor.setAt(0, 1);
  tensor.setAt(0, 1 + 1e-12);
  expect(runs).toBe(3);
  handle.dispose(); tensor.dispose();
});

it('copies overlapping tensor views without overwriting unread source elements', () => {
  const tensor = createGpuTensor([1, 2, 3, 4]);
  tensor.write(tensor.view().subarray(0, 3), 1);
  expect(Array.from(tensor())).toEqual([1, 1, 2, 3]);
  tensor.dispose();
});

it('rejects a foreign parent before allocating child scene nodes', () => {
  const parent = createTransform3D();
  runWithIsolatedContext(() => {
    const before = getGraphTelemetry().nodes;
    expect(() => createTransform3D({ parent })).toThrow('different reactive context');
    expect(getGraphTelemetry().nodes).toBe(before);
  });
  parent.dispose();
});

it.each(['transform', 'camera'] as const)('rejects a foreign mesh %s without allocating geometry or reactive nodes', target => {
  const outsideTransform = createTransform3D(), outsideCamera = createPerspectiveCamera3D();
  runWithIsolatedContext(() => {
    const transform = createTransform3D(), camera = createPerspectiveCamera3D();
    const before = getGraphTelemetry().nodes, bytes = getGpuTelemetry().arenaByteLength;
    expect(() => createMesh3D({ positions: [1, 2, 3],
      transform: target === 'transform' ? outsideTransform : transform,
      camera: target === 'camera' ? outsideCamera : camera })).toThrow('different reactive context');
    expect(getGraphTelemetry().nodes).toBe(before); expect(getGpuTelemetry().arenaByteLength).toBe(bytes);
    camera.dispose(); transform.dispose();
  });
  outsideCamera.dispose(); outsideTransform.dispose();
});

it('rejects disposed scene dependencies before allocating child nodes', () => {
  const transform = createTransform3D(), camera = createPerspectiveCamera3D();
  transform.dispose(); camera.dispose(); const before = getGraphTelemetry().nodes;
  expect(() => createTransform3D({ parent: transform })).toThrow('disposed');
  expect(() => createMesh3D({ positions: [1, 2, 3], transform })).toThrow('disposed');
  const live = createTransform3D(), withLive = getGraphTelemetry().nodes;
  expect(() => createMesh3D({ positions: [1, 2, 3], transform: live, camera })).toThrow('disposed');
  expect(getGraphTelemetry().nodes).toBe(withLive); live.dispose(); expect(getGraphTelemetry().nodes).toBe(before);
});

it('captures scene dependency descriptors instead of following caller mutation', () => {
  const parent = createTransform3D({ position: [1, 0, 0] });
  const other = createTransform3D({ position: [100, 0, 0] });
  const transformOptions = { parent }, child = createTransform3D(transformOptions);
  transformOptions.parent = other; child.position.set([2, 0, 0]); flushEffects();
  expect(child.worldMatrix.peek()[12]).toBe(3);
  const expected = child.worldMatrix.peek()[12];
  const plainOptions = { positions: [1, 0, 0], transform: child }, plain = createMesh3D(plainOptions);
  plainOptions.transform = other; plain.vertices.write([2, 0, 0, 1]); flushEffects();
  expect(plain.transformedVertices.peek()[0]).toBe(2 + expected);
  plain.dispose(); child.dispose(); other.dispose(); parent.dispose();
});

it('retains the original mesh camera after the caller edits its descriptor', () => {
  const transform = createTransform3D(), camera = createPerspectiveCamera3D({ aspect: 1 });
  const other = createPerspectiveCamera3D({ aspect: 4 });
  const options = { positions: [1, 2, -3], transform, camera }, mesh = createMesh3D(options);
  const original = Array.from(mesh.mvpMatrix.peek()); options.camera = other;
  mesh.vertices.write([2, 2, -3, 1]); flushEffects();
  expect(Array.from(mesh.mvpMatrix.peek())).toEqual(original);
  camera.aspect.set(2); flushEffects();
  expect(mesh.mvpMatrix.peek()[0]).toBeCloseTo(original[0] / 2);
  mesh.dispose(); other.dispose(); camera.dispose(); transform.dispose();
});

it('does not partially publish a tensor write when an ArrayLike getter throws', () => {
  const tensor = createGpuTensor([1, 2, 3]); let runs = 0;
  const observer = effect(() => { tensor(); runs++; });
  const values = { length: 3, 0: 9, get 1(): number { throw new Error('source failed'); }, 2: 8 };
  expect(() => tensor.write(values)).toThrow('source failed');
  expect(Array.from(tensor.peek())).toEqual([1, 2, 3]); expect(runs).toBe(1);
  observer.dispose(); tensor.dispose();
});

it('reacquires tensor memory after source conversion grows WASM memory', () => {
  const core = getWasmCore(), tensor = createGpuTensor([1, 2]);
  const before = core.memory.buffer;
  tensor.write({ length: 2, get 0() { core.memory.grow(1); return 7; }, 1: 8 });
  expect(core.memory.buffer).not.toBe(before); expect(Array.from(tensor.peek())).toEqual([7, 8]); tensor.dispose();
});

it('rechecks lifetime after reentrant source conversion disposes and reuses the output', () => {
  const tensor = createGpuTensor([1, 2]); let replacement!: ReturnType<typeof createGpuTensor>;
  expect(() => tensor.write({ length: 2, get 0() { tensor.dispose(); replacement = createGpuTensor([90, 91]); return 7; }, 1: 8 })).toThrow('disposed');
  expect(Array.from(replacement.peek())).toEqual([90, 91]); replacement.dispose();
});

it('captures ArrayLike length once and only converts the portion that fits', () => {
  const tensor = createGpuTensor([1, 2, 3]); let lengths = 0;
  tensor.write({ get length() { lengths++; return 3; }, 0: 8,
    get 1(): number { throw new Error('outside write range'); }, 2: 9 }, 2);
  expect(lengths).toBe(1); expect(Array.from(tensor.peek())).toEqual([1, 2, 8]); tensor.dispose();
});

it('does not allocate tensor nodes or arena storage when initial source conversion throws', () => {
  const nodes = getGraphTelemetry().nodes, bytes = getGpuTelemetry().arenaByteLength;
  expect(() => createGpuTensor({ length: 2, 0: 1, get 1(): number { throw new Error('initial source failed'); } })).toThrow('initial source failed');
  expect(getGraphTelemetry().nodes).toBe(nodes); expect(getGpuTelemetry().arenaByteLength).toBe(bytes);
});

it('initializes from a source that grows WASM memory during conversion', () => {
  const core = getWasmCore();
  const tensor = createGpuTensor({ length: 2, get 0() { core.memory.grow(1); return 7; }, 1: 8 });
  expect(Array.from(tensor.peek())).toEqual([7, 8]); tensor.dispose();
});

it.each(['length', 'element'] as const)('rejects constructor reset during source %s access before allocating into the new generation', where => {
  const core = getWasmCore(); let replacement!: ReturnType<typeof createGpuTensor>;
  const reset = () => { resetReactiveSystem(); replacement = createGpuTensor([90, 91]); };
  const source = { get length() { if (where === 'length') reset(); return 2; },
    get 0() { if (where === 'element') reset(); return 7; }, 1: 8 };
  expect(() => createGpuTensor(source)).toThrow('reset');
  expect(Array.from(replacement.peek())).toEqual([90, 91]); expect(getGraphTelemetry().nodes).toBe(1);
  expect(getWasmCore()).toBe(core); replacement.dispose();
});

it('snapshots borrowed WASM input before allocating a growing destination arena', () => {
  const source = createGpuTensor(1024 * 1024);
  source.setAt(0, 7); source.setAt(source.length - 1, 8);
  const borrowed = source.peek(), oldBuffer = borrowed.buffer;
  const copy = createGpuTensor(borrowed);
  expect(getWasmCore().memory.buffer).not.toBe(oldBuffer);
  expect(copy.peek()[0]).toBe(7); expect(copy.peek()[copy.length - 1]).toBe(8);
  expect(source.peek()[0]).toBe(7); copy.dispose(); source.dispose();
});

it('synchronous host kernel failure leaves output and dependents unchanged', () => {
  const input = createGpuTensor([1, 2]), output = createGpuTensor([90, 91]); let runs = 0;
  const observer = effect(() => { output(); runs++; });
  expect(() => dispatchGpuComputeKernel({ wgsl: '', inputs: [input], output, fallbackKernel(_, values) {
    values[0] = 7; throw new Error('kernel failed');
  } })).toThrow('kernel failed');
  expect(Array.from(output.peek())).toEqual([90, 91]); expect(runs).toBe(1);
  observer.dispose(); output.dispose(); input.dispose();
});

it('synchronous kernels retain private input/output arrays across WASM memory growth', () => {
  const core = getWasmCore(), input = createGpuTensor([2, 3]), output = createGpuTensor([0, 0]);
  dispatchGpuComputeKernel({ wgsl: '', inputs: [input], output, fallbackKernel([values], result) {
    core.memory.grow(1); result.set(values.map(value => value * 2)); values[0] = 999;
  } });
  expect(Array.from(output.peek())).toEqual([4, 6]); expect(Array.from(input.peek())).toEqual([2, 3]);
  output.dispose(); input.dispose();
});

it('synchronous kernel disposal cannot overwrite a replacement tensor at the same offset', () => {
  const output = createGpuTensor([90]); let replacement!: ReturnType<typeof createGpuTensor>;
  expect(() => dispatchGpuComputeKernel({ wgsl: '', inputs: [], output, fallbackKernel(_, values) {
    output.dispose(); replacement = createGpuTensor([91]); values[0] = 7;
  } })).toThrow('output changed');
  expect(Array.from(replacement.peek())).toEqual([91]); replacement.dispose();
});

it('synchronous kernels cannot overwrite a newer reentrant write or redirect output via descriptor edits', () => {
  const output = createGpuTensor([0]), other = createGpuTensor([99]);
  const options = { wgsl: '', inputs: [], output, fallbackKernel(_: Float32Array[], values: Float32Array) { values[0] = 7; options.output = other; } };
  dispatchGpuComputeKernel(options);
  expect(Array.from(output.peek())).toEqual([7]); expect(Array.from(other.peek())).toEqual([99]);
  expect(() => dispatchGpuComputeKernel({ wgsl: '', inputs: [], output, fallbackKernel(_, values) { output.write([8]); values[0] = 9; } })).toThrow('output changed');
  expect(Array.from(output.peek())).toEqual([8]); output.dispose(); other.dispose();
});

it('rejects asynchronous callbacks passed to synchronous compute without publishing late mutations', async () => {
  const output = createGpuTensor([90]); let finish!: () => void;
  const resumed = new Promise<void>(resolve => { finish = resolve; });
  expect(() => dispatchGpuComputeKernel({ wgsl: '', inputs: [], output, async fallbackKernel(_, values) { values[0] = 7; await resumed; values[0] = 8; throw new Error('late failure'); } })).toThrow('asynchronous');
  finish(); await resumed; await Promise.resolve(); expect(Array.from(output.peek())).toEqual([90]); output.dispose();
});

it('rejects a synchronous kernel that resets the graph without touching replacement data', () => {
  const output = createGpuTensor([90]); let replacement!: ReturnType<typeof createGpuTensor>;
  expect(() => dispatchGpuComputeKernel({ wgsl: '', inputs: [], output, fallbackKernel(_, result) {
    resetReactiveSystem(); replacement = createGpuTensor([91]); result[0] = 7;
  } })).toThrow('reset');
  expect(Array.from(replacement.peek())).toEqual([91]); expect(getGraphTelemetry().nodes).toBe(1); replacement.dispose();
});

it('does not publish a synchronous kernel after its owner is disposed inside the callback', () => {
  const output = createGpuTensor([90]);
  createRoot(dispose => {
    expect(() => dispatchGpuComputeKernel({ wgsl: '', inputs: [], output, fallbackKernel(_, result) { dispose(); result[0] = 7; } })).toThrow('owner disposed');
  });
  expect(Array.from(output.peek())).toEqual([90]); output.dispose();
});

it('manual tensor disposal removes owner cleanup registrations during sustained churn', () => {
  createRoot(() => {
    const owner = getCurrentOwner()!, baseline = owner.disposables.length;
    const keep = createGpuTensor([90]);
    for (let i = 0; i < 1000; i++) { const tensor = createGpuTensor([i]); tensor.dispose(); tensor.dispose(); }
    expect(owner.disposables).toHaveLength(baseline + 1);
    expect(Array.from(keep.peek())).toEqual([90]); keep.dispose(); expect(owner.disposables).toHaveLength(baseline);
  });
});

it('scene replacement does not accumulate tensor cleanup closures on its long-lived owner', () => {
  createRoot(() => {
    const owner = getCurrentOwner()!, baseline = owner.disposables.length, nodes = getGraphTelemetry().nodes;
    for (let i = 0; i < 100; i++) {
      const transform = createTransform3D(), camera = createPerspectiveCamera3D();
      const mesh = createMesh3D({ positions: [1, 2, -3], transform, camera });
      mesh.dispose(); camera.dispose(); transform.dispose();
    }
    expect(owner.disposables).toHaveLength(baseline); expect(getGraphTelemetry().nodes).toBe(nodes);
  });
});

it('rejects tensor construction when initial source access disposes its owner', () => {
  createRoot(dispose => {
    const before = getGraphTelemetry().nodes, bytes = getGpuTelemetry().arenaByteLength;
    expect(() => createGpuTensor({ length: 1, get 0() { dispose(); return 7; } })).toThrow('owner disposed');
    expect(getGraphTelemetry().nodes).toBe(before); expect(getGpuTelemetry().arenaByteLength).toBe(bytes);
  });
});

it.each(['transform', 'camera', 'scene'] as const)('owner teardown releases all %s parameter nodes automatically', kind => {
  const before = getGraphTelemetry().nodes; let stop!: () => void;
  createRoot(dispose => {
    stop = dispose;
    if (kind === 'transform') createTransform3D();
    else if (kind === 'camera') createPerspectiveCamera3D();
    else {
      const transform = createTransform3D(), camera = createPerspectiveCamera3D();
      createMesh3D({ positions: [1, 2, -3], transform, camera });
    }
  });
  expect(getGraphTelemetry().nodes).toBeGreaterThan(before); stop(); stop(); expect(getGraphTelemetry().nodes).toBe(before);
});

it('scene owner teardown preserves borrowed parent transforms and cameras', () => {
  const parent = createTransform3D(), camera = createPerspectiveCamera3D(), before = getGraphTelemetry().nodes;
  let stop!: () => void;
  createRoot(dispose => {
    stop = dispose; const child = createTransform3D({ parent });
    createMesh3D({ positions: [1, 2, -3], transform: child, camera });
  });
  stop(); expect(getGraphTelemetry().nodes).toBe(before);
  parent.position.set([2, 0, 0]); camera.aspect.set(2); flushEffects();
  expect(parent.worldMatrix.peek()[12]).toBe(2); expect(camera.projectionMatrix.peek()[0]).toBeGreaterThan(0);
  camera.dispose(); parent.dispose();
});

it('effect-owned scene reconstruction keeps live node counts stable across reruns', () => {
  const source = createGpuTensor([0]); let runs = 0;
  const observer = effect(() => {
    source(); runs++;
    const transform = createTransform3D(), camera = createPerspectiveCamera3D();
    createMesh3D({ positions: [1, 2, -3], transform, camera });
  });
  const live = getGraphTelemetry().nodes;
  for (let i = 1; i <= 20; i++) { source.setAt(0, i); expect(getGraphTelemetry().nodes).toBe(live); }
  expect(runs).toBe(21); observer.dispose(); expect(getGraphTelemetry().nodes).toBe(1); source.dispose();
});

it('a disposed owner cannot mutate GPU configuration or its snapshot budget', () => {
  const gpu = device(); configureGpuSubsystem({device: gpu, architecture: 'DUMA', maxPendingComputeBytes: 100});
  const before = getGpuTelemetry();
  createRoot(stop => {
    stop();
    expect(() => configureGpuSubsystem({device: gpu, architecture: 'UMA', maxPendingComputeBytes: 0})).toThrow('owner disposed');
  });
  expect(getGpuTelemetry()).toEqual(before);
});

it('invalid upload architecture cannot partially change a shared compute budget', () => {
  const gpu = device(); configureGpuSubsystem({device: gpu, architecture: 'DUMA', maxPendingComputeBytes: 100});
  const before = getGpuTelemetry();
  expect(() => configureGpuSubsystem({device: gpu, architecture: 'invalid' as any, maxPendingComputeBytes: 0})).toThrow('architecture');
  expect(getGpuTelemetry()).toEqual(before);
});

it('throwing configuration getters cannot publish an earlier budget field', () => {
  const gpu = device(); configureGpuSubsystem({device: gpu, architecture: 'DUMA', maxPendingComputeBytes: 100});
  const before = getGpuTelemetry();
  expect(() => configureGpuSubsystem({device: gpu, maxPendingComputeBytes: 0, get architecture(): 'UMA' { throw new Error('configuration getter failed'); }})).toThrow('configuration getter failed');
  expect(getGpuTelemetry()).toEqual(before);
});

it('configuration getters that reset the graph cannot install stale device settings', () => {
  const gpu = device();
  expect(() => configureGpuSubsystem({device: gpu, get architecture(): 'UMA' { resetReactiveSystem(); return 'UMA'; }})).toThrow('reset');
  expect(getGpuTelemetry().webgpuAvailable).toBe(false);
});

it('reentrant configuration getters preserve the newer configuration', () => {
  const gpu = device();
  expect(() => configureGpuSubsystem({device: gpu, get architecture(): 'UMA' {
    configureGpuSubsystem({device: gpu, architecture: 'DUMA', maxPendingComputeBytes: 123}); return 'UMA';
  }})).toThrow('changed during configuration');
  expect(getGpuTelemetry().architecture).toBe('DUMA'); expect(getGpuTelemetry().maxPendingComputeBytes).toBe(123);
});

it('configuration reads each user-controlled field once', () => {
  const gpu = device(), reads = [0,0,0];
  configureGpuSubsystem({get device() { reads[0]++; return gpu; }, get architecture(): 'DUMA' { reads[1]++; return 'DUMA'; }, get maxPendingComputeBytes() { reads[2]++; return 100; }});
  expect(reads).toEqual([1,1,1]); expect(getGpuTelemetry().maxPendingComputeBytes).toBe(100);
});


it.each(['root stop', 'effect rerun'] as const)('retiring tensor methods reject reads and writes during %s', mode => {
  const trigger = signal(0); let stop!: () => void, tensor!: ReturnType<typeof createGpuTensor>;
  let borrowed!: Float32Array, observed = -1, inputReads = 0; const refused: boolean[] = [];
  const attempt = (fn: () => unknown) => { try { fn(); refused.push(false); } catch { refused.push(true); } };
  createRoot(dispose => {
    stop = dispose;
    const children = () => {
      runWithOwnerScope([], () => onCleanup(() => {
        attempt(() => tensor.setAt(0, 99));
        attempt(() => tensor.write({length: 1, get 0() { inputReads++; return 88; }}));
        attempt(() => tensor.view()); attempt(() => tensor()); observed = borrowed[0];
      }));
      runWithOwnerScope([], () => { tensor = createGpuTensor([7]); borrowed = tensor.view(); });
    };
    if (mode === 'root stop') children(); else effect(() => { if (trigger() === 0) children(); });
  });
  if (mode === 'root stop') stop(); else trigger.set(1);
  expect(refused).toEqual([true, true, true, true]); expect([inputReads, observed]).toEqual([0, 7]);
  stop(); trigger.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('retiring ambient owners cannot read GPU construction/configuration options or start adapter acquisition', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu, architecture: 'DUMA', maxPendingComputeBytes: 100});
  const before = getGpuTelemetry(); let reads = 0, adapterCalls = 0, unvisited = false;
  let later!: NonNullable<ReturnType<typeof getCurrentOwner>>, initialization!: Promise<unknown>;
  const refused: boolean[] = [];
  const attempt = (fn: () => unknown) => { try { fn(); refused.push(false); } catch { refused.push(true); } };
  vi.stubGlobal('navigator', {gpu: {requestAdapter() { adapterCalls++; return Promise.resolve(null); }}});
  try {
    createRoot(stop => {
      runWithOwnerScope([], () => onCleanup(() => {
        unvisited = !later.disposed && getCurrentOwner() === later;
        attempt(() => configureGpuSubsystem({get device() { reads++; return gpu; }, architecture: 'UMA'}));
        attempt(() => createGpuTensor({get length() { reads++; return 1; }, 0: 1}));
        attempt(() => createPerspectiveCamera3D({get fovYRad() { reads++; return 1; }}));
        initialization = initWebGpuHardware(); void initialization.catch(() => {});
      }));
      runWithOwnerScope([], () => { later = getCurrentOwner()!; stop(); });
    });
    expect(unvisited).toBe(true); expect(refused).toEqual([true, true, true]);
    expect([reads, adapterCalls]).toEqual([0, 0]);
    await expect(initialization).rejects.toThrow('owner disposed');
    expect(getGpuTelemetry()).toEqual(before); expect(getGraphTelemetry().nodes).toBe(0);
  } finally { vi.unstubAllGlobals(); configureGpuSubsystem({device: null}); }
});


it.each(['dispose', 'owner stop', 'reset'] as const)('tensor scalar conversion %s cannot overwrite replacement storage', mode => {
  let stop!: () => void, replacement!: ReturnType<typeof createGpuTensor>, conversions = 0;
  const old = createRoot(dispose => { stop = dispose; return createGpuTensor([7]); });
  const value = {valueOf() {
    conversions++;
    if (mode === 'dispose') old.dispose(); else if (mode === 'owner stop') stop(); else resetReactiveSystem();
    replacement = createGpuTensor([17]); return 99;
  }};
  expect(() => old.setAt(0, value as unknown as number)).toThrow(/disposed|invalidated/);
  expect(conversions).toBe(1); expect(Array.from(replacement.peek())).toEqual([17]);
  old.dispose(); stop(); expect(Array.from(replacement.peek())).toEqual([17]);
  expect(getGraphTelemetry().nodes).toBe(1); replacement.dispose();
});

it('tensor scalar conversion reacquires its view after WASM memory growth', () => {
  const core = getWasmCore(), tensor = createGpuTensor([7]), seen: number[] = [];
  const observer = effect(() => { seen.push(tensor()[0]); });
  tensor.setAt(0, {valueOf() { core.memory.grow(1); return 9; }} as unknown as number);
  expect(Array.from(tensor.peek())).toEqual([9]); expect(seen).toEqual([7, 9]);
  observer.dispose(); tensor.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it('failed standalone transform initialization releases ordinary signals and matrix tensors', () => {
  const failure = new Error('tuple iteration failed');
  const position = {[Symbol.iterator]() { throw failure; }} as unknown as [number, number, number];
  expect(() => createTransform3D({position})).toThrow(failure);
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('transform option reset cannot adopt fresh graph state', () => {
  let fresh!: ReturnType<typeof createGpuTensor>, laterReads = 0;
  expect(() => createTransform3D({
    get position(): [number, number, number] { resetReactiveSystem(); fresh = createGpuTensor([17]); return [0, 0, 0]; },
    get rotation(): [number, number, number] { laterReads++; return [0, 0, 0]; },
  })).toThrow(/reset|invalidated/);
  expect(laterReads).toBe(0); expect(Array.from(fresh.peek())).toEqual([17]);
  expect(getGraphTelemetry().nodes).toBe(1); fresh.dispose();
});

it.each(['stop', 'reset'] as const)('camera stops reading later options after %s', mode => {
  let laterReads = 0, fresh: ReturnType<typeof createGpuTensor> | undefined;
  createRoot(stop => {
    expect(() => createPerspectiveCamera3D({
      get fovYRad() { if (mode === 'stop') stop(); else { resetReactiveSystem(); fresh = createGpuTensor([17]); } return 1; },
      get aspect() { laterReads++; return 1; },
    })).toThrow(/disposed|invalidated|reset/);
    stop();
  });
  expect(laterReads).toBe(0);
  if (fresh) { expect(Array.from(fresh.peek())).toEqual([17]); fresh.dispose(); }
  expect(getGraphTelemetry().nodes).toBe(0);
});


it('transform numeric conversion reset cannot write matrices into replacement storage', () => {
  let fresh!: ReturnType<typeof createGpuTensor>, conversions = 0;
  const tx = {valueOf() { conversions++; resetReactiveSystem(); fresh = createGpuTensor(new Float32Array(32).fill(17)); return 1; }};
  expect(() => createTransform3D({position: [tx as unknown as number, 0, 0]})).toThrow(/reset|invalidated/);
  expect(conversions).toBe(1); expect(Array.from(fresh.peek())).toEqual(new Array(32).fill(17));
  expect(getGraphTelemetry().nodes).toBe(1); fresh.dispose();
});

it('owner stop from initial transform tuple iteration releases ordinary parameter signals', () => {
  createRoot(stop => {
    const position = {[Symbol.iterator]() { stop(); return [0, 0, 0][Symbol.iterator](); }} as unknown as [number, number, number];
    expect(() => createTransform3D({position})).toThrow(/disposed|invalidated/);
  });
  expect(getGraphTelemetry().nodes).toBe(0);
});


it('failed mesh producer initialization releases every unpublished tensor', () => {
  const transform = createTransform3D(), failure = new Error('matrix read failed');
  const matrix = new Proxy(transform.worldMatrix, {apply() { throw failure; }});
  const baseline = getGraphTelemetry().nodes;
  expect(() => createMesh3D({positions: [0, 0, 0], transform: {...transform, worldMatrix: matrix}})).toThrow(failure);
  expect(getGraphTelemetry().nodes).toBe(baseline); transform.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('mesh metadata reset stops before borrowing a replacement transform', () => {
  let transform!: ReturnType<typeof createTransform3D>, fresh!: ReturnType<typeof createGpuTensor>, baseline = 0, laterReads = 0;
  expect(() => createMesh3D({
    get positions() { resetReactiveSystem(); transform = createTransform3D(); fresh = createGpuTensor([17]); baseline = getGraphTelemetry().nodes; return [0, 0, 0]; },
    get transform() { laterReads++; return transform; },
  })).toThrow(/reset|invalidated/);
  expect(laterReads).toBe(0); expect(Array.from(fresh.peek())).toEqual([17]); expect(getGraphTelemetry().nodes).toBe(baseline);
  transform.dispose(); fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('mesh coordinate conversion reset stops later reads and preserves replacement storage', () => {
  const transform = createTransform3D(); let fresh!: ReturnType<typeof createGpuTensor>, laterReads = 0;
  const positions = {length: 3, get 0() { resetReactiveSystem(); fresh = createGpuTensor([17]); return 0; }, get 1() { laterReads++; return 0; }, 2: 0};
  expect(() => createMesh3D({positions, transform})).toThrow(/reset|invalidated/);
  expect(laterReads).toBe(0); expect(Array.from(fresh.peek())).toEqual([17]); expect(getGraphTelemetry().nodes).toBe(1);
  transform.dispose(); fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('mesh topology copy stops after disposal from an index getter', () => {
  const transform = createTransform3D(), mesh = createMesh3D({positions: [0,0,0, 1,0,0, 0,1,0], transform});
  let laterReads = 0; const baseline = getGraphTelemetry().nodes - 5;
  expect(() => mesh.updateIndices({length: 3, get 0() { mesh.dispose(); return 0; }, get 1() { laterReads++; return 1; }, 2: 2})).toThrow(/disposed|invalidated/);
  expect(laterReads).toBe(0); expect(getGraphTelemetry().nodes).toBe(baseline);
  transform.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it('mesh captures borrowed matrix descriptors once instead of following later redirects', () => {
  const first = createTransform3D({position: [1, 0, 0]}), other = createTransform3D({position: [100, 0, 0]});
  const camera = createPerspectiveCamera3D(), otherCamera = createPerspectiveCamera3D({aspect: 1});
  let world = first.worldMatrix, projection = camera.projectionMatrix, worldReads = 0, projectionReads = 0;
  const mesh = createMesh3D({positions: [0,0,-2],
    transform: {...first, get worldMatrix() { worldReads++; return world; }},
    camera: {...camera, get projectionMatrix() { projectionReads++; return projection; }},
  });
  const expected = Array.from(mesh.mvpMatrix.peek()); world = other.worldMatrix; projection = otherCamera.projectionMatrix;
  mesh.vertices.write([2,0,-2,1]);
  expect([worldReads, projectionReads]).toEqual([1, 1]); expect(Array.from(mesh.mvpMatrix.peek())).toEqual(expected);
  mesh.dispose(); first.dispose(); other.dispose(); camera.dispose(); otherCamera.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('mesh topology length reset preserves independent replacement nodes and skips index reads', () => {
  const transform = createTransform3D(), mesh = createMesh3D({positions: [0,0,0], transform});
  let fresh!: ReturnType<typeof createGpuTensor>, indexReads = 0;
  expect(() => mesh.updateIndices({get length() { resetReactiveSystem(); fresh = createGpuTensor([17]); return 3; },
    get 0() { indexReads++; return 0; }, 1: 0, 2: 0})).toThrow(/invalidated/);
  expect(indexReads).toBe(0); mesh.dispose(); transform.dispose(); expect(Array.from(fresh.peek())).toEqual([17]);
  expect(getGraphTelemetry().nodes).toBe(1); fresh.dispose();
});


it('mesh disposal releases storage even when a producer cleanup throws', () => {
  const transform = createTransform3D(), failure = new Error('mesh cleanup failed'), baseline = getGraphTelemetry().nodes; let cleanups = 0;
  const matrix = new Proxy(transform.worldMatrix, {apply(target, receiver, args) {
    onCleanup(() => { cleanups++; throw failure; }); return Reflect.apply(target, receiver, args);
  }});
  const mesh = createMesh3D({positions: [0,0,0], transform: {...transform, worldMatrix: matrix}});
  expect(() => mesh.dispose()).not.toThrow(); expect(cleanups).toBe(1); expect(getGraphTelemetry().nodes).toBe(baseline);
  expect(() => mesh.dispose()).not.toThrow(); transform.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('transform disposal releases every node when tuple-owned producer cleanup throws', () => {
  const failure = new Error('transform cleanup failed'); let cleanups = 0;
  const position = {[Symbol.iterator]() { onCleanup(() => { cleanups++; throw failure; }); return [0,0,0][Symbol.iterator](); }} as unknown as [number, number, number];
  const transform = createTransform3D({position});
  expect(() => transform.dispose()).not.toThrow(); expect(cleanups).toBe(1); expect(getGraphTelemetry().nodes).toBe(0);
  expect(() => transform.dispose()).not.toThrow();
});


it.each(['length', 'index'] as const)('new topology committed from the outer %s getter survives the older update', stage => {
  const transform = createTransform3D(), mesh = createMesh3D({positions: [0,0,0, 1,0,0, 0,1,0], transform});
  let laterReads = 0;
  const replace = () => mesh.updateIndices([2,1,0], 'uint32');
  const values = {get length() { if (stage === 'length') replace(); return 3; },
    get 0() { if (stage === 'index') replace(); return 0; }, get 1() { laterReads++; return 1; }, 2: 2};
  expect(() => mesh.updateIndices(values, 'uint16')).toThrow(/topology changed|superseded/);
  expect(laterReads).toBe(0); expect(mesh.indices).toEqual(new Uint32Array([2,1,0]));
  mesh.updateIndices([0,1,2]); expect(mesh.indices).toBeInstanceOf(Uint32Array);
  mesh.dispose(); transform.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('a failed nested topology candidate does not invalidate the valid outer update', () => {
  const transform = createTransform3D(), mesh = createMesh3D({positions: [0,0,0, 1,0,0, 0,1,0], transform});
  let rejected = false;
  mesh.updateIndices({length: 3, get 0() { try { mesh.updateIndices([0], 'uint32'); } catch (error) { rejected = error instanceof RangeError; } return 0; }, 1: 1, 2: 2});
  expect(rejected).toBe(true); expect(mesh.indices).toEqual(new Uint16Array([0,1,2]));
  mesh.dispose(); transform.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it('topology publication observers may commit a replacement without an older post-write overwrite', () => {
  const transform = createTransform3D(), mesh = createMesh3D({positions: [0,0,0, 1,0,0, 0,1,0], transform});
  let armed = true;
  const observer = effect(() => {
    const indices = mesh.indices;
    if (armed && indices?.[0] === 0) { armed = false; mesh.updateIndices([2,1,0], 'uint32'); }
  });
  expect(() => mesh.updateIndices([0,1,2], 'uint16')).not.toThrow();
  expect(mesh.indices).toEqual(new Uint32Array([2,1,0]));
  mesh.updateIndices([1,2,0]); expect(mesh.indices).toBeInstanceOf(Uint32Array);
  observer.dispose(); mesh.dispose(); transform.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it.each([Math.floor((67_108_864 - 16) / 8) + 1, 67_108_864 / 4 + 1, (Number.MAX_SAFE_INTEGER - 1) / 3])(
  'rejects impossible mesh storage for %i vertices before index reads or large snapshots', vertexCount => {
    const transform = createTransform3D(), baseline = getGraphTelemetry().nodes, arena = getGpuTelemetry().arenaFloatCount;
    const NativeFloat32Array = Float32Array; let largeAllocations = 0, indexReads = 0, coordinateReads = 0;
    // Fail before allocating if a regression reaches the large temporary copy.
    vi.stubGlobal('Float32Array', new Proxy(NativeFloat32Array, {construct(target, args, receiver) {
      if (typeof args[0] === 'number' && args[0] > 1_000_000) { largeAllocations++; throw new Error('large coordinate snapshot attempted'); }
      return Reflect.construct(target, args, receiver);
    }}));
    try {
      const positions = {length: vertexCount * 3, get 0(): number { coordinateReads++; throw new Error('coordinate read attempted'); }};
      const indices = {get length() { indexReads++; return 0; }};
      expect(() => createMesh3D({positions, indices, transform})).toThrow(/arena capacity/);
      expect([largeAllocations, indexReads, coordinateReads]).toEqual([0, 0, 0]);
      expect(getGraphTelemetry().nodes).toBe(baseline); expect(getGpuTelemetry().arenaFloatCount).toBe(arena);
    } finally { vi.unstubAllGlobals(); transform.dispose(); }
    expect(getGraphTelemetry().nodes).toBe(0);
  }
);
