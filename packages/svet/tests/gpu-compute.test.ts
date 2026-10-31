import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { runWithIsolatedContextAsync } from '../src/engine/server';
import { configureGpuSubsystem, GpuComputeCapacityError, createTransform3D, createPerspectiveCamera3D, createMesh3D, flushEffects, createGpuTensor, createRoot, getCurrentOwner, getWasmCore, getGraphTelemetry, initWebGpuHardware, dispatchGpuComputeKernelAsync, dispatchGpuComputeKernel, effect, signal, onCleanup, getGpuTelemetry, resetReactiveSystem } from '../src/engine';

const wgsl = '@group(0) @binding(0) var<storage, read> input: array<f32>; @group(0) @binding(1) var<storage, read_write> output: array<f32>; @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) { if (id.x < arrayLength(&output)) { output[id.x] = input[id.x] * 2.0 + 1.0; } }';
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
function gpu(options: { pauseMap?: boolean; pipelineError?: boolean; validationError?: boolean; allocationError?: boolean; scopeError?: string } = {}) {
  const lost = deferred<{ message: string }>(), mapping = deferred<void>(), release = deferred<void>();
  const filters: string[] = [];
  const buffers: any[] = []; let scopes = 0, maxScopes = 0, shader = '', groups: number[] = [];
  const device = {
    buffers, lost: lost.promise, lose: () => lost.resolve({ message: 'test loss' }), mapping: mapping.promise, release: () => release.resolve(),
    get scopes() { return scopes; }, get maxScopes() { return maxScopes; }, get shader() { return shader; }, get groups() { return groups; },
    pushErrorScope(filter: string) { filters.push(filter); scopes++; maxScopes = Math.max(scopes, maxScopes); },
    async popErrorScope() { scopes--; const filter = filters.pop(); return (options.validationError && filter === 'validation') || options.scopeError === filter ? { message: 'invalid binding' } : null; },
    createBuffer({ size }: { size: number }) {
      if (options.allocationError && buffers.length === 1) throw new Error('allocation failed');
      const buffer = { bytes: new Uint8Array(size), destroyed: false, mapped: false,
        async mapAsync() { mapping.resolve(); if (options.pauseMap) await release.promise; if (this.destroyed) throw new Error('buffer destroyed'); this.mapped = true; },
        getMappedRange() { if (this.destroyed || !this.mapped) throw new Error('invalid map'); return this.bytes.buffer; },
        unmap() { this.mapped = false; }, destroy() { this.destroyed = true; this.mapped = false; },
      }; buffers.push(buffer); return buffer;
    },
    createShaderModule({ code }: { code: string }) { shader = code; return { code }; },
    createComputePipeline() { if (options.pipelineError) throw new Error('pipeline failed'); return { getBindGroupLayout: () => ({}) }; },
    createBindGroup({ entries }: any) { return entries; },
    createCommandEncoder() {
      const commands: Array<() => void> = []; let bindings: any;
      return {
        beginComputePass() { return { setPipeline() {}, setBindGroup(_index: number, group: any) { bindings = group; },
          dispatchWorkgroups(...sizes: number[]) { groups = sizes; commands.push(() => {
            const input = new Float32Array(bindings[0].resource.buffer.bytes.buffer);
            const output = new Float32Array(bindings.at(-1).resource.buffer.bytes.buffer);
            for (let i = 0; i < output.length; i++) output[i] = input[i] * 2 + 1;
          }); }, end() {},
        }; },
        copyBufferToBuffer(source: any, offset: number, target: any, targetOffset: number, length: number) { commands.push(() => target.bytes.set(source.bytes.subarray(offset, offset + length), targetOffset)); },
        finish() { return commands; },
      };
    },
    queue: { writeBuffer(buffer: any, offset: number, data: any, dataOffset = 0, length = 0) {
      buffer.bytes.set(new Uint8Array(data, dataOffset, length), offset);
    }, submit(batches: any[]) { for (const batch of batches) for (const command of batch) command(); } },
  };
  return device;
}
beforeEach(() => { resetReactiveSystem(); configureGpuSubsystem({ device: null }); });
afterEach(() => vi.unstubAllGlobals());

it('submits a shader, publishes readback once, and destroys every temporary buffer', async () => {
  const device = gpu(); configureGpuSubsystem({ device });
  const input = createGpuTensor([1, 2, 3, 4]), output = createGpuTensor(4);
  const values: number[][] = []; const observer = effect(() => { values.push(Array.from(output())); });
  const result = await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
  expect(result).toEqual({ backend: 'webgpu', elements: 4, uploadedBytes: 32, readbackBytes: 16 });
  expect(values).toEqual([[0, 0, 0, 0], [3, 5, 7, 9]]);
  expect(device.shader).toBe(wgsl); expect(device.groups).toEqual([1, 1, 1]);
  expect(device.buffers).toHaveLength(3); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(device.scopes).toBe(0); observer.dispose();
});

it('uses explicit async fallback with input snapshots and no reported GPU traffic', async () => {
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, async fallbackKernel([values], result) {
    await Promise.resolve(); result[0] = values[0] * 2 + 1;
  } });
  input.setAt(0, 99);
  expect(await job).toEqual({ backend: 'fallback', elements: 1, uploadedBytes: 0, readbackBytes: 0 });
  expect(Array.from(output())).toEqual([3]);
});

it('rejects obsolete readback after a host write', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  output.setAt(0, 99); device.release();
  await expect(job).rejects.toThrow('output changed'); expect(Array.from(output())).toEqual([99]);
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
});

