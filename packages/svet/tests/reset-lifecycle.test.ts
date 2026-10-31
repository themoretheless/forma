import { beforeEach, expect, it } from 'vitest';
import { runWithOwnerScope, batch, createRoot, getCurrentOwner, untrack, computed, effect, getGraphTelemetry, resetReactiveSystem, signal } from '../src/engine';
beforeEach(() => resetReactiveSystem());

it('a stale signal cannot notify or dispose a fresh node after graph reset', () => {
  const old = signal(10);
  resetReactiveSystem();
  const fresh = signal(20);
  const values: number[] = [];
  const handle = effect(() => { values.push(fresh()); });
  old.set(11); old.dispose(); fresh.set(21);
  expect(values).toEqual([20, 21]);
  expect(getGraphTelemetry().nodes).toBe(2);
  handle.dispose();
});

it('a stale effect handle cannot dispose a reused WASM ID after graph reset', () => {
  const old = effect(() => {});
  resetReactiveSystem();
  const source = signal(0);
  const values: number[] = [];
  const fresh = effect(() => { values.push(source()); });
  old.dispose(); source.set(1);
  expect(values).toEqual([0, 1]);
  expect(getGraphTelemetry().nodes).toBe(2);
  fresh.dispose();
});

it('a stale computed cannot dispose a new signal using its former ID', () => {
  const old = computed(() => 1);
  expect(old()).toBe(1);
  resetReactiveSystem();
  const fresh = signal(10);
  old.dispose();
  expect(getGraphTelemetry().nodes).toBe(1);
  const values: number[] = [];
  const handle = effect(() => { values.push(fresh()); });
  fresh.set(11);
  expect(values).toEqual([10, 11]);
  handle.dispose();
});

it('stale reads do not add edges to reused IDs and stale updates do not invoke user code', () => {
  const old = signal(10);
  const derived = computed(() => old() * 2);
  expect(derived()).toBe(20);
  resetReactiveSystem();
  const fresh = signal(0);
  let runs = 0, updates = 0;
  const handle = effect(() => { old(); derived(); runs++; });
  old.update(value => { updates++; return value + 1; });
  fresh.set(1);
  expect(old()).toBe(10);
  expect(derived()).toBe(20);
  expect(runs).toBe(1);
  expect(updates).toBe(0);
  expect(getGraphTelemetry().edges).toBe(0);
  handle.dispose();
});

it('does not restore obsolete observers when reset occurs during computed evaluation', () => {
  let fresh!: ReturnType<typeof signal<number>>;
  const values: number[] = [];
  let freshHandle!: ReturnType<typeof effect>;
  const old = effect(() => {
    const derived = computed(() => {
      resetReactiveSystem();
      fresh = signal(10);
      return 1;
    });
    derived();
    freshHandle = effect(() => { values.push(fresh()); });
  });
  fresh.set(11);
  old.dispose();
  fresh.set(12);
  expect(values).toEqual([10, 11, 12]);
  expect(getGraphTelemetry().nodes).toBe(2);
  expect(getGraphTelemetry().edges).toBe(1);
  freshHandle.dispose();
});

it.each(['updater', 'equals'] as const)('a signal disposed inside %s cannot publish into its recycled slot', phase => {
  let old!: ReturnType<typeof signal<number>>, fresh!: ReturnType<typeof signal<number>>;
  let observer!: ReturnType<typeof effect>; const values: number[] = [];
  const replace = () => {
    old.dispose(); fresh = signal(100); expect(fresh.id).toBe(old.id);
    observer = effect(() => { values.push(fresh()); });
  };
  old = signal(10, { equals(a, b) { if (phase === 'equals') replace(); return a === b; } });
  old.set(previous => { if (phase === 'updater') replace(); return previous + 1; });
  expect(old.peek()).toBe(10); expect(fresh.peek()).toBe(100); expect(values).toEqual([100]);
  fresh.set(101); expect(values).toEqual([100, 101]); observer.dispose(); fresh.dispose();
});

