import {beforeEach, expect, it} from 'vitest';
import {batch, effect, signal, getGraphTelemetry, getWasmCore, flushEffects, resetReactiveSystem} from '../src/engine';

beforeEach(() => resetReactiveSystem());

it.each([new Error('body failure'), undefined, null, 0])('batch preserves its body failure %s while draining failing observers', failure => {
  const source = signal(0), values: number[] = [];
  const first = effect(() => { const value = source(); if (value === 1) throw new Error('observer failure'); });
  const second = effect(() => { values.push(source()); });
  let caught = false;
  try { batch(() => { source.set(1); throw failure; }); }
  catch (error) { caught = true; expect(error).toBe(failure); }
  expect(caught).toBe(true); expect(values).toEqual([0,1]);
  batch(() => source.set(2)); expect(values).toEqual([0,1,2]);
  first.dispose(); second.dispose(); source.dispose(); expect(getGraphTelemetry().nodes).toBe(0);
});

it('a successful batch still reports observer failure and retains committed writes', () => {
  const source = signal(0), failure = new Error('observer failure');
  const observer = effect(() => { if (source() === 1) throw failure; });
  expect(() => batch(() => { source.set(1); return 123; })).toThrow(failure);
  expect(source.peek()).toBe(1); batch(() => source.set(2));
  observer.dispose(); source.dispose();
});

it('nested batch body failure remains primary when outermost draining also fails', () => {
  const source = signal(0), failure = new Error('nested body failure');
  const observer = effect(() => { if (source() === 1) throw new Error('observer failure'); });
  expect(() => batch(() => batch(() => { source.set(1); throw failure; }))).toThrow(failure);
  source.set(2); observer.dispose(); source.dispose();
});


it('body failure remains primary over stabilization limits without consuming pending recovery work', () => {
  const a = signal(0), b = signal(0), failure = new Error('body failure'); let armed = false;
  const left = effect(() => { const value = a(); if (armed) b.set(value+1); });
  const right = effect(() => { const value = b(); if (armed) a.set(value+1); });
  armed = true;
  expect(() => batch(() => { a.set(1); throw failure; })).toThrow(failure);
  expect(getWasmCore().svet_pending_effect_count()).toBeGreaterThan(0);
  armed = false; flushEffects(); expect(getWasmCore().svet_pending_effect_count()).toBe(0);
  left.dispose(); right.dispose(); a.dispose(); b.dispose();
});

it('successful nested batches preserve their return value and flush before returning', () => {
  const source = signal(0), values: number[] = [], result = {value: 123};
  const observer = effect(() => { values.push(source()); });
  expect(batch(() => batch(() => { source.set(1); return result; }))).toBe(result);
  expect(values).toEqual([0,1]); observer.dispose(); source.dispose();
});
