import { beforeEach, expect, it } from 'vitest';
import {
  computed,
  createServerAction,
  createWasmCoreInstance,
  effect,
  getGraphTelemetry,
  getWasmCore,
  resetReactiveSystem,
  signal,
  untrack,
  withWasmCore,
} from '../src/engine';

beforeEach(() => resetReactiveSystem());

it.each(['signal', 'computed'] as const)(
  'a foreign %s read never subscribes a paused outer observer',
  kind => {
    const own = getWasmCore(),
      foreign = createWasmCoreInstance();
    const source = signal(0),
      required = signal(0);
    const value = kind === 'computed' ? computed(() => source()) : source;
    let outerRuns = 0,
      inner: ReturnType<typeof effect> | undefined;
    const outer = effect(() => {
      outerRuns++;
      if (!inner)
        inner = withWasmCore(foreign, () =>
          effect(() => {
            value();
          }),
        );
      required();
    });
    try {
      source.set(1);
      expect(outerRuns).toBe(1);
      required.set(1);
      expect(outerRuns).toBe(2);
    } finally {
      outer.dispose();
      inner?.dispose();
      value.dispose();
      source.dispose();
      required.dispose();
    }
    for (const core of [own, foreign])
      expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
  },
);

it.each(['signal', 'computed'] as const)(
  'a throwing foreign %s observer restores the outer tracking domain',
  kind => {
    const own = getWasmCore(),
      foreign = createWasmCoreInstance();
    const source = signal(0),
      required = signal(0);
    const value = kind === 'computed' ? computed(() => source()) : source;
    let runs = 0;
    const outer = effect(() => {
      runs++;
      expect(() =>
        withWasmCore(foreign, () =>
          effect(() => {
            value();
            throw Error('foreign body');
          }),
        ),
      ).toThrow('foreign body');
      required();
    });
    try {
      source.set(1);
      expect(runs).toBe(1);
      required.set(1);
      expect(runs).toBe(2);
    } finally {
      outer.dispose();
      value.dispose();
      source.dispose();
      required.dispose();
    }
    for (const core of [own, foreign])
      expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0);
  },
);

it.each(['untrack', 'throw', 'action'] as const)(
  '%s suppresses reads in every suspended core and restores each observer',
  async mode => {
    const cores = [getWasmCore(), createWasmCoreInstance(), createWasmCoreInstance()];
    const incidental = cores.map(core => withWasmCore(core, () => signal(0)));
    const required = cores.map(core => withWasmCore(core, () => signal(0)));
    const nestedSource = withWasmCore(mode === 'action' ? cores[0] : cores[2], () => signal(0));
    let nestedRuns = 0;
    const handles: ReturnType<typeof effect>[] = [],
      runs = [0, 0, 0];
    let madeB = false,
      madeC = false,
      invoked = false,
      job: Promise<number> | undefined;
    const read = () => {
      incidental.forEach(source => source());
      handles.push(
        effect(() => {
          nestedSource();
          nestedRuns++;
        }),
      );
      return 7;
    };
    const action = createServerAction<void, number>({ rpc: async () => read() });
    try {
      handles.push(
        effect(() => {
          runs[0]++;
          if (!madeB) {
            madeB = true;
            withWasmCore(cores[1], () =>
              handles.push(
                effect(() => {
                  runs[1]++;
                  if (!madeC) {
                    madeC = true;
                    withWasmCore(cores[2], () =>
                      handles.push(
                        effect(() => {
                          runs[2]++;
                          if (!invoked) {
                            invoked = true;
                            if (mode === 'action') {
                              job = action();
                              void job.catch(() => {});
                            } else if (mode === 'throw')
                              expect(() =>
                                untrack(() => {
                                  read();
                                  throw Error('untracked failure');
                                }),
                              ).toThrow('untracked failure');
                            else untrack(read);
                          }
                          required[2]();
                        }),
                      ),
                    );
                  }
                  required[1]();
                }),
              ),
            );
          }
          required[0]();
        }),
      );
      incidental.forEach(source => source.set(1));
      expect(runs).toEqual([1, 1, 1]);
      nestedSource.set(1);
      expect(nestedRuns).toBe(2);
      required.forEach(source => source.set(1));
      expect(runs).toEqual([2, 2, 2]);
      if (job) expect(await job).toBe(7);
    } finally {
      handles.reverse().forEach(handle => handle.dispose());
      action.dispose();
      nestedSource.dispose();
      [...incidental, ...required].forEach(source => source.dispose());
    }
    cores.forEach(core => expect(withWasmCore(core, () => getGraphTelemetry().nodes)).toBe(0));
  },
);

it('does not restore a retired foreign observer after resetting its core inside untrack', () => {
  const own = getWasmCore(),
    other = createWasmCoreInstance();
  let fresh!: ReturnType<typeof signal<number>>,
    freshObserver!: ReturnType<typeof effect>,
    runs = 0;
  const old = withWasmCore(other, () =>
    effect(() => {
      withWasmCore(own, () =>
        untrack(() =>
          withWasmCore(other, () => {
            resetReactiveSystem();
            fresh = signal(17);
            freshObserver = effect(() => {
              fresh();
              runs++;
            });
          }),
        ),
      );
    }),
  );
  old.dispose();
  fresh.set(18);
  expect(runs).toBe(2);
  expect(fresh()).toBe(18);
  freshObserver.dispose();
  fresh.dispose();
  expect(withWasmCore(other, () => getGraphTelemetry().nodes)).toBe(0);
});

it('still reruns a foreign suspended effect when untracked work writes an already-read dependency', () => {
  const other = createWasmCoreInstance(),
    source = signal(0);
  let runs = 0;
  const observer = effect(() => {
    runs++;
    source();
    if (runs === 1) withWasmCore(other, () => untrack(() => source.set(1)));
  });
  expect(runs).toBe(2);
  expect(source()).toBe(1);
  observer.dispose();
  source.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});

it('untrack excludes incidental reads from a suspended computed in another core', () => {
  const other = createWasmCoreInstance(),
    source = signal(0),
    incidental = signal(0);
  let evaluations = 0;
  const value = computed(() => {
    evaluations++;
    withWasmCore(other, () => untrack(() => incidental()));
    return source();
  });
  expect(value()).toBe(0);
  incidental.set(1);
  expect(value()).toBe(0);
  expect(evaluations).toBe(1);
  source.set(1);
  expect(value()).toBe(1);
  expect(evaluations).toBe(2);
  value.dispose();
  source.dispose();
  incidental.dispose();
  expect(getGraphTelemetry().nodes).toBe(0);
});
