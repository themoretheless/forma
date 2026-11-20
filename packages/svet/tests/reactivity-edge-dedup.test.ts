import { beforeEach, expect, it } from 'vitest';
import {
  batch,
  computed,
  createRoot,
  effect,
  getGraphTelemetry,
  getWasmCore,
  resetReactiveSystem,
  signal,
} from '../src/engine';
import { readNodeSources } from '../src/engine/wasm-bridge.ts';

beforeEach(() => {
  resetReactiveSystem();
});

it('tracks 0/1/2/5/48 dependencies accurately and shrinks/expands without permanent JS edge arrays', () => {
  const mode = signal(48);
  const sources = Array.from({ length: 48 }, (_, i) => signal(i + 1));
  const seenTotals: number[] = [];

  const derived = computed(() => {
    const n = mode();
    let sum = 0;
    for (let i = 0; i < n; i++) {
      sum += sources[i]!();
    }
    // Re-read earlier signals to exercise duplicate detection over WASM sources & SEEN_DEPS_THRESHOLD.
    if (n > 36) {
      sum += sources[0]!() + sources[5]!() + sources[34]!();
    }
    return sum;
  });

  const stop = effect(() => {
    seenTotals.push(derived());
  });

  const core = getWasmCore();
  const derivedId = (derived as unknown as { id: number }).id;
  expect(readNodeSources(core, derivedId).length).toBe(49);
  expect(getGraphTelemetry().edges).toBe(50);

  // Repeating the same dependency set keeps topology unchanged cleanly.
  sources[40]!.set(100);
  expect(readNodeSources(core, derivedId).length).toBe(49);
  expect(getGraphTelemetry().edges).toBe(50);

  // Shrink 49 -> 6 -> 3 -> 2 -> 1 -> 0 -> 49.
  mode.set(5);
  expect(readNodeSources(core, derivedId).length).toBe(6);
  expect(getGraphTelemetry().edges).toBe(7);

  mode.set(2);
  expect(readNodeSources(core, derivedId).length).toBe(3);
  expect(getGraphTelemetry().edges).toBe(4);

  mode.set(1);
  expect(readNodeSources(core, derivedId).length).toBe(2);
  expect(getGraphTelemetry().edges).toBe(3);

  mode.set(0);
  expect(readNodeSources(core, derivedId).length).toBe(1);
  expect(getGraphTelemetry().edges).toBe(2);

  mode.set(48);
  expect(readNodeSources(core, derivedId).length).toBe(49);
  expect(getGraphTelemetry().edges).toBe(50);

  stop.dispose();
  derived.dispose();
});

it('preserves staged >=3 dependencies across mid-evaluation source disposal and nested computeds', () => {
  const a = signal(1);
  const b = signal(2);
  const c = signal(3);
  const d = signal(4);
  const shouldDisposeB = signal(false);

  const nested = computed(() => {
    const v = c() * 10;
    if (shouldDisposeB()) {
      b.dispose();
    }
    return v;
  });

  const runs: number[] = [];
  const stop = effect(() => {
    runs.push(a() + (b.peek(), 1) + nested() + d());
    // Also read b reactively before nested when enabled:
  });
  stop.dispose();

  const b2 = signal(20);
  const s1 = signal(1);
  const s2 = signal(2);
  const s3 = signal(3);
  const trigger = signal(0);
  const disposedRuns: number[] = [];

  const killer = computed(() => {
    const t = trigger();
    if (t === 1) {
      b2.dispose();
    }
    return t;
  });

  const eff = effect(() => {
    const v1 = s1();
    const v2 = b2();
    const v3 = s2();
    const v4 = killer();
    const v5 = s3();
    disposedRuns.push(v1 + v2 + v3 + v4 + v5);
  });

  expect(disposedRuns).toEqual([1 + 20 + 2 + 0 + 3]);
  trigger.set(1);
  // After b2 disposed mid-run and retired from committed deps, s1/s2/s3/killer still trigger updates.
  s3.set(30);
  expect(disposedRuns.at(-1)).toBe(1 + 20 + 2 + 1 + 30);

  eff.dispose();
  killer.dispose();
  nested.dispose();
  a.dispose();
  c.dispose();
  d.dispose();
  shouldDisposeB.dispose();
  s1.dispose();
  s2.dispose();
  s3.dispose();
  trigger.dispose();
});

it('handles self-writes at cursor > 2 and cleanup-failure rearming with >2 computed ancestors', () => {
  const x = signal(1);
  const y = signal(2);
  const z = signal(3);
  const trace: number[] = [];

  const eff = effect(() => {
    const vx = x();
    const vy = y();
    const vz = z();
    trace.push(vx + vy + vz);
    if (vz === 3 && vx === 1) {
      // Write a dependency already staged beyond slot 2 during the active effect run.
      z.set(10);
    }
  });

  expect(trace).toEqual([6, 13]);
  eff.dispose();

  // Cleanup-failure rearming across a multi-source computed ancestor chain (>2 sources).
  const c0 = signal(1);
  const c1 = signal(2);
  const c2 = signal(3);
  const c3 = signal(4);
  const gate = signal(0);
  const sum = computed(() => c0() + c1() + c2() + c3());
  let boom = true;

  createRoot(stop => {
    const h = effect(() => {
      const s = sum();
      const g = gate();
      void s;
      return () => {
        if (boom && g === 0) throw new Error('cleanup boom');
      };
    });

    expect(() => gate.set(1)).toThrow('cleanup boom');
    boom = false;
    batch(() => {
      c2.set(30);
    });
    expect(() => stop()).not.toThrow();
    h.dispose();
  });

  sum.dispose();
  c0.dispose();
  c1.dispose();
  c2.dispose();
  c3.dispose();
  gate.dispose();
  x.dispose();
  y.dispose();
  z.dispose();
});
