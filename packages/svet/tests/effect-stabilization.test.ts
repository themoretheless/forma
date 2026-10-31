import { beforeEach, expect, it } from 'vitest';
import { signal, computed, effect, batch, createGpuTensor, getGraphTelemetry, resetReactiveSystem, ReactiveStabilizationError, flushEffects, getWasmCore } from '../src/engine';

beforeEach(() => resetReactiveSystem());

it.each(['initial', 'primed', 'computed', 'tensor'] as const)('%s self-writes stabilize sequentially without recursive callbacks', mode => {
  const source = signal(mode === 'primed' ? -1 : 0), tensor = createGpuTensor([0]);
  const derived = computed(() => source() * 2); let depth = 0, peak = 0; const values: number[] = [];
  const observer = effect(() => {
    depth++; peak = Math.max(peak, depth);
    try {
      const value = mode === 'tensor' ? tensor()[0] : mode === 'computed' ? derived() / 2 : source();
      values.push(value);
      if (value >= 0 && value < 4) {
        if (mode === 'tensor') tensor.setAt(0, value + 1); else source.set(value + 1);
      }
    } finally { depth--; }
  });
  if (mode === 'primed') source.set(0);
  expect(values).toEqual(mode === 'primed' ? [-1, 0, 1, 2, 3, 4] : [0, 1, 2, 3, 4]); expect(peak).toBe(1);
  observer.dispose(); derived.dispose(); source.dispose(); tensor.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('self-write iterations clean up each invocation exactly once', () => {
  const source = signal(0); const cleanups: number[] = [];
  const observer = effect(() => { const value = source(); if (value < 3) source.set(value + 1); return () => { cleanups.push(value); }; });
  expect(cleanups).toEqual([0, 1, 2]); observer.dispose(); expect(cleanups).toEqual([0, 1, 2, 3]); source.dispose();
});

it('a nonstabilizing initial effect throws a typed bounded error and frees its resources', () => {
  const source = signal(0); let runs = 0;
  let failure: unknown;
  try { effect(() => { runs++; source.set(source() + 1); }); } catch (error) { failure = error; }
  expect(failure).toBeInstanceOf(ReactiveStabilizationError); expect(runs).toBe(100);
  expect((failure as ReactiveStabilizationError).iterations).toBe(100);
  expect((failure as ReactiveStabilizationError).effectId).toBeTypeOf('number');
  expect(getGraphTelemetry().nodes).toBe(1); source.dispose();
});

it('a running effect can recover after a bounded self-write error', () => {
  const source = signal(0); let looping = false, runs = 0;
  const observer = effect(() => { runs++; const value = source(); if (looping) source.set(value + 1); });
  looping = true; expect(() => source.set(1)).toThrow(ReactiveStabilizationError); expect(runs).toBe(101);
  looping = false; source.set(0); expect(runs).toBe(102);
  observer.dispose(); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('mutual effect cycles report the flush limit while preserving pending work for recovery', () => {
  const a = signal(0), b = signal(0), gate = signal(0); let armed = false;
  const first = effect(() => { const value = a(); gate(); if (armed) b.set(value + 1); });
  const second = effect(() => { const value = b(); gate(); if (armed) a.set(value + 1); });
  armed = true;
  expect(() => a.set(1)).toThrow(ReactiveStabilizationError);
  expect(getWasmCore().svet_pending_effect_count()).toBeGreaterThan(0);
  armed = false; flushEffects(); expect(getWasmCore().svet_pending_effect_count()).toBe(0);
  first.dispose(); second.dispose(); a.dispose(); b.dispose(); gate.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('initial writes notify pre-existing effects before effect construction returns', () => {
  const source = signal(0); const values: number[] = [];
  const observer = effect(() => { values.push(source()); });
  const writer = effect(() => { source.set(1); });
  expect(values).toEqual([0, 1]); writer.dispose(); observer.dispose(); source.dispose();
});

it('writing an unrelated output after reading a computed does not request a false rerun', () => {
  const source = signal(1), output = signal(0), derived = computed(() => source() * 2); let runs = 0;
  const observer = effect(() => { runs++; output.set(derived()); });
  expect(runs).toBe(1); expect(output.peek()).toBe(2);
  source.set(2); expect(runs).toBe(2); expect(output.peek()).toBe(4);
  observer.dispose(); derived.dispose(); source.dispose(); output.dispose();
});

it('each stabilization round releases previous owned tensor resources', () => {
  const source = signal(0), tensors: Array<ReturnType<typeof createGpuTensor>> = [];
  const observer = effect(() => {
    const value = source(); tensors.push(createGpuTensor([value]));
    expect(getGraphTelemetry().nodes).toBe(3);
    if (value < 3) source.set(value + 1);
  });
  tensors.slice(0, -1).forEach(tensor => expect(() => tensor.peek()).toThrow('disposed'));
  expect(tensors.at(-1)!.peek()[0]).toBe(3);
  observer.dispose(); tensors.forEach(tensor => expect(() => tensor.peek()).toThrow('disposed'));
  expect(getGraphTelemetry().nodes).toBe(1); source.dispose();
});

it('graph reset during stabilization abandons the old invocation without restoring stale tracking', () => {
  const oldSource = signal(0); let fresh!: ReturnType<typeof signal<number>>, freshObserver!: ReturnType<typeof effect>; const values: number[] = [];
  const oldObserver = effect(() => {
    const value = oldSource();
    if (value === 0) oldSource.set(1);
    else {
      resetReactiveSystem(); fresh = signal(10);
      freshObserver = effect(() => { values.push(fresh()); });
    }
  });
  oldObserver.dispose(); oldSource.dispose(); fresh.set(11); expect(values).toEqual([10, 11]);
  freshObserver.dispose(); fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['direct', 'linked'] as const)('%s long acyclic effect cascades drain without recursive callbacks or false round limits', mode => {
  const sources = Array.from({length: 1501}, () => signal(0)), gate = signal(0);
  let depth = 0, peak = 0;
  const observers = sources.slice(0,-1).map((source, i) => effect(() => {
    depth++; peak = Math.max(peak, depth);
    try { const value = source(); if (mode === 'linked') gate(); if (value > 0) sources[i+1].set(value+1); }
    finally { depth--; }
  }));
  expect(() => sources[0].set(1)).not.toThrow();
  expect(sources.at(-1)!.peek()).toBe(1501); expect(peak).toBe(1);
  observers.forEach(observer => observer.dispose()); sources.forEach(source => source.dispose()); gate.dispose();
});

it('mutual unique-subscriber cycles hit a typed limit and retain pending recovery work', () => {
  const a = signal(0), b = signal(0); let armed = false;
  const first = effect(() => { const value = a(); if (armed) b.set(value+1); });
  const second = effect(() => { const value = b(); if (armed) a.set(value+1); });
  armed = true; expect(() => a.set(1)).toThrow(ReactiveStabilizationError);
  expect(getWasmCore().svet_pending_effect_count()).toBeGreaterThan(0);
  armed = false; flushEffects(); expect(getWasmCore().svet_pending_effect_count()).toBe(0);
  first.dispose(); second.dispose(); a.dispose(); b.dispose();
});

it('nested explicit flush and batch defer downstream callbacks until the writer commits', () => {
  const source = signal(0), events: string[] = [];
  const downstream = effect(() => { events.push('read:' + source()); });
  const writer = effect(() => {
    events.push('writer:start');
    batch(() => { source.set(1); source.set(2); }); flushEffects();
    events.push('writer:end');
  });
  expect(events).toEqual(['read:0','writer:start','writer:end','read:2']);
  writer.dispose(); downstream.dispose(); source.dispose();
});

it('queued GPU tensor cascades run after each writer callback without stack growth', () => {
  const tensors = Array.from({length: 301}, () => createGpuTensor([0])); let depth = 0, peak = 0;
  const observers = tensors.slice(0,-1).map((tensor, i) => effect(() => {
    depth++; peak = Math.max(peak, depth);
    try { const value = tensor()[0]; if (value > 0) tensors[i+1].setAt(0,value+1); }
    finally { depth--; }
  }));
  tensors[0].setAt(0,1); expect(tensors.at(-1)!.peek()[0]).toBe(301); expect(peak).toBe(1);
  observers.forEach(observer => observer.dispose()); tensors.forEach(tensor => tensor.dispose());
});

it('an effect reaching the queued-run budget can settle before an unrelated long tail drains', () => {
  const a = signal(0), b = signal(0), tail = Array.from({length: 151}, () => signal(0)); let armed = false;
  const first = effect(() => { const value = a(); if (armed) b.set(value+1); });
  const second = effect(() => { const value = b(); if (armed) { if (value < 200) a.set(value+1); else tail[0].set(1); } });
  const observers = tail.slice(0,-1).map((source,i) => effect(() => { const value = source(); if (value > 0) tail[i+1].set(value+1); }));
  armed = true; expect(() => a.set(1)).not.toThrow(); expect(b.peek()).toBe(200); expect(tail.at(-1)!.peek()).toBe(151);
  first.dispose(); second.dispose(); observers.forEach(observer => observer.dispose()); tail.forEach(source => source.dispose()); a.dispose(); b.dispose();
});