it('owner teardown cancels an active readback and frees buffers before rejection', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]); let stop!: () => void;
  const job = createRoot(dispose => { stop = dispose; return dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); });
  await device.mapping; const rejected = expect(job).rejects.toThrow('owner disposed'); stop(); await rejected;
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); expect(device.scopes).toBe(0);
  expect(Array.from(output())).toEqual([0]); device.release();
});

it('an AbortSignal cancels readback without changing the output', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const controller = new AbortController(), input = createGpuTensor([1]), output = createGpuTensor([0]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, signal: controller.signal });
  await device.mapping; const rejected = expect(job).rejects.toThrow('cancelled'); controller.abort(new Error('cancelled')); await rejected;
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); expect(Array.from(output())).toEqual([0]); device.release();
});

for (const failure of ['pipelineError', 'allocationError', 'validationError'] as const) it(`cleans resources and avoids publication after ${failure}`, async () => {
  const device = gpu({ [failure]: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow();
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); expect(device.scopes).toBe(0);
  expect(Array.from(output())).toEqual([0]);
});

it('device loss rejects active work and a replacement device can compute again', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  const rejected = expect(job).rejects.toThrow('GPU device lost'); device.lose(); await rejected;
  expect(getGpuTelemetry().webgpuAvailable).toBe(false); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  const replacement = gpu(); configureGpuSubsystem({ device: replacement });
  expect((await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).backend).toBe('webgpu');
  expect(Array.from(output())).toEqual([3]); device.release();
});

it('serializes error scopes for separate outputs sharing a device', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const a = createGpuTensor([1]), b = createGpuTensor([2]), outA = createGpuTensor([0]), outB = createGpuTensor([0]);
  const jobA = dispatchGpuComputeKernelAsync({ wgsl, inputs: [a], output: outA }); await device.mapping;
  const jobB = dispatchGpuComputeKernelAsync({ wgsl, inputs: [b], output: outB });
  expect(device.buffers).toHaveLength(3); device.release(); await Promise.all([jobA, jobB]);
  expect(device.maxScopes).toBe(3); expect(Array.from(outA())).toEqual([3]); expect(Array.from(outB())).toEqual([5]);
});

it('only the latest dispatch into one output can publish', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  input.setAt(0, 2); const rejected = expect(first).rejects.toThrow('output changed');
  const second = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); device.release();
  await rejected; await second; expect(Array.from(output())).toEqual([5]);
});

it('reacquires the output view if the WASM arena grows during readback', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  const extra = createGpuTensor(100000); device.release(); await job;
  expect(Array.from(output())).toEqual([3]); extra.dispose();
});

it('graph reset cancels pending work without waiting for the device mapping', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  const rejected = expect(job).rejects.toThrow('GPU context reset'); resetReactiveSystem(); await rejected;
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true); device.release();
});

it('an old lost device cannot invalidate its replacement or be reattached as live', async () => {
  const old = gpu(), current = gpu(); configureGpuSubsystem({ device: old }); configureGpuSubsystem({ device: current });
  old.lose(); await Promise.resolve();
  expect(getGpuTelemetry().webgpuAvailable).toBe(true);
  const input = createGpuTensor([2]), output = createGpuTensor([0]);
  await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
  expect(Array.from(output())).toEqual([5]);
  configureGpuSubsystem({ device: old });
  expect(getGpuTelemetry().webgpuAvailable).toBe(false);
  expect((await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, fallbackKernel([values], result) { result[0] = values[0]; } })).backend).toBe('fallback');
});

it('reset also cancels a pending asynchronous host fallback', async () => {
  const ready = deferred<void>(), release = deferred<void>();
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, async fallbackKernel([values], result) {
    ready.resolve(); await release.promise; result[0] = values[0];
  } });
  await ready.promise; const rejected = expect(job).rejects.toThrow('GPU context reset');
  resetReactiveSystem(); await rejected; release.resolve();
});

it('rejects invalid workgroups before allocating GPU resources', async () => {
  const device = gpu(); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  for (const count of [0, -1, 0.5, NaN, Infinity, 65536]) {
    await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, workgroups: [count] })).rejects.toThrow(RangeError);
  }
  expect(device.buffers).toHaveLength(0);
});

it('rejects pre-aborted work without invalidating an existing dispatch', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  const controller = new AbortController(); controller.abort(new Error('already cancelled'));
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, signal: controller.signal })).rejects.toThrow('already cancelled');
  device.release(); await first; expect(Array.from(output())).toEqual([3]);
});

it('cancels queued work before it allocates buffers or invokes a shader', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), firstOut = createGpuTensor([0]), secondOut = createGpuTensor([0]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output: firstOut }); await device.mapping;
  const controller = new AbortController();
  const second = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output: secondOut, signal: controller.signal });
  const rejected = expect(second).rejects.toThrow('queued cancellation'); controller.abort(new Error('queued cancellation')); await rejected;
  expect(device.buffers).toHaveLength(3); device.release(); await first;
  await Promise.resolve(); expect(device.buffers).toHaveLength(3); expect(Array.from(secondOut())).toEqual([0]);
});

it('reports shader diagnostics without invoking a fallback on a hardware error', async () => {
  const device = gpu(); device.createShaderModule = ({ code }) => ({ code,
    getCompilationInfo: async () => ({ messages: [{ type: 'error', message: 'invalid shader', lineNum: 2, linePos: 3 }] }),
  });
  configureGpuSubsystem({ device }); const input = createGpuTensor([1]), output = createGpuTensor([0]); let fallbacks = 0;
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, fallbackKernel() { fallbacks++; } })).rejects.toThrow('WGSL 2:3: invalid shader');
  expect(fallbacks).toBe(0); expect(Array.from(output())).toEqual([0]); expect(device.scopes).toBe(0);
});

