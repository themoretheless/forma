import { beforeEach, expect, it, vi } from 'vitest';
import { configureGpuSubsystem, createGpuTensor, createRoot, createWasmCoreInstance,
  dispatchGpuComputeKernelAsync, dispatchGpuComputeResident, effect, getGpuTelemetry,
  getWasmCore, GpuResidentCapacityError, readGpuResidentTensor, resetReactiveSystem,
  withWasmCore } from '../src/engine';
import { device } from './helpers/resident-device';
const wgsl = '@compute @workgroup_size(64) fn main() {}';
beforeEach(() => { resetReactiveSystem(); configureGpuSubsystem({device: null}); });

it('keeps chained and branching results resident until one explicit readback', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu});
  const writes = vi.spyOn(gpu.queue, 'writeBuffer'), submissions = vi.spyOn(gpu.queue, 'submit');
  const input = createGpuTensor([1,2]), output = createGpuTensor([99,99]); let runs = 0;
  const observer = effect(() => { output(); runs++; });
  const nodes = getWasmCore().svet_get_node_count();
  const first = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 2});
  const left = await dispatchGpuComputeResident({wgsl, inputs: [first], outputLength: 2});
  const right = await dispatchGpuComputeResident({wgsl, inputs: [first], outputLength: 2});
  const joined = await dispatchGpuComputeResident({wgsl, inputs: [left, right], outputLength: 2});
  expect(getWasmCore().svet_get_node_count()).toBe(nodes);
  expect(writes).toHaveBeenCalledTimes(1); expect(first.uploadedBytes).toBe(8); expect(joined.uploadedBytes).toBe(0);
  expect(gpu.buffers.reduce((sum, buffer) => sum + buffer.maps, 0)).toBe(0); expect(runs).toBe(1);
  expect(getGpuTelemetry()).toMatchObject({residentComputeBytes: 32, residentComputeCount: 4, pendingComputeCount: 0});
  const report = await readGpuResidentTensor(joined, output);
  expect(Array.from(output())).toEqual([42,58]); expect(runs).toBe(2);
  expect(report).toEqual({backend: 'webgpu', elements: 2, uploadedBytes: 0, readbackBytes: 8});
  expect(submissions).toHaveBeenCalledTimes(5);
  for (const handle of [first,left,right,joined]) { handle.dispose(); handle.dispose(); expect(handle.active).toBe(false); }
  expect(gpu.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(getGpuTelemetry()).toMatchObject({residentComputeBytes: 0, residentComputeCount: 0});
  observer.dispose(); input.dispose(); output.dispose();
});

it('captures host inputs and descriptor metadata before queue execution', async () => {
  const gpu = device(); gpu.pauseDone = true; configureGpuSubsystem({device: gpu});
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const first = dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1}); await gpu.started.promise;
  const options = {wgsl, inputs: [input], outputLength: 1}; const second = dispatchGpuComputeResident(options);
  input.setAt(0, 100); options.outputLength = 20; options.inputs.length = 0;
  gpu.pauseDone = false; gpu.done.resolve(); const a = await first, b = await second;
  await readGpuResidentTensor(b, output); expect(Array.from(output())).toEqual([4]); a.dispose(); b.dispose();
});

it('separates immutable resident layouts from auto-layout mapped pipeline entries', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu}); const compile = vi.spyOn(gpu, 'createComputePipeline');
  const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const handle = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1});
  await dispatchGpuComputeKernelAsync({wgsl, inputs: [input], output}); expect(compile).toHaveBeenCalledTimes(2);
  const next = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1});
  expect(compile).toHaveBeenCalledTimes(2); next.dispose();
  handle.dispose(); expect(gpu.filters).toHaveLength(0);
});

it('reserves a shared device budget before allocating and releases it on disposal', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu, maxResidentComputeBytes: 8});
  const input = createGpuTensor([1]); const handle = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 2});
  const before = gpu.buffers.length;
  const otherCore = createWasmCoreInstance();
  await withWasmCore(otherCore, async () => {
    configureGpuSubsystem({device: gpu}); const other = createGpuTensor([1]);
    await expect(dispatchGpuComputeResident({wgsl, inputs: [other], outputLength: 1})).rejects.toBeInstanceOf(GpuResidentCapacityError);
    expect(gpu.buffers).toHaveLength(before); expect(getGpuTelemetry().residentComputeBytes).toBe(8); other.dispose();
  });
  handle.dispose(); expect(getGpuTelemetry().residentComputeBytes).toBe(0);
  const next = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1}); next.dispose();
});

