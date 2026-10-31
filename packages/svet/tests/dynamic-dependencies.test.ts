import { beforeEach, expect, it } from 'vitest';
import { batch, computed, effect, resetReactiveSystem, signal } from '../src/engine';
beforeEach(() => resetReactiveSystem());

it('tracks effect branches after repeated executions with unchanged dependencies', () => {
  const chooseA = signal(true), a = signal(0), b = signal(10);
  const values: number[] = [];
  const handle = effect(() => { values.push(chooseA() ? a() : b()); });
  a.set(1);
  a.set(2);
  chooseA.set(false);
  b.set(11);
  a.set(3);
  expect(values).toEqual([0, 1, 2, 10, 11]);
  handle.dispose();
});

it('tracks computed branches after repeated executions with unchanged dependencies', () => {
  const chooseA = signal(true), a = signal(0), b = signal(10);
  const value = computed(() => chooseA() ? a() : b());
  const values: number[] = [];
  const handle = effect(() => { values.push(value()); });
  a.set(1);
  a.set(2);
  chooseA.set(false);
  b.set(11);
  a.set(3);
  expect(values).toEqual([0, 1, 2, 10, 11]);
  handle.dispose();
  value.dispose();
});

it('tracks newly introduced dependencies in a formerly single-source effect', () => {
  const enabled = signal(false), value = signal(1);
  const values: number[] = [];
  const handle = effect(() => { values.push(enabled() ? value() : 0); });
  enabled.set(true);
  value.set(2);
  expect(values).toEqual([0, 1, 2]);
  handle.dispose();
});

it('does not subscribe an outer effect to the internal sources of a computed', () => {
  const inner = signal(0), outer = signal(0);
  let computes = 0, runs = 0;
  const value = computed(() => { computes++; return Math.floor(inner() / 2); });
  const handle = effect(() => { outer(); value(); runs++; });
  inner.set(2);
  inner.set(4);
  outer.set(1);
  expect(value()).toBe(2);
  expect(computes).toBe(3);
  expect(runs).toBe(4);
  handle.dispose();
});

it('does not return a stale cached value after a computed throws', () => {
  const source = signal(0);
  const value = computed(() => {
    if (source() === 1) throw new Error('retry me');
    return source();
  });
  expect(value()).toBe(0);
  source.set(1);
  expect(() => value()).toThrow('retry me');
  expect(() => value()).toThrow('retry me');
  source.set(2);
  expect(value()).toBe(2);
});

it('matches a reference model across batched branch changes and a computed diamond', () => {
  const select = signal(true), a = signal(0), b = signal(10);
  const left = computed(() => select() ? a() : b());
  const right = computed(() => select() ? b() * 2 : a() * 3);
  const total = computed(() => left() + right());
  const observed: number[] = [];
  const handle = effect(() => { observed.push(total()); });
  let seed = 246813579;
  const next = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed; };
  for (let i = 0; i < 200; i++) {
    const priorRuns = observed.length;
    batch(() => {
      select.set((next() & 4) !== 0);
      a.set(next() % 1000);
      b.set(next() % 1000);
    });
    const expected = select.peek() ? a.peek() + b.peek() * 2 : b.peek() + a.peek() * 3;
    expect(total.peek(), `computed at iteration ${i}`).toBe(expected);
    expect(observed.at(-1), `effect at iteration ${i}`).toBe(expected);
    expect(observed.length - priorRuns).toBeLessThanOrEqual(1);
  }
  handle.dispose();
  total.dispose(); left.dispose(); right.dispose();
});