it('invalid descriptors do not supersede a valid in-flight operation', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, workgroups: [0] })).rejects.toThrow(RangeError);
  device.release(); await first; expect(Array.from(output())).toEqual([3]);
});

it('removes completed operation cleanup registrations from their owner', async () => {
  const input = createGpuTensor([1]), output = createGpuTensor([0]); let owner!: NonNullable<ReturnType<typeof getCurrentOwner>>;
  const job = createRoot(() => {
    owner = getCurrentOwner()!;
    return dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, fallbackKernel([values], result) { result[0] = values[0]; } });
  });
  await job; expect(owner.disposables).toHaveLength(0);
});

it('hardware initialization retains its original owner across await and destroys its owned device', async () => {
  const device = { ...gpu(), destroy: vi.fn() };
  vi.stubGlobal('navigator', { gpu: { requestAdapter: async () => ({ info: {}, requestDevice: async () => device }) } });
  let stop!: () => void;
  const initialization = createRoot(dispose => { stop = dispose; return initWebGpuHardware(); });
  expect((await initialization).webgpuAvailable).toBe(true); stop();
  expect(device.destroy).toHaveBeenCalledOnce(); expect(getGpuTelemetry().webgpuAvailable).toBe(false);
});

it('a device arriving after owner disposal is destroyed without being configured', async () => {
  const ready = deferred<void>(), reply = deferred<ReturnType<typeof gpu> & { destroy(): void }>();
  const device = { ...gpu(), destroy: vi.fn() };
  vi.stubGlobal('navigator', { gpu: { requestAdapter: async () => ({ info: {}, requestDevice: () => { ready.resolve(); return reply.promise; } }) } });
  let stop!: () => void;
  const initialization = createRoot(dispose => { stop = dispose; return initWebGpuHardware(); });
  await ready.promise; const rejected = expect(initialization).rejects.toThrow('initialization cancelled'); stop(); reply.resolve(device); await rejected;
  expect(device.destroy).toHaveBeenCalledOnce(); expect(getGpuTelemetry().webgpuAvailable).toBe(false);
});

it('an explicit replacement configuration is not overwritten by a late initializer', async () => {
  const ready = deferred<void>(), reply = deferred<ReturnType<typeof gpu> & { destroy(): void }>();
  const late = { ...gpu(), destroy: vi.fn() }, current = gpu();
  vi.stubGlobal('navigator', { gpu: { requestAdapter: async () => ({ info: {}, requestDevice: () => { ready.resolve(); return reply.promise; } }) } });
  const initialization = initWebGpuHardware(); await ready.promise; configureGpuSubsystem({ device: current });
  const rejected = expect(initialization).rejects.toThrow('initialization cancelled'); reply.resolve(late); await rejected;
  expect(late.destroy).toHaveBeenCalledOnce(); expect(getGpuTelemetry().webgpuAvailable).toBe(true);
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); expect(Array.from(output())).toEqual([3]);
});

it('shared-device request subscriptions stay outside request context and scopes stay isolated', async () => {
  const device = gpu(), observed = deferred<ReturnType<typeof getWasmCore>>();
  const defaultCore = getWasmCore(), originalThen = device.lost.then.bind(device.lost);
  const subscription = vi.spyOn(device.lost, 'then').mockImplementation((fulfilled, rejected) => {
    queueMicrotask(() => observed.resolve(getWasmCore()));
    return originalThen(fulfilled, rejected);
  });
  const request = (value: number) => runWithIsolatedContextAsync(async () => {
    configureGpuSubsystem({ device }); const input = createGpuTensor([value]), output = createGpuTensor([0]);
    await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
    return Array.from(output());
  });
  expect(await Promise.all([request(1), request(10)])).toEqual([[3], [21]]);
  expect(await observed.promise).toBe(defaultCore);
  expect(subscription).toHaveBeenCalledOnce(); expect(device.maxScopes).toBe(3);
  expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(getGraphTelemetry().nodes).toBe(0); subscription.mockRestore();
});

it('a GPU-free fallback failure never partially publishes its working copy', async () => {
  const input = createGpuTensor([1]), output = createGpuTensor([99]);
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, async fallbackKernel(_, result) {
    result[0] = 2; await Promise.resolve(); throw new Error('fallback failed');
  } })).rejects.toThrow('fallback failed');
  expect(Array.from(output())).toEqual([99]);
});

it('reconfiguring a device keeps only the current owner cleanup registration', () => {
  const device = gpu(), tensor = createGpuTensor([1]); let owner!: NonNullable<ReturnType<typeof getCurrentOwner>>, stop!: () => void;
  createRoot(dispose => {
    stop = dispose; owner = getCurrentOwner()!;
    for (let i = 0; i < 100; i++) configureGpuSubsystem({ device, architecture: i % 2 ? 'UMA' : 'DUMA' });
  });
  expect(owner.disposables).toHaveLength(1); stop(); expect(getGpuTelemetry().webgpuAvailable).toBe(false); tensor.dispose();
});

it('captures the descriptor so caller mutation cannot redirect publication', async () => {
  const device = gpu(); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]), other = createGpuTensor([99]);
  const descriptor = { wgsl, inputs: [input], output };
  const job = dispatchGpuComputeKernelAsync(descriptor);
  descriptor.output = other; descriptor.wgsl = 'changed shader'; descriptor.inputs.length = 0;
  await job;
  expect(device.shader).toBe(wgsl); expect(Array.from(output())).toEqual([3]); expect(Array.from(other())).toEqual([99]);
});


