import { beforeEach, expect, it } from 'vitest';
import { createRoot, effect, resetReactiveSystem, signal } from '../src/engine';

beforeEach(() => resetReactiveSystem());

it('a disposed handle cannot dispose a later effect that reuses its pooled entry', () => {
  const source = signal(0);
  const first = effect(() => { source(); });
  const firstId = first.id;
  first.dispose();
  const values: number[] = [];
  const second = effect(() => { values.push(source()); });
  expect(second.id).toBe(firstId); // Exercise actual slot reuse.
  first.dispose();
  source.set(1);
  expect(values).toEqual([0, 1]);
  second.dispose();
});

it('owner cleanup cannot dispose an unrelated replacement of a manually disposed effect', () => {
  const source = signal(0);
  let stop!: () => void;
  createRoot(dispose => {
    stop = dispose;
    const handle = effect(() => { source(); });
    handle.dispose();
  });
  const values: number[] = [];
  const replacement = effect(() => { values.push(source()); });
  stop();
  source.set(1);
  expect(values).toEqual([0, 1]);
  replacement.dispose();
});

it('keeps public handles distinct and IDs stable across slot reuse', () => {
  const first = effect(() => {});
  const id = first.id;
  first.dispose();
  const second = effect(() => {});
  expect(first).not.toBe(second);
  expect(first.id).toBe(id);
  expect(second.id).toBe(id);
  second.dispose();
});

it('stale disposal neither runs replacement cleanup nor stops a WASM-linked effect', () => {
  const a = signal(0);
  const b = signal(0);
  const first = effect(() => { a(); b(); });
  first.dispose();
  let cleanups = 0;
  const values: number[] = [];
  const second = effect(() => {
    values.push(a() + b());
    return () => { cleanups++; };
  });
  first.dispose();
  expect(cleanups).toBe(0);
  a.set(1);
  expect(values).toEqual([0, 1]);
  expect(cleanups).toBe(1);
  second.dispose();
  second.dispose();
  expect(cleanups).toBe(2);
  b.set(1);
  expect(values).toEqual([0, 1]);
});
