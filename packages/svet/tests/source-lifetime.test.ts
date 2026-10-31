import { beforeEach, expect, it } from 'vitest';
import { signal, computed, effect, createGpuTensor, createRoot, untrack, getGraphTelemetry, resetReactiveSystem } from '../src/engine';

beforeEach(() => resetReactiveSystem());

it('a retired scalar read does not subscribe to the signal reusing its ID before commit', () => {
  const old = signal(5); let fresh!: ReturnType<typeof signal<number>>, runs = 0;
  const observer = effect(() => {
    old(); runs++;
    if (!fresh) { old.dispose(); fresh = signal(100); expect(fresh.id).toBe(old.id); }
  });
  fresh.set(101); expect(runs).toBe(1);
  observer.dispose(); fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('a retired computed read does not bind a replacement computation to the old observer', () => {
  const input = signal(100), old = computed(() => 5); let fresh!: ReturnType<typeof computed<number>>, runs = 0;
  const observer = effect(() => {
    old(); runs++;
    if (!fresh) { old.dispose(); fresh = computed(() => input()); expect(fresh.id).toBe(old.id); fresh.peek(); }
  });
  input.set(101); expect(runs).toBe(1);
  observer.dispose(); fresh.dispose(); input.dispose();
});

it('a retired GPU tensor read cannot bind a recycled tensor and run a stale callback', () => {
  const old = createGpuTensor([5]); let fresh!: ReturnType<typeof createGpuTensor>, runs = 0;
  const observer = effect(() => {
    old(); runs++;
    if (!fresh) { old.dispose(); fresh = createGpuTensor([100]); expect(fresh.id).toBe(old.id); expect(fresh.offset).toBe(old.offset); }
  });
  expect(() => fresh.setAt(0,101)).not.toThrow(); expect(runs).toBe(1);
  observer.dispose(); fresh.dispose();
});

it('a lazy computed cannot acquire a replacement dependency from its retired source read', () => {
  const old = signal(5); let fresh!: ReturnType<typeof signal<number>>, evaluations = 0;
  const value = computed(() => { evaluations++; const result = old(); if (!fresh) { old.dispose(); fresh = signal(100); } return result; });
  const values: number[] = [], observer = effect(() => { values.push(value()); });
  fresh.set(101); expect(values).toEqual([5]); expect(evaluations).toBe(1);
  observer.dispose(); value.dispose(); fresh.dispose();
});

it('nested fresh readers cannot revive a retired dependency in their paused outer observer', () => {
  const old = signal(5); let fresh!: ReturnType<typeof signal<number>>, stop!: () => void, outerRuns = 0; const values: number[] = [];
  const observer = effect(() => {
    old(); outerRuns++;
    if (!fresh) createRoot(dispose => {
      stop = dispose;
      effect(() => { if (!fresh) { old.dispose(); fresh = signal(100); } values.push(fresh()); });
    });
  });
  fresh.set(101); expect(values).toEqual([100,101]); expect(outerRuns).toBe(1);
  observer.dispose(); stop(); fresh.dispose();
});

it('an inactive observer rebinds a reused source ID when another dependency runs it again', () => {
  const gate = signal(0), old = signal(5); let current = old; const values: number[] = [];
  const observer = effect(() => { gate(); values.push(current()); });
  old.dispose(); current = signal(100); expect(current.id).toBe(old.id);
  gate.set(1); current.set(101); expect(values).toEqual([5,100,101]);
  observer.dispose(); current.dispose(); gate.dispose();
});

it('a fresh source reread by the same observer remains subscribed', () => {
  let current = signal(5); const old = current, values: number[] = [];
  const observer = effect(() => {
    const value = current();
    if (value === 6) { current.dispose(); current = signal(100); values.push(current()); }
    else values.push(value);
  });
  old.set(6); current.set(101); expect(values).toEqual([5,100,101]);
  observer.dispose(); current.dispose();
});

it('untracked replacement reads cannot revive a retired subscription', () => {
  const old = signal(5); let fresh!: ReturnType<typeof signal<number>>, runs = 0;
  const observer = effect(() => {
    old(); runs++;
    untrack(() => { old.dispose(); fresh = signal(100); fresh(); });
  });
  fresh.set(101); expect(runs).toBe(1);
  observer.dispose(); fresh.dispose();
});

it('retired reads are compacted across inline and overflow dependencies', () => {
  const sources = Array.from({length: 7}, (_, i) => signal(i));
  const fresh: ReturnType<typeof signal<number>>[] = []; let runs = 0;
  const observer = effect(() => {
    runs++;
    for (let i = 0; i < sources.length; i++) {
      if (runs === 1 || ![0,2,4].includes(i)) sources[i]();
    }
    if (runs === 1) for (const i of [0,2,4]) {
      sources[i].dispose(); const replacement = signal(100); expect(replacement.id).toBe(sources[i].id); fresh.push(replacement);
    }
  });
  for (const replacement of fresh) replacement.set(101);
  expect(runs).toBe(1); sources[6].set(20); expect(runs).toBe(2);
  observer.dispose(); sources.forEach(source => source.dispose()); fresh.forEach(source => source.dispose());
});