it.each(['out-of-memory', 'internal'])('compute rejects %s errors without publishing readback or invoking fallback', async scopeError => {
  const options = { scopeError }, device = gpu(options); configureGpuSubsystem({ device });
  const input = createGpuTensor([2]), output = createGpuTensor([99]); let fallbacks = 0;
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, fallbackKernel() { fallbacks++; } })).rejects.toThrow(scopeError);
  expect(Array.from(output())).toEqual([99]); expect(fallbacks).toBe(0);
  expect(device.scopes).toBe(0); expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
  options.scopeError = ''; await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
  expect(Array.from(output())).toEqual([5]);
});

it('partial compute scope setup closes opened scopes without allocating buffers', async () => {
  const device = gpu(), push = device.pushErrorScope.bind(device);
  device.pushErrorScope = filter => { if (filter === 'internal') throw new Error('scope setup failed'); push(filter); };
  configureGpuSubsystem({ device }); const input = createGpuTensor([1]), output = createGpuTensor([99]);
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow('scope setup failed');
  expect(device.scopes).toBe(0); expect(device.buffers).toHaveLength(0); expect(Array.from(output())).toEqual([99]);
});

it('compute keeps readback private until every error scope succeeds', async () => {
  const device = gpu(), pop = device.popErrorScope.bind(device), pending = deferred<{ message: string } | null>(), popped = deferred<void>(); let calls = 0;
  device.popErrorScope = () => { const result = pop(); calls++; if (calls === 3) popped.resolve(); return calls === 2 ? pending.promise : result; };
  configureGpuSubsystem({ device }); const input = createGpuTensor([1]), output = createGpuTensor([99]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await popped.promise; await Promise.resolve();
  expect(device.scopes).toBe(0); expect(Array.from(output())).toEqual([99]);
  pending.resolve(null); await job; expect(Array.from(output())).toEqual([3]);
});

it('bounds device-wide pending snapshots before reading tensors or superseding accepted output', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([2]), output = createGpuTensor([99]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
  await device.mapping;
  const jobs = Array.from({ length: 31 }, () => dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output: createGpuTensor([0]) }));
  const peek = vi.spyOn(output, 'peek');
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow('queue is full');
  expect(peek).not.toHaveBeenCalled(); expect(device.buffers).toHaveLength(3);
  device.release(); await Promise.all([first, ...jobs]);
  expect(Array.from(output())).toEqual([5]);
  await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
});

it('cancelled queued snapshots keep admission credit until the device queue drains', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  const controller = new AbortController();
  const jobs = Array.from({ length: 31 }, () => dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output: createGpuTensor([0]), signal: controller.signal }));
  const settled = Promise.allSettled(jobs); controller.abort(new Error('cancel queued'));
  expect((await settled).every(result => result.status === 'rejected')).toBe(true);
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow('queue is full');
  device.release(); await first;
  // Give cancelled queue closures time to drain; they never allocate GPU buffers.
  for (let i = 0; i < 200; i++) await Promise.resolve();
  expect(device.buffers).toHaveLength(3);
  await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
  expect(Array.from(output())).toEqual([3]);
});

it('invalid descriptors and failed executions release compute admission', async () => {
  const options = { pipelineError: true }, device = gpu(options); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  for (let i = 0; i < 40; i++) {
    await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, workgroups: [0] })).rejects.toThrow('workgroup');
    await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow('pipeline failed');
  }
  options.pipelineError = false;
  await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
  expect(Array.from(output())).toEqual([3]);
});

it('shares admission across isolated contexts using one physical device', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]);
  const jobs = Array.from({ length: 32 }, () => dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output: createGpuTensor([0]) }));
  await device.mapping;
  await runWithIsolatedContextAsync(async () => {
    configureGpuSubsystem({ device });
    await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [createGpuTensor([2])], output: createGpuTensor([0]) })).rejects.toThrow('queue is full');
  });
  const other = gpu();
  await runWithIsolatedContextAsync(async () => {
    configureGpuSubsystem({ device: other }); const output = createGpuTensor([0]);
    await dispatchGpuComputeKernelAsync({ wgsl, inputs: [createGpuTensor([2])], output });
    expect(Array.from(output())).toEqual([5]);
  });
  device.release(); await Promise.all(jobs);
});

it('accounts exact snapshot bytes before copies and preserves accepted output on overload', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device, maxPendingComputeBytes: 16 });
  const input = createGpuTensor([2, 3]), output = createGpuTensor([99, 99]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  const peek = vi.spyOn(output, 'peek');
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow('byte budget');
  expect(peek).not.toHaveBeenCalled(); device.release(); await first;
  expect(Array.from(output())).toEqual([5, 7]);
  await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
});

it('lowering the shared byte budget preserves accepted work and blocks new admissions', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device, maxPendingComputeBytes: 16 });
  const input = createGpuTensor([1]), output = createGpuTensor([99]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  await runWithIsolatedContextAsync(async () => {
    configureGpuSubsystem({ device, maxPendingComputeBytes: 4 });
    await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [createGpuTensor([1])], output: createGpuTensor([0]) })).rejects.toThrow('byte budget');
  });
  device.release(); await first; expect(Array.from(output())).toEqual([3]);
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow('byte budget');
  configureGpuSubsystem({ maxPendingComputeBytes: 8 });
  await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
});