it.each([-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1])('rejects invalid resident budget %s atomically', maxResidentComputeBytes => {
  const gpu = device(); configureGpuSubsystem({device: gpu, maxPendingComputeBytes: 16});
  expect(() => configureGpuSubsystem({device: gpu, maxPendingComputeBytes: 1, maxResidentComputeBytes})).toThrow('maxResidentComputeBytes');
  expect(getGpuTelemetry().maxPendingComputeBytes).toBe(16);
});

it.each([0, -1, 0.5, Infinity, NaN, Number.MAX_SAFE_INTEGER])('rejects invalid output length %s before allocation', async outputLength => {
  const gpu = device(); configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]);
  await expect(dispatchGpuComputeResident({wgsl, inputs: [input], outputLength})).rejects.toThrow(/outputLength|binding/);
  expect(gpu.buffers).toHaveLength(0); expect(getGpuTelemetry().residentComputeBytes).toBe(0);
});

it('keeps cancellation credits until a queued closure drains, then returns both budgets', async () => {
  const gpu = device(); gpu.pauseDone = true; configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]);
  const first = dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1}); await gpu.started.promise;
  const abort = new AbortController();
  const pending = dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1, signal: abort.signal});
  const rejected = expect(pending).rejects.toThrow('cancel queued'); abort.abort(Error('cancel queued')); await rejected;
  expect(getGpuTelemetry()).toMatchObject({pendingComputeCount: 2, residentComputeBytes: 8});
  gpu.pauseDone = false; gpu.done.resolve(); const handle = await first;
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(getGpuTelemetry()).toMatchObject({pendingComputeCount: 0, residentComputeBytes: 4});
  handle.dispose(); expect(getGpuTelemetry().residentComputeBytes).toBe(0);
});

it('frees all private allocations on validation failure and allows recovery', async () => {
  const gpu = device(); gpu.validationError = true; configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]);
  await expect(dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1})).rejects.toThrow('invalid kernel');
  expect(gpu.buffers.every(buffer => buffer.destroyed)).toBe(true);
  expect(getGpuTelemetry()).toMatchObject({pendingComputeCount: 0, residentComputeBytes: 0});
  gpu.validationError = false; const next = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1}); next.dispose();
});

it.each(['owner', 'reset', 'detach', 'loss'] as const)('retires resident buffers on %s', async mode => {
  const gpu = device(); configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]); let stop!: () => void;
  const pending = createRoot(dispose => { stop = dispose; return dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1}); });
  const handle = await pending;
  if (mode === 'owner') stop(); else if (mode === 'reset') resetReactiveSystem();
  else if (mode === 'detach') configureGpuSubsystem({device: null}); else { gpu.lose(); await Promise.resolve(); await Promise.resolve(); }
  expect(handle.active).toBe(false); expect(gpu.buffers.every(buffer => buffer.destroyed)).toBe(true);
  if (mode !== 'reset') stop();
});

it('does not invalidate an independent result when its already-consumed input is disposed', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]), output = createGpuTensor([0]);
  const first = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1});
  const second = await dispatchGpuComputeResident({wgsl, inputs: [first], outputLength: 1}); first.dispose();
  expect(second.active).toBe(true); await readGpuResidentTensor(second, output); expect(Array.from(output())).toEqual([10]); second.dispose();
});

it('rejects disposed/foreign handles and size mismatch without changing host output', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]), output = createGpuTensor([9,9]);
  const handle = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1});
  await expect(readGpuResidentTensor(handle, output)).rejects.toThrow('length mismatch');
  await withWasmCore(createWasmCoreInstance(), async () => {
    configureGpuSubsystem({device: gpu});
    await expect(dispatchGpuComputeResident({wgsl, inputs: [handle], outputLength: 1})).rejects.toThrow('another context');
  });
  handle.dispose(); await expect(dispatchGpuComputeResident({wgsl, inputs: [handle], outputLength: 1})).rejects.toThrow('disposed');
  expect(Array.from(output())).toEqual([9,9]);
});

it('rejects host writes during mapped readback without publishing stale values', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]), output = createGpuTensor([9]);
  const handle = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1}); gpu.pauseMap = true;
  const pending = readGpuResidentTensor(handle, output); await gpu.mapping.promise; output.setAt(0, 17); gpu.mapped.resolve();
  await expect(pending).rejects.toThrow('output changed'); expect(Array.from(output())).toEqual([17]);
  expect(gpu.buffers.at(-1).destroyed).toBe(true); handle.dispose();
});

