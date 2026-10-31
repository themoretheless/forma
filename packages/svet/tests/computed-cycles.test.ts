import { beforeEach, expect, it } from 'vitest';
import { computed, signal, effect, getGraphTelemetry, resetReactiveSystem, ReactiveCycleError, runWithIsolatedContext } from '../src/engine';

beforeEach(() => resetReactiveSystem());

it('reports a direct computed cycle before overflowing the stack and can retry after correction', () => {
  const cyclic = signal(true); let value!: ReturnType<typeof computed<number>>;
  value = computed(() => cyclic() ? value() : 7);
  expect(() => value()).toThrow('Cyclic computed dependency');
  cyclic.set(false); expect(value()).toBe(7);
  value.dispose(); cyclic.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('a failed computed read keeps its observer subscribed for automatic recovery', () => {
  const source = signal(0); const value = computed(() => { const n = source(); if (n === 1) throw new Error('temporary'); return n; });
  const values: number[] = [], observer = effect(() => { values.push(value()); });
  expect(() => source.set(1)).toThrow('temporary'); source.set(2);
  expect(values).toEqual([0, 2]); observer.dispose(); value.dispose(); source.dispose();
});

it('reports an immutable closed chain for a three-node cycle', () => {
  let a!: ReturnType<typeof computed<number>>, b!: ReturnType<typeof computed<number>>, c!: ReturnType<typeof computed<number>>;
  a = computed(() => b() + 1); b = computed(() => c() + 1); c = computed(() => a() + 1);
  const error = (() => { try { a(); } catch (error) { return error; } })();
  expect(error).toBeInstanceOf(ReactiveCycleError);
  expect((error as ReactiveCycleError).nodeIds).toEqual([a.id,b.id,c.id,a.id]);
  expect(Object.isFrozen((error as ReactiveCycleError).nodeIds)).toBe(true);
  expect(() => a()).toThrow(ReactiveCycleError);
  a.dispose(); b.dispose(); c.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('detects recursion through peek and recovers after changing the branch', () => {
  const cyclic = signal(true); let value!: ReturnType<typeof computed<number>>;
  value = computed(() => cyclic() ? value.peek() : 7);
  expect(() => value.peek()).toThrow(ReactiveCycleError);
  cyclic.set(false); expect(value.peek()).toBe(7); value.dispose(); cyclic.dispose();
});

it('detects comparator recursion and retries without needing another input write', () => {
  const source = signal(0); let recursive = false, value!: ReturnType<typeof computed<number>>;
  value = computed(() => source(), { equals(a,b) { return recursive ? value() === b : a === b; } });
  expect(value()).toBe(0); recursive = true; source.set(1);
  expect(() => value()).toThrow(ReactiveCycleError);
  recursive = false; expect(value()).toBe(1); value.dispose(); source.dispose();
});

it('an observer recovers automatically when an indirect cycle is broken', () => {
  const cyclic = signal(false); let a!: ReturnType<typeof computed<number>>, b!: ReturnType<typeof computed<number>>;
  a = computed(() => cyclic() ? b() : 1); b = computed(() => a() + 1);
  const values: number[] = [], observer = effect(() => { values.push(b()); });
  expect(() => cyclic.set(true)).toThrow(ReactiveCycleError); expect(values).toEqual([2]);
  cyclic.set(false); expect(values).toEqual([2,2]);
  observer.dispose(); a.dispose(); b.dispose(); cyclic.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('ordinary failures recover through multiple computed layers', () => {
  const source = signal(0), inner = computed(() => { const n = source(); if (n === 1) throw new Error('temporary'); return n; });
  const outer = computed(() => inner() * 2), values: number[] = [], observer = effect(() => { values.push(outer()); });
  expect(() => source.set(1)).toThrow('temporary'); source.set(2); expect(values).toEqual([0,4]);
  observer.dispose(); outer.dispose(); inner.dispose(); source.dispose();
});

it('caught failures keep effect dependencies and recover on later input', () => {
  const source = signal(0), value = computed(() => { const n = source(); if (n === 1) throw new Error('temporary'); return n; });
  let errors = 0; const values: number[] = [];
  const observer = effect(() => { try { values.push(value()); } catch { errors++; } });
  source.set(1); source.set(2); expect(errors).toBe(1); expect(values).toEqual([0,2]);
  observer.dispose(); value.dispose(); source.dispose();
});

it('comparator exceptions preserve observer subscriptions for later valid values', () => {
  const source = signal(0), value = computed(() => source(), { equals(a,b) { if (b === 1) throw new Error('comparison failed'); return a === b; } });
  const values: number[] = [], observer = effect(() => { values.push(value()); });
  expect(() => source.set(1)).toThrow('comparison failed'); source.set(2); expect(values).toEqual([0,2]);
  observer.dispose(); value.dispose(); source.dispose();
});

it('does not mistake equal numeric IDs in an isolated context for a cycle', () => {
  const outer = computed(() => runWithIsolatedContext(() => { const inner = computed(() => 7); const value = inner(); inner.dispose(); return value; }));
  expect(outer()).toBe(7); outer.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('reset during evaluation does not restore an obsolete evaluation stack', () => {
  let fresh!: ReturnType<typeof computed<number>>;
  const old = computed(() => { resetReactiveSystem(); fresh = computed(() => 7); return fresh(); });
  old(); old.dispose(); expect(fresh()).toBe(7); expect(getGraphTelemetry().nodes).toBe(1);
  fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('cycle traces distinguish a reused native ID from the disposed active entry', () => {
  let old!: ReturnType<typeof computed<number>>, fresh!: ReturnType<typeof computed<number>>;
  old = computed(() => { old.dispose(); fresh = computed(() => fresh()); expect(fresh.id).toBe(old.id); return fresh(); });
  const error = (() => { try { old(); } catch (error) { return error; } })();
  expect(error).toBeInstanceOf(ReactiveCycleError); expect((error as ReactiveCycleError).nodeIds).toEqual([fresh.id,fresh.id]);
  fresh.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('an effect can repair a cycle in its error handler and stabilize without recursive entry', () => {
  const cyclic = signal(true); let value!: ReturnType<typeof computed<number>>; const values: number[] = [];
  value = computed(() => cyclic() ? value() : 7);
  let depth = 0, peak = 0;
  const observer = effect(() => {
    depth++; peak = Math.max(peak, depth);
    try { values.push(value()); } catch (error) { expect(error).toBeInstanceOf(ReactiveCycleError); cyclic.set(false); }
    finally { depth--; }
  });
  expect(values).toEqual([7]); expect(peak).toBe(1);
  observer.dispose(); value.dispose(); cyclic.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('all queued observers retain their reads after a shared computed failure', () => {
  const source = signal(0), value = computed(() => { const n = source(); if (n === 1) throw new Error('shared failure'); return n; });
  const left: number[] = [], right: number[] = [];
  const a = effect(() => { left.push(value()); }), b = effect(() => { right.push(value()); });
  expect(() => source.set(1)).toThrow('shared failure'); source.set(2);
  expect(left).toEqual([0,2]); expect(right).toEqual([0,2]);
  a.dispose(); b.dispose(); value.dispose(); source.dispose();
});
