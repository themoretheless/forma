import { beforeEach, expect, it } from 'vitest';
import {
  createRoot,
  effect,
  getCurrentOwner,
  getGraphTelemetry,
  onCleanup,
  resetReactiveSystem,
  runWithOwnerScope,
  signal,
} from '../src/engine';

beforeEach(() => resetReactiveSystem());

it('rejects parent disposal from context iteration before invoking the child body', () => {
  let calls = 0;
  createRoot(dispose => {
    const entries: Array<[symbol, unknown]> = [];
    entries[Symbol.iterator] = function* () {
      dispose();
      yield [Symbol(), 1];
      return undefined;
    };
    expect(() =>
      runWithOwnerScope(entries, () => {
        calls++;
      }),
    ).toThrow('disposed');
  });
  expect(calls).toBe(0);
  expect(getCurrentOwner()).toBeNull();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('rejects graph reset from context iteration without poisoning the new owner state', () => {
  const entries: Array<[symbol, unknown]> = [];
  let fresh!: ReturnType<typeof signal<number>>,
    calls = 0;
  entries[Symbol.iterator] = function* () {
    resetReactiveSystem();
    fresh = signal(1);
    yield [Symbol(), 1];
    return undefined;
  };
  expect(() =>
    runWithOwnerScope(entries, () => {
      calls++;
      effect(() => {});
    }),
  ).toThrow('reset');
  expect(calls).toBe(0);
  expect(getCurrentOwner()).toBeNull();
  let seen = 0;
  const observer = effect(() => {
    seen = fresh();
  });
  fresh.set(2);
  expect(seen).toBe(2);
  observer.dispose();
  fresh.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('rechecks lifetime after entry destructuring getters too', () => {
  let calls = 0;
  createRoot(dispose => {
    const entry: [symbol, unknown] = [Symbol(), 1];
    Object.defineProperty(entry, '1', {
      get() {
        dispose();
        return 1;
      },
    });
    expect(() => runWithOwnerScope([entry], () => calls++)).toThrow('disposed');
  });
  expect(calls).toBe(0);
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('a bound child disposer unlinks resources and remains idempotent after returning', () => {
  let cleaned = 0;
  createRoot(stop => {
    const parent = getCurrentOwner()!;
    let childStop!: () => void;
    runWithOwnerScope([], dispose => {
      childStop = dispose;
      onCleanup(() => cleaned++);
      effect(() => {});
    });
    expect(parent.children?.size).toBe(1);
    childStop();
    childStop();
    expect(parent.children?.size).toBe(0);
    expect(getGraphTelemetry().nodes).toBe(0);
    stop();
  });
  expect(cleaned).toBe(1);
  expect(getCurrentOwner()).toBeNull();
});