it('releases byte credits after descriptor and execution failures', async () => {
  const options = { pipelineError: true }, device = gpu(options);
  configureGpuSubsystem({ device, maxPendingComputeBytes: 8 });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, workgroups: [0] })).rejects.toThrow('workgroup');
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow('pipeline failed');
  options.pipelineError = false; await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output });
  expect(Array.from(output())).toEqual([3]);
});

it.each([-1, 0.5, Infinity, NaN, Number.MAX_SAFE_INTEGER + 1])('rejects invalid snapshot budget %s without replacing the device', async maxPendingComputeBytes => {
  const device = gpu(); configureGpuSubsystem({ device });
  expect(() => configureGpuSubsystem({ device: gpu(), maxPendingComputeBytes })).toThrow('safe integer');
  const output = createGpuTensor([0]); await dispatchGpuComputeKernelAsync({ wgsl, inputs: [createGpuTensor([1])], output });
  expect(device.buffers).toHaveLength(3); expect(Array.from(output())).toEqual([3]);
});

it('retains cancelled snapshot byte credits until queued closures drain', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device, maxPendingComputeBytes: 16 });
  const input = createGpuTensor([1]), firstOutput = createGpuTensor([0]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output: firstOutput }); await device.mapping;
  const controller = new AbortController(), output = createGpuTensor([0]);
  const queued = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, signal: controller.signal });
  const rejected = expect(queued).rejects.toThrow('cancel snapshot'); controller.abort(new Error('cancel snapshot')); await rejected;
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow('byte budget');
  device.release(); await first;
  for (let i = 0; i < 20; i++) await Promise.resolve();
  await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); expect(Array.from(output())).toEqual([3]);
});

it('supports a zero hardware snapshot budget and requires a device for configuration', async () => {
  expect(() => configureGpuSubsystem({ maxPendingComputeBytes: 0 })).toThrow('device is required');
  configureGpuSubsystem({ device: gpu(), maxPendingComputeBytes: 0 });
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [createGpuTensor([1])], output: createGpuTensor([0]) })).rejects.toThrow('byte budget');
});

for (const backend of ['webgpu', 'fallback'] as const) {
  it.each(['local', 'world', 'projection', 'mvp', 'vertices'] as const)(`${backend} cannot overwrite a newer internally produced %s tensor`, async target => {
    const device = gpu({ pauseMap: true });
    configureGpuSubsystem({ device: backend === 'webgpu' ? device : null });
    const parent = createTransform3D(), transform = createTransform3D({ parent });
    const camera = createPerspectiveCamera3D();
    const mesh = createMesh3D({ positions: [1, 2, -3], transform, camera });
    const output = { local: transform.localMatrix, world: transform.worldMatrix,
      projection: camera.projectionMatrix, mvp: mesh.mvpMatrix, vertices: mesh.transformedVertices }[target];
    const input = createGpuTensor(new Float32Array(output.length).fill(100));
    const entered = deferred<void>(), resume = deferred<void>();
    const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output,
      async fallbackKernel(_, values) { entered.resolve(); await resume.promise; values.fill(201); } });
    if (backend === 'webgpu') await device.mapping; else await entered.promise;
    if (target === 'projection') camera.aspect.set(2);
    else if (target === 'world') parent.position.set([2, 0, 0]);
    else if (target === 'vertices') mesh.vertices.write([4, 5, -6, 1]);
    else transform.position.set([2, 3, 4]);
    flushEffects(); const latest = Array.from(output.peek());
    const rejected = expect(job).rejects.toThrow('output changed');
    device.release(); resume.resolve(); await rejected;
    expect(Array.from(output.peek())).toEqual(latest);
    expect(device.buffers.every(buffer => buffer.destroyed)).toBe(true);
    mesh.dispose(); camera.dispose(); transform.dispose(); parent.dispose(); input.dispose();
  });
}

it('a failed ArrayLike write preserves both pending compute revision and live output', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1, 2]), output = createGpuTensor([99, 98]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  expect(() => output.write({ length: 2, 0: 7, get 1(): number { throw new Error('bad source'); } })).toThrow('bad source');
  expect(Array.from(output.peek())).toEqual([99, 98]);
  device.release(); await job; expect(Array.from(output.peek())).toEqual([3, 5]);
});

it('reports shared device admission snapshots without mutating earlier telemetry', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device, maxPendingComputeBytes: 24 });
  const input = createGpuTensor([1]), firstOutput = createGpuTensor([0]);
  const idle = getGpuTelemetry();
  expect(idle).toMatchObject({ pendingComputeCount: 0, pendingComputeSnapshotBytes: 0, maxPendingComputeCount: 32, maxPendingComputeBytes: 24 });
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output: firstOutput }); await device.mapping;
  const controller = new AbortController();
  const queued = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output: createGpuTensor([0]), signal: controller.signal });
  expect(getGpuTelemetry()).toMatchObject({ pendingComputeCount: 2, pendingComputeSnapshotBytes: 16 });
  await runWithIsolatedContextAsync(async () => {
    configureGpuSubsystem({ device });
    expect(getGpuTelemetry()).toMatchObject({ pendingComputeCount: 2, pendingComputeSnapshotBytes: 16, maxPendingComputeBytes: 24 });
  });
  const rejected = expect(queued).rejects.toThrow('telemetry cancellation'); controller.abort(new Error('telemetry cancellation')); await rejected;
  expect(getGpuTelemetry()).toMatchObject({ pendingComputeCount: 2, pendingComputeSnapshotBytes: 16 });
  expect(idle.pendingComputeCount).toBe(0); expect(idle.pendingComputeSnapshotBytes).toBe(0);
  device.release(); await first;
  for (let i = 0; i < 20; i++) await Promise.resolve();
  expect(getGpuTelemetry()).toMatchObject({ pendingComputeCount: 0, pendingComputeSnapshotBytes: 0 });
});