it.each(['compute', 'equals'] as const)('a computed disposed during %s cannot cache or bind dependencies into its replacement', phase => {
  const source = signal(1); let derived!: ReturnType<typeof computed<number>>;
  let fresh!: ReturnType<typeof signal<number>>, observer!: ReturnType<typeof effect>; const values: number[] = [];
  const replace = () => {
    derived.dispose(); fresh = signal(100); expect(fresh.id).toBe(derived.id);
    observer = effect(() => { values.push(fresh()); });
  };
  derived = computed(() => { const value = source(); if (value === 2 && phase === 'compute') replace(); return value; },
    { equals(a, b) { if (phase === 'equals') replace(); return a === b; } });
  expect(derived()).toBe(1); source.set(2); expect(derived()).toBe(1);
  expect(fresh.peek()).toBe(100); source.set(3); expect(values).toEqual([100]);
  fresh.set(101); expect(values).toEqual([100, 101]); observer.dispose(); fresh.dispose(); source.dispose();
});

it('an outer observer does not subscribe to the recycled slot of a computed disposed while read', () => {
  const source = signal(1), tail = signal(0); let derived!: ReturnType<typeof computed<number>>, fresh!: ReturnType<typeof signal<number>>;
  derived = computed(() => {
    const value = source();
    if (value === 2) { derived.dispose(); fresh = signal(100); expect(fresh.id).toBe(derived.id); }
    return value;
  });
  const values: number[] = [], observer = effect(() => { values.push(derived()); tail(); });
  source.set(2); const afterDisposal = [...values];
  fresh.set(101); expect(values).toEqual(afterDisposal);
  tail.set(1); expect(values).toEqual([...afterDisposal, 1]);
  observer.dispose(); fresh.dispose(); source.dispose(); tail.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('a disposed computed that throws cannot mark its replacement dirty or bind old sources', () => {
  const source = signal(1); let derived!: ReturnType<typeof computed<number>>, fresh!: ReturnType<typeof signal<number>>;
  derived = computed(() => {
    const value = source();
    if (value === 2) { derived.dispose(); fresh = signal(100); throw new Error('disposed computation failed'); }
    return value;
  });
  expect(derived()).toBe(1); source.set(2); expect(() => derived()).toThrow('disposed computation failed');
  const values: number[] = [], observer = effect(() => { values.push(fresh()); });
  source.set(3); expect(values).toEqual([100]); fresh.set(101); expect(values).toEqual([100, 101]);
  expect(derived.peek()).toBe(1); observer.dispose(); fresh.dispose(); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});


it('reset inside a batch does not poison batching in the replacement graph', () => {
  batch(() => resetReactiveSystem());
  const source = signal(0), values: number[] = [], observer = effect(() => { values.push(source()); });
  batch(() => { source.set(1); source.set(2); expect(values).toEqual([0]); });
  expect(values).toEqual([0,2]); observer.dispose(); source.dispose();
});

it('reset inside untrack cannot restore the retired active observer', () => {
  let fresh!: ReturnType<typeof signal<number>>;
  const old = effect(() => {
    untrack(() => resetReactiveSystem());
    expect(getCurrentOwner()).toBeNull();
    fresh = signal(10); fresh();
  });
  expect(getGraphTelemetry().edges).toBe(0);
  const values: number[] = [], observer = effect(() => { values.push(fresh()); });
  fresh.set(11); expect(values).toEqual([10,11]); old.dispose(); observer.dispose(); fresh.dispose();
});

it('reset inside nested roots cannot restore a retired parent owner', () => {
  createRoot(() => {
    createRoot(() => resetReactiveSystem());
    expect(getCurrentOwner()).toBeNull();
  });
  expect(getCurrentOwner()).toBeNull();
});


it.each([false, true])('owner scopes do not restore retired parents after reset (throws=%s)', throws => {
  createRoot(() => {
    const run = () => runWithOwnerScope([], () => {
      resetReactiveSystem();
      if (throws) throw new Error('after reset');
    });
    if (throws) expect(run).toThrow('after reset'); else run();
    expect(getCurrentOwner()).toBeNull();
  });
});

it('nested batches crossing reset do not end a fresh transaction or lose callback errors', () => {
  expect(() => batch(() => batch(() => { resetReactiveSystem(); throw new Error('after reset'); }))).toThrow('after reset');
  const source = signal(0), values: number[] = [], observer = effect(() => { values.push(source()); });
  batch(() => { source.set(1); batch(() => source.set(2)); expect(values).toEqual([0]); });
  expect(values).toEqual([0,2]); observer.dispose(); source.dispose();
});
