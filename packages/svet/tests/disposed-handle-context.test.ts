import { beforeEach, expect, it } from 'vitest';
import {
  computed,
  createRoot,
  effect,
  getCurrentOwner,
  getGraphTelemetry,
  onCleanup,
  resetReactiveSystem,
  signal,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());
it('keeps captured signal methods inert and cached after disposal', () => {
  const source = signal(4),
    read = source.peek,
    set = source.set,
    update = source.update;
  source.dispose();
  set(9);
  update(() => {
    throw Error('late updater');
  });
  source.dispose();
  expect(read()).toBe(4);
  expect(source()).toBe(4);
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('permits disposal from a signal comparator without accessing its retired context', () => {
  const source = signal(4, {
    equals: () => {
      source.dispose();
      return false;
    },
  });
  expect(() => source.set(5)).not.toThrow();
  expect(source()).toBe(4);
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('unwinds an active computed frame before detaching a disposed owner context', () => {
  const source = signal(4);
  let stop = () => {};
  const derived = createRoot(dispose => {
    stop = dispose;
    return computed(() => {
      source();
      stop();
      return 9;
    });
  });
  expect(() => derived()).not.toThrow();
  expect(derived.peek()).toBeUndefined();
  const observed: number[] = [];
  const live = effect(() => {
    observed.push(source());
  });
  source.set(5);
  expect(observed).toEqual([4, 5]);
  live.dispose();
  source.dispose();
  derived.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('preserves a cached computed value and rejects late reevaluation after disposal', () => {
  const source = signal(2);
  let calls = 0;
  const derived = computed(() => {
    calls++;
    return source() * 2;
  });
  expect(derived()).toBe(4);
  const peek = derived.peek;
  derived.dispose();
  source.set(3);
  expect(derived()).toBe(4);
  expect(peek()).toBe(4);
  expect(calls).toBe(1);
  source.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('keeps root teardown ordered and idempotent after a throwing cleanup', () => {
  const calls: string[] = [];
  let stop = () => {};
  createRoot(dispose => {
    stop = dispose;
    computed(() => 1)();
    onCleanup(() => {
      calls.push('first');
    });
    onCleanup(() => {
      calls.push('last');
      stop();
      throw Error('cleanup refused');
    });
  });
  expect(() => stop()).not.toThrow();
  stop();
  expect(calls).toEqual(['last', 'first']);
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('makes an escaped disposer inert after construction failure', () => {
  let stop = () => {};
  let calls = 0;
  const failure = Error('construction refused');
  expect(() =>
    createRoot(dispose => {
      stop = dispose;
      computed(() => 1)();
      onCleanup(() => {
        calls++;
      });
      throw failure;
    }),
  ).toThrow(failure);
  stop();
  stop();
  expect(calls).toBe(1);
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('retires cached handles at reset without calling replaced public disposers', () => {
  const source = signal(2),
    derived = createRoot(() => computed(() => source() * 2));
  expect(derived()).toBe(4);
  const releaseSource = source.dispose,
    releaseDerived = derived.dispose;
  source.dispose = () => {
    throw Error('public signal disposer invoked');
  };
  derived.dispose = () => {
    throw Error('public computed disposer invoked');
  };
  expect(() => resetReactiveSystem()).not.toThrow();
  const fresh = signal(8);
  source.set(99);
  releaseSource();
  releaseDerived();
  expect(source()).toBe(2);
  expect(derived.peek()).toBe(4);
  expect(fresh()).toBe(8);
  expect(getGraphTelemetry().nodes).toBe(1);
  fresh.dispose();
});
it('detaches public effect methods even when cleanup reenters and throws', () => {
  const source = signal(1),
    failure = Error('effect cleanup refused');
  let stop = () => {};
  let calls = 0;
  const handle = effect(register => {
    source();
    register(() => {
      calls++;
      stop();
      throw failure;
    });
  });
  stop = handle.dispose;
  expect(() => stop()).toThrow(failure);
  expect(calls).toBe(1);
  expect(() => stop()).not.toThrow();
  const values: number[] = [];
  const replacement = effect(() => {
    values.push(source());
  });
  stop();
  source.set(2);
  expect(values).toEqual([1, 2]);
  replacement.dispose();
  source.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('preserves deferred root cleanup after reset and releases it once on late disposal', () => {
  let stop = () => {};
  let cleanups = 0;
  createRoot(dispose => {
    stop = dispose;
    onCleanup(() => {
      cleanups++;
    });
    effect(() => {});
  });
  resetReactiveSystem();
  expect(cleanups).toBe(0);
  stop();
  stop();
  expect(cleanups).toBe(1);
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('preserves pending cleanup and immediately drains late registration after in-effect reset', () => {
  let cleanups = 0;
  let runs = 0;
  const handle = createRoot(() =>
    effect(register => {
      runs++;
      register(() => {
        cleanups++;
      });
      resetReactiveSystem();
      register(() => {
        cleanups++;
      });
      return () => {
        cleanups++;
      };
    }),
  );
  expect(runs).toBe(1);
  expect(cleanups).toBe(2);
  const fresh = signal(8),
    values: number[] = [];
  const observer = effect(() => {
    values.push(fresh());
  });
  handle.dispose();
  handle.dispose();
  expect(cleanups).toBe(3);
  fresh.set(9);
  expect(values).toEqual([8, 9]);
  observer.dispose();
  fresh.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('reset preserves owner cleanup array identity and user callback order across many computeds', () => {
  let stop = () => {};
  let owner!: NonNullable<ReturnType<typeof getCurrentOwner>>;
  const events: number[] = [];
  const callbacks = Array.from({ length: 32 }, (_, n) => () => {
    events.push(n);
  });
  createRoot(dispose => {
    stop = dispose;
    owner = getCurrentOwner()!;
    for (const callback of callbacks) {
      onCleanup(callback);
      for (let n = 0; n < 8; n++) computed(() => n)();
    }
  });
  const originalArray = owner.disposables;
  resetReactiveSystem();
  expect(owner.disposables).toBe(originalArray);
  expect(owner.disposables).toEqual(callbacks);
  expect(events).toEqual([]);
  const fresh = signal(9);
  stop();
  stop();
  expect(fresh()).toBe(9);
  expect(events).toEqual(Array.from({ length: 32 }, (_, n) => 31 - n));
  fresh.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