it('reports the selected device queue after switching devices while old work is draining', async () => {
  const oldDevice = gpu({ pauseMap: true }), replacement = gpu();
  configureGpuSubsystem({ device: oldDevice, maxPendingComputeBytes: 16 });
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [createGpuTensor([1])], output: createGpuTensor([0]) }); await oldDevice.mapping;
  expect(getGpuTelemetry().pendingComputeCount).toBe(1);
  const cancelled = expect(job).rejects.toThrow('configuration changed');
  configureGpuSubsystem({ device: replacement, maxPendingComputeBytes: 100 });
  expect(getGpuTelemetry()).toMatchObject({ pendingComputeCount: 0, pendingComputeSnapshotBytes: 0, maxPendingComputeBytes: 100 });
  await cancelled; oldDevice.release();
  configureGpuSubsystem({ device: oldDevice });
  expect(getGpuTelemetry()).toMatchObject({ pendingComputeCount: 0, pendingComputeSnapshotBytes: 0, maxPendingComputeBytes: 16 });
});

it('does not count host fallback execution as hardware queue or arena transfer traffic', async () => {
  const hold = deferred<void>(), entered = deferred<void>();
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const job = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, async fallbackKernel(_, values) { entered.resolve(); await hold.promise; values[0] = 3; } });
  await entered.promise;
  expect(getGpuTelemetry()).toMatchObject({ pendingComputeCount: 0, pendingComputeSnapshotBytes: 0, maxPendingComputeCount: 32, maxPendingComputeBytes: 256 * 1024 * 1024, totalBytesTransferred: 0 });
  hold.resolve(); await job; expect(Array.from(output.peek())).toEqual([3]);
});

it('returns admission telemetry to zero after rejected storage validation and execution', async () => {
  const device = gpu({ pipelineError: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output, workgroups: [0] })).rejects.toThrow('workgroup');
  expect(getGpuTelemetry()).toMatchObject({ pendingComputeCount: 0, pendingComputeSnapshotBytes: 0 });
  await expect(dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output })).rejects.toThrow('pipeline failed');
  expect(getGpuTelemetry()).toMatchObject({ pendingComputeCount: 0, pendingComputeSnapshotBytes: 0 });
});

it('a failing synchronous kernel does not supersede an already accepted asynchronous output', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]), output = createGpuTensor([90]);
  const accepted = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  expect(() => dispatchGpuComputeKernel({ wgsl, inputs: [input], output, fallbackKernel(_, result) { result[0] = 7; throw new Error('sync failed'); } })).toThrow('sync failed');
  expect(Array.from(output.peek())).toEqual([90]); device.release(); await accepted;
  expect(Array.from(output.peek())).toEqual([3]);
});

it('returns an immutable typed capacity error with exact byte demand and rejection-time occupancy', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device, maxPendingComputeBytes: 8 });
  const input = createGpuTensor([1]), output = createGpuTensor([90]);
  const first = dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }); await device.mapping;
  const error = await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }).catch(error => error);
  expect(error).toBeInstanceOf(GpuComputeCapacityError); expect(error).toBeInstanceOf(Error);
  expect(error.name).toBe('GpuComputeCapacityError'); expect(error.reason).toBe('snapshot-bytes');
  expect(error.requestedSnapshotBytes).toBe(8);
  expect(error.queue).toEqual({ pendingComputeCount: 1, pendingComputeSnapshotBytes: 8, maxPendingComputeCount: 32, maxPendingComputeBytes: 8 });
  expect(Object.isFrozen(error.queue)).toBe(true);
  configureGpuSubsystem({ maxPendingComputeBytes: 16 }); device.release(); await first;
  expect(getGpuTelemetry().pendingComputeCount).toBe(0); expect(error.queue.pendingComputeCount).toBe(1); expect(error.queue.maxPendingComputeBytes).toBe(8);
  expect(Array.from(output.peek())).toEqual([3]);
});

it('classifies the entry-count limit without inspecting tensor values or estimating unaccepted bytes', async () => {
  const device = gpu({ pauseMap: true }); configureGpuSubsystem({ device });
  const input = createGpuTensor([1]);
  const jobs = Array.from({ length: 32 }, () => dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output: createGpuTensor([0]) }));
  await device.mapping;
  const output = createGpuTensor([99]), peek = vi.spyOn(output, 'peek');
  const error = await dispatchGpuComputeKernelAsync({ wgsl, inputs: [input], output }).catch(error => error);
  expect(error).toBeInstanceOf(GpuComputeCapacityError); expect(error.reason).toBe('count');
  expect(error.requestedSnapshotBytes).toBeNull(); expect(error.queue.pendingComputeCount).toBe(32);
  expect(error.queue.pendingComputeSnapshotBytes).toBe(256); expect(peek).not.toHaveBeenCalled();
  device.release(); await Promise.all(jobs);
});

it.each(['pipeline', 'descriptor', 'fallback'] as const)('does not misclassify a %s failure as queue capacity', async failure => {
  configureGpuSubsystem({ device: failure === 'fallback' ? null : gpu({ pipelineError: true }) });
  const error = await dispatchGpuComputeKernelAsync({ wgsl, inputs: [createGpuTensor([1])], output: createGpuTensor([0]),
    ...(failure === 'descriptor' ? { workgroups: [0] as [number] } : {}),
    fallbackKernel() { throw new Error('fallback failed'); },
  }).catch(error => error);
  expect(error).toBeInstanceOf(Error); expect(error).not.toBeInstanceOf(GpuComputeCapacityError);
  expect(error.message).toMatch(failure === 'pipeline' ? /pipeline failed/ : failure === 'descriptor' ? /workgroup/ : /fallback failed/);
});

