import { beforeEach, expect, it } from 'vitest';
import {
  computed,
  createGpuTensor,
  effect,
  getGraphTelemetry,
  resetReactiveSystem,
  signal,
} from '../src/engine';

beforeEach(() => resetReactiveSystem());

it.each(['read', 'peek'] as const)(
  'keeps an effect-free computed current across repeated %s calls',
  mode => {
    const source = signal(0);
    let runs = 0;
    const value = computed(() => {
      runs++;
      return source() * 3;
    });
    for (let i = 0; i < 5; i++) {
      source.set(i);
      expect(mode === 'peek' ? value.peek() : value()).toBe(i * 3);
    }
    expect(runs).toBe(5);
    value.dispose();
    source.dispose();
    expect(getGraphTelemetry().nodes).toBe(0);
  },
);

it('invalidates an effect-free diamond on every source write', () => {
  const source = signal(0),
    a = computed(() => source() + 1),
    b = computed(() => source() * 2);
  const value = computed(() => a() + b());
  for (let i = 0; i < 5; i++) {
    source.set(i);
    expect(value.peek()).toBe(i * 3 + 1);
  }
  value.dispose();
  a.dispose();
  b.dispose();
  source.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('keeps lazy tensor-derived values current without an effect subscriber', () => {
  const tensor = createGpuTensor([0]),
    value = computed(() => tensor()[0] * 2);
  for (let i = 0; i < 5; i++) {
    tensor.setAt(0, i);
    expect(value.peek()).toBe(i * 2);
  }
  value.dispose();
  tensor.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('keeps invalidating after a terminal effect is added and removed', () => {
  const source = signal(0),
    value = computed(() => source() + 1);
  expect(value()).toBe(1);
  source.set(1);
  expect(value()).toBe(2);
  const seen: number[] = [],
    observer = effect(() => {
      seen.push(value());
    });
  source.set(2);
  expect(seen).toEqual([2, 3]);
  observer.dispose();
  source.set(3);
  expect(value()).toBe(4);
  source.set(4);
  expect(value()).toBe(5);
  value.dispose();
  source.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('does not cache unrelated dirty computeds as downstream of a terminal source', () => {
  const a = signal(0),
    b = signal(0);
  let bRuns = 0;
  const derivedA = computed(() => a()),
    derivedB = computed(() => {
      bRuns++;
      return b();
    });
  const observer = effect(() => {
    derivedA();
  });
  expect(derivedB()).toBe(0);
  b.set(1);
  a.set(1);
  expect(derivedB()).toBe(1);
  expect(bRuns).toBe(2);
  a.set(2);
  expect(derivedB()).toBe(1);
  expect(bRuns).toBe(2);
  observer.dispose();
  derivedA.dispose();
  derivedB.dispose();
  a.dispose();
  b.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