it('cancels readback on source disposal and destroys staging without host publication', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]), output = createGpuTensor([9]);
  const handle = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1}); gpu.pauseMap = true;
  const pending = readGpuResidentTensor(handle, output); await gpu.mapping.promise;
  const rejected = expect(pending).rejects.toThrow('disposed'); handle.dispose(); await rejected;
  expect(Array.from(output())).toEqual([9]); expect(gpu.buffers.every(buffer => buffer.destroyed)).toBe(true); gpu.mapped.resolve();
});

it('requires actual hardware support without a silent host fallback', async () => {
  await expect(dispatchGpuComputeResident({wgsl, inputs: [], outputLength: 1})).rejects.toThrow('complete WebGPU device');
  expect(getGpuTelemetry().residentComputeBytes).toBe(0);
});

it('resetting one core preserves another core resident on the shared device', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]);
  const first = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1});
  const other = createWasmCoreInstance();
  const second = await withWasmCore(other, () => {
    configureGpuSubsystem({device: gpu}); const source = createGpuTensor([2]);
    return dispatchGpuComputeResident({wgsl, inputs: [source], outputLength: 1});
  });
  resetReactiveSystem(); expect(first.active).toBe(false); expect(second.active).toBe(true);
  expect(getGpuTelemetry().residentComputeBytes).toBe(4);
  const output = withWasmCore(other, () => createGpuTensor([0]));
  await withWasmCore(other, () => readGpuResidentTensor(second, output));
  withWasmCore(other, () => { expect(Array.from(output())).toEqual([6]); output.dispose(); });
  second.dispose(); withWasmCore(other, () => resetReactiveSystem());
  expect(getGpuTelemetry().residentComputeBytes).toBe(0);
});

it('does not use a device replaced from a capability getter', async () => {
  const gpu = device(), replacement = device(); configureGpuSubsystem({device: gpu});
  const create = gpu.createBuffer.bind(gpu);
  Object.defineProperty(gpu, 'createBuffer', {get() { configureGpuSubsystem({device: replacement}); return create; }});
  await expect(dispatchGpuComputeResident({wgsl, inputs: [], outputLength: 1})).rejects.toThrow(/configuration changed/);
  expect(gpu.buffers).toHaveLength(0); expect(replacement.buffers).toHaveLength(0);
});

it('caller abort after successful publication leaves the owned result valid', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu}); const input = createGpuTensor([1]), abort = new AbortController();
  const handle = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1, signal: abort.signal});
  abort.abort(); expect(handle.active).toBe(true); handle.dispose();
});

it('enforces the 1024-result device count boundary and returns every reservation', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu});
  const handles: Awaited<ReturnType<typeof dispatchGpuComputeResident>>[] = [];
  try {
    for (let i = 0; i < 1024; i++) handles.push(await dispatchGpuComputeResident({wgsl, inputs: [], outputLength: 1}));
    expect(getGpuTelemetry()).toMatchObject({residentComputeCount: 1024, residentComputeBytes: 4096});
    const allocations = gpu.buffers.length;
    await expect(dispatchGpuComputeResident({wgsl, inputs: [], outputLength: 1})).rejects.toBeInstanceOf(GpuResidentCapacityError);
    expect(gpu.buffers).toHaveLength(allocations);
    resetReactiveSystem(); expect(handles.every(handle => !handle.active)).toBe(true);
  } finally { handles.forEach(handle => handle.dispose()); }
  expect(getGpuTelemetry()).toMatchObject({residentComputeCount: 0, residentComputeBytes: 0, pendingComputeCount: 0});
  expect(gpu.buffers.every(buffer => buffer.destroyed)).toBe(true);
});

it('uses explicit read-only input bindings and a writable private output', async () => {
  const gpu = device(); configureGpuSubsystem({device: gpu});
  const layouts = vi.spyOn(gpu, 'createBindGroupLayout'); const input = createGpuTensor([1]);
  const first = await dispatchGpuComputeResident({wgsl, inputs: [input], outputLength: 1});
  const second = await dispatchGpuComputeResident({wgsl, inputs: [first, first], outputLength: 1});
  expect(layouts.mock.calls.map(([descriptor]) => descriptor.entries.map((entry: any) => entry.buffer.type)))
    .toEqual([['read-only-storage', 'storage'], ['read-only-storage', 'read-only-storage', 'storage']]);
  first.dispose(); second.dispose();
});