it('initialization from a disposed owner makes no adapter request', async () => {
  const requestAdapter = vi.fn(async () => null);
  vi.stubGlobal('navigator', {gpu: {requestAdapter}});
  const job = createRoot(stop => { stop(); return initWebGpuHardware(); });
  await expect(job).rejects.toThrow('owner disposed');
  expect(requestAdapter).not.toHaveBeenCalled();
});

it('a disposed-owner initializer cannot supersede a valid pending initialization', async () => {
  const reply = deferred<any>(), device = {...gpu(), destroy: vi.fn()};
  const requestAdapter = vi.fn(() => reply.promise);
  vi.stubGlobal('navigator', {gpu: {requestAdapter}});
  const live = initWebGpuHardware();
  void live.catch(() => {});
  const stale = createRoot(stop => { stop(); return initWebGpuHardware(); });
  reply.resolve({info: {}, requestDevice: async () => device});
  await expect(stale).rejects.toThrow('owner disposed');
  expect((await live).webgpuAvailable).toBe(true);
  expect(requestAdapter).toHaveBeenCalledOnce(); expect(device.destroy).not.toHaveBeenCalled();
  configureGpuSubsystem({device: null}); expect(device.destroy).toHaveBeenCalledOnce();
});

it('owner disposal while awaiting an adapter prevents requesting its device', async () => {
  const reply = deferred<any>(), requestDevice = vi.fn(async () => gpu());
  vi.stubGlobal('navigator', {gpu: {requestAdapter: () => reply.promise}});
  let stop!: () => void;
  const job = createRoot(dispose => { stop = dispose; return initWebGpuHardware(); });
  const rejected = expect(job).rejects.toThrow('initialization cancelled'); stop();
  reply.resolve({info: {}, requestDevice}); await rejected;
  expect(requestDevice).not.toHaveBeenCalled(); expect(getGpuTelemetry().webgpuAvailable).toBe(false);
});

it('owner disposal while awaiting adapter information prevents device allocation', async () => {
  const reply = deferred<any>(), ready = deferred<void>(), requestDevice = vi.fn(async () => gpu());
  vi.stubGlobal('navigator', {gpu: {requestAdapter: async () => ({requestDevice, requestAdapterInfo() { ready.resolve(); return reply.promise; }})}});
  let stop!: () => void;
  const job = createRoot(dispose => { stop = dispose; return initWebGpuHardware(); });
  await ready.promise; const rejected = expect(job).rejects.toThrow('initialization cancelled'); stop();
  reply.resolve({vendor: 'Apple'}); await rejected; expect(requestDevice).not.toHaveBeenCalled();
});

it('late device cleanup failures cannot replace initialization cancellation', async () => {
  const ready = deferred<void>(), reply = deferred<any>();
  const late = {...gpu(), destroy: vi.fn(() => { throw new Error('device teardown failed'); })};
  vi.stubGlobal('navigator', {gpu: {requestAdapter: async () => ({info: {}, requestDevice() { ready.resolve(); return reply.promise; }})}});
  let stop!: () => void;
  const job = createRoot(dispose => { stop = dispose; return initWebGpuHardware(); });
  await ready.promise; const rejected = expect(job).rejects.toThrow('initialization cancelled'); stop(); reply.resolve(late); await rejected;
  expect(late.destroy).toHaveBeenCalledOnce(); expect(getGpuTelemetry().webgpuAvailable).toBe(false);
});

it('invalid allocated device cleanup failures do not prevent the fallback state', async () => {
  const invalid = {queue: {}, destroy: vi.fn(() => { throw new Error('invalid device teardown failed'); })};
  configureGpuSubsystem({device: gpu()});
  vi.stubGlobal('navigator', {gpu: {requestAdapter: async () => ({info: {}, requestDevice: async () => invalid})}});
  expect((await initWebGpuHardware()).webgpuAvailable).toBe(false);
  expect(invalid.destroy).toHaveBeenCalledOnce();
});

it('an already lost returned device is released once even when destroy throws', async () => {
  const lostDevice = {...gpu(), destroy: vi.fn(() => { throw new Error('lost device teardown failed'); })};
  configureGpuSubsystem({device: lostDevice}); lostDevice.lose();
  await Promise.resolve(); await Promise.resolve();
  expect(getGpuTelemetry().webgpuAvailable).toBe(false);
  vi.stubGlobal('navigator', {gpu: {requestAdapter: async () => ({info: {}, requestDevice: async () => lostDevice})}});
  expect((await initWebGpuHardware()).webgpuAvailable).toBe(false);
  expect(lostDevice.destroy).toHaveBeenCalledOnce();
});


it.each(['sync', 'async'] as const)('%s compute rejects reset from an option getter without adopting replacement output', async mode => {
  let output!: ReturnType<typeof createGpuTensor>, calls = 0, outputReads = 0;
  const options = {
    get wgsl() { resetReactiveSystem(); output = createGpuTensor([17]); return ''; },
    inputs: [], get output() { outputReads++; return output; },
    fallbackKernel(_: Float32Array[], values: Float32Array) { calls++; values[0] = 99; },
  };
  if (mode === 'sync') expect(() => dispatchGpuComputeKernel(options)).toThrow(/reset/);
  else await expect(dispatchGpuComputeKernelAsync(options)).rejects.toThrow(/reset/);
  expect([calls, outputReads]).toEqual([0, 0]); expect(Array.from(output.peek())).toEqual([17]);
  expect(getGraphTelemetry().nodes).toBe(1); output.dispose();
});

