import { beforeEach, expect, it } from 'vitest';
import { signal, effect, batch, getGraphTelemetry, resetReactiveSystem } from '../src/engine';

beforeEach(() => resetReactiveSystem());

it('bounds native stale queue entries and stabilizes WASM memory during batch churn', () => {
  const source = signal(0), gate = signal(0), before = getGraphTelemetry(); let warmedBytes = 0;
  batch(() => {
    for (let i = 0; i < 5000; i++) {
      const observer = effect(() => { source(); gate(); });
      source.set(i + 1); observer.dispose();
      if (i === 200) warmedBytes = getGraphTelemetry().wasmMemoryBytes;
    }
    const pending = getGraphTelemetry();
    expect(pending.nodes).toBe(2); expect(pending.pendingEffectCount).toBe(0);
    expect(pending.pendingEffectEntryCount).toBeLessThanOrEqual(68);
    expect(pending.wasmMemoryBytes).toBe(warmedBytes);
    expect(before.pendingEffectCount).toBe(0); expect(before.pendingEffectEntryCount).toBe(0);
  });
  expect(getGraphTelemetry().pendingEffectEntryCount).toBe(0); source.dispose(); gate.dispose();
});

it('queue compaction preserves real jobs and their callback order in a batch', () => {
  const churn = signal(0), gate = signal(0), left = signal(0), right = signal(0), values: string[] = [];
  const a = effect(() => { values.push('a' + left()); }), b = effect(() => { values.push('b' + right()); });
  values.length = 0;
  batch(() => {
    left.set(1); right.set(1);
    for (let i = 0; i < 1000; i++) {
      const observer = effect(() => { churn(); gate(); }); churn.set(i + 1); observer.dispose();
    }
    const pending = getGraphTelemetry();
    expect(pending.pendingEffectCount).toBe(2); expect(pending.pendingEffectEntryCount).toBeGreaterThanOrEqual(2);
    expect(pending.pendingEffectEntryCount).toBeLessThanOrEqual(76);
  });
  expect(values).toEqual(['a1','b1']);
  expect(getGraphTelemetry()).toMatchObject({ pendingEffectCount: 0, pendingEffectEntryCount: 0 });
  a.dispose(); b.dispose(); churn.dispose(); gate.dispose(); left.dispose(); right.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
