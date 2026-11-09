import { beforeEach, expect, it } from 'vitest';
import {
  batch,
  computed,
  effect,
  getGraphTelemetry,
  resetReactiveSystem,
  signal,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());

// Long dependency lists take the Set-based duplicate check and the indexed
// subscriber lists; these cases pin their behaviour to the short-list rules.
const WIDE = 200;

it('reads each of many dependencies once per run despite repeats and reordering', () => {
  const sources = Array.from({ length: WIDE }, (_, i) => signal(i));
  const reversed = signal(false);
  let runs = 0,
    sum = 0;
  const handle = effect(() => {
    runs++;
    sum = 0;
    const order = reversed() ? [...sources].reverse() : sources;
    for (const source of order) sum += source() + source();
    // Re-read the first dependencies after the cursor has moved far past them.
    sum += sources[0]() + sources[1]() + sources[WIDE - 1]();
  });
  const edges = getGraphTelemetry().edges;
  expect(runs).toBe(1);
  sources[WIDE - 1].set(1000);
  expect(runs).toBe(2);
  expect(getGraphTelemetry().edges).toBe(edges);
  reversed.set(true);
  expect(runs).toBe(3);
  expect(getGraphTelemetry().edges).toBe(edges);
  for (const index of [0, 1, 57, WIDE - 1]) {
    const before = runs;
    sources[index].set(-index - 1);
    expect(runs).toBe(before + 1);
  }
  batch(() => sources.forEach((source, i) => source.set(i * 2)));
  expect(sum).toBe(sources.reduce((total, _, i) => total + i * 4, 0) + 0 + 2 + (WIDE - 1) * 2);
  handle.dispose();
  expect(getGraphTelemetry().edges).toBe(0);
});

it('drops dependencies a long run stops reading and picks up new ones', () => {
  const sources = Array.from({ length: WIDE }, (_, i) => signal(i));
  const from = signal(0);
  let runs = 0;
  const handle = effect(() => {
    runs++;
    for (let i = from(); i < WIDE; i += 2) sources[i]();
  });
  expect(getGraphTelemetry().edges).toBe(1 + WIDE / 2);
  from.set(1);
  expect(runs).toBe(2);
  expect(getGraphTelemetry().edges).toBe(1 + WIDE / 2);
  sources[0].set(-1);
  expect(runs).toBe(2);
  sources[1].set(-1);
  expect(runs).toBe(3);
  from.set(WIDE - 2);
  expect(getGraphTelemetry().edges).toBe(2);
  sources[1].set(-2);
  expect(runs).toBe(4);
  handle.dispose();
});

it('keeps every subscriber of a widely shared signal through churn', () => {
  const shared = signal(0);
  const seen = new Array<number>(WIDE).fill(-1);
  const handles = seen.map((_, i) =>
    effect(() => {
      seen[i] = shared();
    }),
  );
  const doubled = Array.from({ length: WIDE }, () => computed(() => shared() * 2));
  doubled.forEach(value => expect(value()).toBe(0));
  // Removing from the middle of the subscriber list must not lose its neighbours.
  for (let i = 0; i < WIDE; i += 3) handles[i].dispose();
  for (let i = 1; i < WIDE; i += 3) doubled[i].dispose();
  shared.set(7);
  seen.forEach((value, i) => expect(value).toBe(i % 3 === 0 ? 0 : 7));
  doubled.forEach((value, i) => {
    if (i % 3 !== 1) expect(value()).toBe(14);
  });
  const late = effect(() => {
    seen[0] = shared() + 100;
  });
  shared.set(8);
  expect(seen[0]).toBe(108);
  expect(seen[1]).toBe(8);
  late.dispose();
  handles.forEach(handle => handle.dispose());
  doubled.forEach(value => value.dispose());
  expect(getGraphTelemetry().edges).toBe(0);
});
