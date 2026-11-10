import { beforeEach, expect, it } from 'vitest';
import { effect, getGraphTelemetry, resetReactiveSystem, signal } from '../src/engine';
beforeEach(() => resetReactiveSystem());

// Signal methods live on a shared prototype and are bound on first access.
// These cases pin what callers could already rely on when each signal carried
// its own closures.

it('keeps methods usable when detached from the signal', () => {
  const count = signal(1);
  const { set, update, peek, dispose } = count;
  const seen: number[] = [];
  effect(() => {
    seen.push(count());
  });
  set(2);
  update(value => value + 1);
  [10, 11].forEach(set);
  expect(peek()).toBe(11);
  expect(seen).toEqual([1, 2, 3, 10, 11]);
  dispose();
  set(12);
  expect(count()).toBe(11);
});

it('returns the same function on every access and keeps signals apart', () => {
  const a = signal('a'),
    b = signal('b');
  expect(a.set).toBe(a.set);
  expect(a.peek).toBe(a.peek);
  expect(a.set).not.toBe(b.set);
  a.set('changed');
  expect([a.peek(), b.peek()]).toEqual(['changed', 'b']);
  const internals = a as unknown as Record<string, unknown>;
  expect(internals.__svet_sig).toBe(true);
  expect(internals.__svet_cached_value).toBe(a.peek);
  expect(typeof a.id).toBe('number');
  expect(a.id).not.toBe(b.id);
});

it('routes update through a replaced set, and a replaced dispose does not affect reset', () => {
  const size = signal(4);
  const original = size.set;
  const rejected: unknown[] = [];
  size.set = next => {
    const value = typeof next === 'function' ? next(size.peek()) : next;
    if (value < 0) rejected.push(value);
    else original(value);
  };
  size.update(value => value - 10);
  size.update(value => value + 1);
  expect(rejected).toEqual([-6]);
  expect(size()).toBe(5);

  let called = 0;
  (size as unknown as { dispose: () => void }).dispose = () => {
    called++;
  };
  resetReactiveSystem();
  expect(called).toBe(0);
  expect(getGraphTelemetry().nodes).toBe(0);
  size.set(9);
  expect(size()).toBe(5);
});