it.each(['sync', 'async'] as const)('%s compute reads its input getter once', async mode => {
  const output = createGpuTensor([0]); let reads = 0;
  const options = {wgsl: '', get inputs() { reads++; return []; }, output, fallbackKernel(_: Float32Array[], values: Float32Array) { values[0] = 7; }};
  if (mode === 'sync') dispatchGpuComputeKernel(options); else await dispatchGpuComputeKernelAsync(options);
  expect(reads).toBe(1); expect(Array.from(output.peek())).toEqual([7]); output.dispose();
});


it.each(['sync', 'async'] as const)('%s compute preflights disposed owners before metadata traps', async mode => {
  let traps = 0, job: Promise<unknown> | undefined, failure: unknown;
  const options = new Proxy({wgsl: '', inputs: [], output: undefined!, fallbackKernel() {}}, {ownKeys(target) { traps++; return Reflect.ownKeys(target); }});
  createRoot(stop => {
    stop();
    if (mode === 'sync') { try { dispatchGpuComputeKernel(options); } catch (error) { failure = error; } }
    else { job = dispatchGpuComputeKernelAsync(options); void job.catch(() => {}); }
  });
  if (job) await expect(job).rejects.toThrow('owner disposed'); else expect(String(failure)).toContain('owner disposed');
  expect(traps).toBe(0); expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['sync', 'async'] as const)('%s compute closes an invalidated input iterator before requesting another item', async mode => {
  const old = createGpuTensor([0]); let fresh!: ReturnType<typeof createGpuTensor>, steps = 0, closed = 0, calls = 0;
  const inputs: ReturnType<typeof createGpuTensor>[] = [];
  Object.defineProperty(inputs, Symbol.iterator, {value: () => ({
    next() {
      steps++; if (steps > 1) return {done: true, value: undefined};
      resetReactiveSystem(); fresh = createGpuTensor([17]); return {done: false, value: fresh};
    },
    return() { closed++; return {done: true, value: undefined}; },
  })});
  const options = {wgsl: '', inputs, output: old, fallbackKernel() { calls++; }};
  if (mode === 'sync') expect(() => dispatchGpuComputeKernel(options)).toThrow('reset');
  else await expect(dispatchGpuComputeKernelAsync(options)).rejects.toThrow('reset');
  expect([steps, closed, calls]).toEqual([1, 1, 0]); expect(Array.from(fresh.peek())).toEqual([17]);
  old.dispose(); fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

for (const inherited of [false, true]) it.each(['sync', 'async'] as const)(`%s compute accepts ${inherited ? 'inherited' : 'nonenumerable'} inputs with one read`, async mode => {
  let reads = 0; const input = createGpuTensor([3]), output = createGpuTensor([0]);
  const holder = Object.defineProperty({}, 'inputs', {get() { reads++; return [input]; }});
  const options = inherited ? Object.create(holder) : holder;
  Object.assign(options, {wgsl: '', output, fallbackKernel([values]: Float32Array[], result: Float32Array) { result[0] = values[0] * 2; }});
  if (mode === 'sync') dispatchGpuComputeKernel(options); else await dispatchGpuComputeKernelAsync(options);
  expect(reads).toBe(1); expect(Array.from(output.peek())).toEqual([6]);
  input.dispose(); output.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it('synchronous fallback reads stay untracked while input tensors remain reactive', () => {
  const input = createGpuTensor([2]), output = createGpuTensor([0]), incidental = signal(0); let calls = 0;
  const observer = effect(() => dispatchGpuComputeKernel({wgsl: '', inputs: [input], output, fallbackKernel([values], result) {
    calls++; incidental(); result[0] = values[0] * 2;
  }}));
  incidental.set(1); expect(calls).toBe(1); input.setAt(0, 3); expect(calls).toBe(2);
  expect(Array.from(output.peek())).toEqual([6]);
  observer.dispose(); input.dispose(); output.dispose(); incidental.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('deferred fallback restores its owned core and owner for synchronous resources', async () => {
  const {createWasmCoreInstance, getWasmCore, withWasmCore} = await import('../src/engine/wasm-bridge');
  const own = createWasmCoreInstance(); let stop!: () => void, seenCore: unknown, runs = 0, cleanups = 0;
  const input = withWasmCore(own, () => createGpuTensor([2]));
  const output = withWasmCore(own, () => createGpuTensor([0]));
  const source = withWasmCore(own, () => signal(0));
  const job = withWasmCore(own, () => createRoot(dispose => {
    stop = dispose;
    return dispatchGpuComputeKernelAsync({wgsl: '', inputs: [input], output, fallbackKernel([values], result) {
      seenCore = getWasmCore(); output.peek();
      effect(() => { source(); runs++; }); onCleanup(() => { cleanups++; });
      result[0] = values[0] * 2;
    }});
  }));
  expect(await job).toMatchObject({backend: 'fallback'}); expect(seenCore).toBe(own);
  expect(withWasmCore(own, () => Array.from(output.peek()))).toEqual([4]);
  source.set(1); expect(runs).toBe(2); stop(); source.set(2); expect([runs, cleanups]).toEqual([2, 1]);
  input.dispose(); output.dispose(); source.dispose(); expect(withWasmCore(own, () => getGraphTelemetry().nodes)).toBe(0);
  expect(getGraphTelemetry().nodes).toBe(0);
});
